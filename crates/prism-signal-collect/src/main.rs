// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Streams evidence from a source as NDJSON on stdout.
//!
//! ```text
//! prism-signal-collect telegram <channel> backfill [--max-pages N] [--delay-ms MS]
//! prism-signal-collect telegram <channel> follow   [--after ID] [--interval-secs S] [--delay-ms MS]
//! ```
//!
//! stdout carries one `Evidence` JSON object per line and nothing else; progress and errors go
//! to stderr. The collector keeps no files: redirect stdout to persist, and pass the last
//! `newest` cursor printed on stderr back as `--after` to resume.

use std::process::ExitCode;
use std::time::Duration;

use prism_signal_core::Evidence;
use prism_signal_source::{
    Cursor, EvidenceSource, Page, PageRequest, SourceError, SourceErrorKind,
};
use prism_signal_source_telegram::{ChannelName, ReqwestPreviewFetcher, TelegramPreviewSource};
use tokio::io::{AsyncWriteExt, Stdout};

const USAGE: &str = "usage:
  prism-signal-collect telegram <channel> backfill [--max-pages N] [--delay-ms MS]
  prism-signal-collect telegram <channel> follow [--after ID] [--interval-secs S] [--delay-ms MS]";

const MAX_ATTEMPTS: u32 = 5;

#[derive(Debug)]
enum Mode {
    Backfill {
        max_pages: Option<u64>,
    },
    Follow {
        after: Option<String>,
        interval: Duration,
    },
}

#[derive(Debug)]
struct Args {
    channel: ChannelName,
    mode: Mode,
    delay: Duration,
}

fn parse_args(raw: &[String]) -> Result<Args, String> {
    let [family, channel, mode, rest @ ..] = raw else {
        return Err(USAGE.to_owned());
    };
    if family != "telegram" {
        return Err(format!("unsupported source family `{family}`\n{USAGE}"));
    }
    let channel =
        ChannelName::parse(channel).map_err(|_| format!("invalid channel `{channel}`"))?;

    let mut max_pages = None;
    let mut after = None;
    let mut interval = Duration::from_secs(60);
    let mut delay = Duration::from_millis(1500);
    let mut flags = rest.iter();
    while let Some(flag) = flags.next() {
        let value = flags
            .next()
            .ok_or_else(|| format!("missing value for `{flag}`"))?;
        let number = || {
            value
                .parse::<u64>()
                .map_err(|_| format!("`{flag}` expects a non-negative integer"))
        };
        match (mode.as_str(), flag.as_str()) {
            ("backfill", "--max-pages") => max_pages = Some(number()?),
            ("follow", "--after") => after = Some(number()?.to_string()),
            ("follow", "--interval-secs") => interval = Duration::from_secs(number()?.max(10)),
            (_, "--delay-ms") => delay = Duration::from_millis(number()?),
            _ => return Err(format!("unknown flag `{flag}` for `{mode}`\n{USAGE}")),
        }
    }

    let mode = match mode.as_str() {
        "backfill" => Mode::Backfill { max_pages },
        "follow" => Mode::Follow { after, interval },
        other => return Err(format!("unknown mode `{other}`\n{USAGE}")),
    };
    Ok(Args {
        channel,
        mode,
        delay,
    })
}

/// Reads one page, backing off on rate limits and transient failures.
async fn read_with_retry(
    source: &dyn EvidenceSource,
    request: PageRequest,
) -> Result<Page, SourceError> {
    let mut attempt = 0;
    loop {
        match source.read(request.clone()).await {
            Ok(page) => return Ok(page),
            Err(error)
                if matches!(
                    error.kind,
                    SourceErrorKind::RateLimited | SourceErrorKind::Unavailable
                ) && attempt + 1 < MAX_ATTEMPTS =>
            {
                attempt += 1;
                let wait = Duration::from_secs(15 * u64::from(attempt));
                eprintln!("{error}; retry {attempt}/{} in {wait:?}", MAX_ATTEMPTS - 1);
                tokio::time::sleep(wait).await;
            }
            Err(error) => return Err(error),
        }
    }
}

async fn emit(out: &mut Stdout, evidence: &[Evidence]) -> std::io::Result<()> {
    for item in evidence {
        let mut line = serde_json::to_vec(item).map_err(std::io::Error::other)?;
        line.push(b'\n');
        out.write_all(&line).await?;
    }
    out.flush().await
}

async fn backfill(
    source: &dyn EvidenceSource,
    out: &mut Stdout,
    max_pages: Option<u64>,
    delay: Duration,
) -> Result<(), String> {
    let mut request = PageRequest::Latest;
    let mut pages = 0u64;
    let mut total = 0usize;
    let mut newest: Option<Cursor> = None;
    loop {
        let page = read_with_retry(source, request)
            .await
            .map_err(|e| e.to_string())?;
        pages += 1;
        total += page.evidence.len();
        if newest.is_none() {
            newest.clone_from(&page.newest);
        }
        emit(out, &page.evidence).await.map_err(|e| e.to_string())?;
        eprintln!(
            "page {pages}: {} items, older={:?}",
            page.evidence.len(),
            page.older.as_ref().map(Cursor::as_str)
        );
        match page.older {
            Some(older) if max_pages.is_none_or(|max| pages < max) => {
                request = PageRequest::Before(older);
                tokio::time::sleep(delay).await;
            }
            _ => break,
        }
    }
    eprintln!(
        "done: {total} items in {pages} pages; resume with --after {}",
        newest.as_ref().map_or("<none>", Cursor::as_str)
    );
    Ok(())
}

async fn follow(
    source: &dyn EvidenceSource,
    out: &mut Stdout,
    after: Option<String>,
    interval: Duration,
    delay: Duration,
) -> Result<(), String> {
    let mut cursor = match after {
        Some(after) => Cursor::new(after),
        None => {
            let page = read_with_retry(source, PageRequest::Latest)
                .await
                .map_err(|e| e.to_string())?;
            emit(out, &page.evidence).await.map_err(|e| e.to_string())?;
            page.newest.ok_or("source has no posts to follow from")?
        }
    };
    eprintln!("following after {}", cursor.as_str());
    loop {
        // Drain every newer page, then wait for the next poll.
        loop {
            let page = read_with_retry(source, PageRequest::After(cursor.clone()))
                .await
                .map_err(|e| e.to_string())?;
            emit(out, &page.evidence).await.map_err(|e| e.to_string())?;
            match page.newest {
                Some(newest) if newest != cursor => {
                    eprintln!("+{} items, newest={}", page.evidence.len(), newest.as_str());
                    cursor = newest;
                    tokio::time::sleep(delay).await;
                }
                _ => break,
            }
        }
        tokio::time::sleep(interval).await;
    }
}

async fn run(args: Args) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("prism-signal-collect/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let fetcher = ReqwestPreviewFetcher::new(client).map_err(|e| e.to_string())?;
    let source = TelegramPreviewSource::new(args.channel, fetcher);
    let mut out = tokio::io::stdout();
    match args.mode {
        Mode::Backfill { max_pages } => backfill(&source, &mut out, max_pages, args.delay).await,
        Mode::Follow { after, interval } => {
            follow(&source, &mut out, after, interval, args.delay).await
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&raw) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    match run(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Args, String> {
        parse_args(&list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn parses_backfill_and_follow() {
        let a = args(&[
            "telegram",
            "@vanek_nikolaev",
            "backfill",
            "--max-pages",
            "3",
        ])
        .unwrap();
        assert!(matches!(a.mode, Mode::Backfill { max_pages: Some(3) }));
        let a = args(&["telegram", "vanek_nikolaev", "follow", "--after", "42"]).unwrap();
        assert!(matches!(a.mode, Mode::Follow { after: Some(ref id), .. } if id == "42"));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(args(&["telegram", "vanek_nikolaev"]).is_err());
        assert!(args(&["rss", "vanek_nikolaev", "backfill"]).is_err());
        assert!(args(&["telegram", "vanek_nikolaev", "backfill", "--after", "1"]).is_err());
        assert!(args(&["telegram", "vanek_nikolaev", "follow", "--after", "x"]).is_err());
    }
}
