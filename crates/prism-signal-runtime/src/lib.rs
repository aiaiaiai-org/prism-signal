// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! The stateless `prism-signal.v1` runtime: one request in, one response out.
//!
//! A [`Runtime`] holds only immutable vocabulary. It reads no clock, opens no network, and keeps
//! nothing between requests, so the same request always gets the same answer. The caller owns
//! scheduling, storage, and who is told.

use prism_signal_bridge::{BridgeRules, Skip, bridge};
use prism_signal_core::{Evidence, SignalObservation};
use prism_signal_fusion::{FUSION_V1, FusionPolicy, assess};
use prism_signal_geo::{cover, expand};
use prism_signal_normalize::Normalizer;
use prism_signal_observe::{Gazetteer, NormalizeRules, Normalizer as Observe, SkipReason};
use prism_signal_protocol::{
    AssessPayload, Call, Capabilities, CoverPayload, CoverResult, Limits, MAX_COVER_CELLS,
    MAX_EVIDENCE, MAX_OBSERVATIONS, NormalizePayload, NormalizeResult, PROTOCOL_VERSION, Reader,
    Request, Response, ResultBody, SkippedItem,
};
use serde_json::Value;

const OPERATIONS: [&str; 4] = ["capabilities", "normalize", "cover", "assess"];

/// The runtime.
pub struct Runtime {
    normalize: Normalizer,
    gazetteer: Gazetteer,
}

impl Runtime {
    /// A runtime with the vocabulary shipped in the workspace.
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            normalize: Normalizer::embedded().map_err(|e| e.to_string())?,
            gazetteer: Gazetteer::ukraine(),
        })
    }

    /// Answers one request given as JSON text. Never panics on bad input: a request that cannot
    /// be read is answered with a typed failure.
    pub fn handle(&self, text: &str) -> Response {
        let value: Value = match serde_json::from_str(text) {
            Ok(value) => value,
            Err(_) => {
                return Response::failed("", "invalid_request", "the request is not valid JSON");
            }
        };
        let request_id = value
            .get("request_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if let Some(operation) = value.get("operation").and_then(Value::as_str) {
            if !OPERATIONS.contains(&operation) {
                return Response::failed(request_id, "unsupported_operation", "unknown operation");
            }
        }
        let request: Request = match serde_json::from_value(value) {
            Ok(request) => request,
            Err(_) => {
                return Response::failed(
                    request_id,
                    "invalid_request",
                    "the request does not match the protocol",
                );
            }
        };
        if request.protocol_version != PROTOCOL_VERSION {
            return Response::failed(
                request.request_id,
                "invalid_request",
                "unsupported protocol_version",
            );
        }
        let id = request.request_id;
        match request.call {
            Call::Capabilities => Response::ok(id, ResultBody::Capabilities(capabilities())),
            Call::Normalize(payload) => self.normalize(id, payload),
            Call::Cover(payload) => cover_request(id, payload),
            Call::Assess(payload) => assess_request(id, payload),
        }
    }

    fn normalize(&self, id: String, payload: NormalizePayload) -> Response {
        if payload.evidence.len() > MAX_EVIDENCE {
            return Response::failed(id, "window_too_large", "too many evidence items");
        }
        let mut observations = Vec::new();
        let mut skipped = Vec::new();
        for evidence in &payload.evidence {
            match payload.reader {
                Reader::Normalize => self.read_normalize(evidence, &mut observations, &mut skipped),
                Reader::Observe => self.read_observe(evidence, &mut observations, &mut skipped),
            }
        }
        Response::ok(
            id,
            ResultBody::Normalize(NormalizeResult {
                reader: payload.reader,
                observations,
                skipped,
            }),
        )
    }

    fn read_normalize(
        &self,
        evidence: &Evidence,
        observations: &mut Vec<SignalObservation>,
        skipped: &mut Vec<SkippedItem>,
    ) {
        let readings = self.normalize.read(evidence);
        let bridged = bridge(&readings, &BridgeRules::default());
        observations.extend(bridged.observations);
        for skip in bridged.skipped {
            let code = match skip {
                Skip::Forwarded => "forwarded",
                Skip::UnlocatedClear => "unlocated_clear",
                Skip::Geometry(_) => "invalid_geometry",
            };
            skipped.push(SkippedItem {
                evidence_id: evidence.external_id.clone(),
                code: code.to_owned(),
            });
        }
    }

    fn read_observe(
        &self,
        evidence: &Evidence,
        observations: &mut Vec<SignalObservation>,
        skipped: &mut Vec<SkippedItem>,
    ) {
        let normalized =
            Observe::new(&self.gazetteer, NormalizeRules::default()).normalize(evidence);
        observations.extend(normalized.observations);
        for skip in normalized.skipped {
            let code = match skip.reason {
                SkipReason::Forwarded => "forwarded",
                SkipReason::Unlocated { .. } => "unlocated",
                SkipReason::AmbiguousPlace { .. } => "ambiguous_place",
            };
            skipped.push(SkippedItem {
                evidence_id: evidence.external_id.clone(),
                code: code.to_owned(),
            });
        }
    }
}

fn capabilities() -> Capabilities {
    use prism_signal_core::HazardKind::{
        AttackDrone, BallisticMissile, GuidedBomb, JetDrone, Missile,
    };
    Capabilities {
        protocol_version: PROTOCOL_VERSION.to_owned(),
        operations: OPERATIONS.iter().map(|s| (*s).to_owned()).collect(),
        readers: vec![Reader::Normalize, Reader::Observe],
        policy_versions: vec![FUSION_V1.to_owned()],
        hazard_kinds: vec![BallisticMissile, Missile, AttackDrone, JetDrone, GuidedBomb],
        grid_resolution: FusionPolicy::v1().resolution,
        limits: Limits {
            max_evidence: MAX_EVIDENCE,
            max_observations: MAX_OBSERVATIONS,
            max_cells: MAX_COVER_CELLS,
        },
    }
}

fn cover_request(id: String, payload: CoverPayload) -> Response {
    let resolution = payload.resolution.unwrap_or(FusionPolicy::v1().resolution);
    let max_cells = payload
        .max_cells
        .unwrap_or(MAX_COVER_CELLS)
        .min(MAX_COVER_CELLS);
    let base = match cover(&payload.geometry, resolution, max_cells) {
        Ok(cells) => cells,
        Err(error) => return Response::failed(id, error.code(), error.to_string()),
    };
    let cells = match expand(&base, payload.rings, max_cells) {
        Ok(cells) => cells,
        Err(error) => return Response::failed(id, error.code(), error.to_string()),
    };
    Response::ok(id, ResultBody::Cover(CoverResult { cells }))
}

fn assess_request(id: String, payload: AssessPayload) -> Response {
    let Ok(policy) = FusionPolicy::named(&payload.policy_version) else {
        return Response::failed(id, "unknown_policy", "unknown policy_version");
    };
    if payload.observations.len() > MAX_OBSERVATIONS {
        return Response::failed(id, "window_too_large", "too many observations");
    }
    let assessed = assess(&payload.observations, &policy, payload.evaluation_time);
    Response::ok(id, ResultBody::Assess(assessed))
}
