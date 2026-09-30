// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Wire types of `prism-signal.v1`.
//!
//! The protocol lets a hub, or any caller, invoke Prism Signal without reimplementing reading,
//! cover, or fusion. It follows the shape of `prism-execution.v1`: one JSON envelope in, one out,
//! stdout carrying the protocol and nothing else.
//!
//! ```json
//! {"protocol_version":"prism-signal.v1","request_id":"r1","operation":"assess",
//!  "payload":{"policy_version":"fusion.v1","evaluation_time":"2026-09-28T22:30:00Z","observations":[]}}
//! ```
//!
//! Every response repeats `request_id` and carries either a `result` or a typed `failure`.

use prism_signal_core::{
    CellResolution, Evidence, ExternalId, Geometry, HazardKind, SignalObservation, Timestamp,
};
use prism_signal_fusion::Assessed;
use prism_signal_geo::CellSet;
use serde::{Deserialize, Serialize};

/// The only protocol version so far.
pub const PROTOCOL_VERSION: &str = "prism-signal.v1";

/// Most evidence items one `normalize` request may carry.
pub const MAX_EVIDENCE: usize = 500;

/// Most observations one `assess` request may carry.
pub const MAX_OBSERVATIONS: usize = 5000;

/// Default and largest bound on the cells of one `cover` request.
pub const MAX_COVER_CELLS: usize = 400;

/// One request.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Request {
    /// Exactly [`PROTOCOL_VERSION`].
    pub protocol_version: String,
    /// Caller-owned correlation reference, repeated in the response.
    pub request_id: String,
    /// What to do, with its payload.
    #[serde(flatten)]
    pub call: Call,
}

/// An operation and its payload.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "operation", content = "payload", rename_all = "snake_case")]
pub enum Call {
    /// What this runtime supports. Takes no payload.
    Capabilities,
    /// Reads evidence into located observations.
    Normalize(NormalizePayload),
    /// Turns a geometry into grid cells.
    Cover(CoverPayload),
    /// Fuses a window of observations into assessments.
    Assess(AssessPayload),
}

/// Which reader turns text into observations.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reader {
    /// `prism-signal-normalize`: place roles, so a launch site is never an alert.
    #[default]
    Normalize,
    /// `prism-signal-observe`: a larger gazetteer, counts, no roles.
    Observe,
}

/// Payload of `normalize`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizePayload {
    /// The reader. Defaults to `normalize`.
    #[serde(default)]
    pub reader: Reader,
    /// Evidence to read, at most [`MAX_EVIDENCE`] items.
    pub evidence: Vec<Evidence>,
}

/// Result of `normalize`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NormalizeResult {
    /// The reader that ran.
    pub reader: Reader,
    /// Observations, in evidence order.
    pub observations: Vec<SignalObservation>,
    /// What was left out, with stable codes.
    pub skipped: Vec<SkippedItem>,
}

/// Something a reader left out.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct SkippedItem {
    /// The evidence item.
    pub evidence_id: ExternalId,
    /// Why: `forwarded`, `unlocated`, `unlocated_clear`, `ambiguous_place`, or `invalid_geometry`.
    pub code: String,
}

/// Payload of `cover`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoverPayload {
    /// The zone.
    pub geometry: Geometry,
    /// Grid resolution of the answer. Defaults to the policy's.
    #[serde(default)]
    pub resolution: Option<CellResolution>,
    /// Rings of neighbouring cells to add. Defaults to none.
    #[serde(default)]
    pub rings: u32,
    /// Bound on the answer, at most [`MAX_COVER_CELLS`]. Larger covers fail, never truncate.
    #[serde(default)]
    pub max_cells: Option<usize>,
}

/// Result of `cover`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CoverResult {
    /// Sorted, deduplicated cells.
    pub cells: CellSet,
}

/// Payload of `assess`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssessPayload {
    /// The policy, such as `fusion.v1`.
    pub policy_version: String,
    /// The instant fusion is evaluated at. Required: the runtime never reads a clock.
    pub evaluation_time: Timestamp,
    /// The observation window, at most [`MAX_OBSERVATIONS`].
    pub observations: Vec<SignalObservation>,
}

/// Result of `capabilities`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Capabilities {
    /// [`PROTOCOL_VERSION`].
    pub protocol_version: String,
    /// Operations this runtime answers.
    pub operations: Vec<String>,
    /// Readers `normalize` accepts.
    pub readers: Vec<Reader>,
    /// Policies `assess` accepts.
    pub policy_versions: Vec<String>,
    /// Hazard kinds observations may carry.
    pub hazard_kinds: Vec<HazardKind>,
    /// Grid resolution of an assessment's cells under the default policy.
    pub grid_resolution: CellResolution,
    /// Limits of one request.
    pub limits: Limits,
}

/// Limits of one request.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Limits {
    /// Evidence items in one `normalize`.
    pub max_evidence: usize,
    /// Observations in one `assess`.
    pub max_observations: usize,
    /// Cells in one `cover`.
    pub max_cells: usize,
}

/// A typed failure. `code` is stable and machine-readable; `message` is for people.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Failure {
    /// One of `invalid_request`, `unsupported_operation`, `unknown_policy`, `invalid_geometry`,
    /// `cover_too_large`, `window_too_large`.
    pub code: String,
    /// What went wrong, without echoing the request.
    pub message: String,
}

/// The body of a successful response.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ResultBody {
    /// Answer to `capabilities`.
    Capabilities(Capabilities),
    /// Answer to `normalize`.
    Normalize(NormalizeResult),
    /// Answer to `cover`.
    Cover(CoverResult),
    /// Answer to `assess`.
    Assess(Assessed),
}

/// One response.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Response {
    /// [`PROTOCOL_VERSION`].
    pub protocol_version: String,
    /// The request's `request_id`, or empty when the request had none.
    pub request_id: String,
    /// Present on success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<ResultBody>,
    /// Present on failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
}

impl Response {
    /// A success.
    pub fn ok(request_id: impl Into<String>, result: ResultBody) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION.to_owned(),
            request_id: request_id.into(),
            result: Some(result),
            failure: None,
        }
    }

    /// A failure.
    pub fn failed(request_id: impl Into<String>, code: &str, message: impl Into<String>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION.to_owned(),
            request_id: request_id.into(),
            result: None,
            failure: Some(Failure {
                code: code.to_owned(),
                message: message.into(),
            }),
        }
    }
}
