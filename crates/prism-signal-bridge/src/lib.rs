// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Turns `prism-signal-normalize` readings into [`SignalObservation`]s.
//!
//! Fusion consumes observations, whichever reader produced them. This bridge is what lets the
//! role-aware reader feed it, and it keeps the roles' meaning inside the contract:
//!
//! - a threat becomes an observation only for a place that is a `target` or `via`. A place the
//!   text names as where something came from, or in passing, becomes nothing, so a launch site is
//!   never an alert;
//! - the observation's geometry is a disc around the place with the place's own reach, so fusion
//!   needs no knowledge of how big a town is. A region (`на Київщину`) is its own cells instead,
//!   from official boundaries, so an exclave is in and a city it surrounds is out;
//! - an all-clear becomes an observation only when it names both a kind and a place. One that
//!   names a kind alone, or a place alone, cannot be expressed as located evidence, so it is not
//!   emitted: the threat it meant to end expires on its validity window instead. A missed
//!   all-clear leaves a threat to lapse; a misapplied one would hide a live threat;
//! - a forwarded post is not the source's own report, so it yields nothing.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use prism_signal_core::{
    CellId, CellResolution, Geometry, HazardKind as Kind, ObservationProvenance, PlaceRole as Role,
    Position, SignalObservation, Stance, TtlSeconds, is_region_place,
};
use prism_signal_geo::{CellSet, CoverError, disc, uncompact};
use prism_signal_normalize::{HazardKind, Phase, PlaceMention, PlaceRole, Reading};
use serde::Deserialize;

/// Normalizer name recorded in observation provenance.
pub const NORMALIZER: &str = concat!("prism-signal-normalize/", env!("CARGO_PKG_VERSION"));

/// How long observations stay valid, and how finely a place's disc is drawn.
///
/// The validity windows are rounded up from measured data: on 495 posts from one channel, the
/// 90th percentile of the time from a kind's last report to the channel's own all-clear was 22
/// minutes for drones, 9 for guided bombs, and 18 for missiles. One channel and a small sample:
/// a deployment should set its own.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BridgeRules {
    /// Validity of a drone threat, in minutes.
    pub drone_minutes: u32,
    /// Validity of a guided bomb threat, in minutes.
    pub bomb_minutes: u32,
    /// Validity of a missile threat of any kind, in minutes.
    pub missile_minutes: u32,
    /// Validity of an all-clear, in minutes.
    pub clear_minutes: u32,
    /// Corners of the polygon drawn around a place.
    pub disc_vertices: usize,
}

impl Default for BridgeRules {
    fn default() -> Self {
        Self {
            drone_minutes: 30,
            bomb_minutes: 15,
            missile_minutes: 20,
            clear_minutes: 10,
            disc_vertices: 48,
        }
    }
}

fn kind_of(kind: HazardKind) -> Kind {
    match kind {
        HazardKind::Drone => Kind::AttackDrone,
        HazardKind::GuidedBomb => Kind::GuidedBomb,
        HazardKind::CruiseMissile | HazardKind::Missile => Kind::Missile,
        HazardKind::BallisticMissile => Kind::BallisticMissile,
    }
}

fn ttl(minutes: u32) -> TtlSeconds {
    TtlSeconds::new(minutes.max(1).saturating_mul(60)).expect("at least one minute is positive")
}

impl BridgeRules {
    fn threat_ttl(&self, kind: HazardKind) -> TtlSeconds {
        ttl(match kind {
            HazardKind::Drone => self.drone_minutes,
            HazardKind::GuidedBomb => self.bomb_minutes,
            HazardKind::CruiseMissile | HazardKind::BallisticMissile | HazardKind::Missile => {
                self.missile_minutes
            }
        })
    }
}

/// Why a reading, or a place in it, produced no observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Skip {
    /// The post was forwarded from elsewhere.
    Forwarded,
    /// An all-clear names a kind but no place, or a place but no kind.
    UnlocatedClear,
    /// A place could not be drawn as a disc.
    Geometry(&'static str),
}

/// Observations from the readings of one post, and what was left out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bridged {
    /// Observations, in reading order then place order.
    pub observations: Vec<SignalObservation>,
    /// What was left out and why.
    pub skipped: Vec<Skip>,
}

fn located(place: &PlaceMention) -> bool {
    matches!(place.role, PlaceRole::Target | PlaceRole::Via)
}

/// Converts the readings of one post.
pub fn bridge(readings: &[Reading], rules: &BridgeRules) -> Bridged {
    let mut out = Bridged::default();
    for (statement, reading) in readings.iter().enumerate() {
        if reading.forwarded {
            if !out.skipped.contains(&Skip::Forwarded) {
                out.skipped.push(Skip::Forwarded);
            }
            continue;
        }
        let Some(kind) = reading.kind else {
            // A kindless all-clear cannot be located evidence.
            if reading.phase == Phase::Cleared {
                out.skipped.push(Skip::UnlocatedClear);
            }
            continue;
        };
        let places: Vec<&PlaceMention> = reading.places.iter().filter(|p| located(p)).collect();
        if reading.phase == Phase::Cleared && places.is_empty() {
            out.skipped.push(Skip::UnlocatedClear);
            continue;
        }
        for (order, place) in places.into_iter().enumerate() {
            let position = InPost {
                statement: u16::try_from(statement).ok(),
                place_order: u16::try_from(order).ok(),
            };
            match observation(reading, kind, place, position, rules) {
                Ok(observation) => {
                    if observation.stance == Stance::Neutralized {
                        // Until tracks can subtract objects, an interception keeps saying what
                        // it said before: something of that kind was in the air there.
                        let mut threat = observation.clone();
                        threat.stance = Stance::Threat;
                        threat.ttl = rules.threat_ttl(kind);
                        // The count is of objects ended, not of objects still flying.
                        threat.count = None;
                        out.observations.push(threat);
                    }
                    out.observations.push(observation);
                }
                Err(error) => out.skipped.push(Skip::Geometry(match error {
                    CoverError::InvalidGeometry(what) => what,
                    CoverError::TooLarge { .. } => "too large",
                })),
            }
        }
    }
    out
}

#[derive(Deserialize)]
struct RegionsDoc {
    version: String,
    regions: Vec<RegionDoc>,
}

#[derive(Deserialize)]
struct RegionDoc {
    id: String,
    resolution: u8,
    cells: Vec<String>,
}

/// The most cells one region may hold; the largest oblast has under 1,000.
const REGION_MAX_CELLS: usize = 5_000;

/// The cells of a first-level region, such as `ua-adm1:UA32`, from the shipped
/// `data/regions.v1.json` (OCHA COD-AB boundaries, CC BY-IGO 3.0). `None` for an unknown id.
pub fn region_cells(id: &str) -> Option<&'static CellSet> {
    static REGIONS: OnceLock<BTreeMap<String, CellSet>> = OnceLock::new();
    REGIONS
        .get_or_init(|| {
            let doc: RegionsDoc = serde_json::from_str(include_str!("../data/regions.v1.json"))
                .expect("the shipped regions parse");
            assert_eq!(
                doc.version, "regions.v1",
                "the shipped regions are regions.v1"
            );
            doc.regions
                .into_iter()
                .map(|region| {
                    let resolution = CellResolution::try_from(region.resolution)
                        .expect("a shipped region has a valid resolution");
                    let ids: Vec<CellId> = region
                        .cells
                        .iter()
                        .map(|cell| CellId::try_from(cell.clone()).expect("a shipped cell id"))
                        .collect();
                    let cells = uncompact(&ids, resolution, REGION_MAX_CELLS)
                        .expect("a shipped region uncompacts");
                    (region.id, cells)
                })
                .collect()
        })
        .get(id)
}

/// Where an observation stands in its post.
struct InPost {
    statement: Option<u16>,
    place_order: Option<u16>,
}

fn observation(
    reading: &Reading,
    kind: HazardKind,
    place: &PlaceMention,
    position: InPost,
    rules: &BridgeRules,
) -> Result<SignalObservation, CoverError> {
    let centre = Position::new(place.lon, place.lat)
        .map_err(|_| CoverError::InvalidGeometry("coordinate"))?;
    let geometry = if is_region_place(&place.place_id) {
        let cells =
            region_cells(&place.place_id).ok_or(CoverError::InvalidGeometry("unknown region"))?;
        Geometry::Cells {
            resolution: cells.resolution,
            cells: cells.cells.clone(),
        }
    } else {
        disc(centre, f64::from(place.reach_km), rules.disc_vertices)?
    };
    let (stance, ttl) = match reading.phase {
        Phase::Threat => (Stance::Threat, rules.threat_ttl(kind)),
        Phase::Cleared => (Stance::Clear, ttl(rules.clear_minutes)),
        Phase::Intercepted => (Stance::Neutralized, ttl(rules.clear_minutes)),
    };
    let mut matched = Vec::new();
    if reading.kind_inherited {
        matched.push("kind_inherited".to_owned());
    }
    Ok(SignalObservation {
        source_id: reading.source_id.clone(),
        kind: kind_of(kind),
        stance,
        count: reading.count,
        observed_at: reading.published_at,
        geometry,
        ttl,
        confidence: None,
        provenance: ObservationProvenance {
            evidence_id: reading.external_id.clone(),
            evidence_url: reading.url.clone(),
            normalizer: NORMALIZER.to_owned(),
            matched,
            place_id: Some(place.place_id.clone()),
            place_name: Some(place.name.clone()),
            place_role: Some(match place.role {
                PlaceRole::Via => Role::Via,
                _ => Role::Target,
            }),
            statement: position.statement,
            place_order: position.place_order,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use prism_signal_core::Evidence;
    use prism_signal_normalize::Normalizer;

    fn post(text: &str) -> Evidence {
        serde_json::from_value(serde_json::json!({
            "source_id": "telegram.channel:vanek_nikolaev",
            "external_id": "vanek_nikolaev/1",
            "published_at": "2026-09-28T22:19:42Z",
            "text": text,
            "provenance": {"url": "https://t.me/vanek_nikolaev/1", "collector": "test/0"}
        }))
        .unwrap()
    }

    fn bridged(text: &str) -> Bridged {
        let readings = Normalizer::embedded().unwrap().read(&post(text));
        bridge(&readings, &BridgeRules::default())
    }

    fn names(b: &Bridged) -> Vec<(&str, Stance)> {
        b.observations
            .iter()
            .map(|o| (o.provenance.place_name.as_deref().unwrap(), o.stance))
            .collect()
    }

    #[test]
    fn a_target_becomes_a_threat_observation_with_the_places_own_footprint() {
        let b = bridged("2 баллистики на Киев !");
        assert_eq!(names(&b), [("Київ", Stance::Threat)]);
        let o = &b.observations[0];
        assert_eq!(o.kind, Kind::BallisticMissile);
        assert_eq!(o.ttl.get(), 20 * 60);
        assert_eq!(o.provenance.place_id.as_deref(), Some("geonames:703448"));
        assert_eq!(o.provenance.place_role, Some(Role::Target));
        assert_eq!(o.provenance.evidence_id.as_str(), "vanek_nikolaev/1");
        assert!(o.confidence.is_none());
        assert!(matches!(
            o.geometry,
            prism_signal_core::Geometry::Polygon { .. }
        ));
    }

    #[test]
    fn an_interception_is_a_counted_neutralization_and_still_a_threat_until_tracks_exist() {
        let b = bridged("сбит 1 мопед над Киевом");
        assert_eq!(
            names(&b),
            [("Київ", Stance::Threat), ("Київ", Stance::Neutralized)]
        );
        assert_eq!(b.observations[0].count, None);
        assert_eq!(b.observations[0].ttl.get(), 30 * 60);
        assert_eq!(b.observations[1].count, Some(1));
        assert_eq!(bridged("3 мопеда на Киев").observations[0].count, Some(3));
    }

    #[test]
    fn each_observation_knows_its_statement_and_where_its_place_stands_in_it() {
        let b = bridged("мопед через Бровары на Киев");
        let order: Vec<_> = b
            .observations
            .iter()
            .map(|o| {
                (
                    o.provenance.place_name.as_deref().unwrap(),
                    o.provenance.statement,
                    o.provenance.place_order,
                )
            })
            .collect();
        assert_eq!(
            order,
            [("Бровари", Some(0), Some(0)), ("Київ", Some(0), Some(1))]
        );
        let two = bridged("2 шахеда на Одессу. 3 шахеда на Киев");
        let statements: Vec<_> = two
            .observations
            .iter()
            .map(|o| (o.provenance.statement, o.count))
            .collect();
        assert_eq!(statements, [(Some(0), Some(2)), (Some(1), Some(3))]);
    }

    #[test]
    fn a_region_is_its_own_cells_not_a_disc() {
        let b = bridged("шахеды на Киевщину");
        assert_eq!(names(&b), [("Київська область", Stance::Threat)]);
        let o = &b.observations[0];
        assert_eq!(o.provenance.place_id.as_deref(), Some("ua-adm1:UA32"));
        let prism_signal_core::Geometry::Cells { resolution, cells } = &o.geometry else {
            panic!("a region is cells");
        };
        assert_eq!(resolution.get(), 6);
        assert!((700..1000).contains(&cells.len()), "{}", cells.len());
        assert!(region_cells("ua-adm1:UA00").is_none());
        // Every shipped region loads.
        for code in ["UA01", "UA05", "UA32", "UA74", "UA80", "UA85"] {
            assert!(region_cells(&format!("ua-adm1:{code}")).is_some(), "{code}");
        }
    }

    #[test]
    fn where_a_launch_came_from_is_never_an_observation() {
        let b = bridged("1 реактивный мопед подлетает к Киеву со стороны Гостомеля");
        assert_eq!(names(&b), [("Київ", Stance::Threat)]);
        let passed = bridged("3 реактивных мопеда пролетели Киев дальше в сторону Фастова");
        assert_eq!(names(&passed), [("Фастів", Stance::Threat)]);
    }

    #[test]
    fn a_nearby_place_is_kept_and_the_kinds_map_to_the_contract_vocabulary() {
        let b = bridged("КАБы пролетают южнее Каменского");
        assert_eq!(names(&b), [("Кам'янське", Stance::Threat)]);
        assert_eq!(b.observations[0].provenance.place_role, Some(Role::Via));
        assert_eq!(b.observations[0].kind, Kind::GuidedBomb);
        assert_eq!(b.observations[0].ttl.get(), 15 * 60);
        assert_eq!(
            bridged("Циркон на Киев").observations[0].kind,
            Kind::Missile
        );
        assert_eq!(
            bridged("шахеды на Киев").observations[0].kind,
            Kind::AttackDrone
        );
        assert_eq!(bridged("шахеды на Киев").observations[0].ttl.get(), 30 * 60);
    }

    #[test]
    fn an_all_clear_with_a_kind_and_a_place_is_a_clear_observation() {
        let b = bridged("минус по мопеду на Ровно");
        assert_eq!(names(&b), [("Рівне", Stance::Clear)]);
        assert_eq!(b.observations[0].ttl.get(), 10 * 60);
    }

    #[test]
    fn an_all_clear_without_a_kind_or_without_a_place_is_left_out_and_said_so() {
        for text in [
            "минус по всем этим 4 реактивным мопедам", // a kind, no place
            "минус по всему на Маяки или Одессу",      // places, no kind
        ] {
            let b = bridged(text);
            assert!(b.observations.is_empty(), "{text}");
            assert_eq!(b.skipped, [Skip::UnlocatedClear], "{text}");
        }
    }

    #[test]
    fn a_forwarded_post_and_unresolved_words_give_nothing() {
        let mut evidence = post("2 баллистики на Киев !");
        evidence.forwarded_from = Some("Інший канал".to_owned());
        let readings = Normalizer::embedded().unwrap().read(&evidence);
        let b = bridge(&readings, &BridgeRules::default());
        assert!(b.observations.is_empty());
        assert_eq!(b.skipped, [Skip::Forwarded]);

        assert!(
            bridged("3 бандероли летят в сторону Виноградара")
                .observations
                .is_empty()
        );
    }

    #[test]
    fn an_inherited_kind_is_recorded_in_the_provenance() {
        let b = bridged("общая по мопедам:\n\n1 под Киевом");
        assert_eq!(b.observations.len(), 1);
        assert_eq!(b.observations[0].provenance.matched, ["kind_inherited"]);
    }
}
