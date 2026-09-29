// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Parser checks against pages captured from the live channel preview.
//!
//! The fixtures are verbatim `t.me/s/vanek_nikolaev` pages from 2026-09-29. They pin the real
//! markup; the synthetic `latest.html` covers cases the live pages lack (service messages,
//! standalone unsupported media).

use prism_signal_core::{Evidence, MediaKind};
use prism_signal_source::{Cursor, EvidenceSource, PageRequest, SourceError};
use prism_signal_source_telegram::{
    ChannelName, ParsedPage, PreviewFetcher, PreviewQuery, TelegramPreviewSource, parse_preview,
};

use async_trait::async_trait;

const LIVE_LATEST: &str = include_str!("fixtures/live_latest.html");
const LIVE_BEFORE_43055: &str = include_str!("fixtures/live_before_43055.html");

fn channel() -> ChannelName {
    ChannelName::parse("vanek_nikolaev").unwrap()
}

fn page(html: &str) -> ParsedPage {
    parse_preview(&channel(), html).unwrap()
}

fn post<'a>(page: &'a ParsedPage, id: &str) -> &'a Evidence {
    let external_id = format!("vanek_nikolaev/{id}");
    page.evidence()
        .find(|e| e.external_id.as_str() == external_id)
        .unwrap_or_else(|| panic!("post {id} not on the page"))
}

fn media_kinds(evidence: &Evidence) -> Vec<MediaKind> {
    evidence.media.iter().map(|m| m.kind).collect()
}

#[test]
fn live_latest_page_has_twenty_ordered_posts() {
    let page = page(LIVE_LATEST);
    let ids: Vec<u64> = page
        .evidence()
        .map(|e| {
            e.external_id.as_str()["vanek_nikolaev/".len()..]
                .parse()
                .unwrap()
        })
        .collect();
    assert_eq!(ids, (43218..=43237).collect::<Vec<_>>());
    assert!(page.evidence().all(|e| e.text.is_some()));
}

#[test]
fn live_reply_quote_is_not_the_body() {
    let page = page(LIVE_LATEST);
    let reply = post(&page, "43218");
    assert_eq!(
        reply.text.as_deref(),
        Some("ракета без дальнейшей фиксации")
    );
    assert_eq!(reply.published_at.to_string(), "2026-09-28T10:42:36Z");
    assert!(!reply.edited);
    assert!(reply.media.is_empty());
    assert_eq!(reply.forwarded_from, None);
}

#[test]
fn live_text_keeps_line_breaks_and_decodes_entities() {
    let page = page(LIVE_LATEST);
    assert_eq!(
        post(&page, "43219").text.as_deref(),
        Some(
            "1 реактивный мопед над Киевом\n\n1 реактивный мопед над Днепром\n\nможет быть громко!"
        )
    );
    let custom_emoji = post(&page, "43223").text.as_deref().unwrap();
    assert!(
        custom_emoji.starts_with("📹 Удар прийшовся"),
        "{custom_emoji}"
    );
}

#[test]
fn live_edit_mark_is_read_from_meta() {
    let page = page(LIVE_LATEST);
    let edited: Vec<_> = page
        .evidence()
        .filter(|e| e.edited)
        .map(|e| e.external_id.as_str())
        .collect();
    assert_eq!(edited, ["vanek_nikolaev/43224"]);
    assert_eq!(
        post(&page, "43224").published_at.to_string(),
        "2026-09-28T14:03:45Z"
    );
}

#[test]
fn live_forwarded_video_counts_once_and_points_at_the_file() {
    let page = page(LIVE_LATEST);
    // 43222 nests an "unsupported" notice inside the player; 43223 also repeats it as a
    // fallback block after the player. Neither is separate media.
    for (id, from, file) in [
        ("43222", "Zelenskiy / Official", "0173e497e5.mp4"),
        ("43223", "Радіо Свобода", "0484be1fb5.mp4"),
    ] {
        let post = post(&page, id);
        assert_eq!(post.forwarded_from.as_deref(), Some(from));
        assert_eq!(media_kinds(post), [MediaKind::Video], "{id}");
        let url = post.media[0].url.as_deref().unwrap();
        assert!(
            url.starts_with(&format!("https://cdn4.telesco.pe/file/{file}?token=")),
            "{url}"
        );
    }
}

#[test]
fn live_video_album_keeps_every_item() {
    let page = page(LIVE_BEFORE_43055);
    // The album spans ids 43049..=43051 but the preview renders it as one post.
    let ids: Vec<_> = page.evidence().map(|e| e.external_id.as_str()).collect();
    assert!(ids.contains(&"vanek_nikolaev/43049"));
    assert!(!ids.contains(&"vanek_nikolaev/43050"));
    assert!(!ids.contains(&"vanek_nikolaev/43051"));

    let album = post(&page, "43049");
    assert_eq!(album.forwarded_from.as_deref(), Some("Exilenova+"));
    assert_eq!(
        album.text.as_deref(),
        Some("Фаєр-шоу на аеродромі в Ростові.")
    );
    assert_eq!(media_kinds(album), [MediaKind::Video; 3]);
    let urls: Vec<_> = album
        .media
        .iter()
        .map(|m| m.url.as_deref().unwrap())
        .collect();
    assert!(urls[0].contains("/65d4c1c11a.mp4?token="));
    assert!(urls[1].contains("/5f55c8c0d9.mp4?token="));
    // Too big for the preview: no file, so the thumbnail stands in.
    assert!(urls[2].starts_with("https://cdn4.telesco.pe/file/"));
    assert!(!urls[2].contains(".mp4"));
}

#[test]
fn live_photo_url_comes_from_the_wrapper_background() {
    let page = page(LIVE_BEFORE_43055);
    let photo = post(&page, "43052");
    assert_eq!(
        photo.forwarded_from.as_deref(),
        Some("Служба безпеки України")
    );
    assert_eq!(media_kinds(photo), [MediaKind::Photo]);
    let url = photo.media[0].url.as_deref().unwrap();
    assert!(
        url.starts_with("https://cdn4.telesco.pe/file/") && url.ends_with(".jpg"),
        "{url}"
    );
}

struct LiveFetcher;

#[async_trait]
impl PreviewFetcher for LiveFetcher {
    async fn fetch(
        &self,
        _channel: &ChannelName,
        query: PreviewQuery,
    ) -> Result<String, SourceError> {
        Ok(match query {
            PreviewQuery::Before(43055) => LIVE_BEFORE_43055,
            _ => LIVE_LATEST,
        }
        .to_owned())
    }
}

#[tokio::test]
async fn live_pagination_follows_data_before() {
    let source = TelegramPreviewSource::new(channel(), LiveFetcher);

    let latest = source.read(PageRequest::Latest).await.unwrap();
    assert_eq!(latest.older, Some(Cursor::new("43218")));
    assert_eq!(latest.newest, Some(Cursor::new("43237")));

    let older = source
        .read(PageRequest::Before(Cursor::new("43055")))
        .await
        .unwrap();
    assert_eq!(older.evidence.len(), 18);
    assert_eq!(older.older, Some(Cursor::new("43035")));
    assert_eq!(older.newest, Some(Cursor::new("43054")));
}
