// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Fusion of located observations into assessments with a lifecycle.
//!
//! [`assess`] is a pure function of `(observations, policy, evaluation time)`. It reads no clock
//! and keeps no state: the caller supplies the window and the time, and the same window always
//! gives byte-identical output. Nothing here knows who is subscribed or where anyone is.
//!
//! # What an assessment is
//!
//! One assessment is one episode of one class of hazard at one place: "drones at Kyiv, since
//! 22:19". It carries the cells a person must be standing in to be concerned, the window it is
//! valid for, and the evidence behind it. It is a proposal. Whether to tell anyone is the
//! consumer's decision.
//!
//! # Lifecycle
//!
//! | Event | When |
//! | --- | --- |
//! | `issued` | the first threat observation of an episode |
//! | `superseded` | a later post reports the same hazard at the same place while it is valid; the window is extended |
//! | `expired` | the window ran out with no newer report |
//! | `retracted` | the source calls that hazard off at that place |
//!
//! After a retraction or expiry the next threat starts a new episode with a new id.
//!
//! # Not saying "all clear" when it is not
//!
//! A false all-clear is the worst thing this layer can produce, so a retraction is narrow:
//!
//! - it needs an explicit `clear` observation, which the reader only produces from explicit
//!   all-clear words;
//! - it applies to the **same class at the same place**. Overlapping cells are not enough: an
//!   all-clear for Brovary must not end the alert for Kyiv;
//! - it applies only to episodes that started before it, and to episodes still valid;
//! - it is ignored when the same post also reports that class at that place as a threat.
//!
//! An all-clear that cannot be located, or that names no kind, is not evidence this layer can
//! use. The threat it meant to end lapses on its validity window instead.

use std::collections::{BTreeMap, BTreeSet};

use prism_signal_core::{
    CellId, CellResolution, ConfidenceBand, ExternalId, Geometry, HazardKind, SignalObservation,
    SourceId, Stance, Timestamp,
};
use prism_signal_geo::{CellSet, CoverError, cover, disc, expand};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::{Duration, OffsetDateTime};

/// Identifier of the only policy so far.
pub const FUSION_V1: &str = "fusion.v1";

/// A policy is unknown.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("unknown policy `{0}`")]
pub struct UnknownPolicy(pub String);

/// Hazards that mean the same thing to a person deciding what to do.
///
/// Cruise, ballistic, and unspecified missiles are one class; attack and jet drones are one.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HazardClass {
    /// Attack drones, piston or jet.
    Drone,
    /// Guided aerial bombs.
    Bomb,
    /// Missiles of any kind.
    Missile,
}

impl HazardClass {
    /// The class a hazard kind belongs to.
    pub fn of(kind: HazardKind) -> Self {
        match kind {
            HazardKind::AttackDrone | HazardKind::JetDrone => Self::Drone,
            HazardKind::GuidedBomb => Self::Bomb,
            HazardKind::Missile | HazardKind::BallisticMissile => Self::Missile,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Drone => "drone",
            Self::Bomb => "bomb",
            Self::Missile => "missile",
        }
    }
}

/// The parameters of fusion, named and versioned so results are replayable.
#[derive(Clone, Debug, PartialEq)]
pub struct FusionPolicy {
    /// Policy identifier, [`FUSION_V1`].
    pub version: &'static str,
    /// Grid resolution of the cells an assessment covers.
    pub resolution: CellResolution,
    /// Rings of neighbouring cells added around a cover; see [`prism_signal_geo::expand`].
    pub edge_rings: u32,
    /// The most cells one assessment may cover.
    pub max_cells: usize,
    /// Radius of the disc drawn around an observation that is a bare point, in km.
    pub point_radius_km: f64,
    /// Corners of that disc.
    pub disc_vertices: usize,
}

impl FusionPolicy {
    /// Looks a policy up by its identifier.
    pub fn named(version: &str) -> Result<Self, UnknownPolicy> {
        match version {
            FUSION_V1 => Ok(Self::v1()),
            other => Err(UnknownPolicy(other.to_owned())),
        }
    }

    /// `fusion.v1`: cells at H3 resolution 6 (about 3 km across), one ring of margin, and a
    /// 10 km disc around a bare point.
    pub fn v1() -> Self {
        Self {
            version: FUSION_V1,
            resolution: CellResolution::try_from(6).expect("6 is a valid resolution"),
            edge_rings: 1,
            max_cells: 400,
            point_radius_km: 10.0,
            disc_vertices: 48,
        }
    }
}

/// A piece of evidence behind an assessment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EvidenceRef {
    /// Declared source.
    pub source_id: SourceId,
    /// Item inside the source.
    pub evidence_id: ExternalId,
    /// Public URL of the item.
    pub url: String,
    /// When the source reported it.
    pub observed_at: Timestamp,
}

/// The place an assessment is about.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlaceRef {
    /// Gazetteer identifier.
    pub id: String,
    /// Display name, when the reader gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Where an assessment stands as of the evaluation time.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Valid at the evaluation time.
    Active,
    /// Its window ran out.
    Expired,
    /// The source called it off.
    Retracted,
}

/// One episode of one class of hazard at one place.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Assessment {
    /// Stable identifier: policy, class, place, and the first evidence.
    pub assessment_id: String,
    /// Policy that produced it.
    pub policy_version: String,
    /// Class of hazard.
    pub class: HazardClass,
    /// The specific kinds the evidence named, in vocabulary order.
    pub kinds: Vec<HazardKind>,
    /// The place, when the observations carried one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<PlaceRef>,
    /// Cells a person must be standing in to be concerned.
    pub cells: CellSet,
    /// When the first report was made.
    pub valid_from: Timestamp,
    /// When it lapses unless renewed.
    pub valid_until: Timestamp,
    /// Where it stands.
    pub status: Status,
    /// Starts at 1 and grows with each `superseded` event.
    pub revision: u32,
    /// A band, never a percentage: `moderate` for one source, `high` for two or more.
    pub likelihood: ConfidenceBand,
    /// Distinct sources behind it.
    pub corroboration_count: u32,
    /// The evidence, oldest first.
    pub evidence: Vec<EvidenceRef>,
}

/// What happened to an assessment.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// The first report of an episode.
    Issued,
    /// A later report of the same hazard at the same place while it was valid.
    Superseded,
    /// The window ran out.
    Expired,
    /// The source called it off.
    Retracted,
}

/// Why an assessment was retracted.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetractionReason {
    /// The source reported an explicit all-clear for this hazard at this place.
    SourceAllClear,
}

/// A change to an assessment. `(assessment_id, seq)` is unique and never reused, so a consumer
/// can apply events idempotently.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AssessmentEvent {
    /// The assessment it belongs to.
    pub assessment_id: String,
    /// Position among that assessment's events, from 1.
    pub seq: u32,
    /// What happened.
    pub kind: EventKind,
    /// When it took effect, derived from the evidence or the window and never from a clock.
    pub effective_at: Timestamp,
    /// The report that caused it; absent for `expired`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<EvidenceRef>,
    /// Set on `retracted`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<RetractionReason>,
}

/// An observation that could not be used.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Skipped {
    /// The evidence it came from.
    pub evidence_id: ExternalId,
    /// A stable code such as `invalid_geometry` or `cover_too_large`.
    pub code: String,
}

/// The result of one fusion.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Assessed {
    /// Every episode in the window, oldest first, with its status as of the evaluation time.
    pub assessments: Vec<Assessment>,
    /// Every event the window implies, in the order they took effect.
    pub events: Vec<AssessmentEvent>,
    /// Observations that could not be used.
    pub skipped: Vec<Skipped>,
}

struct Footprint {
    cells: CellSet,
}

fn footprint(
    observation: &SignalObservation,
    policy: &FusionPolicy,
) -> Result<Footprint, CoverError> {
    let geometry = match &observation.geometry {
        Geometry::Point { coordinates } => {
            disc(*coordinates, policy.point_radius_km, policy.disc_vertices)?
        }
        other => other.clone(),
    };
    let base = cover(&geometry, policy.resolution, policy.max_cells)?;
    let cells = expand(&base, policy.edge_rings, policy.max_cells)?;
    Ok(Footprint { cells })
}

/// Where an episode is. An observation with a place is keyed by it; one without is keyed by its
/// first cell.
fn place_key(observation: &SignalObservation, footprint: &Footprint) -> String {
    match &observation.provenance.place_id {
        Some(id) => id.clone(),
        None => format!(
            "cells:{}",
            footprint.cells.cells.first().map_or("none", CellId::as_str)
        ),
    }
}

struct Episode {
    id: String,
    class: HazardClass,
    place: Option<PlaceRef>,
    cells: CellSet,
    first_at: OffsetDateTime,
    valid_until: OffsetDateTime,
    kinds: BTreeSet<HazardKind>,
    sources: BTreeSet<SourceId>,
    evidence: Vec<EvidenceRef>,
    revision: u32,
    seq: u32,
}

impl Episode {
    fn has(&self, source: &SourceId, evidence: &ExternalId) -> bool {
        self.evidence
            .iter()
            .any(|e| &e.source_id == source && &e.evidence_id == evidence)
    }

    fn finish(self, status: Status, policy: &FusionPolicy) -> Assessment {
        let likelihood = if self.sources.len() >= 2 {
            ConfidenceBand::High
        } else {
            ConfidenceBand::Moderate
        };
        Assessment {
            assessment_id: self.id,
            policy_version: policy.version.to_owned(),
            class: self.class,
            kinds: self.kinds.into_iter().collect(),
            place: self.place,
            cells: self.cells,
            valid_from: Timestamp::from_datetime(self.first_at),
            valid_until: Timestamp::from_datetime(self.valid_until),
            status,
            revision: self.revision,
            likelihood,
            corroboration_count: u32::try_from(self.sources.len()).unwrap_or(u32::MAX),
            evidence: self.evidence,
        }
    }
}

fn evidence_ref(observation: &SignalObservation) -> EvidenceRef {
    EvidenceRef {
        source_id: observation.source_id.clone(),
        evidence_id: observation.provenance.evidence_id.clone(),
        url: observation.provenance.evidence_url.clone(),
        observed_at: observation.observed_at,
    }
}

fn valid_until(observation: &SignalObservation) -> OffsetDateTime {
    observation.observed_at.as_datetime() + Duration::seconds(i64::from(observation.ttl.get()))
}

/// Fuses a window of observations, as of `evaluation_time`.
///
/// Observations reported after `evaluation_time` are ignored: they are not yet known. Equal
/// input always gives equal output, whatever the order of `observations`.
pub fn assess(
    observations: &[SignalObservation],
    policy: &FusionPolicy,
    evaluation_time: Timestamp,
) -> Assessed {
    let mut out = Assessed::default();
    let now = evaluation_time.as_datetime();

    // Usable observations with their footprints, in a total order.
    let mut usable: Vec<(&SignalObservation, Footprint, HazardClass, String)> = Vec::new();
    for observation in observations {
        if observation.observed_at.as_datetime() > now {
            continue;
        }
        if observation.geometry.validate().is_err() {
            out.skipped.push(Skipped {
                evidence_id: observation.provenance.evidence_id.clone(),
                code: "invalid_geometry".to_owned(),
            });
            continue;
        }
        match footprint(observation, policy) {
            Ok(fp) => {
                let key = place_key(observation, &fp);
                usable.push((observation, fp, HazardClass::of(observation.kind), key));
            }
            Err(error) => out.skipped.push(Skipped {
                evidence_id: observation.provenance.evidence_id.clone(),
                code: error.code().to_owned(),
            }),
        }
    }
    usable.sort_by(|a, b| {
        (
            a.0.observed_at,
            &a.0.source_id,
            &a.0.provenance.evidence_id,
            a.2,
            &a.3,
            a.0.stance as u8,
        )
            .cmp(&(
                b.0.observed_at,
                &b.0.source_id,
                &b.0.provenance.evidence_id,
                b.2,
                &b.3,
                b.0.stance as u8,
            ))
    });
    out.skipped
        .sort_by(|a, b| (&a.evidence_id, &a.code).cmp(&(&b.evidence_id, &b.code)));

    // A clear is contradicted when its own post also reports that class at that place.
    let contradicted = |clear: &(&SignalObservation, Footprint, HazardClass, String)| {
        usable.iter().any(|(other, _, class, key)| {
            other.stance == Stance::Threat
                && *class == clear.2
                && *key == clear.3
                && other.source_id == clear.0.source_id
                && other.provenance.evidence_id == clear.0.provenance.evidence_id
        })
    };

    let mut open: BTreeMap<(HazardClass, String), Episode> = BTreeMap::new();
    let mut closed: Vec<Assessment> = Vec::new();
    let mut events: Vec<AssessmentEvent> = Vec::new();

    for item in &usable {
        let (observation, fp, class, key) = item;
        let at = observation.observed_at.as_datetime();
        let slot = (*class, key.clone());
        match observation.stance {
            Stance::Threat => {
                if let Some(episode) = open.get_mut(&slot) {
                    if at <= episode.valid_until {
                        episode.kinds.insert(observation.kind);
                        episode.sources.insert(observation.source_id.clone());
                        episode.valid_until = episode.valid_until.max(valid_until(observation));
                        if !episode.has(&observation.source_id, &observation.provenance.evidence_id)
                        {
                            episode.evidence.push(evidence_ref(observation));
                            episode.revision += 1;
                            episode.seq += 1;
                            events.push(AssessmentEvent {
                                assessment_id: episode.id.clone(),
                                seq: episode.seq,
                                kind: EventKind::Superseded,
                                effective_at: observation.observed_at,
                                evidence: Some(evidence_ref(observation)),
                                reason: None,
                            });
                        }
                        continue;
                    }
                    // The window ran out before this report: close that episode, start anew.
                    if let Some(episode) = open.remove(&slot) {
                        events.push(AssessmentEvent {
                            assessment_id: episode.id.clone(),
                            seq: episode.seq + 1,
                            kind: EventKind::Expired,
                            effective_at: Timestamp::from_datetime(episode.valid_until),
                            evidence: None,
                            reason: None,
                        });
                        closed.push(episode.finish(Status::Expired, policy));
                    }
                }
                let id = format!(
                    "{}/{}/{}/{}",
                    policy.version,
                    class.as_str(),
                    key,
                    observation.provenance.evidence_id.as_str()
                );
                let episode = Episode {
                    id: id.clone(),
                    class: *class,
                    place: observation
                        .provenance
                        .place_id
                        .as_ref()
                        .map(|place| PlaceRef {
                            id: place.clone(),
                            name: observation.provenance.place_name.clone(),
                        }),
                    cells: fp.cells.clone(),
                    first_at: at,
                    valid_until: valid_until(observation),
                    kinds: BTreeSet::from([observation.kind]),
                    sources: BTreeSet::from([observation.source_id.clone()]),
                    evidence: vec![evidence_ref(observation)],
                    revision: 1,
                    seq: 1,
                };
                events.push(AssessmentEvent {
                    assessment_id: id,
                    seq: 1,
                    kind: EventKind::Issued,
                    effective_at: observation.observed_at,
                    evidence: Some(evidence_ref(observation)),
                    reason: None,
                });
                open.insert(slot, episode);
            }
            Stance::Clear => {
                if contradicted(item) {
                    continue;
                }
                // Same class, same place, still valid. Observations are handled in time order,
                // so an all-clear can only meet an episode that has already started.
                let hit = open.get(&slot).is_some_and(|e| at <= e.valid_until);
                if hit {
                    if let Some(mut episode) = open.remove(&slot) {
                        episode.seq += 1;
                        events.push(AssessmentEvent {
                            assessment_id: episode.id.clone(),
                            seq: episode.seq,
                            kind: EventKind::Retracted,
                            effective_at: observation.observed_at,
                            evidence: Some(evidence_ref(observation)),
                            reason: Some(RetractionReason::SourceAllClear),
                        });
                        closed.push(episode.finish(Status::Retracted, policy));
                    }
                }
            }
        }
    }

    for (_, episode) in open {
        if episode.valid_until <= now {
            events.push(AssessmentEvent {
                assessment_id: episode.id.clone(),
                seq: episode.seq + 1,
                kind: EventKind::Expired,
                effective_at: Timestamp::from_datetime(episode.valid_until),
                evidence: None,
                reason: None,
            });
            closed.push(episode.finish(Status::Expired, policy));
        } else {
            closed.push(episode.finish(Status::Active, policy));
        }
    }

    closed.sort_by(|a, b| (a.valid_from, &a.assessment_id).cmp(&(b.valid_from, &b.assessment_id)));
    events.sort_by(|a, b| {
        (a.effective_at, &a.assessment_id, a.seq).cmp(&(b.effective_at, &b.assessment_id, b.seq))
    });
    out.assessments = closed;
    out.events = events;
    out
}
