// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Located, normalized observations and the geometry they carry.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ExternalId, SourceId, Timestamp};

/// Invalid geometry or observation value.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum GeometryError {
    /// Longitude must be finite and within `[-180, 180]`, latitude within `[-90, 90]`.
    #[error("coordinate out of range")]
    CoordinateOutOfRange,
    /// A polygon ring needs at least four positions and must be closed.
    #[error("polygon ring is not closed or too short")]
    InvalidRing,
    /// A cell list must be non-empty.
    #[error("empty cell list")]
    EmptyCells,
    /// A cell identifier must be 1–16 lowercase hexadecimal digits.
    #[error("invalid cell id")]
    InvalidCellId,
    /// An H3 resolution is an integer from 0 to 15.
    #[error("invalid cell resolution")]
    InvalidResolution,
    /// A time-to-live must be positive.
    #[error("ttl must be positive")]
    InvalidTtl,
}

/// A WGS84 position as `[longitude, latitude]` in degrees.
///
/// The array order matches GeoJSON. Coordinates are evidence about an event and never an
/// identity.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 2]", into = "[f64; 2]")]
pub struct Position {
    lon: f64,
    lat: f64,
}

impl Position {
    /// Validates a longitude and latitude in degrees.
    pub fn new(lon: f64, lat: f64) -> Result<Self, GeometryError> {
        let valid = lon.is_finite()
            && lat.is_finite()
            && (-180.0..=180.0).contains(&lon)
            && (-90.0..=90.0).contains(&lat);
        if valid {
            Ok(Self { lon, lat })
        } else {
            Err(GeometryError::CoordinateOutOfRange)
        }
    }

    /// Longitude in degrees.
    pub fn lon(&self) -> f64 {
        self.lon
    }

    /// Latitude in degrees.
    pub fn lat(&self) -> f64 {
        self.lat
    }
}

impl TryFrom<[f64; 2]> for Position {
    type Error = GeometryError;

    fn try_from([lon, lat]: [f64; 2]) -> Result<Self, Self::Error> {
        Self::new(lon, lat)
    }
}

impl From<Position> for [f64; 2] {
    fn from(value: Position) -> Self {
        [value.lon, value.lat]
    }
}

/// A closed linear ring: at least four positions, first equal to last.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<Position>", into = "Vec<Position>")]
pub struct Ring(Vec<Position>);

impl Ring {
    /// Positions in order, including the closing position.
    pub fn positions(&self) -> &[Position] {
        &self.0
    }
}

impl TryFrom<Vec<Position>> for Ring {
    type Error = GeometryError;

    fn try_from(value: Vec<Position>) -> Result<Self, Self::Error> {
        if value.len() >= 4 && value.first() == value.last() {
            Ok(Self(value))
        } else {
            Err(GeometryError::InvalidRing)
        }
    }
}

impl From<Ring> for Vec<Position> {
    fn from(value: Ring) -> Self {
        value.0
    }
}

/// H3 resolution, 0 (coarsest) to 15 (finest).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct CellResolution(u8);

impl CellResolution {
    /// The numeric resolution.
    pub fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for CellResolution {
    type Error = GeometryError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if value <= 15 {
            Ok(Self(value))
        } else {
            Err(GeometryError::InvalidResolution)
        }
    }
}

impl From<CellResolution> for u8 {
    fn from(value: CellResolution) -> Self {
        value.0
    }
}

/// An H3 cell index as lowercase hexadecimal, the binding-safe form used on the wire.
///
/// Only the shape is checked here; whether the value is a valid H3 cell is checked by
/// `prism-signal-geo`, which owns the grid.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CellId(String);

impl CellId {
    /// Canonical string form.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CellId {
    type Error = GeometryError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let valid = (1..=16).contains(&value.len())
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if valid {
            Ok(Self(value))
        } else {
            Err(GeometryError::InvalidCellId)
        }
    }
}

impl From<CellId> for String {
    fn from(value: CellId) -> Self {
        value.0
    }
}

impl fmt::Display for CellId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where an observation applies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Geometry {
    /// A single position.
    Point {
        /// The position.
        coordinates: Position,
    },
    /// An area: the first ring is the exterior, any further rings are holes.
    Polygon {
        /// Exterior ring followed by holes.
        rings: Vec<Ring>,
    },
    /// An explicit list of cells at one resolution.
    Cells {
        /// Resolution of every listed cell.
        resolution: CellResolution,
        /// Cell identifiers.
        cells: Vec<CellId>,
    },
}

impl Geometry {
    /// A point geometry.
    pub fn point(position: Position) -> Self {
        Self::Point {
            coordinates: position,
        }
    }

    /// Checks invariants that serde cannot express: a polygon has an exterior ring and a
    /// cell list is non-empty.
    pub fn validate(&self) -> Result<(), GeometryError> {
        match self {
            Self::Point { .. } => Ok(()),
            Self::Polygon { rings } if rings.is_empty() => Err(GeometryError::InvalidRing),
            Self::Polygon { .. } => Ok(()),
            Self::Cells { cells, .. } if cells.is_empty() => Err(GeometryError::EmptyCells),
            Self::Cells { .. } => Ok(()),
        }
    }
}

/// What an observation is about. Closed vocabulary: an unknown value fails to parse.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum HazardKind {
    /// Ballistic missile.
    #[serde(rename = "air.ballistic_missile")]
    BallisticMissile,
    /// Missile whose type the source does not state, or a cruise missile.
    #[serde(rename = "air.missile")]
    Missile,
    /// Piston-engine attack drone (Shahed type).
    #[serde(rename = "air.attack_drone")]
    AttackDrone,
    /// Jet-powered attack drone.
    #[serde(rename = "air.jet_drone")]
    JetDrone,
    /// Guided aerial bomb.
    #[serde(rename = "air.guided_bomb")]
    GuidedBomb,
}

impl HazardKind {
    /// Wire name, such as `air.attack_drone`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BallisticMissile => "air.ballistic_missile",
            Self::Missile => "air.missile",
            Self::AttackDrone => "air.attack_drone",
            Self::JetDrone => "air.jet_drone",
            Self::GuidedBomb => "air.guided_bomb",
        }
    }
}

/// Whether the source reports a threat or reports that one is over.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stance {
    /// The hazard is reported as present or approaching.
    Threat,
    /// The source reports the hazard as destroyed, gone, or over.
    Clear,
}

/// Source-declared confidence. Never a percentage: a source that does not declare one
/// leaves it absent.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceBand {
    /// The source flags the report as tentative.
    Low,
    /// The source states the report without qualification.
    Moderate,
    /// The source states the report as confirmed.
    High,
}

/// Validity window length in whole seconds, always positive.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct TtlSeconds(u32);

impl TtlSeconds {
    /// Validates a positive number of seconds.
    pub fn new(seconds: u32) -> Result<Self, GeometryError> {
        if seconds > 0 {
            Ok(Self(seconds))
        } else {
            Err(GeometryError::InvalidTtl)
        }
    }

    /// Seconds.
    pub fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for TtlSeconds {
    type Error = GeometryError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<TtlSeconds> for u32 {
    fn from(value: TtlSeconds) -> Self {
        value.0
    }
}

/// How an observation was derived from evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ObservationProvenance {
    /// The evidence item this observation was derived from.
    pub evidence_id: ExternalId,
    /// Public URL of that evidence.
    pub evidence_url: String,
    /// Normalizer name and version, such as `prism-signal-observe/0.1.0`.
    pub normalizer: String,
    /// The text fragments that produced the kind and the place, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched: Vec<String>,
    /// Identifier of the gazetteer entry that gave the geometry, when one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place_id: Option<String>,
    /// Display name of that place, so a consumer can name it without holding the gazetteer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place_name: Option<String>,
}

/// One normalized, located piece of evidence. Immutable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignalObservation {
    /// Declared source.
    pub source_id: SourceId,
    /// Hazard kind.
    pub kind: HazardKind,
    /// Threat or clear.
    pub stance: Stance,
    /// Number of objects the source reports, when it states one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    /// When the source reported it.
    pub observed_at: Timestamp,
    /// Where it applies.
    pub geometry: Geometry,
    /// How long after `observed_at` the observation stays valid.
    pub ttl: TtlSeconds,
    /// Source-declared confidence, if the source declares one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<ConfidenceBand>,
    /// Derivation record.
    pub provenance: ObservationProvenance,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_rejects_out_of_range_and_non_finite() {
        assert!(Position::new(30.5, 50.45).is_ok());
        assert_eq!(
            Position::new(181.0, 0.0),
            Err(GeometryError::CoordinateOutOfRange)
        );
        assert_eq!(
            Position::new(0.0, -90.5),
            Err(GeometryError::CoordinateOutOfRange)
        );
        assert!(Position::new(f64::NAN, 0.0).is_err());
    }

    #[test]
    fn ring_must_be_closed() {
        let p = |lon, lat| Position::new(lon, lat).unwrap();
        let open = vec![p(0.0, 0.0), p(1.0, 0.0), p(1.0, 1.0), p(0.0, 1.0)];
        assert_eq!(Ring::try_from(open), Err(GeometryError::InvalidRing));
        let closed = vec![p(0.0, 0.0), p(1.0, 0.0), p(1.0, 1.0), p(0.0, 0.0)];
        assert!(Ring::try_from(closed).is_ok());
    }

    #[test]
    fn cell_id_is_lowercase_hex() {
        assert!(CellId::try_from("871e64d4affffff".to_owned()).is_ok());
        assert!(CellId::try_from("871E64D4AFFFFFF".to_owned()).is_err());
        assert!(CellId::try_from(String::new()).is_err());
        assert!(CellId::try_from("0x871e".to_owned()).is_err());
    }

    #[test]
    fn geometry_uses_tagged_geojson_like_json() {
        let point = Geometry::point(Position::new(30.5234, 50.4501).unwrap());
        let json = serde_json::to_string(&point).unwrap();
        assert_eq!(json, r#"{"type":"point","coordinates":[30.5234,50.4501]}"#);
        assert_eq!(serde_json::from_str::<Geometry>(&json).unwrap(), point);

        let bad = r#"{"type":"point","coordinates":[200.0,0.0]}"#;
        assert!(serde_json::from_str::<Geometry>(bad).is_err());

        let cells = r#"{"type":"cells","resolution":7,"cells":[]}"#;
        let parsed: Geometry = serde_json::from_str(cells).unwrap();
        assert_eq!(parsed.validate(), Err(GeometryError::EmptyCells));
    }

    #[test]
    fn hazard_kind_is_a_closed_vocabulary() {
        let json = serde_json::to_string(&HazardKind::JetDrone).unwrap();
        assert_eq!(json, r#""air.jet_drone""#);
        assert_eq!(HazardKind::JetDrone.as_str(), "air.jet_drone");
        assert!(serde_json::from_str::<HazardKind>(r#""air.balloon""#).is_err());
    }

    #[test]
    fn ttl_must_be_positive() {
        assert!(TtlSeconds::new(0).is_err());
        assert!(serde_json::from_str::<TtlSeconds>("0").is_err());
        assert_eq!(TtlSeconds::new(600).unwrap().get(), 600);
    }
}
