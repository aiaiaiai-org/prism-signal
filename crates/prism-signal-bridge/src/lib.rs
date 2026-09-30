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
//!   needs no knowledge of how big a town is;
//! - an all-clear becomes an observation only when it names both a kind and a place. One that
//!   names a kind alone, or a place alone, cannot be expressed as located evidence, so it is not
//!   emitted: the threat it meant to end expires on its validity window instead. A missed
//!   all-clear leaves a threat to lapse; a misapplied one would hide a live threat;
//! - a forwarded post is not the source's own report, so it yields nothing.

use prism_signal_core::{
    HazardKind as Kind, ObservationProvenance, PlaceRole as Role, Position, SignalObservation,
    Stance, TtlSeconds,
};
use prism_signal_geo::{CoverError, disc};
use prism_signal_normalize::{HazardKind, Phase, PlaceMention, PlaceRole, Reading};

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
    for reading in readings {
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
        for place in places {
            match observation(reading, kind, place, rules) {
                Ok(observation) => out.observations.push(observation),
                Err(error) => out.skipped.push(Skip::Geometry(match error {
                    CoverError::InvalidGeometry(what) => what,
                    CoverError::TooLarge { .. } => "too large",
                })),
            }
        }
    }
    out
}

fn observation(
    reading: &Reading,
    kind: HazardKind,
    place: &PlaceMention,
    rules: &BridgeRules,
) -> Result<SignalObservation, CoverError> {
    let centre = Position::new(place.lon, place.lat)
        .map_err(|_| CoverError::InvalidGeometry("coordinate"))?;
    let geometry = disc(centre, f64::from(place.reach_km), rules.disc_vertices)?;
    let (stance, ttl) = match reading.phase {
        Phase::Threat => (Stance::Threat, rules.threat_ttl(kind)),
        Phase::Cleared => (Stance::Clear, ttl(rules.clear_minutes)),
    };
    let mut matched = Vec::new();
    if reading.kind_inherited {
        matched.push("kind_inherited".to_owned());
    }
    Ok(SignalObservation {
        source_id: reading.source_id.clone(),
        kind: kind_of(kind),
        stance,
        count: None,
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
