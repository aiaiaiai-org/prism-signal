// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! The normalizer on posts captured verbatim from `t.me/s/vanek_nikolaev` on 2026-09-29.
//!
//! The fixtures belong to the Telegram source adapter; this test reads them through that
//! adapter's parser, so it checks the whole path from channel markup to hazard reading.

use prism_signal_normalize::{HazardKind, Normalizer, Phase, PlaceRole, Reading};
use prism_signal_source_telegram::{ChannelName, parse_preview};

const NEWEST: &str =
    include_str!("../../prism-signal-source-telegram/tests/fixtures/live_latest.html");
const OLDER: &str =
    include_str!("../../prism-signal-source-telegram/tests/fixtures/live_before_43055.html");

fn channel() -> Vec<(u64, Vec<Reading>)> {
    let normalizer = Normalizer::embedded().unwrap();
    let name = ChannelName::parse("vanek_nikolaev").unwrap();
    [OLDER, NEWEST]
        .into_iter()
        .flat_map(|html| {
            parse_preview(&name, html)
                .unwrap()
                .evidence()
                .map(|evidence| {
                    let id = evidence
                        .external_id
                        .as_str()
                        .rsplit('/')
                        .next()
                        .unwrap()
                        .parse()
                        .unwrap();
                    (id, normalizer.read(evidence))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn post(id: u64) -> Vec<Reading> {
    channel()
        .into_iter()
        .find(|(post, _)| *post == id)
        .unwrap_or_else(|| panic!("post {id} is in the fixtures"))
        .1
}

fn places(reading: &Reading) -> Vec<(&str, PlaceRole)> {
    reading
        .places
        .iter()
        .map(|p| (p.name.as_str(), p.role))
        .collect()
}

#[test]
fn news_reposts_and_chatter_give_no_readings() {
    for id in [
        43036, 43041, 43042, 43049, 43052, 43222, 43223, 43229, 43230,
    ] {
        assert!(post(id).is_empty(), "post {id}");
    }
}

#[test]
fn ballistic_missiles_on_kyiv() {
    let readings = post(43231);
    assert_eq!(readings.len(), 1);
    assert_eq!(readings[0].kind, Some(HazardKind::BallisticMissile));
    assert_eq!(places(&readings[0]), [("Київ", PlaceRole::Target)]);
    assert_eq!(readings[0].url, "https://t.me/vanek_nikolaev/43231");
}

#[test]
fn a_two_line_post_gives_a_reading_per_line() {
    let readings = post(43221);
    assert_eq!(readings.len(), 2);
    assert_eq!(
        places(&readings[0]),
        [
            ("Київ", PlaceRole::Target),
            ("Гостомель", PlaceRole::Origin)
        ]
    );
    assert_eq!(
        places(&readings[1]),
        [
            ("Авангард", PlaceRole::Target),
            ("Одеса", PlaceRole::Target)
        ]
    );
}

#[test]
fn a_vector_lists_every_town_on_it() {
    let readings = post(43043);
    assert_eq!(
        places(&readings[0]),
        [
            ("Конотоп", PlaceRole::Target),
            ("Ніжин", PlaceRole::Target),
            ("Київ", PlaceRole::Target)
        ]
    );
}

#[test]
fn a_place_already_passed_is_not_the_target() {
    let readings = post(43225);
    assert_eq!(
        places(&readings[0]),
        [("Київ", PlaceRole::Mention), ("Фастів", PlaceRole::Target)]
    );
}

#[test]
fn the_channel_all_clear_calls_off_a_kind() {
    let cleared = post(43227);
    assert_eq!(cleared.len(), 1);
    assert_eq!(cleared[0].phase, Phase::Cleared);
    assert_eq!(cleared[0].kind, Some(HazardKind::Drone));

    // "на сейчас минус по ракетам ... угроза баллистики ... пока актуальна"
    let mixed = post(43235);
    let summary: Vec<_> = mixed.iter().map(|r| (r.kind, r.phase)).collect();
    assert_eq!(
        summary,
        [
            (Some(HazardKind::Missile), Phase::Cleared),
            (Some(HazardKind::BallisticMissile), Phase::Threat)
        ]
    );
}

#[test]
fn an_unknown_town_is_reported_not_guessed() {
    let readings = post(43054);
    let missile = readings
        .iter()
        .find(|r| r.kind == Some(HazardKind::Missile))
        .unwrap();
    assert!(missile.unresolved.contains(&"Виноградара".to_owned()));
    assert!(!missile.actionable());
}

#[test]
fn no_news_post_yields_an_actionable_reading() {
    let actionable: Vec<u64> = channel()
        .into_iter()
        .filter(|(_, readings)| readings.iter().any(Reading::actionable))
        .map(|(id, _)| id)
        .collect();
    for news in [43222, 43223, 43041, 43042, 43052] {
        assert!(!actionable.contains(&news), "post {news}");
    }
    assert!(
        actionable.len() >= 20,
        "{} actionable posts",
        actionable.len()
    );
}
