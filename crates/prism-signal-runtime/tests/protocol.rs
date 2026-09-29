// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! The runtime through its wire: requests as text in, responses as text out.

use std::io::Write;
use std::process::{Command, Stdio};

use prism_signal_runtime::Runtime;
use serde_json::{Value, json};

fn runtime() -> Runtime {
    Runtime::new().unwrap()
}

fn ask(runtime: &Runtime, request: &Value) -> Value {
    serde_json::to_value(runtime.handle(&request.to_string())).unwrap()
}

fn request(operation: &str, payload: Value) -> Value {
    json!({"protocol_version": "prism-signal.v1", "request_id": "r-1", "operation": operation, "payload": payload})
}

fn evidence(id: u64, at: &str, text: &str) -> Value {
    json!({
        "source_id": "telegram.channel:vanek_nikolaev",
        "external_id": format!("vanek_nikolaev/{id}"),
        "published_at": at,
        "text": text,
        "provenance": {"url": format!("https://t.me/vanek_nikolaev/{id}"), "collector": "test/0"}
    })
}

fn failure_code(response: &Value) -> &str {
    response["failure"]["code"].as_str().unwrap()
}

#[test]
fn capabilities_say_what_is_supported() {
    let r = ask(
        &runtime(),
        &json!({"protocol_version": "prism-signal.v1", "request_id": "c", "operation": "capabilities"}),
    );
    assert_eq!(r["request_id"], "c");
    let result = &r["result"];
    assert_eq!(
        result["operations"],
        json!(["capabilities", "normalize", "cover", "assess"])
    );
    assert_eq!(result["policy_versions"], json!(["fusion.v1"]));
    assert_eq!(result["readers"], json!(["normalize", "observe"]));
    assert_eq!(result["grid_resolution"], 6);
    assert_eq!(result["limits"]["max_cells"], 400);
    assert!(
        result["hazard_kinds"]
            .as_array()
            .unwrap()
            .contains(&json!("air.attack_drone"))
    );
}

#[test]
fn normalize_reads_evidence_into_observations_with_either_reader() {
    let rt = runtime();
    let payload = json!({"evidence": [evidence(1, "2026-09-28T22:19:42Z", "1 реактивный мопед подлетает к Киеву со стороны Гостомеля")]});
    let r = ask(&rt, &request("normalize", payload.clone()));
    assert_eq!(r["result"]["reader"], "normalize");
    let observations = r["result"]["observations"].as_array().unwrap();
    assert_eq!(
        observations.len(),
        1,
        "the launch site is not an observation"
    );
    assert_eq!(observations[0]["kind"], "air.attack_drone");
    assert_eq!(observations[0]["stance"], "threat");
    assert_eq!(observations[0]["provenance"]["place_name"], "Київ");

    let mut observe = payload;
    observe["reader"] = json!("observe");
    let r = ask(&rt, &request("normalize", observe));
    assert_eq!(r["result"]["reader"], "observe");
    let places: Vec<_> = r["result"]["observations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["provenance"]["place_id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        places.len(),
        2,
        "this reader gives no roles, so the launch site is one too"
    );
}

#[test]
fn normalize_says_what_it_left_out() {
    let mut forwarded = evidence(2, "2026-09-28T22:19:42Z", "2 баллистики на Киев !");
    forwarded["forwarded_from"] = json!("Інший канал");
    let payload = json!({"evidence": [
        forwarded,
        evidence(3, "2026-09-28T22:20:00Z", "минус по всем этим мопедам"),
    ]});
    let r = ask(&runtime(), &request("normalize", payload));
    let codes: Vec<_> = r["result"]["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["evidence_id"].as_str().unwrap(),
                s["code"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        codes,
        [
            ("vanek_nikolaev/2", "forwarded"),
            ("vanek_nikolaev/3", "unlocated_clear")
        ]
    );
}

#[test]
fn a_window_over_the_limit_is_refused() {
    let many: Vec<Value> = (0..501)
        .map(|i| evidence(i, "2026-09-28T22:19:42Z", "x"))
        .collect();
    let r = ask(&runtime(), &request("normalize", json!({"evidence": many})));
    assert_eq!(failure_code(&r), "window_too_large");
}

#[test]
fn cover_returns_sorted_cells_widened_on_request_and_refuses_what_is_too_large() {
    let rt = runtime();
    let point = json!({"type": "point", "coordinates": [30.5238, 50.4547]});
    let one = ask(&rt, &request("cover", json!({"geometry": point})));
    assert_eq!(one["result"]["cells"]["resolution"], 6);
    assert_eq!(one["result"]["cells"]["cells"].as_array().unwrap().len(), 1);

    let wide = ask(
        &rt,
        &request("cover", json!({"geometry": point, "rings": 1})),
    );
    assert_eq!(
        wide["result"]["cells"]["cells"].as_array().unwrap().len(),
        7
    );
    let cells: Vec<&str> = wide["result"]["cells"]["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    let mut sorted = cells.clone();
    sorted.sort_unstable();
    assert_eq!(cells, sorted);

    let refused = ask(
        &rt,
        &request(
            "cover",
            json!({"geometry": point, "rings": 3, "max_cells": 5}),
        ),
    );
    assert_eq!(failure_code(&refused), "cover_too_large");

    let bad = ask(
        &rt,
        &request(
            "cover",
            json!({"geometry": {"type": "cells", "resolution": 6, "cells": []}}),
        ),
    );
    assert_eq!(failure_code(&bad), "invalid_geometry");
}

/// A scenario end to end: read three posts, fuse them.
fn assess_scenario(rt: &Runtime, posts: &[(u64, &str, &str)], now: &str) -> Value {
    let evidence: Vec<Value> = posts
        .iter()
        .map(|(id, at, text)| evidence(*id, at, text))
        .collect();
    let read = ask(rt, &request("normalize", json!({"evidence": evidence})));
    let observations = read["result"]["observations"].clone();
    ask(
        rt,
        &request(
            "assess",
            json!({"policy_version": "fusion.v1", "evaluation_time": now, "observations": observations}),
        ),
    )
}

#[test]
fn assess_turns_reports_into_assessments_and_events() {
    let r = assess_scenario(
        &runtime(),
        &[
            (1, "2026-09-28T22:19:42Z", "2 баллистики на Киев !"),
            (2, "2026-09-28T22:27:00Z", "минус по баллистике на Киев"),
        ],
        "2026-09-28T22:28:00Z",
    );
    let assessments = r["result"]["assessments"].as_array().unwrap();
    assert_eq!(assessments.len(), 1);
    assert_eq!(assessments[0]["status"], "retracted");
    assert_eq!(assessments[0]["class"], "missile");
    let kinds: Vec<_> = r["result"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["issued", "retracted"]);
}

#[test]
fn the_same_request_always_gets_the_same_answer() {
    let rt = runtime();
    let posts = [
        (
            1,
            "2026-09-28T22:19:42Z",
            "1 реактивный мопед над Киевом\n\n2 баллистики на Одессу",
        ),
        (2, "2026-09-28T22:21:00Z", "ещё 1 мопед над Киевом"),
    ];
    let first = assess_scenario(&rt, &posts, "2026-09-28T22:30:00Z");
    assert_eq!(first, assess_scenario(&rt, &posts, "2026-09-28T22:30:00Z"));
    assert_eq!(
        first,
        assess_scenario(&Runtime::new().unwrap(), &posts, "2026-09-28T22:30:00Z")
    );
}

#[test]
fn assess_needs_a_known_policy_and_an_evaluation_time() {
    let rt = runtime();
    let unknown = ask(
        &rt,
        &request(
            "assess",
            json!({"policy_version": "fusion.v9", "evaluation_time": "2026-09-28T22:28:00Z", "observations": []}),
        ),
    );
    assert_eq!(failure_code(&unknown), "unknown_policy");
    let no_time = ask(
        &rt,
        &request(
            "assess",
            json!({"policy_version": "fusion.v1", "observations": []}),
        ),
    );
    assert_eq!(
        failure_code(&no_time),
        "invalid_request",
        "the runtime never reads a clock"
    );
}

#[test]
fn a_request_that_cannot_be_read_gets_a_typed_failure_and_keeps_its_id() {
    let rt = runtime();
    let r = rt.handle("not json");
    assert_eq!(
        (
            r.request_id.as_str(),
            r.failure.as_ref().unwrap().code.as_str()
        ),
        ("", "invalid_request")
    );

    let wrong_version = ask(
        &rt,
        &json!({"protocol_version": "prism-signal.v2", "request_id": "v", "operation": "capabilities"}),
    );
    assert_eq!(
        (
            wrong_version["request_id"].as_str(),
            failure_code(&wrong_version)
        ),
        (Some("v"), "invalid_request")
    );

    let unknown = ask(
        &rt,
        &json!({"protocol_version": "prism-signal.v1", "request_id": "u", "operation": "explode"}),
    );
    assert_eq!(
        (unknown["request_id"].as_str(), failure_code(&unknown)),
        (Some("u"), "unsupported_operation")
    );

    let extra = ask(
        &rt,
        &request("normalize", json!({"evidence": [], "surprise": 1})),
    );
    assert_eq!(
        failure_code(&extra),
        "invalid_request",
        "unknown fields fail explicitly"
    );
    assert!(
        !extra["failure"]["message"]
            .as_str()
            .unwrap()
            .contains("surprise"),
        "the request is not echoed"
    );
}

fn run_binary(args: &[&str], input: &str) -> (String, String, bool) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_prism-signal-runtime"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
        output.status.success(),
    )
}

#[test]
fn the_binary_answers_one_line_per_request_and_writes_only_protocol_to_stdout() {
    let caps = json!({"protocol_version": "prism-signal.v1", "request_id": "a", "operation": "capabilities"}).to_string();
    let bad = "{oops";
    let (stdout, _stderr, ok) = run_binary(&[], &format!("{caps}\n\n{bad}\n{caps}\n"));
    assert!(ok);
    let lines: Vec<Value> = stdout
        .lines()
        .map(|l| serde_json::from_str(l).expect("every stdout line is JSON"))
        .collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["request_id"], "a");
    assert_eq!(failure_code(&lines[1]), "invalid_request");
    assert_eq!(lines[2]["request_id"], "a");
}

#[test]
fn the_binary_reads_a_single_request_in_json_mode_and_rejects_unknown_flags() {
    let caps = json!({"protocol_version": "prism-signal.v1", "request_id": "one", "operation": "capabilities"}).to_string();
    let (stdout, _, ok) = run_binary(&["--json"], &caps);
    assert!(ok);
    assert_eq!(stdout.lines().count(), 1);
    assert_eq!(
        serde_json::from_str::<Value>(&stdout).unwrap()["request_id"],
        "one"
    );

    let (_, stderr, ok) = run_binary(&["--nope"], "");
    assert!(!ok);
    assert!(stderr.contains("usage"));
}
