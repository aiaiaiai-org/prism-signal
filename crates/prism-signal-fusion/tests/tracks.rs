// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Tracks, read from channel text the way the hub reads it: one object each, ended only by
//! explicit words.

use prism_signal_bridge::{BridgeRules, bridge};
use prism_signal_core::{Evidence, Geometry, Position, SignalObservation, Timestamp};
use prism_signal_fusion::{
    Assessed, FusionPolicy, Motion, Track, TrackEventKind, TrackStatus, assess,
};
use prism_signal_geo::cover;
use prism_signal_normalize::Normalizer;

fn post(source: &str, id: u64, at: &str, text: &str) -> Vec<SignalObservation> {
    let evidence: Evidence = serde_json::from_value(serde_json::json!({
        "source_id": format!("telegram.channel:{source}"),
        "external_id": format!("{source}/{id}"),
        "published_at": at,
        "text": text,
        "provenance": {"url": format!("https://t.me/{source}/{id}"), "collector": "test/0"}
    }))
    .unwrap();
    bridge(
        &Normalizer::embedded().unwrap().read(&evidence),
        &BridgeRules::default(),
    )
    .observations
}

fn window(posts: &[Vec<SignalObservation>]) -> Vec<SignalObservation> {
    posts.iter().flatten().cloned().collect()
}

fn assessed(observations: &[SignalObservation], at: &str) -> Assessed {
    assess(
        observations,
        &FusionPolicy::v1(),
        Timestamp::parse(at).unwrap(),
    )
}

fn live(result: &Assessed) -> Vec<&Track> {
    result
        .tracks
        .iter()
        .filter(|t| t.status.is_live())
        .collect()
}

fn count(result: &Assessed, status: TrackStatus) -> usize {
    result.tracks.iter().filter(|t| t.status == status).count()
}

const T0: &str = "2026-10-27T01:00:00Z";
const T5: &str = "2026-10-27T01:05:00Z";
const T10: &str = "2026-10-27T01:10:00Z";
const T15: &str = "2026-10-27T01:15:00Z";

#[test]
fn four_reported_objects_are_four_tracks_with_one_target() {
    let result = assessed(&post("kpszsu", 1, T0, "4 шахеда на Киев"), T5);
    let tracks = live(&result);
    assert_eq!(tracks.len(), 4);
    for track in &tracks {
        assert!(!track.count_unknown);
        assert_eq!(track.target.as_ref().unwrap().name.as_deref(), Some("Київ"));
        assert_eq!(track.motion, Motion::Heading);
        assert!(!track.cells.cells.is_empty());
    }
    assert_eq!(
        result
            .track_events
            .iter()
            .filter(|e| e.kind == TrackEventKind::Started)
            .count(),
        4
    );
}

#[test]
fn one_shot_down_ends_one_and_the_rest_may_still_be_coming() {
    let observations = window(&[
        post("kpszsu", 1, T0, "4 шахеда на Киев"),
        post("kpszsu", 2, T10, "сбит 1 шахед над Киевом"),
    ]);
    let result = assessed(&observations, T15);
    assert_eq!(live(&result).len(), 3);
    assert_eq!(count(&result, TrackStatus::Neutralized), 1);
    // `4 знищено` ends all of them.
    let observations = window(&[
        post("kpszsu", 1, T0, "4 шахеда на Киев"),
        post("kpszsu", 2, T10, "знищено 4 шахеди над Києвом"),
    ]);
    let result = assessed(&observations, T15);
    assert!(live(&result).is_empty());
    assert_eq!(count(&result, TrackStatus::Neutralized), 4);
    assert!(
        result.tracks.iter().all(|t| t.cells.cells.is_empty()),
        "an ended track concerns nobody"
    );
}

#[test]
fn a_track_of_unknown_count_is_never_subtracted_to_zero() {
    let observations = window(&[
        post("kpszsu", 1, T0, "шахеды на Киев"),
        post("kpszsu", 2, T10, "сбит 1 шахед над Киевом"),
    ]);
    let result = assessed(&observations, T15);
    let tracks = live(&result);
    assert_eq!(tracks.len(), 1);
    assert!(tracks[0].count_unknown);
    assert_eq!(count(&result, TrackStatus::Neutralized), 0);
}

#[test]
fn a_clear_ends_only_its_own_sources_tracks_at_the_place_it_names() {
    let reported = post("kpszsu", 1, T0, "2 шахеда на Киев");
    let own = window(&[
        reported.clone(),
        post("kpszsu", 2, T10, "минус по шахедам на Киев"),
    ]);
    let result = assessed(&own, T15);
    assert!(live(&result).is_empty());
    assert_eq!(count(&result, TrackStatus::Cleared), 2);

    let other_source = window(&[
        reported.clone(),
        post("war_monitor", 2, T10, "минус по шахедам на Киев"),
    ]);
    assert_eq!(live(&assessed(&other_source, T15)).len(), 2);

    // Reported by two channels, it ends only when both have called it off.
    let both = window(&[
        reported.clone(),
        post("war_monitor", 1, T5, "2 шахеда на Киев"),
        post("kpszsu", 2, T10, "минус по шахедам на Киев"),
    ]);
    assert_eq!(live(&assessed(&both, T15)).len(), 2);

    let other_place = window(&[
        reported,
        post("kpszsu", 2, T10, "минус по шахедам на Одессу"),
    ]);
    assert_eq!(live(&assessed(&other_place, T15)).len(), 2);
}

#[test]
fn silence_makes_a_track_quiet_and_then_stale_and_never_clear() {
    let observations = post("kpszsu", 1, T0, "шахед на Киев");
    let quiet = assessed(&observations, "2026-10-27T02:01:00Z");
    assert_eq!(quiet.tracks[0].status, TrackStatus::Quiet);
    assert!(quiet.tracks[0].status.is_live());
    let stale = assessed(&observations, "2026-10-27T07:01:00Z");
    assert_eq!(stale.tracks[0].status, TrackStatus::Stale);
    let kinds: Vec<_> = stale.track_events.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        [
            TrackEventKind::Started,
            TrackEventKind::Quiet,
            TrackEventKind::Stale
        ]
    );
    // A report after the quiet time continues it.
    let resumed = window(&[
        observations,
        post("kpszsu", 2, "2026-10-27T02:10:00Z", "шахед на Киев"),
    ]);
    let result = assessed(&resumed, "2026-10-27T02:11:00Z");
    assert_eq!(result.tracks.len(), 1);
    assert_eq!(result.tracks[0].status, TrackStatus::Active);
}

#[test]
fn a_reachable_report_continues_a_track_and_a_far_one_starts_another() {
    // Chernihiv to Brovary is about 115 km: a Shahed covers it in 40 minutes.
    let observations = window(&[
        post("kpszsu", 1, T0, "шахед над Черниговом"),
        post("kpszsu", 2, "2026-10-27T01:40:00Z", "шахед на Бровары"),
        post("kpszsu", 3, T5, "шахед над Одессой"),
    ]);
    let result = assessed(&observations, "2026-10-27T01:41:00Z");
    assert_eq!(live(&result).len(), 2, "{:#?}", result.tracks);
    let north = result
        .tracks
        .iter()
        .find(|t| t.path.len() == 2)
        .expect("Chernihiv then Brovary is one track");
    assert_eq!(north.position.name.as_deref(), Some("Бровари"));
    // Five minutes is too soon for the same Shahed to have flown that far.
    let too_soon = window(&[
        post("kpszsu", 1, T0, "шахед над Черниговом"),
        post("kpszsu", 2, T5, "шахед на Бровары"),
    ]);
    assert_eq!(live(&assessed(&too_soon, T10)).len(), 2);
}

#[test]
fn a_track_concerns_everyone_on_its_way_to_the_target() {
    let result = assessed(
        &post("kpszsu", 1, T0, "шахед над Черниговом курсом на Киев"),
        T0,
    );
    let track = &live(&result)[0];
    assert_eq!(track.position.name.as_deref(), Some("Чернігів"));
    assert_eq!(track.target.as_ref().unwrap().name.as_deref(), Some("Київ"));
    let policy = FusionPolicy::v1();
    let cell = |lon: f64, lat: f64| {
        cover(
            &Geometry::point(Position::new(lon, lat).unwrap()),
            policy.resolution,
            1,
        )
        .unwrap()
        .cells
        .remove(0)
    };
    // Kozelets lies between them; Lviv does not.
    assert!(track.cells.cells.contains(&cell(31.1156, 50.9136)));
    assert!(track.cells.cells.contains(&cell(30.5238, 50.4547)));
    assert!(!track.cells.cells.contains(&cell(24.0297, 49.8397)));
}

#[test]
fn separate_statements_are_separate_objects() {
    let result = assessed(
        &post("kpszsu", 1, T0, "2 шахеда на Одессу. 3 шахеда на Киев"),
        T5,
    );
    let to = |name: &str| {
        live(&result)
            .iter()
            .filter(|t| t.target.as_ref().and_then(|p| p.name.as_deref()) == Some(name))
            .count()
    };
    assert_eq!((to("Одеса"), to("Київ")), (2, 3));
}

#[test]
fn equal_input_in_any_order_gives_equal_output() {
    let mut observations = window(&[
        post("kpszsu", 1, T0, "4 шахеда на Киев"),
        post("war_monitor", 1, T5, "2 шахеда на Киев"),
        post("kpszsu", 2, T10, "сбит 1 шахед над Киевом"),
    ]);
    let forward = assessed(&observations, T15);
    observations.reverse();
    assert_eq!(forward, assessed(&observations, T15));
    // The second channel's two continue two of the first four rather than adding two more.
    assert_eq!(forward.tracks.len(), 4);
    assert_eq!(live(&forward).len(), 3);
}

fn cell_at(lon: f64, lat: f64) -> prism_signal_core::CellId {
    cover(
        &Geometry::point(Position::new(lon, lat).unwrap()),
        FusionPolicy::v1().resolution,
        1,
    )
    .unwrap()
    .cells
    .remove(0)
}

#[test]
fn a_region_concerns_the_whole_oblast_its_exclaves_included_and_its_enclaves_left_out() {
    let result = assessed(&post("kpszsu", 1, T0, "4 шахеда на Киевщину"), T5);
    assert!(result.assessments.is_empty(), "a region is no assessment");
    assert!(result.skipped.iter().any(|s| s.code == "region"));
    let tracks = live(&result);
    assert_eq!(tracks.len(), 4);
    let track = tracks[0];
    assert_eq!(
        track.target.as_ref().unwrap().name.as_deref(),
        Some("Київська область")
    );
    assert!(track.cells.cells.is_empty(), "nothing says where in it");
    let region = &track.region_cells.cells;
    // Slavutych, an exclave inside Chernihiv oblast, and Kotsiubynske, an enclave inside Kyiv
    // city, are Kyiv oblast; the centre of Kyiv and Lviv are not.
    assert!(region.contains(&cell_at(30.7472, 51.5229)), "Slavutych");
    assert!(region.contains(&cell_at(30.3333, 50.4889)), "Kotsiubynske");
    assert!(region.contains(&cell_at(30.7911, 50.511)), "Brovary");
    assert!(!region.contains(&cell_at(30.5238, 50.4547)), "Kyiv city");
    assert!(!region.contains(&cell_at(24.0297, 49.8397)), "Lviv");
}

#[test]
fn a_track_heading_for_a_region_draws_no_corridor_to_its_middle() {
    let result = assessed(
        &post("kpszsu", 1, T0, "шахед над Черниговом курсом на Киевщину"),
        T0,
    );
    let track = &live(&result)[0];
    assert_eq!(track.position.name.as_deref(), Some("Чернігів"));
    assert!(track.cells.cells.contains(&cell_at(31.2866, 51.5054)));
    assert!(
        !track.cells.cells.contains(&cell_at(30.7911, 50.511)),
        "Brovary is not on a known way"
    );
    assert!(track.region_cells.cells.contains(&cell_at(30.7911, 50.511)));
    // A later report of the same region continues these tracks; an all-clear for it ends them.
    let observations = window(&[
        post("kpszsu", 1, T0, "2 шахеда на Киевщину"),
        post("kpszsu", 2, T5, "шахеды на Киевщину"),
        post("kpszsu", 3, T10, "минус по шахедам на Киевщину"),
    ]);
    let result = assessed(&observations, T15);
    assert_eq!(result.tracks.len(), 2);
    assert_eq!(count(&result, TrackStatus::Cleared), 2);
}
