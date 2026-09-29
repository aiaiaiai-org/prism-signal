// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

use std::sync::Mutex;

use async_trait::async_trait;
use prism_signal_core::MediaKind;
use prism_signal_source::{Cursor, EvidenceSource, PageRequest, SourceError, SourceErrorKind};
use prism_signal_source_telegram::{
    ChannelName, PreviewFetcher, PreviewQuery, TelegramPreviewSource, parse_preview,
};

const LATEST: &str = include_str!("fixtures/latest.html");
const DISABLED: &str = include_str!("fixtures/disabled.html");

fn channel() -> ChannelName {
    ChannelName::parse("vanek_nikolaev").unwrap()
}

#[test]
fn parses_posts_in_id_order_and_skips_service_messages() {
    let page = parse_preview(&channel(), LATEST).unwrap();
    let ids: Vec<_> = page.evidence().map(|e| e.external_id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "vanek_nikolaev/101",
            "vanek_nikolaev/103",
            "vanek_nikolaev/105",
            "vanek_nikolaev/106"
        ]
    );
}

#[test]
fn extracts_text_time_and_provenance() {
    let page = parse_preview(&channel(), LATEST).unwrap();
    let first = page.evidence().next().unwrap();
    assert_eq!(first.source_id.as_str(), "telegram.channel:vanek_nikolaev");
    assert_eq!(
        first.text.as_deref(),
        Some("Миколаїв: тихо 😌\nДруга   лінія\nпосилання")
    );
    assert_eq!(first.published_at.to_string(), "2026-09-29T07:15:00Z");
    assert!(!first.edited);
    assert!(first.media.is_empty());
    assert_eq!(first.provenance.url, "https://t.me/vanek_nikolaev/101");
    assert!(
        first
            .provenance
            .collector
            .starts_with("prism-signal-source-telegram/")
    );
}

#[test]
fn ignores_reply_quote_and_reads_album_and_edit_mark() {
    let page = parse_preview(&channel(), LATEST).unwrap();
    let post = page.evidence().nth(1).unwrap();
    assert_eq!(post.text.as_deref(), Some("Оновлення по області"));
    assert!(post.edited);
    let urls: Vec<_> = post
        .media
        .iter()
        .map(|m| (m.kind, m.url.as_deref()))
        .collect();
    assert_eq!(
        urls,
        [
            (MediaKind::Photo, Some("https://cdn4.telesco.pe/file/a.jpg")),
            (MediaKind::Photo, Some("https://cdn4.telesco.pe/file/b.jpg")),
        ]
    );
}

#[test]
fn reads_forwarded_video_and_unsupported_media() {
    let page = parse_preview(&channel(), LATEST).unwrap();
    let forwarded = page.evidence().nth(2).unwrap();
    assert_eq!(forwarded.forwarded_from.as_deref(), Some("Other Channel"));
    assert_eq!(forwarded.text, None);
    assert_eq!(forwarded.media.len(), 1);
    assert_eq!(forwarded.media[0].kind, MediaKind::Video);

    let unsupported = page.evidence().nth(3).unwrap();
    assert_eq!(unsupported.media[0].kind, MediaKind::Other);
}

#[test]
fn missing_history_is_not_found() {
    let error = parse_preview(&channel(), DISABLED).unwrap_err();
    assert_eq!(error.kind, SourceErrorKind::NotFound);
}

#[test]
fn foreign_post_is_rejected() {
    let other = ChannelName::parse("another_channel").unwrap();
    let error = parse_preview(&other, LATEST).unwrap_err();
    assert_eq!(error.code, "telegram.preview.foreign_post");
}

struct FixtureFetcher {
    calls: Mutex<Vec<PreviewQuery>>,
}

#[async_trait]
impl PreviewFetcher for FixtureFetcher {
    async fn fetch(
        &self,
        _channel: &ChannelName,
        query: PreviewQuery,
    ) -> Result<String, SourceError> {
        self.calls.lock().unwrap().push(query);
        Ok(LATEST.to_owned())
    }
}

fn source() -> TelegramPreviewSource<FixtureFetcher> {
    TelegramPreviewSource::new(
        channel(),
        FixtureFetcher {
            calls: Mutex::new(Vec::new()),
        },
    )
}

#[tokio::test]
async fn latest_page_exposes_both_cursors() {
    let page = source().read(PageRequest::Latest).await.unwrap();
    assert_eq!(page.evidence.len(), 4);
    assert_eq!(page.older, Some(Cursor::new("101")));
    assert_eq!(page.newest, Some(Cursor::new("106")));
}

#[tokio::test]
async fn window_is_strict_and_query_is_forwarded() {
    let source = source();
    let page = source
        .read(PageRequest::After(Cursor::new("103")))
        .await
        .unwrap();
    let ids: Vec<_> = page
        .evidence
        .iter()
        .map(|e| e.external_id.as_str())
        .collect();
    assert_eq!(ids, ["vanek_nikolaev/105", "vanek_nikolaev/106"]);

    let page = source
        .read(PageRequest::Before(Cursor::new("103")))
        .await
        .unwrap();
    assert_eq!(page.evidence.len(), 1);
    assert_eq!(page.newest, Some(Cursor::new("101")));
}

#[tokio::test]
async fn repeated_older_cursor_ends_the_walk() {
    // The fixture always answers with `data-before="101"`; asking before 101 must not
    // hand back 101 again, or a history walk would loop forever.
    let page = source()
        .read(PageRequest::Before(Cursor::new("101")))
        .await
        .unwrap();
    assert!(page.evidence.is_empty());
    assert_eq!(page.older, None);
}

#[tokio::test]
async fn rejects_foreign_cursor() {
    let error = source()
        .read(PageRequest::Before(Cursor::new("abc")))
        .await
        .unwrap_err();
    assert_eq!(error.kind, SourceErrorKind::InvalidRequest);
}
