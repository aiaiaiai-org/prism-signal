// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Reads hazard reports out of collected evidence.
//!
//! ```text
//! prism-signal-collect telegram vanek_nikolaev backfill | prism-signal-normalize [--actionable]
//! ```
//!
//! stdin carries one `Evidence` JSON object per line, as `prism-signal-collect` prints them.
//! stdout carries one `Reading` JSON object per line and nothing else. A summary and the words
//! the gazetteer did not know go to stderr, so a run doubles as a coverage report. With
//! `--actionable` only readings that report a threat of a known kind at a known place are
//! printed.

use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use prism_signal_core::Evidence;
use prism_signal_normalize::Normalizer;

const USAGE: &str = "usage: prism-signal-normalize [--actionable] < evidence.ndjson";
const TOP_UNRESOLVED: usize = 20;

fn main() -> ExitCode {
    let mut actionable_only = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--actionable" => actionable_only = true,
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    match run(actionable_only) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(actionable_only: bool) -> Result<(), String> {
    let normalizer = Normalizer::embedded().map_err(|e| e.to_string())?;
    let stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();

    let (mut posts, mut readings, mut actionable) = (0_u64, 0_u64, 0_u64);
    let mut unresolved: BTreeMap<String, u64> = BTreeMap::new();

    for (number, line) in stdin.lines().enumerate() {
        let line = line.map_err(|e| format!("line {}: {e}", number + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let evidence: Evidence =
            serde_json::from_str(&line).map_err(|e| format!("line {}: {e}", number + 1))?;
        posts += 1;
        for reading in normalizer.read(&evidence) {
            readings += 1;
            for word in &reading.unresolved {
                *unresolved.entry(word.clone()).or_default() += 1;
            }
            if reading.actionable() {
                actionable += 1;
            } else if actionable_only {
                continue;
            }
            let json = serde_json::to_string(&reading).map_err(|e| e.to_string())?;
            writeln!(stdout, "{json}").map_err(|e| e.to_string())?;
        }
    }
    stdout.flush().map_err(|e| e.to_string())?;

    eprintln!("{posts} posts, {readings} readings, {actionable} actionable");
    if !unresolved.is_empty() {
        let mut ranked: Vec<_> = unresolved.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        eprintln!("not in the gazetteer (candidates to add):");
        for (word, count) in ranked.into_iter().take(TOP_UNRESOLVED) {
            eprintln!("  {count:>4}  {word}");
        }
    }
    eprintln!("place data: {}", normalizer.gazetteer().attribution());
    Ok(())
}
