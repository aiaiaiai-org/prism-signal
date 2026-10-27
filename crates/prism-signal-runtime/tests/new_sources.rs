// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! New-source smoke cases, including normalization, bridge and assessment.
//! Three short public excerpts were inspected on 2026-10-20 (Europe/Kyiv).
//! Replay timestamps are synthetic; they are not the original publication times.

use prism_signal_runtime::Runtime;
use serde_json::{Value, json};

fn post(channel: &str, id: &str, text: &str) -> Value {
    json!({
        "source_id": format!("telegram.channel:{channel}"),
        "external_id": format!("{channel}/{id}"),
        "published_at": "2026-10-19T12:00:00Z",
        "text": text,
        "provenance": {
            "url": format!("https://t.me/{channel}/{id}"),
            "collector": "source-review-fixture/1"
        }
    })
}

fn ask(operation: &str, payload: Value) -> Value {
    let request = json!({
        "protocol_version": "prism-signal.v1",
        "request_id": "source-review",
        "operation": operation,
        "payload": payload
    });
    serde_json::to_value(Runtime::new().unwrap().handle(&request.to_string())).unwrap()
}

fn normalize(posts: Vec<Value>) -> Value {
    ask("normalize", json!({"evidence": posts}))
}

#[test]
fn public_excerpts_create_located_signals_with_original_provenance() {
    for (channel, id, text, kind, place) in [
        (
            "kpszsu",
            "88489",
            "🛵 БпЛА на Суми з півночі.",
            "air.attack_drone",
            "Суми",
        ),
        (
            "war_monitor",
            "47650",
            "☄ БР на Київ",
            "air.ballistic_missile",
            "Київ",
        ),
        (
            "monitorwarr",
            "42828",
            "⚠️ Реактив на Ковель.",
            "air.attack_drone",
            "Ковель",
        ),
    ] {
        let result = normalize(vec![post(channel, id, text)]);
        let observations = result["result"]["observations"].as_array().unwrap();
        assert_eq!(observations.len(), 1, "{channel}: {result}");
        let signal = &observations[0];
        assert_eq!(signal["kind"], kind);
        assert_eq!(signal["stance"], "threat");
        assert_eq!(signal["source_id"], format!("telegram.channel:{channel}"));
        assert_eq!(signal["provenance"]["place_name"], place);
        assert_eq!(
            signal["provenance"]["evidence_id"],
            format!("{channel}/{id}")
        );
        assert_eq!(
            signal["provenance"]["evidence_url"],
            format!("https://t.me/{channel}/{id}")
        );
        let assessed = ask(
            "assess",
            json!({
                "policy_version": "fusion.v1",
                "evaluation_time": "2026-10-19T12:01:00Z",
                "observations": observations
            }),
        );
        assert_eq!(assessed["result"]["assessments"][0]["status"], "active");
        assert_eq!(assessed["result"]["events"][0]["kind"], "issued");
    }
}

#[test]
fn shorthand_is_source_scoped_and_adjectives_are_not_drone_names() {
    // Synthetic negatives, not captured channel posts.
    for (channel, text) in [
        ("kpszsu", "Реактив на Ковель"),
        ("vanek_nikolaev", "БР на Київ"),
        ("monitorwarr", "Реактивний літак на Київ"),
        ("monitorwarr", "Реактивна артилерія на Київ"),
    ] {
        let result = normalize(vec![post(channel, "synthetic-negative", text)]);
        assert_eq!(result["result"]["observations"], json!([]), "{text}");
    }
}

#[test]
fn source_shorthand_obeys_existing_location_and_clear_boundaries() {
    // Synthetic boundary cases; no inference between separate posts.
    let result = normalize(vec![
        post("monitorwarr", "synthetic-region", "Реактиви на Київщину"),
        post("monitorwarr", "synthetic-origin", "Реактив з Києва"),
        post("monitorwarr", "synthetic-recon", "Дорозвідка"),
        post("war_monitor", "synthetic-clear", "Відбій БР"),
        post("war_monitor", "synthetic-minus", "Мінус"),
        post("monitorwarr", "synthetic-followup", "Кружляє поблизу Києва"),
    ]);
    // The region is the only observation, as its own cells; fusion keeps regions out of
    // assessments and gives them to tracks.
    let observations = result["result"]["observations"].as_array().unwrap();
    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0]["provenance"]["place_id"], "ua-adm1:UA32");
    assert_eq!(observations[0]["geometry"]["type"], "cells");
    let skipped = result["result"]["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 2);
    assert!(skipped.iter().all(|item| item["code"] == "unlocated_clear"));
}

#[test]
fn a_clear_cannot_spill_across_a_comma_onto_a_new_shorthand_threat() {
    let result = normalize(vec![post(
        "war_monitor",
        "synthetic-comma",
        "Відбій БР на Київ, БР на Суми",
    )]);
    let observations = result["result"]["observations"].as_array().unwrap();
    assert_eq!(observations.len(), 2);
    assert_eq!(observations[0]["stance"], "clear");
    assert_eq!(observations[0]["provenance"]["place_name"], "Київ");
    assert_eq!(observations[1]["stance"], "threat");
    assert_eq!(observations[1]["provenance"]["place_name"], "Суми");
}

#[test]
fn forwarded_shorthand_never_becomes_independent_evidence() {
    let mut forwarded = post("monitorwarr", "synthetic-forward", "Реактив на Ковель");
    forwarded["forwarded_from"] = json!("kpszsu");
    let result = normalize(vec![forwarded]);
    assert_eq!(result["result"]["observations"], json!([]));
    assert_eq!(result["result"]["skipped"][0]["code"], "forwarded");
}
