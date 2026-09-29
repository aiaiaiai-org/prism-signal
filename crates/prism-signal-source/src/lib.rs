// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Source adapter port for Prism Signal.
//!
//! A source is read one page at a time. The caller owns the cursor, scheduling, and storage;
//! an adapter keeps no state between calls.

use async_trait::async_trait;
use prism_signal_core::{Evidence, SourceId};
use thiserror::Error;

/// Opaque, adapter-defined position inside a source.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Cursor(String);

impl Cursor {
    /// Wraps an adapter-produced cursor value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Raw cursor value.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Which page to read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PageRequest {
    /// The newest page the source currently exposes.
    Latest,
    /// The page of items strictly older than the cursor.
    Before(Cursor),
    /// The page of items strictly newer than the cursor.
    After(Cursor),
}

/// One page of evidence.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Page {
    /// Evidence ordered from oldest to newest.
    pub evidence: Vec<Evidence>,
    /// Cursor for [`PageRequest::Before`] when older items exist.
    pub older: Option<Cursor>,
    /// Cursor for [`PageRequest::After`] positioned at the newest item on this page.
    pub newest: Option<Cursor>,
}

/// Classified source failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceErrorKind {
    /// The source could not be reached or answered with a transient error.
    Unavailable,
    /// The source asked the caller to slow down.
    RateLimited,
    /// The source does not exist or is not publicly readable.
    NotFound,
    /// The source answered with content the adapter cannot interpret.
    InvalidResponse,
    /// The request was malformed, such as a cursor from another adapter.
    InvalidRequest,
}

/// Typed source failure. `code` is a stable machine-readable reason.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{code}")]
pub struct SourceError {
    /// Failure class.
    pub kind: SourceErrorKind,
    /// Stable reason code such as `telegram.preview.not_found`.
    pub code: &'static str,
}

impl SourceError {
    /// Builds a typed failure.
    pub fn new(kind: SourceErrorKind, code: &'static str) -> Self {
        Self { kind, code }
    }
}

/// A readable evidence source.
#[async_trait]
pub trait EvidenceSource: Send + Sync {
    /// The declared source this adapter reads.
    fn source_id(&self) -> &SourceId;

    /// Reads one page. An empty page is a valid answer, never a substituted value.
    async fn read(&self, request: PageRequest) -> Result<Page, SourceError>;
}
