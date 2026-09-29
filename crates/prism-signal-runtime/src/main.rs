// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! ```text
//! prism-signal-runtime           # one JSON request per line on stdin, one response per line
//! prism-signal-runtime --json    # the whole of stdin is one request, one response on stdout
//! ```
//!
//! stdout carries the protocol and nothing else; diagnostics go to stderr.

use std::io::{BufRead, Read, Write};
use std::process::ExitCode;

use prism_signal_runtime::Runtime;

fn main() -> ExitCode {
    let single = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--json") => true,
        Some(_) => {
            eprintln!("usage: prism-signal-runtime [--json]");
            return ExitCode::from(2);
        }
    };
    let runtime = match Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut stdout = std::io::stdout().lock();
    let outcome = if single {
        let mut text = String::new();
        std::io::stdin()
            .lock()
            .read_to_string(&mut text)
            .and_then(|_| respond(&runtime, &text, &mut stdout))
    } else {
        std::io::stdin().lock().lines().try_for_each(|line| {
            let line = line?;
            if line.trim().is_empty() {
                return Ok(());
            }
            respond(&runtime, &line, &mut stdout)
        })
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn respond(runtime: &Runtime, text: &str, out: &mut impl Write) -> std::io::Result<()> {
    let response = runtime.handle(text);
    let json = serde_json::to_string(&response).map_err(std::io::Error::other)?;
    writeln!(out, "{json}")?;
    out.flush()
}
