// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Telegram public channel preview adapter for Prism Signal.
//!
//! Reads the public web preview at `https://t.me/s/<channel>`, which needs no account or
//! credentials and exists only for channels whose owners left it enabled. Each page holds
//! about twenty posts; `?before=<id>` walks older history and `?after=<id>` reads newer.
//!
//! The adapter is stateless. Transport is injected through [`PreviewFetcher`], so parsing is
//! tested without network and a caller controls pacing, proxies, and retries.

mod http;
mod parse;

use async_trait::async_trait;
use prism_signal_core::{SourceId, ValueError};
use prism_signal_source::{
    Cursor, EvidenceSource, Page, PageRequest, SourceError, SourceErrorKind,
};

pub use http::{ReqwestPreviewFetcher, TransportConfigError};
pub use parse::{ParsedPage, parse_preview};

/// Source family used in [`SourceId`] values produced by this adapter.
pub const SOURCE_FAMILY: &str = "telegram.channel";

/// Collector string recorded in evidence provenance.
pub const COLLECTOR: &str = concat!("prism-signal-source-telegram/", env!("CARGO_PKG_VERSION"));

/// A public Telegram channel username, stored lowercase.
///
/// Telegram usernames are 5–32 characters of ASCII letters, digits, and underscores, start
/// with a letter, and compare case-insensitively.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ChannelName(String);

impl ChannelName {
    /// Validates a username, accepting an optional leading `@` or a `t.me` link.
    pub fn parse(value: &str) -> Result<Self, ValueError> {
        let trimmed = value.trim();
        let name = trimmed
            .strip_prefix("https://t.me/s/")
            .or_else(|| trimmed.strip_prefix("https://t.me/"))
            .or_else(|| trimmed.strip_prefix('@'))
            .unwrap_or(trimmed)
            .trim_end_matches('/');
        let valid = (5..=32).contains(&name.len())
            && name.as_bytes()[0].is_ascii_alphabetic()
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if valid {
            Ok(Self(name.to_ascii_lowercase()))
        } else {
            Err(ValueError::InvalidSourceId)
        }
    }

    /// Lowercase username without `@`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Position in a channel preview, as sent to the server.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewQuery {
    /// Newest page.
    Latest,
    /// Posts with ids strictly below this id.
    Before(u64),
    /// Posts with ids strictly above this id.
    After(u64),
}

/// Fetches raw preview HTML. Implementations map transport failures to [`SourceError`].
#[async_trait]
pub trait PreviewFetcher: Send + Sync {
    /// Returns the HTML body for one preview page.
    async fn fetch(
        &self,
        channel: &ChannelName,
        query: PreviewQuery,
    ) -> Result<String, SourceError>;
}

/// Public channel preview as an [`EvidenceSource`].
pub struct TelegramPreviewSource<F> {
    channel: ChannelName,
    source_id: SourceId,
    fetcher: F,
}

impl<F: PreviewFetcher> TelegramPreviewSource<F> {
    /// Binds a channel to a fetcher.
    pub fn new(channel: ChannelName, fetcher: F) -> Self {
        let source_id = SourceId::new(SOURCE_FAMILY, channel.as_str())
            .expect("a validated channel name is always a valid source id part");
        Self {
            channel,
            source_id,
            fetcher,
        }
    }

    /// The channel this source reads.
    pub fn channel(&self) -> &ChannelName {
        &self.channel
    }
}

fn cursor_id(cursor: &Cursor) -> Result<u64, SourceError> {
    cursor
        .as_str()
        .parse::<u64>()
        .map_err(|_| SourceError::new(SourceErrorKind::InvalidRequest, "telegram.cursor.invalid"))
}

#[async_trait]
impl<F: PreviewFetcher> EvidenceSource for TelegramPreviewSource<F> {
    fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    async fn read(&self, request: PageRequest) -> Result<Page, SourceError> {
        let query = match &request {
            PageRequest::Latest => PreviewQuery::Latest,
            PageRequest::Before(cursor) => PreviewQuery::Before(cursor_id(cursor)?),
            PageRequest::After(cursor) => PreviewQuery::After(cursor_id(cursor)?),
        };
        let html = self.fetcher.fetch(&self.channel, query).await?;
        let mut page = parse_preview(&self.channel, &html)?;
        // Items outside the requested window can appear at page edges; the port promises a
        // strict window, so they are dropped here rather than leaked to the caller.
        match query {
            PreviewQuery::Latest => {}
            PreviewQuery::Before(id) => {
                page.retain(|post| post.id < id);
                page.require_older_below(id);
            }
            PreviewQuery::After(id) => page.retain(|post| post.id > id),
        }
        Ok(page.into_port_page())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_name_accepts_common_spellings() {
        for input in [
            "vanek_nikolaev",
            "@vanek_nikolaev",
            "https://t.me/vanek_nikolaev",
            "https://t.me/s/vanek_nikolaev/",
            "Vanek_Nikolaev",
        ] {
            assert_eq!(
                ChannelName::parse(input).unwrap().as_str(),
                "vanek_nikolaev"
            );
        }
    }

    #[test]
    fn channel_name_rejects_invalid_usernames() {
        for input in ["abc", "1vanek", "vanek-nikolaev", "", &"a".repeat(33)] {
            assert!(ChannelName::parse(input).is_err(), "{input}");
        }
    }
}
