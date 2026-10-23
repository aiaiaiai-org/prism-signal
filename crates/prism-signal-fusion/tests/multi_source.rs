// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Regressions: no single Telegram channel owns the truth about an active hazard.

use prism_signal_bridge::{BridgeRules, bridge};
use prism_signal_core::{Evidence, ExternalId, SignalObservation, SourceId, Stance, Timestamp};
use prism_signal_fusion::{EventKind, FusionPolicy, Status, assess};
use prism_signal_normalize::Normalizer;

fn time(value: &str) -> Timestamp {
    Timestamp::parse(value).unwrap()
}

fn report(source: &str, id: u64, at: &str, stance: Stance) -> SignalObservation {
    let evidence: Evidence = serde_json::from_value(serde_json::json!({
        "source_id": "telegram.channel:vanek_nikolaev",
        "external_id": "vanek_nikolaev/1",
        "published_at": "2026-09-28T22:00:00Z",
        "text": "1 реактивный мопед над Киевом",
        "provenance": {
            "url": "https://t.me/vanek_nikolaev/1",
            "collector": "regression/1"
        }
    }))
    .unwrap();
    let mut observations = bridge(
        &Normalizer::embedded().unwrap().read(&evidence),
        &BridgeRules::default(),
    )
    .observations;
    assert_eq!(observations.len(), 1);
    let mut observation = observations.remove(0);
    observation.source_id = SourceId::new("telegram.channel", source).unwrap();
    observation.provenance.evidence_id = ExternalId::try_from(format!("{source}/{id}")).unwrap();
    observation.provenance.evidence_url = format!("https://t.me/{source}/{id}");
    observation.observed_at = time(at);
    observation.stance = stance;
    observation
}

fn assessed(observations: &[SignalObservation], at: &str) -> prism_signal_fusion::Assessed {
    assess(observations, &FusionPolicy::v1(), time(at))
}

#[test]
fn a_clear_affects_only_its_own_source_not_other_live_sources() {
    let observations = [
        report("kpszsu", 1, "2026-09-28T22:00:00Z", Stance::Threat),
        report("war_monitor", 1, "2026-09-28T22:02:00Z", Stance::Threat),
        report("kpszsu", 2, "2026-09-28T22:04:00Z", Stance::Clear),
    ];
    let result = assessed(&observations, "2026-09-28T22:05:00Z");
    assert_eq!(result.assessments.len(), 1);
    assert_eq!(result.assessments[0].status, Status::Active);
    assert_eq!(result.assessments[0].corroboration_count, 1);
    assert_eq!(
        result
            .events
            .iter()
            .filter(|e| e.kind == EventKind::Retracted)
            .count(),
        0
    );
    assert_eq!(
        result.assessments[0].valid_until,
        time("2026-09-28T22:32:00Z")
    );

    let mut reversed = observations.to_vec();
    reversed.reverse();
    assert_eq!(result, assessed(&reversed, "2026-09-28T22:05:00Z"));
}

#[test]
fn an_unknown_source_cannot_clear_another_channels_hazard() {
    let observations = [
        report("kpszsu", 1, "2026-09-28T22:00:00Z", Stance::Threat),
        report("war_monitor", 1, "2026-09-28T22:02:00Z", Stance::Threat),
        report("monitorwarr", 1, "2026-09-28T22:05:00Z", Stance::Clear),
    ];
    let result = assessed(&observations, "2026-09-28T22:06:00Z");
    assert_eq!(result.assessments[0].status, Status::Active);
    assert_eq!(result.assessments[0].corroboration_count, 2);
    assert_eq!(result.events.len(), 2);
}

#[test]
fn retraction_is_emitted_only_after_the_last_live_source_clears() {
    let observations = [
        report("kpszsu", 1, "2026-09-28T22:00:00Z", Stance::Threat),
        report("war_monitor", 1, "2026-09-28T22:02:00Z", Stance::Threat),
        report("kpszsu", 2, "2026-09-28T22:04:00Z", Stance::Clear),
        report("war_monitor", 2, "2026-09-28T22:07:00Z", Stance::Clear),
    ];
    let result = assessed(&observations, "2026-09-28T22:08:00Z");
    assert_eq!(result.assessments.len(), 1);
    assert_eq!(result.assessments[0].status, Status::Retracted);
    let retracts: Vec<_> = result
        .events
        .iter()
        .filter(|e| e.kind == EventKind::Retracted)
        .collect();
    assert_eq!(retracts.len(), 1);
    assert_eq!(
        retracts[0].evidence.as_ref().unwrap().source_id.as_str(),
        "telegram.channel:war_monitor"
    );
}

#[test]
fn cleared_source_can_become_live_again_without_reissuing_the_episode() {
    let observations = [
        report("kpszsu", 1, "2026-09-28T22:00:00Z", Stance::Threat),
        report("war_monitor", 1, "2026-09-28T22:02:00Z", Stance::Threat),
        report("kpszsu", 2, "2026-09-28T22:04:00Z", Stance::Clear),
        report("kpszsu", 3, "2026-09-28T22:06:00Z", Stance::Threat),
    ];
    let result = assessed(&observations, "2026-09-28T22:07:00Z");
    assert_eq!(result.assessments.len(), 1);
    assert_eq!(result.assessments[0].status, Status::Active);
    assert_eq!(result.assessments[0].corroboration_count, 2);
    assert_eq!(result.assessments[0].revision, 3);
    assert_eq!(result.events.len(), 3);
    assert_eq!(
        result.assessments[0].valid_until,
        time("2026-09-28T22:36:00Z")
    );
}

#[test]
fn expired_source_does_not_correspond_to_active_corroboration_or_a_late_clear() {
    let observations = [
        report("kpszsu", 1, "2026-09-28T22:00:00Z", Stance::Threat),
        report("war_monitor", 1, "2026-09-28T22:20:00Z", Stance::Threat),
        report("kpszsu", 2, "2026-09-28T22:35:00Z", Stance::Clear),
    ];
    let result = assessed(&observations, "2026-09-28T22:36:00Z");
    assert_eq!(result.assessments[0].status, Status::Active);
    assert_eq!(result.assessments[0].corroboration_count, 1);
    assert_eq!(
        result.assessments[0].valid_until,
        time("2026-09-28T22:50:00Z")
    );
    assert!(result.events.iter().all(|e| e.kind != EventKind::Retracted));
}

#[test]
fn an_expired_other_source_never_prevents_the_final_live_source_from_clearing() {
    let observations = [
        report("kpszsu", 1, "2026-09-28T22:00:00Z", Stance::Threat),
        report("war_monitor", 1, "2026-09-28T22:20:00Z", Stance::Threat),
        report("war_monitor", 2, "2026-09-28T22:35:00Z", Stance::Clear),
    ];
    let result = assessed(&observations, "2026-09-28T22:36:00Z");
    assert_eq!(result.assessments[0].status, Status::Retracted);
    assert_eq!(
        result
            .events
            .iter()
            .filter(|e| e.kind == EventKind::Retracted)
            .count(),
        1
    );
}

#[test]
fn no_source_has_special_priority_in_the_retraction_policy() {
    for (first, second) in [
        ("vanek_nikolaev", "kpszsu"),
        ("kpszsu", "vanek_nikolaev"),
        ("war_monitor", "monitorwarr"),
        ("monitorwarr", "war_monitor"),
    ] {
        let observations = [
            report(first, 1, "2026-09-28T22:00:00Z", Stance::Threat),
            report(second, 1, "2026-09-28T22:02:00Z", Stance::Threat),
            report(first, 2, "2026-09-28T22:04:00Z", Stance::Clear),
        ];
        assert_eq!(
            assessed(&observations, "2026-09-28T22:05:00Z").assessments[0].status,
            Status::Active
        );
    }
}
