// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! The live channel captures read with the bundled GeoNames gazetteer, down to H3 cells.

use prism_signal_core::{Evidence, Geometry, HazardKind, SignalObservation};
use prism_signal_geo::{GridProfile, cover};
use prism_signal_observe::{Gazetteer, NormalizeRules, Normalized, Normalizer, SkipReason};
use prism_signal_source_telegram::{ChannelName, parse_preview};

const LIVE_PAGES: [&str; 2] = [
    include_str!("../../prism-signal-source-telegram/tests/fixtures/live_latest.html"),
    include_str!("../../prism-signal-source-telegram/tests/fixtures/live_before_43055.html"),
];

fn post(id: u64) -> Evidence {
    let channel = ChannelName::parse("vanek_nikolaev").unwrap();
    let external_id = format!("vanek_nikolaev/{id}");
    LIVE_PAGES
        .iter()
        .find_map(|html| {
            parse_preview(&channel, html)
                .unwrap()
                .evidence()
                .find(|e| e.external_id.as_str() == external_id)
                .cloned()
        })
        .unwrap_or_else(|| panic!("post {id} not in fixtures"))
}

fn normalize(id: u64) -> Normalized {
    let gazetteer = Gazetteer::ukraine();
    Normalizer::new(&gazetteer, NormalizeRules::default()).normalize(&post(id))
}

fn places(observations: &[SignalObservation]) -> Vec<&str> {
    observations
        .iter()
        .map(|o| o.provenance.place_id.as_deref().unwrap())
        .collect()
}

fn lat_lon(observation: &SignalObservation) -> (f64, f64) {
    let Geometry::Point { coordinates } = observation.geometry else {
        panic!("normalizer emits points");
    };
    (coordinates.lat(), coordinates.lon())
}

#[test]
fn cities_resolve_to_geonames_positions() {
    let out = normalize(43219);
    assert_eq!(
        places(&out.observations),
        ["geonames:703448", "geonames:709930"]
    );
    let (lat, lon) = lat_lon(&out.observations[0]);
    assert!((lat - 50.45466).abs() < 1e-6 && (lon - 30.5238).abs() < 1e-6);
}

#[test]
fn oblast_village_is_not_displaced_by_a_city_district() {
    // `в сторону Виноградара Одесской области`: GeoNames also lists Vynohradar, a district of
    // Kyiv, which the gazetteer leaves out, so the Odesa-oblast village is the match.
    let out = normalize(43054);
    let drones: Vec<_> = out
        .observations
        .iter()
        .filter(|o| o.kind == HazardKind::AttackDrone)
        .collect();
    assert_eq!(drones.len(), 1);
    assert_eq!(
        drones[0].provenance.place_id.as_deref(),
        Some("geonames:689554")
    );
}

#[test]
fn district_name_is_not_its_town() {
    let out = normalize(43035);
    assert!(out.observations.is_empty());
    assert_eq!(
        out.skipped[0].reason,
        SkipReason::Unlocated {
            kinds: vec![HazardKind::AttackDrone]
        }
    );
}

#[test]
fn close_namesakes_are_reported_ambiguous() {
    let out = normalize(43053);
    assert!(out.skipped.iter().any(|s| matches!(
        &s.reason,
        SkipReason::AmbiguousPlace { name, candidates }
            if name == "Черноморского" && candidates.len() == 2
    )));
    assert!(places(&out.observations).contains(&"geonames:709114"));
}

#[test]
fn observation_covers_to_a_broadcast_cell() {
    let out = normalize(43231);
    let profile = GridProfile::zerox1_current();
    let cells = cover(&out.observations[0].geometry, profile.broadcast, 1).unwrap();
    assert_eq!(cells.resolution.get(), 7);
    assert_eq!(cells.cells.len(), 1);
    let again = cover(&out.observations[0].geometry, profile.broadcast, 1).unwrap();
    assert_eq!(cells, again);
}
