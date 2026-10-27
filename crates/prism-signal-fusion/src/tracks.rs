// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Tracks: one reported object each, followed from its first report to an explicit end.
//!
//! Assessments follow places. A track follows an object: `4 шахеди на Київ` is four tracks,
//! `збито 1 над Броварами` ends one of them, and the other three stay. See `docs/tracking.md`.
//!
//! Like [`assess`](crate::assess), this is a pure function of the window, the policy and the
//! evaluation time.
//!
//! # Not saying "all clear" when it is not
//!
//! - Only explicit words end a track: a neutralization ends exactly the number it counts, and a
//!   clear ends only the tracks its own source reported at the place it names.
//! - A track whose count was never stated (`шахеди на Київ`) stands for "at least one" and is
//!   never ended by a neutralization, only by a clear.
//! - Silence ends nothing. A track with no news becomes `quiet`, still live, and after much
//!   longer `stale`, which a consumer must show as "no news since", never as an all-clear.
//! - Ambiguity resolves toward more tracks: a report that matches nothing starts its own.

use std::collections::BTreeMap;

use prism_signal_core::{
    ExternalId, Geometry, HazardKind, PlaceRole, Position, SignalObservation, SourceId, Stance,
    Timestamp,
};
use prism_signal_geo::{CellSet, corridor, cover, disc, expand};
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

use crate::{EvidenceRef, FusionPolicy, HazardClass, PlaceRef, evidence_ref};

/// The parameters of tracking.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackPolicy {
    /// Seconds without news before a drone track is `quiet`.
    pub drone_quiet_secs: i64,
    /// Seconds without news before a missile or bomb track is `quiet`.
    pub fast_quiet_secs: i64,
    /// Seconds without news before any track is `stale` and closed.
    pub stale_secs: i64,
    /// Top speeds, km/h, used to decide whether a report can be the same object.
    pub attack_drone_kmh: f64,
    /// Jet drones.
    pub jet_drone_kmh: f64,
    /// Cruise or unspecified missiles.
    pub missile_kmh: f64,
    /// How far a guided bomb glides from where it was reported, km.
    pub bomb_reach_km: f64,
    /// Added to every reachability test, for the coarseness of places, km.
    pub margin_km: f64,
    /// The most a position circle grows with time since the last report, km.
    pub circle_cap_km: f64,
    /// Half-width of a drone corridor, km.
    pub drone_band_km: f64,
    /// Half-width of a missile or bomb corridor, km.
    pub fast_band_km: f64,
    /// The most cells one track may cover.
    pub max_cells: usize,
}

impl TrackPolicy {
    /// The working values of `docs/tracking.md`, to be calibrated on the archive.
    pub fn v1() -> Self {
        Self {
            drone_quiet_secs: 3600,
            fast_quiet_secs: 1800,
            stale_secs: 6 * 3600,
            attack_drone_kmh: 200.0,
            jet_drone_kmh: 700.0,
            missile_kmh: 1000.0,
            bomb_reach_km: 70.0,
            margin_km: 10.0,
            circle_cap_km: 40.0,
            drone_band_km: 15.0,
            fast_band_km: 25.0,
            max_cells: 3000,
        }
    }

    fn quiet_after(&self, class: HazardClass) -> Duration {
        Duration::seconds(match class {
            HazardClass::Drone => self.drone_quiet_secs,
            HazardClass::Bomb | HazardClass::Missile => self.fast_quiet_secs,
        })
    }

    fn band_km(&self, class: HazardClass) -> f64 {
        match class {
            HazardClass::Drone => self.drone_band_km,
            HazardClass::Bomb | HazardClass::Missile => self.fast_band_km,
        }
    }

    /// km/h, or `None` for kinds that are matched by place only.
    fn speed_kmh(&self, kind: HazardKind) -> Option<f64> {
        match kind {
            HazardKind::AttackDrone => Some(self.attack_drone_kmh),
            HazardKind::JetDrone => Some(self.jet_drone_kmh),
            HazardKind::Missile => Some(self.missile_kmh),
            HazardKind::BallisticMissile | HazardKind::GuidedBomb => None,
        }
    }
}

/// Where a track stands as of the evaluation time.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackStatus {
    /// Reported recently.
    Active,
    /// Live, but no news for the kind's quiet time.
    Quiet,
    /// Reported shot down.
    Neutralized,
    /// Called off by the source that reported it.
    Cleared,
    /// No news for the stale time. Not an all-clear: "no news since".
    Stale,
}

impl TrackStatus {
    /// Whether the track is still a threat.
    pub fn is_live(self) -> bool {
        matches!(self, Self::Active | Self::Quiet)
    }
}

/// How a track moves, as far as the reports say.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Motion {
    /// A report named where it is heading.
    Heading,
    /// No report named a target.
    Unknown,
}

/// A place a track was reported at, and when.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PathPoint {
    /// The place.
    pub place: PlaceRef,
    /// When it was reported there.
    pub at: Timestamp,
}

/// One reported object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// Stable: policy, class, the first evidence, its statement, and the object's index.
    pub track_id: String,
    /// Policy that produced it.
    pub policy_version: String,
    /// Class of hazard.
    pub class: HazardClass,
    /// The most specific kind reported for it.
    pub kind: HazardKind,
    /// No report stated how many: this track stands for "at least one" and a neutralization
    /// never ends it.
    pub count_unknown: bool,
    /// Where it was last reported.
    pub position: PlaceRef,
    /// Where the last report that named one said it is heading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<PlaceRef>,
    /// How it moves.
    pub motion: Motion,
    /// Every place it was reported at, oldest first.
    pub path: Vec<PathPoint>,
    /// Cells a person must be standing in to be concerned: its position circle, grown with the
    /// time since the last report, and its corridor to the target.
    pub cells: CellSet,
    /// Where it stands.
    pub status: TrackStatus,
    /// The first report.
    pub first_at: Timestamp,
    /// The newest report that concerned it.
    pub last_seen_at: Timestamp,
    /// Every report that concerned it, oldest first.
    pub evidence: Vec<EvidenceRef>,
}

/// What happened to a track.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackEventKind {
    /// The first report of it.
    Started,
    /// A later report continued it.
    Moved,
    /// No news for the kind's quiet time. Still live.
    Quiet,
    /// Reported shot down.
    Neutralized,
    /// Called off by the source that reported it.
    Cleared,
    /// No news for the stale time. Closed, and never an all-clear.
    Stale,
}

/// A change to a track. `(track_id, seq)` is unique and never reused.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TrackEvent {
    /// The track.
    pub track_id: String,
    /// Position among that track's events, from 1.
    pub seq: u32,
    /// What happened.
    pub kind: TrackEventKind,
    /// When it took effect, from the evidence or the policy's times, never from a clock.
    pub effective_at: Timestamp,
    /// The report that caused it; absent for `quiet` and `stale`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<EvidenceRef>,
}

/// A place with where it is and how far it reaches.
#[derive(Clone, Debug)]
struct Spot {
    place: PlaceRef,
    center: Position,
    radius_km: f64,
}

impl Spot {
    fn of(observation: &SignalObservation, point_radius_km: f64) -> Option<Self> {
        let place = PlaceRef {
            id: observation.provenance.place_id.clone()?,
            name: observation.provenance.place_name.clone(),
        };
        let (center, radius_km) = match &observation.geometry {
            Geometry::Point { coordinates } => (*coordinates, point_radius_km),
            Geometry::Polygon { rings } => {
                let corners = rings.first()?.positions();
                let corners = &corners[..corners.len().saturating_sub(1)];
                if corners.is_empty() {
                    return None;
                }
                let n = corners.len() as f64;
                let lon = corners.iter().map(Position::lon).sum::<f64>() / n;
                let lat = corners.iter().map(Position::lat).sum::<f64>() / n;
                let center = Position::new(lon, lat).ok()?;
                let radius = corners.iter().map(|c| km(center, *c)).sum::<f64>() / n;
                (center, radius.max(1.0))
            }
            Geometry::Cells { .. } => return None,
        };
        Some(Self {
            place,
            center,
            radius_km,
        })
    }

    fn same(&self, other: &Spot) -> bool {
        self.place.id == other.place.id
    }
}

/// Great-circle distance in km.
fn km(a: Position, b: Position) -> f64 {
    let (p, q) = (a.lat().to_radians(), b.lat().to_radians());
    let d = ((q - p) / 2.0).sin().powi(2)
        + p.cos() * q.cos() * ((b.lon() - a.lon()).to_radians() / 2.0).sin().powi(2);
    2.0 * 6371.0088 * d.sqrt().asin()
}

/// The observations of one statement of one post about one class, with one stance.
struct Report {
    at: OffsetDateTime,
    source: SourceId,
    evidence_id: ExternalId,
    statement: Option<u16>,
    class: HazardClass,
    stance: Stance,
    kind: HazardKind,
    count: Option<u32>,
    evidence: EvidenceRef,
    /// Located places in the order the text names them, each with whether it is aimed at.
    places: Vec<(Spot, bool)>,
}

impl Report {
    fn position(&self) -> &Spot {
        &self.places[0].0
    }

    /// The last place the text aims the hazard at.
    fn target(&self) -> Option<&Spot> {
        self.places
            .iter()
            .rev()
            .find(|(_, aimed)| *aimed)
            .map(|(spot, _)| spot)
    }
}

fn stance_rank(stance: Stance) -> u8 {
    match stance {
        Stance::Threat => 0,
        Stance::Neutralized => 1,
        Stance::Clear => 2,
    }
}

/// The more specific of two kinds of one class.
fn specific(a: HazardKind, b: HazardKind) -> HazardKind {
    let rank = |kind: HazardKind| match kind {
        HazardKind::JetDrone | HazardKind::BallisticMissile => 0,
        HazardKind::AttackDrone | HazardKind::GuidedBomb | HazardKind::Missile => 1,
    };
    if rank(b) < rank(a) { b } else { a }
}

fn reports(observations: &[&SignalObservation], policy: &FusionPolicy) -> Vec<Report> {
    type Key = (
        OffsetDateTime,
        SourceId,
        ExternalId,
        Option<u16>,
        String,
        HazardClass,
        u8,
    );
    let mut grouped: BTreeMap<Key, Report> = BTreeMap::new();
    let mut places: BTreeMap<Key, Vec<(u16, String, Spot, bool)>> = BTreeMap::new();
    for observation in observations {
        let Some(spot) = Spot::of(observation, policy.point_radius_km) else {
            continue;
        };
        let class = HazardClass::of(observation.kind);
        // Without a statement each place is its own report, as an assessment is.
        let alone = match observation.provenance.statement {
            Some(_) => String::new(),
            None => spot.place.id.clone(),
        };
        let key = (
            observation.observed_at.as_datetime(),
            observation.source_id.clone(),
            observation.provenance.evidence_id.clone(),
            observation.provenance.statement,
            alone,
            class,
            stance_rank(observation.stance),
        );
        let report = grouped.entry(key.clone()).or_insert_with(|| Report {
            at: observation.observed_at.as_datetime(),
            source: observation.source_id.clone(),
            evidence_id: observation.provenance.evidence_id.clone(),
            statement: observation.provenance.statement,
            class,
            stance: observation.stance,
            kind: observation.kind,
            count: None,
            evidence: evidence_ref(observation),
            places: Vec::new(),
        });
        report.kind = specific(report.kind, observation.kind);
        if let Some(count) = observation.count {
            report.count = Some(report.count.map_or(count, |c| c.max(count)));
        }
        let aimed = !matches!(observation.provenance.place_role, Some(PlaceRole::Via));
        let order = observation.provenance.place_order.unwrap_or(u16::MAX);
        let entry = places.entry(key).or_default();
        if !entry.iter().any(|(_, id, _, _)| *id == spot.place.id) {
            entry.push((order, spot.place.id.clone(), spot, aimed));
        }
    }
    let mut out = Vec::with_capacity(grouped.len());
    for (key, mut report) in grouped {
        let mut list = places.remove(&key).unwrap_or_default();
        list.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        report.places = list
            .into_iter()
            .map(|(_, _, spot, aimed)| (spot, aimed))
            .collect();
        if !report.places.is_empty() {
            out.push(report);
        }
    }
    out
}

struct Live {
    id: String,
    class: HazardClass,
    kind: HazardKind,
    count_unknown: bool,
    position: Spot,
    target: Option<Spot>,
    path: Vec<PathPoint>,
    status: TrackStatus,
    quiet_noted: bool,
    first_at: OffsetDateTime,
    last_seen: OffsetDateTime,
    last_evidence: (SourceId, ExternalId),
    evidence: Vec<EvidenceRef>,
    sources: Vec<SourceId>,
    seq: u32,
}

impl Live {
    fn event(
        &mut self,
        kind: TrackEventKind,
        at: OffsetDateTime,
        evidence: Option<EvidenceRef>,
    ) -> TrackEvent {
        self.seq += 1;
        TrackEvent {
            track_id: self.id.clone(),
            seq: self.seq,
            kind,
            effective_at: Timestamp::from_datetime(at),
            evidence,
        }
    }
}

/// Follows every reported object in the window, as of `now`.
pub(crate) fn follow(
    observations: &[&SignalObservation],
    policy: &FusionPolicy,
    now: OffsetDateTime,
) -> (Vec<Track>, Vec<TrackEvent>) {
    let rules = &policy.tracks;
    let mut tracks: Vec<Live> = Vec::new();
    let mut events: Vec<TrackEvent> = Vec::new();
    let all = reports(observations, policy);

    for report in &all {
        age(&mut tracks, &mut events, rules, report.at);
        match report.stance {
            Stance::Threat => threat(&mut tracks, &mut events, report, rules, policy),
            Stance::Neutralized => neutralize(&mut tracks, &mut events, report, rules),
            Stance::Clear => {
                let contradicted = all.iter().any(|other| {
                    other.stance == Stance::Threat
                        && other.class == report.class
                        && other.source == report.source
                        && other.evidence_id == report.evidence_id
                        && other
                            .places
                            .iter()
                            .any(|(spot, _)| report.places.iter().any(|(mine, _)| mine.same(spot)))
                });
                if !contradicted {
                    clear(&mut tracks, &mut events, report);
                }
            }
        }
    }
    age(&mut tracks, &mut events, rules, now);

    let mut out: Vec<Track> = tracks
        .into_iter()
        .map(|live| finish(live, policy, now))
        .collect();
    out.sort_by(|a, b| (a.first_at, &a.track_id).cmp(&(b.first_at, &b.track_id)));
    events.sort_by(|a, b| {
        (a.effective_at, &a.track_id, a.seq).cmp(&(b.effective_at, &b.track_id, b.seq))
    });
    (out, events)
}

/// Marks tracks quiet or stale as of `at`.
fn age(tracks: &mut [Live], events: &mut Vec<TrackEvent>, rules: &TrackPolicy, at: OffsetDateTime) {
    for track in tracks.iter_mut().filter(|t| t.status.is_live()) {
        let quiet_at = track.last_seen + rules.quiet_after(track.class);
        let stale_at = track.last_seen + Duration::seconds(rules.stale_secs);
        if !track.quiet_noted && quiet_at <= at {
            track.quiet_noted = true;
            track.status = TrackStatus::Quiet;
            let event = track.event(TrackEventKind::Quiet, quiet_at, None);
            events.push(event);
        }
        if stale_at <= at {
            track.status = TrackStatus::Stale;
            let event = track.event(TrackEventKind::Stale, stale_at, None);
            events.push(event);
        }
    }
}

/// How far a track can be from `spot` and still be the same object, or `None` when only the
/// same place will do.
fn reach_km(track: &Live, spot: &Spot, at: OffsetDateTime, rules: &TrackPolicy) -> Option<f64> {
    let hours = (at - track.last_seen).as_seconds_f64().max(0.0) / 3600.0;
    let radii = track.position.radius_km + spot.radius_km + rules.margin_km;
    match track.kind {
        HazardKind::GuidedBomb => Some(rules.bomb_reach_km + radii),
        kind => rules.speed_kmh(kind).map(|speed| speed * hours + radii),
    }
}

/// Whether a track is the object `position` and `target` describe, and how well: lower is
/// better, `None` is no match.
fn fit(
    track: &Live,
    position: &Spot,
    target: Option<&Spot>,
    at: OffsetDateTime,
    rules: &TrackPolicy,
) -> Option<(u8, u64)> {
    let distance = km(track.position.center, position.center);
    let meters = (distance * 1000.0) as u64;
    let same_target = match (&track.target, target) {
        (Some(mine), Some(theirs)) => mine.same(theirs),
        _ => false,
    };
    let at_place =
        track.position.same(position) || track.target.as_ref().is_some_and(|t| t.same(position));
    if same_target || at_place {
        return Some((0, meters));
    }
    let reach = reach_km(track, position, at, rules)?;
    (distance <= reach).then_some((1, meters))
}

fn threat(
    tracks: &mut Vec<Live>,
    events: &mut Vec<TrackEvent>,
    report: &Report,
    rules: &TrackPolicy,
    policy: &FusionPolicy,
) {
    let position = report.position();
    let target = report.target();
    let this = (report.source.clone(), report.evidence_id.clone());
    let mut candidates: Vec<((u8, u64), usize)> = tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.status.is_live() && t.class == report.class && t.last_evidence != this)
        .filter_map(|(i, t)| fit(t, position, target, report.at, rules).map(|score| (score, i)))
        .collect();
    candidates.sort_by(|a, b| (a.0, &tracks[a.1].id).cmp(&(b.0, &tracks[b.1].id)));
    let chosen: Vec<usize> = match report.count {
        Some(n) => candidates
            .iter()
            .take(n as usize)
            .map(|(_, i)| *i)
            .collect(),
        // "At least one": every track at the same place or target is renewed, or else the
        // one that fits best.
        None => {
            let exact: Vec<usize> = candidates
                .iter()
                .filter(|(score, _)| score.0 == 0)
                .map(|(_, i)| *i)
                .collect();
            if exact.is_empty() {
                candidates.first().map(|(_, i)| *i).into_iter().collect()
            } else {
                exact
            }
        }
    };
    for &i in &chosen {
        let track = &mut tracks[i];
        track.kind = specific(track.kind, report.kind);
        track.position = position.clone();
        if let Some(target) = target {
            track.target = Some(target.clone());
        }
        track.path.push(PathPoint {
            place: position.place.clone(),
            at: Timestamp::from_datetime(report.at),
        });
        track.status = TrackStatus::Active;
        track.quiet_noted = false;
        track.last_seen = report.at;
        track.last_evidence = this.clone();
        if !track.sources.contains(&report.source) {
            track.sources.push(report.source.clone());
        }
        track.evidence.push(report.evidence.clone());
        let event = track.event(
            TrackEventKind::Moved,
            report.at,
            Some(report.evidence.clone()),
        );
        events.push(event);
    }
    let new = match report.count {
        Some(n) => (n as usize).saturating_sub(chosen.len()),
        None => usize::from(chosen.is_empty()),
    };
    for index in 1..=new {
        let mut track = Live {
            id: format!(
                "{}/track/{}/{}#{}#{}",
                policy.version,
                report.class.as_str(),
                report.evidence_id.as_str(),
                report
                    .statement
                    .map_or_else(|| position.place.id.clone(), |s| s.to_string()),
                index
            ),
            class: report.class,
            kind: report.kind,
            count_unknown: report.count.is_none(),
            position: position.clone(),
            target: target.cloned(),
            path: vec![PathPoint {
                place: position.place.clone(),
                at: Timestamp::from_datetime(report.at),
            }],
            status: TrackStatus::Active,
            quiet_noted: false,
            first_at: report.at,
            last_seen: report.at,
            last_evidence: this.clone(),
            evidence: vec![report.evidence.clone()],
            sources: vec![report.source.clone()],
            seq: 0,
        };
        let event = track.event(
            TrackEventKind::Started,
            report.at,
            Some(report.evidence.clone()),
        );
        events.push(event);
        tracks.push(track);
    }
}

fn neutralize(
    tracks: &mut [Live],
    events: &mut Vec<TrackEvent>,
    report: &Report,
    rules: &TrackPolicy,
) {
    let place = report.position();
    let mut candidates: Vec<((u8, u8, u64), usize)> = tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.status.is_live() && t.class == report.class && !t.count_unknown)
        .filter_map(|(i, t)| {
            fit(t, place, None, report.at, rules)
                .map(|(how, meters)| ((u8::from(t.kind != report.kind), how, meters), i))
        })
        .collect();
    candidates.sort_by(|a, b| (a.0, &tracks[a.1].id).cmp(&(b.0, &tracks[b.1].id)));
    let n = report.count.unwrap_or(1) as usize;
    for (_, i) in candidates.into_iter().take(n) {
        let track = &mut tracks[i];
        track.status = TrackStatus::Neutralized;
        track.evidence.push(report.evidence.clone());
        let event = track.event(
            TrackEventKind::Neutralized,
            report.at,
            Some(report.evidence.clone()),
        );
        events.push(event);
    }
}

/// A clear withdraws its source's report of every track it names; a track ends only when no
/// source still reports it, as an assessment does.
fn clear(tracks: &mut [Live], events: &mut Vec<TrackEvent>, report: &Report) {
    for track in tracks.iter_mut().filter(|t| {
        t.status.is_live() && t.class == report.class && t.sources.contains(&report.source)
    }) {
        let named = report.places.iter().any(|(spot, _)| {
            track.position.same(spot) || track.target.as_ref().is_some_and(|t| t.same(spot))
        });
        if !named {
            continue;
        }
        track.sources.retain(|source| *source != report.source);
        if track.sources.is_empty() {
            track.status = TrackStatus::Cleared;
            track.evidence.push(report.evidence.clone());
            let event = track.event(
                TrackEventKind::Cleared,
                report.at,
                Some(report.evidence.clone()),
            );
            events.push(event);
        }
    }
}

/// The cells a track concerns as of `now`: its position circle grown with the time since the
/// last report, and its corridor to the target. An ended track concerns nobody.
fn cells(live: &Live, policy: &FusionPolicy, now: OffsetDateTime) -> CellSet {
    let rules = &policy.tracks;
    if !live.status.is_live() {
        return CellSet {
            resolution: policy.resolution,
            cells: Vec::new(),
        };
    }
    let hours = (now - live.last_seen).as_seconds_f64().max(0.0) / 3600.0;
    let grown = rules
        .speed_kmh(live.kind)
        .map_or(0.0, |speed| (speed * hours).min(rules.circle_cap_km));
    let band = rules.band_km(live.class);
    let here = (live.position.radius_km + grown).max(band);
    let geometry = match &live.target {
        Some(target) if !target.same(&live.position) => corridor(
            live.position.center,
            here,
            target.center,
            target.radius_km.max(band),
            policy.disc_vertices,
        ),
        _ => disc(live.position.center, here, policy.disc_vertices),
    };
    let covered = geometry
        .and_then(|g| cover(&g, policy.resolution, rules.max_cells))
        .and_then(|set| expand(&set, policy.edge_rings, rules.max_cells));
    match covered {
        Ok(set) => set,
        // Too large for the bound: the place's own disc, so the track still concerns someone.
        Err(_) => disc(
            live.position.center,
            live.position.radius_km,
            policy.disc_vertices,
        )
        .and_then(|g| cover(&g, policy.resolution, rules.max_cells))
        .and_then(|set| expand(&set, policy.edge_rings, rules.max_cells))
        .unwrap_or(CellSet {
            resolution: policy.resolution,
            cells: Vec::new(),
        }),
    }
}

fn finish(live: Live, policy: &FusionPolicy, now: OffsetDateTime) -> Track {
    let cells = cells(&live, policy, now);
    Track {
        track_id: live.id,
        policy_version: policy.version.to_owned(),
        class: live.class,
        kind: live.kind,
        count_unknown: live.count_unknown,
        position: live.position.place,
        motion: if live.target.is_some() {
            Motion::Heading
        } else {
            Motion::Unknown
        },
        target: live.target.map(|t| t.place),
        path: live.path,
        cells,
        status: live.status,
        first_at: Timestamp::from_datetime(live.first_at),
        last_seen_at: Timestamp::from_datetime(live.last_seen),
        evidence: live.evidence,
    }
}
