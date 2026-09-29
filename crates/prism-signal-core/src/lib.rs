// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Source-neutral evidence values for Prism Signal.
//!
//! [`Evidence`] is what a source adapter returns: one unit of raw material exactly as the
//! source published it, with provenance. It carries no geometry and no hazard meaning;
//! normalization into a located observation is a later, separate step.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};

/// Invalid core value.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ValueError {
    /// A source identifier must be `<family>:<name>` with non-empty, lowercase ASCII parts.
    #[error("invalid source id")]
    InvalidSourceId,
    /// An external identifier must be non-empty printable ASCII without whitespace.
    #[error("invalid external id")]
    InvalidExternalId,
    /// A timestamp must be RFC 3339 with an explicit offset.
    #[error("invalid timestamp")]
    InvalidTimestamp,
}

/// Stable identifier of one declared source, such as `telegram.channel:vanek_nikolaev`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SourceId(String);

impl SourceId {
    /// Builds a source identifier from a family and a source-local name.
    pub fn new(family: &str, name: &str) -> Result<Self, ValueError> {
        Self::try_from(format!("{family}:{name}"))
    }

    /// Canonical string form.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for SourceId {
    type Error = ValueError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let valid_part = |part: &str| {
            !part.is_empty()
                && part.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'-')
                })
        };
        match value.split_once(':') {
            Some((family, name)) if valid_part(family) && valid_part(name) => Ok(Self(value)),
            _ => Err(ValueError::InvalidSourceId),
        }
    }
}

impl From<SourceId> for String {
    fn from(value: SourceId) -> Self {
        value.0
    }
}

impl fmt::Display for SourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Identifier of one item inside its source, unique per [`SourceId`].
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ExternalId(String);

impl ExternalId {
    /// Canonical string form.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ExternalId {
    type Error = ValueError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if !value.is_empty() && value.bytes().all(|b| b.is_ascii_graphic()) {
            Ok(Self(value))
        } else {
            Err(ValueError::InvalidExternalId)
        }
    }
}

impl From<ExternalId> for String {
    fn from(value: ExternalId) -> Self {
        value.0
    }
}

/// A UTC instant rendered canonically as RFC 3339 with a `Z` suffix.
///
/// Parsing accepts any explicit offset and converts it to UTC, so equal instants always have
/// equal wire forms.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Timestamp(OffsetDateTime);

impl Timestamp {
    /// Parses an RFC 3339 timestamp with an explicit offset.
    pub fn parse(value: &str) -> Result<Self, ValueError> {
        OffsetDateTime::parse(value, &Rfc3339)
            .map(|t| Self(t.to_offset(UtcOffset::UTC)))
            .map_err(|_| ValueError::InvalidTimestamp)
    }

    /// Wraps an instant, converting it to UTC.
    pub fn from_datetime(value: OffsetDateTime) -> Self {
        Self(value.to_offset(UtcOffset::UTC))
    }

    /// The UTC instant.
    pub fn as_datetime(&self) -> OffsetDateTime {
        self.0
    }
}

impl TryFrom<String> for Timestamp {
    type Error = ValueError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<Timestamp> for String {
    fn from(value: Timestamp) -> Self {
        value.to_string()
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // UTC and RFC 3339 formatting of a valid instant cannot fail.
        let rendered = self.0.format(&Rfc3339).map_err(|_| fmt::Error)?;
        f.write_str(&rendered)
    }
}

/// Kind of media attached to evidence. Media bytes are never fetched by adapters.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    /// Still image.
    Photo,
    /// Video or animation.
    Video,
    /// Voice note or audio track.
    Audio,
    /// File attachment.
    Document,
    /// Media the adapter recognised as present but could not classify.
    Other,
}

/// Reference to media attached to evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MediaRef {
    /// Media kind.
    pub kind: MediaKind,
    /// Source-provided URL, if any. It may expire and is not content-addressed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Where evidence came from and how it was collected.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    /// Public URL of the item at its source.
    pub url: String,
    /// Collector name and version, such as `prism-signal-source-telegram/0.1.0`.
    pub collector: String,
}

/// One unit of raw material exactly as a source published it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    /// Declared source.
    pub source_id: SourceId,
    /// Item identifier inside the source; `(source_id, external_id)` is the dedup key.
    pub external_id: ExternalId,
    /// When the source says the item was published.
    pub published_at: Timestamp,
    /// Whether the source marks the item as edited after publication.
    #[serde(default)]
    pub edited: bool,
    /// Plain text with line breaks preserved. `None` for media-only items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Attached media, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<MediaRef>,
    /// Display name of the original author when the item was forwarded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forwarded_from: Option<String>,
    /// Collection provenance.
    pub provenance: Provenance,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_id_requires_family_and_lowercase_name() {
        assert!(SourceId::new("telegram.channel", "vanek_nikolaev").is_ok());
        assert_eq!(
            SourceId::new("telegram.channel", "Vanek"),
            Err(ValueError::InvalidSourceId)
        );
        assert_eq!(
            SourceId::try_from("nofamily".to_owned()),
            Err(ValueError::InvalidSourceId)
        );
        assert_eq!(SourceId::new("", "x"), Err(ValueError::InvalidSourceId));
    }

    #[test]
    fn external_id_rejects_whitespace_and_empty() {
        assert!(ExternalId::try_from("vanek_nikolaev/42".to_owned()).is_ok());
        assert!(ExternalId::try_from(String::new()).is_err());
        assert!(ExternalId::try_from("a b".to_owned()).is_err());
    }

    #[test]
    fn timestamp_normalizes_offset_to_utc() {
        let a = Timestamp::parse("2026-09-29T11:30:00+03:00").unwrap();
        let b = Timestamp::parse("2026-09-29T08:30:00Z").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.to_string(), "2026-09-29T08:30:00Z");
        assert!(Timestamp::parse("2026-09-29 08:30").is_err());
    }

    #[test]
    fn evidence_round_trips_through_json() {
        let evidence = Evidence {
            source_id: SourceId::new("telegram.channel", "vanek_nikolaev").unwrap(),
            external_id: ExternalId::try_from("vanek_nikolaev/1".to_owned()).unwrap(),
            published_at: Timestamp::parse("2026-09-29T08:30:00+00:00").unwrap(),
            edited: false,
            text: Some("line one\nline two".to_owned()),
            media: vec![MediaRef {
                kind: MediaKind::Photo,
                url: None,
            }],
            forwarded_from: None,
            provenance: Provenance {
                url: "https://t.me/vanek_nikolaev/1".to_owned(),
                collector: "test/0".to_owned(),
            },
        };
        let json = serde_json::to_string(&evidence).unwrap();
        assert!(json.contains(r#""published_at":"2026-09-29T08:30:00Z""#));
        assert!(!json.contains("forwarded_from"));
        let back: Evidence = serde_json::from_str(&json).unwrap();
        assert_eq!(back, evidence);
    }
}
