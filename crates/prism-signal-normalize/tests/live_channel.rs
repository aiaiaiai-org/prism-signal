// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Normalization of real channel text: the live `t.me/s/vanek_nikolaev` captures kept by
//! `prism-signal-source-telegram`, read with a synthetic gazetteer whose coordinates are
//! placeholders. The assertions are about which places and kinds are read, not positions.

use prism_signal_core::{
    Evidence, ExternalId, HazardKind, Provenance, SignalObservation, SourceId, Stance, Timestamp,
};
use prism_signal_normalize::{
    Gazetteer, NORMALIZER, NormalizeRules, Normalized, Normalizer, SkipReason,
};
use prism_signal_source_telegram::{ChannelName, parse_preview};

const GAZETTEER: &str = include_str!("fixtures/gazetteer-synthetic.tsv");
const LIVE_PAGES: [&str; 2] = [
    include_str!("../../prism-signal-source-telegram/tests/fixtures/live_latest.html"),
    include_str!("../../prism-signal-source-telegram/tests/fixtures/live_before_43055.html"),
];

fn gazetteer() -> Gazetteer {
    Gazetteer::parse(GAZETTEER).unwrap()
}

fn live_evidence() -> Vec<Evidence> {
    let channel = ChannelName::parse("vanek_nikolaev").unwrap();
    LIVE_PAGES
        .iter()
        .flat_map(|html| {
            parse_preview(&channel, html)
                .unwrap()
                .evidence()
                .cloned()
                .collect::<Vec<_>>()
        })
        .collect()
}

fn normalize_post(id: u64) -> Normalized {
    let g = gazetteer();
    let evidence = live_evidence()
        .into_iter()
        .find(|e| e.external_id.as_str() == format!("vanek_nikolaev/{id}"))
        .unwrap_or_else(|| panic!("post {id} not in fixtures"));
    Normalizer::new(&g, NormalizeRules::default()).normalize(&evidence)
}

fn summary(observations: &[SignalObservation]) -> Vec<(HazardKind, Option<u32>, &str)> {
    observations
        .iter()
        .map(|o| (o.kind, o.count, o.provenance.place_id.as_deref().unwrap()))
        .collect()
}

#[test]
fn one_observation_per_line_kind_and_place() {
    let out = normalize_post(43219);
    assert_eq!(
        summary(&out.observations),
        [
            (HazardKind::JetDrone, Some(1), "test:kyiv"),
            (HazardKind::JetDrone, Some(1), "test:dnipro"),
        ]
    );
    assert!(out.observations.iter().all(|o| o.stance == Stance::Threat));
    assert!(out.skipped.is_empty());
}

#[test]
fn slash_separated_places_and_inflections() {
    let out = normalize_post(43226);
    assert_eq!(
        summary(&out.observations),
        [
            (HazardKind::JetDrone, Some(2), "test:kyiv"),
            (HazardKind::JetDrone, Some(2), "test:irpin"),
            (HazardKind::JetDrone, Some(2), "test:bucha"),
        ]
    );

    let out = normalize_post(43043);
    assert_eq!(
        summary(&out.observations),
        [
            (HazardKind::BallisticMissile, Some(2), "test:konotop"),
            (HazardKind::BallisticMissile, Some(2), "test:nizhyn"),
            (HazardKind::BallisticMissile, Some(2), "test:kyiv"),
        ]
    );
}

#[test]
fn guided_bombs_and_hyphenated_places() {
    let out = normalize_post(43054);
    let bombs: Vec<_> = summary(&out.observations)
        .into_iter()
        .filter(|(kind, ..)| *kind == HazardKind::GuidedBomb)
        .collect();
    assert_eq!(
        bombs,
        [
            (HazardKind::GuidedBomb, Some(3), "test:karolino"),
            (HazardKind::GuidedBomb, Some(3), "test:ovidiopol"),
            (HazardKind::GuidedBomb, Some(3), "test:zatoka"),
        ]
    );
}

#[test]
fn specific_missile_wins_over_generic_word() {
    let out = normalize_post(43045);
    assert_eq!(
        summary(&out.observations),
        [(HazardKind::BallisticMissile, None, "test:zaporizhzhia")]
    );
}

#[test]
fn slang_names_attack_drones() {
    let out = normalize_post(43053);
    assert!(summary(&out.observations).contains(&(HazardKind::AttackDrone, Some(8), "test:odesa")));
}

#[test]
fn forwarded_posts_are_not_the_source_own_reports() {
    for id in [43222, 43049] {
        let out = normalize_post(id);
        assert!(out.observations.is_empty());
        assert_eq!(out.skipped[0].reason, SkipReason::Forwarded);
    }
}

#[test]
fn hazard_without_place_is_reported_not_guessed() {
    let out = normalize_post(43227);
    assert!(out.observations.is_empty());
    assert_eq!(
        out.skipped[0].reason,
        SkipReason::Unlocated {
            kinds: vec![HazardKind::JetDrone]
        }
    );
}

#[test]
fn provenance_ttl_and_wire_form() {
    let out = normalize_post(43231);
    let [observation] = &out.observations[..] else {
        panic!("expected one observation");
    };
    assert_eq!(
        observation.source_id.as_str(),
        "telegram.channel:vanek_nikolaev"
    );
    assert_eq!(observation.observed_at.to_string(), "2026-09-28T22:19:42Z");
    assert_eq!(observation.ttl.get(), 15 * 60);
    assert_eq!(observation.confidence, None);
    assert_eq!(
        observation.provenance.evidence_url,
        "https://t.me/vanek_nikolaev/43231"
    );
    assert_eq!(observation.provenance.normalizer, NORMALIZER);
    assert_eq!(observation.provenance.matched, ["баллистики", "Киев"]);

    let json = serde_json::to_value(observation).unwrap();
    assert_eq!(json["kind"], "air.ballistic_missile");
    assert_eq!(json["stance"], "threat");
    assert_eq!(json["geometry"]["type"], "point");
    assert!(json.get("confidence").is_none());
    let back: SignalObservation = serde_json::from_value(json).unwrap();
    assert_eq!(&back, observation);
}

#[test]
fn whole_capture_is_deterministic() {
    let g = gazetteer();
    let normalizer = Normalizer::new(&g, NormalizeRules::default());
    let evidence = live_evidence();
    let first: Vec<_> = evidence.iter().map(|e| normalizer.normalize(e)).collect();
    let second: Vec<_> = evidence.iter().map(|e| normalizer.normalize(e)).collect();
    assert_eq!(first, second);
    let located: usize = first.iter().map(|n| n.observations.len()).sum();
    assert!(located >= 30, "{located}");
}

fn evidence(text: &str) -> Evidence {
    Evidence {
        source_id: SourceId::new("telegram.channel", "example").unwrap(),
        external_id: ExternalId::try_from("example/1".to_owned()).unwrap(),
        published_at: Timestamp::parse("2026-09-29T10:00:00Z").unwrap(),
        edited: false,
        text: Some(text.to_owned()),
        media: Vec::new(),
        forwarded_from: None,
        provenance: Provenance {
            url: "https://t.me/example/1".to_owned(),
            collector: "test/0".to_owned(),
        },
    }
}

fn stances(text: &str) -> Vec<(HazardKind, Stance, String)> {
    let g = gazetteer();
    Normalizer::new(&g, NormalizeRules::default())
        .normalize(&evidence(text))
        .observations
        .into_iter()
        .map(|o| (o.kind, o.stance, o.provenance.place_id.unwrap()))
        .collect()
}

#[test]
fn clear_words_turn_a_located_line_into_clear() {
    let g = gazetteer();
    let out = Normalizer::new(&g, NormalizeRules::default())
        .normalize(&evidence("минус по мопеду над Киевом"));
    let [observation] = &out.observations[..] else {
        panic!("expected one observation");
    };
    assert_eq!(observation.stance, Stance::Clear);
    assert_eq!(observation.kind, HazardKind::AttackDrone);
    assert_eq!(observation.ttl.get(), 10 * 60);
}

#[test]
fn mixed_line_keeps_clear_and_threat_with_their_own_places() {
    assert_eq!(
        stances("минус по мопеду над Одессой, 2 баллистики на Киев"),
        [
            (
                HazardKind::AttackDrone,
                Stance::Clear,
                "test:odesa".to_owned()
            ),
            (
                HazardKind::BallisticMissile,
                Stance::Threat,
                "test:kyiv".to_owned()
            ),
        ]
    );
}

#[test]
fn trailing_clear_applies_to_the_hazard_before_it() {
    assert_eq!(
        stances("мопеды над Бучей - минус"),
        [(
            HazardKind::AttackDrone,
            Stance::Clear,
            "test:bucha".to_owned()
        )]
    );
    assert_eq!(
        stances("минус, 2 шахеда на Одессу"),
        [(
            HazardKind::AttackDrone,
            Stance::Threat,
            "test:odesa".to_owned()
        )]
    );
}

#[test]
fn place_before_the_first_hazard_belongs_to_it() {
    assert_eq!(
        stances("Киев: 2 баллистики"),
        [(
            HazardKind::BallisticMissile,
            Stance::Threat,
            "test:kyiv".to_owned()
        )]
    );
}

#[test]
fn hazard_without_a_place_in_its_span_is_unlocated() {
    let g = gazetteer();
    let out = Normalizer::new(&g, NormalizeRules::default())
        .normalize(&evidence("2 баллистики на Киев. мопеды пока над морем"));
    assert_eq!(out.observations.len(), 1);
    assert_eq!(
        out.skipped[0].reason,
        SkipReason::Unlocated {
            kinds: vec![HazardKind::AttackDrone]
        }
    );
}
