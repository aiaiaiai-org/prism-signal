// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

use std::sync::LazyLock;

use prism_signal_core::{Evidence, ExternalId, MediaKind, MediaRef, Provenance, Timestamp};
use prism_signal_source::{Cursor, Page, SourceError, SourceErrorKind};
use scraper::{ElementRef, Html, Node, Selector};

use crate::{COLLECTOR, ChannelName, SOURCE_FAMILY};

fn selector(css: &str) -> Selector {
    Selector::parse(css).expect("static selector is valid")
}

static HISTORY: LazyLock<Selector> = LazyLock::new(|| selector(".tgme_channel_history"));
static MESSAGE: LazyLock<Selector> = LazyLock::new(|| selector(".tgme_widget_message[data-post]"));
static DATE: LazyLock<Selector> =
    LazyLock::new(|| selector(".tgme_widget_message_date time[datetime]"));
static META: LazyLock<Selector> = LazyLock::new(|| selector(".tgme_widget_message_meta"));
// `js-message_text` is the post body; a quoted reply uses `js-message_reply_text` instead.
static TEXT: LazyLock<Selector> =
    LazyLock::new(|| selector(".tgme_widget_message_text.js-message_text"));
static FORWARDED: LazyLock<Selector> =
    LazyLock::new(|| selector(".tgme_widget_message_forwarded_from_name"));
static MORE_BEFORE: LazyLock<Selector> =
    LazyLock::new(|| selector(".tme_messages_more[data-before]"));
static MEDIA: LazyLock<Selector> = LazyLock::new(|| {
    selector(concat!(
        ".tgme_widget_message_photo_wrap, ",
        ".tgme_widget_message_video_player, ",
        ".tgme_widget_message_roundvideo_player, ",
        ".tgme_widget_message_voice_player, ",
        ".tgme_widget_message_document_wrap"
    ))
});
// A placeholder for media the preview cannot show. Telegram also nests it inside every video
// player and repeats it as a fallback block after supported media, so it only counts on its own.
static NOT_SUPPORTED: LazyLock<Selector> =
    LazyLock::new(|| selector(".message_media_not_supported"));
// The playable file inside a video player; the blurred backdrop copy has a different class.
static VIDEO_FILE: LazyLock<Selector> = LazyLock::new(|| selector("video.js-message_video[src]"));
static VIDEO_THUMB: LazyLock<Selector> = LazyLock::new(|| {
    selector(".tgme_widget_message_video_thumb, .tgme_widget_message_roundvideo_thumb")
});

fn invalid(code: &'static str) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidResponse, code)
}

/// One parsed post with its numeric id kept for window filtering.
#[derive(Debug)]
pub(crate) struct ParsedPost {
    pub(crate) id: u64,
    pub(crate) evidence: Evidence,
}

/// A parsed preview page before it becomes a port [`Page`].
#[derive(Debug)]
pub struct ParsedPage {
    pub(crate) posts: Vec<ParsedPost>,
    older: Option<u64>,
}

impl ParsedPage {
    /// Drops an `older` cursor that would not move strictly backwards from `bound`, so a
    /// caller walking history can never loop on a repeated page.
    pub(crate) fn require_older_below(&mut self, bound: u64) {
        self.older = self.older.filter(|older| *older < bound);
    }

    pub(crate) fn retain(&mut self, keep: impl Fn(&ParsedPost) -> bool) {
        self.posts.retain(keep);
    }

    /// Evidence ordered oldest to newest.
    pub fn evidence(&self) -> impl Iterator<Item = &Evidence> {
        self.posts.iter().map(|post| &post.evidence)
    }

    pub(crate) fn into_port_page(self) -> Page {
        let newest = self
            .posts
            .last()
            .map(|post| Cursor::new(post.id.to_string()));
        Page {
            evidence: self.posts.into_iter().map(|post| post.evidence).collect(),
            older: self.older.map(|id| Cursor::new(id.to_string())),
            newest,
        }
    }
}

/// Parses one `t.me/s/<channel>` page.
///
/// Service messages (channel created, photo changed) are not publications and are skipped.
/// A page without the channel history container means the preview is unavailable, which is
/// reported as `NotFound` rather than an empty page.
pub fn parse_preview(channel: &ChannelName, html: &str) -> Result<ParsedPage, SourceError> {
    let document = Html::parse_document(html);
    if document.select(&HISTORY).next().is_none() {
        return Err(SourceError::new(
            SourceErrorKind::NotFound,
            "telegram.preview.unavailable",
        ));
    }

    let mut posts = Vec::new();
    for message in document.select(&MESSAGE) {
        if has_class(message, "service_message") {
            continue;
        }
        posts.push(parse_message(channel, message)?);
    }
    posts.sort_by_key(|post| post.id);
    posts.dedup_by_key(|post| post.id);

    let older = document
        .select(&MORE_BEFORE)
        .filter_map(|more| more.value().attr("data-before"))
        .filter_map(|id| id.parse::<u64>().ok())
        .min();

    Ok(ParsedPage { posts, older })
}

fn has_class(element: ElementRef<'_>, class: &str) -> bool {
    element.value().classes().any(|c| c == class)
}

fn parse_message(
    channel: &ChannelName,
    message: ElementRef<'_>,
) -> Result<ParsedPost, SourceError> {
    let data_post = message.value().attr("data-post").unwrap_or_default();
    let (post_channel, raw_id) = data_post
        .split_once('/')
        .ok_or_else(|| invalid("telegram.preview.post_ref"))?;
    if !post_channel.eq_ignore_ascii_case(channel.as_str()) {
        return Err(invalid("telegram.preview.foreign_post"));
    }
    let id = raw_id
        .parse::<u64>()
        .map_err(|_| invalid("telegram.preview.post_id"))?;

    let published_at = message
        .select(&DATE)
        .next()
        .and_then(|time| time.value().attr("datetime"))
        .ok_or_else(|| invalid("telegram.preview.missing_date"))
        .and_then(|raw| Timestamp::parse(raw).map_err(|_| invalid("telegram.preview.bad_date")))?;

    let edited = message
        .select(&META)
        .next()
        .is_some_and(|meta| meta.text().any(|t| t.contains("edited")));

    let text = message
        .select(&TEXT)
        .next()
        .map(plain_text)
        .filter(|t| !t.is_empty());

    let mut media: Vec<MediaRef> = message.select(&MEDIA).map(media_ref).collect();
    if media.is_empty() && message.select(&NOT_SUPPORTED).next().is_some() {
        media.push(MediaRef {
            kind: MediaKind::Other,
            url: None,
        });
    }

    let forwarded_from = message
        .select(&FORWARDED)
        .next()
        .map(|name| collapse_whitespace(&name.text().collect::<String>()))
        .filter(|name| !name.is_empty());

    let external_id = ExternalId::try_from(format!("{}/{id}", channel.as_str()))
        .map_err(|_| invalid("telegram.preview.post_ref"))?;
    let source_id = prism_signal_core::SourceId::new(SOURCE_FAMILY, channel.as_str())
        .map_err(|_| invalid("telegram.preview.post_ref"))?;

    Ok(ParsedPost {
        id,
        evidence: Evidence {
            source_id,
            external_id,
            published_at,
            edited,
            text,
            media,
            forwarded_from,
            provenance: Provenance {
                url: format!("https://t.me/{}/{id}", channel.as_str()),
                collector: COLLECTOR.to_owned(),
            },
        },
    })
}

/// Text content with `<br>` as newlines, trimmed per line ends and overall.
fn plain_text(element: ElementRef<'_>) -> String {
    let mut out = String::new();
    for node in element.descendants() {
        match node.value() {
            Node::Text(text) => out.push_str(text),
            Node::Element(el) if el.name() == "br" => out.push('\n'),
            _ => {}
        }
    }
    out.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn media_ref(element: ElementRef<'_>) -> MediaRef {
    let kind = if has_class(element, "tgme_widget_message_photo_wrap") {
        MediaKind::Photo
    } else if has_class(element, "tgme_widget_message_video_player")
        || has_class(element, "tgme_widget_message_roundvideo_player")
    {
        MediaKind::Video
    } else if has_class(element, "tgme_widget_message_voice_player") {
        MediaKind::Audio
    } else {
        MediaKind::Document
    };
    // Photos carry the image as the wrapper background; video players hold the file in a
    // nested `<video>` (absent when the file is too big for the preview) and a thumbnail.
    // The wrapper `href` is the post link on t.me, already in provenance, so it is not media.
    let url = element
        .value()
        .attr("style")
        .and_then(background_image_url)
        .or_else(|| {
            element
                .select(&VIDEO_FILE)
                .filter_map(|video| video.value().attr("src"))
                .find(|src| src.starts_with("https://"))
                .map(str::to_owned)
        })
        .or_else(|| {
            element
                .select(&VIDEO_THUMB)
                .find_map(|thumb| thumb.value().attr("style").and_then(background_image_url))
        });
    MediaRef { kind, url }
}

/// Extracts `url('…')` from an inline `background-image` style.
fn background_image_url(style: &str) -> Option<String> {
    let start = style.find("background-image:")?;
    let rest = &style[start..];
    let open = rest.find("url(")? + 4;
    let close = rest[open..].find(')')? + open;
    let url = rest[open..close].trim().trim_matches(['\'', '"']);
    url.starts_with("https://").then(|| url.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_image_url_handles_quotes() {
        assert_eq!(
            background_image_url("width:1px;background-image:url('https://cdn/x.jpg')").as_deref(),
            Some("https://cdn/x.jpg")
        );
        assert_eq!(
            background_image_url("background-image:url(\"https://cdn/y.jpg\")").as_deref(),
            Some("https://cdn/y.jpg")
        );
        assert_eq!(
            background_image_url("background-image:url('http://x')"),
            None
        );
        assert_eq!(background_image_url("color:red"), None);
    }
}
