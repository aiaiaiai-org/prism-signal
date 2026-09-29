// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

use async_trait::async_trait;
use prism_signal_source::{SourceError, SourceErrorKind};
use reqwest::{Client, StatusCode, Url};
use thiserror::Error;

use crate::{ChannelName, PreviewFetcher, PreviewQuery};

const DEFAULT_BASE: &str = "https://t.me/";
/// A preview page is tens of kilobytes; anything far larger is not a preview page.
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

/// Invalid transport configuration.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum TransportConfigError {
    /// Base URL is malformed or cannot accept path segments.
    #[error("invalid Telegram preview base URL")]
    InvalidBaseUrl,
    /// Preview pages must be read over HTTPS.
    #[error("Telegram preview base URL must use HTTPS")]
    InsecureBaseUrl,
}

/// Reqwest implementation of [`PreviewFetcher`].
///
/// The client is injected so the caller sets timeouts, proxy, and user agent. Redirects away
/// from `/s/` mean the channel has no public preview and are reported as `NotFound`.
#[derive(Clone, Debug)]
pub struct ReqwestPreviewFetcher {
    client: Client,
    base: Url,
}

impl ReqwestPreviewFetcher {
    /// Fetcher for `https://t.me/`.
    pub fn new(client: Client) -> Result<Self, TransportConfigError> {
        Self::with_base(client, DEFAULT_BASE)
    }

    /// Fetcher for an explicit HTTPS base, such as a mirror under test.
    pub fn with_base(client: Client, base: &str) -> Result<Self, TransportConfigError> {
        let base = Url::parse(base).map_err(|_| TransportConfigError::InvalidBaseUrl)?;
        if base.scheme() != "https" {
            return Err(TransportConfigError::InsecureBaseUrl);
        }
        if base.cannot_be_a_base() || base.query().is_some() || base.fragment().is_some() {
            return Err(TransportConfigError::InvalidBaseUrl);
        }
        Ok(Self { client, base })
    }

    fn url(&self, channel: &ChannelName, query: PreviewQuery) -> Url {
        let mut url = self.base.clone();
        if let Ok(mut segments) = url.path_segments_mut() {
            segments.pop_if_empty().push("s").push(channel.as_str());
        }
        match query {
            PreviewQuery::Latest => {}
            PreviewQuery::Before(id) => {
                url.query_pairs_mut().append_pair("before", &id.to_string());
            }
            PreviewQuery::After(id) => {
                url.query_pairs_mut().append_pair("after", &id.to_string());
            }
        }
        url
    }
}

fn unavailable() -> SourceError {
    SourceError::new(
        SourceErrorKind::Unavailable,
        "telegram.preview.unavailable_transport",
    )
}

#[async_trait]
impl PreviewFetcher for ReqwestPreviewFetcher {
    async fn fetch(
        &self,
        channel: &ChannelName,
        query: PreviewQuery,
    ) -> Result<String, SourceError> {
        let url = self.url(channel, query);
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|_| unavailable())?;

        match response.status() {
            status if status.is_success() => {}
            StatusCode::NOT_FOUND => {
                return Err(SourceError::new(
                    SourceErrorKind::NotFound,
                    "telegram.preview.not_found",
                ));
            }
            StatusCode::TOO_MANY_REQUESTS => {
                return Err(SourceError::new(
                    SourceErrorKind::RateLimited,
                    "telegram.preview.rate_limited",
                ));
            }
            status if status.is_server_error() => return Err(unavailable()),
            _ => {
                return Err(SourceError::new(
                    SourceErrorKind::InvalidResponse,
                    "telegram.preview.unexpected_status",
                ));
            }
        }

        if !response.url().path().starts_with("/s/") {
            return Err(SourceError::new(
                SourceErrorKind::NotFound,
                "telegram.preview.disabled",
            ));
        }

        let body = response.bytes().await.map_err(|_| unavailable())?;
        if body.len() > MAX_BODY_BYTES {
            return Err(SourceError::new(
                SourceErrorKind::InvalidResponse,
                "telegram.preview.too_large",
            ));
        }
        String::from_utf8(body.to_vec()).map_err(|_| {
            SourceError::new(
                SourceErrorKind::InvalidResponse,
                "telegram.preview.not_utf8",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fetcher() -> ReqwestPreviewFetcher {
        ReqwestPreviewFetcher::new(Client::new()).unwrap()
    }

    #[test]
    fn builds_preview_urls() {
        let channel = ChannelName::parse("vanek_nikolaev").unwrap();
        let f = fetcher();
        assert_eq!(
            f.url(&channel, PreviewQuery::Latest).as_str(),
            "https://t.me/s/vanek_nikolaev"
        );
        assert_eq!(
            f.url(&channel, PreviewQuery::Before(100)).as_str(),
            "https://t.me/s/vanek_nikolaev?before=100"
        );
        assert_eq!(
            f.url(&channel, PreviewQuery::After(7)).as_str(),
            "https://t.me/s/vanek_nikolaev?after=7"
        );
    }

    #[test]
    fn rejects_plaintext_base() {
        assert_eq!(
            ReqwestPreviewFetcher::with_base(Client::new(), "http://t.me/").unwrap_err(),
            TransportConfigError::InsecureBaseUrl
        );
    }
}
