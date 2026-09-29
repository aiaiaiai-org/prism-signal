// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Deterministic H3 covers for Prism Signal geometry.
//!
//! [`cover`] turns a [`Geometry`] into a sorted, deduplicated [`CellSet`] at one resolution,
//! with an explicit bound: a cover larger than `max_cells` is a [`CoverError::TooLarge`]
//! failure, never a silent truncation. The resolution comes from the caller, normally from
//! a [`GridProfile`]; this crate hard-codes none.

use std::collections::BTreeSet;

use geo::{Coord, LineString, Polygon};
use h3o::geom::{ContainmentMode, TilerBuilder};
use h3o::{CellIndex, LatLng, Resolution};
use prism_signal_core::{CellId, CellResolution, Geometry, Position, Ring};
use thiserror::Error;

/// Why a cover could not be produced.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CoverError {
    /// The geometry is structurally invalid for the grid.
    #[error("invalid geometry: {0}")]
    InvalidGeometry(&'static str),
    /// The cover would exceed the requested bound.
    #[error("cover exceeds {limit} cells")]
    TooLarge {
        /// The requested `max_cells`.
        limit: usize,
    },
}

impl CoverError {
    /// Stable failure code for the wire: `invalid_geometry` or `cover_too_large`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidGeometry(_) => "invalid_geometry",
            Self::TooLarge { .. } => "cover_too_large",
        }
    }
}

/// Resolutions a deployment uses, bound to the `0x1` contract that defines them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GridProfile {
    /// Grid used to fan out broadcasts.
    pub broadcast: CellResolution,
    /// Grid used for proximity and map aggregation.
    pub proximity: CellResolution,
}

impl GridProfile {
    /// The profile stated by the current `0x1` Proximity, Relay, and Broadcast document:
    /// broadcast at resolution 7, proximity at 8.
    ///
    /// A deployment passes its profile in configuration; this constructor only names the
    /// values that document gives today.
    pub fn zerox1_current() -> Self {
        let res = |r: u8| CellResolution::try_from(r).expect("static resolution is valid");
        Self {
            broadcast: res(7),
            proximity: res(8),
        }
    }
}

/// Cells at one resolution, sorted by index and deduplicated.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CellSet {
    /// Resolution of every cell.
    pub resolution: CellResolution,
    /// Cell identifiers in ascending index order.
    pub cells: Vec<CellId>,
}

/// Covers a geometry at `resolution` with at most `max_cells` cells.
///
/// - A point gives the one cell that contains it.
/// - A polygon gives the cells whose centers fall inside it; a polygon smaller than a cell
///   may give an empty set.
/// - An explicit cell list is re-expressed at `resolution`: finer cells map to their
///   parents, coarser cells expand to their children.
///
/// Equal input always gives equal output.
pub fn cover(
    geometry: &Geometry,
    resolution: CellResolution,
    max_cells: usize,
) -> Result<CellSet, CoverError> {
    geometry
        .validate()
        .map_err(|_| CoverError::InvalidGeometry("structure"))?;
    let target = h3_resolution(resolution);
    let cells = match geometry {
        Geometry::Point { coordinates } => BTreeSet::from([latlng(*coordinates)?.to_cell(target)]),
        Geometry::Polygon { rings } => cover_polygon(rings, target, max_cells)?,
        Geometry::Cells {
            resolution: declared,
            cells,
        } => recast_cells(cells, h3_resolution(*declared), target, max_cells)?,
    };
    if cells.len() > max_cells {
        return Err(CoverError::TooLarge { limit: max_cells });
    }
    Ok(CellSet {
        resolution,
        cells: cells.into_iter().map(cell_id).collect(),
    })
}

/// Parses a wire cell identifier into an H3 cell, rejecting values that are not cells.
pub fn parse_cell(id: &CellId) -> Result<CellIndex, CoverError> {
    id.as_str()
        .parse::<CellIndex>()
        .map_err(|_| CoverError::InvalidGeometry("not an h3 cell"))
}

/// Renders an H3 cell in the wire form.
pub fn cell_id(cell: CellIndex) -> CellId {
    CellId::try_from(format!("{cell:x}")).expect("an h3 index renders as lowercase hex")
}

fn h3_resolution(resolution: CellResolution) -> Resolution {
    Resolution::try_from(resolution.get()).expect("CellResolution is always 0..=15")
}

fn latlng(position: Position) -> Result<LatLng, CoverError> {
    LatLng::new(position.lat(), position.lon())
        .map_err(|_| CoverError::InvalidGeometry("coordinate"))
}

fn line_string(ring: &Ring) -> LineString {
    ring.positions()
        .iter()
        .map(|p| Coord {
            x: p.lon(),
            y: p.lat(),
        })
        .collect()
}

fn cover_polygon(
    rings: &[Ring],
    resolution: Resolution,
    max_cells: usize,
) -> Result<BTreeSet<CellIndex>, CoverError> {
    let (exterior, holes) = rings
        .split_first()
        .ok_or(CoverError::InvalidGeometry("polygon without exterior"))?;
    let polygon = Polygon::new(
        line_string(exterior),
        holes.iter().map(line_string).collect(),
    );
    let mut tiler = TilerBuilder::new(resolution)
        .containment_mode(ContainmentMode::ContainsCentroid)
        .build();
    tiler
        .add(polygon)
        .map_err(|_| CoverError::InvalidGeometry("polygon"))?;

    let mut cells = BTreeSet::new();
    for cell in tiler.into_coverage() {
        cells.insert(cell);
        if cells.len() > max_cells {
            return Err(CoverError::TooLarge { limit: max_cells });
        }
    }
    Ok(cells)
}

fn recast_cells(
    ids: &[CellId],
    declared: Resolution,
    target: Resolution,
    max_cells: usize,
) -> Result<BTreeSet<CellIndex>, CoverError> {
    let parsed = ids.iter().map(parse_cell).collect::<Result<Vec<_>, _>>()?;
    if parsed.iter().any(|cell| cell.resolution() != declared) {
        return Err(CoverError::InvalidGeometry("cell resolution mismatch"));
    }
    if target <= declared {
        return Ok(parsed
            .into_iter()
            .map(|cell| {
                cell.parent(target)
                    .expect("target is not finer than the cell")
            })
            .collect());
    }

    // Children of distinct cells never overlap, so the sum is exact for distinct input.
    let unique: BTreeSet<CellIndex> = parsed.into_iter().collect();
    let total: u64 = unique.iter().map(|cell| cell.children_count(target)).sum();
    if total > max_cells as u64 {
        return Err(CoverError::TooLarge { limit: max_cells });
    }
    Ok(unique
        .into_iter()
        .flat_map(|cell| cell.children(target))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res(r: u8) -> CellResolution {
        CellResolution::try_from(r).unwrap()
    }

    fn pos(lon: f64, lat: f64) -> Position {
        Position::new(lon, lat).unwrap()
    }

    fn square(lon: f64, lat: f64, half: f64) -> Geometry {
        let ring = vec![
            pos(lon - half, lat - half),
            pos(lon + half, lat - half),
            pos(lon + half, lat + half),
            pos(lon - half, lat + half),
            pos(lon - half, lat - half),
        ];
        Geometry::Polygon {
            rings: vec![Ring::try_from(ring).unwrap()],
        }
    }

    #[test]
    fn point_matches_h3_reference_cell() {
        // Reference value from the H3 documentation example (San Francisco, res 9).
        let cells = cover(
            &Geometry::point(pos(-122.418_307_5, 37.775_938_7)),
            res(9),
            1,
        )
        .unwrap();
        assert_eq!(cells.cells[0].as_str(), "8928308280fffff");
    }

    #[test]
    fn polygon_cover_is_sorted_deduplicated_and_repeatable() {
        let zone = square(30.52, 50.45, 0.05);
        let a = cover(&zone, res(7), 1_000).unwrap();
        let b = cover(&zone, res(7), 1_000).unwrap();
        assert_eq!(a, b);
        assert!(!a.cells.is_empty());
        let indexes: Vec<u64> = a
            .cells
            .iter()
            .map(|c| u64::from(parse_cell(c).unwrap()))
            .collect();
        assert!(indexes.windows(2).all(|w| w[0] < w[1]));
        assert!(
            a.cells
                .iter()
                .all(|c| parse_cell(c).unwrap().resolution() == Resolution::Seven)
        );
    }

    #[test]
    fn oversize_cover_fails_instead_of_truncating() {
        let zone = square(30.52, 50.45, 0.5);
        let error = cover(&zone, res(9), 10).unwrap_err();
        assert_eq!(error, CoverError::TooLarge { limit: 10 });
        assert_eq!(error.code(), "cover_too_large");
    }

    #[test]
    fn cells_are_recast_to_the_target_resolution() {
        let fine = cover(&Geometry::point(pos(30.52, 50.45)), res(9), 1).unwrap();
        let as_cells = Geometry::Cells {
            resolution: res(9),
            cells: fine.cells.clone(),
        };
        let parent = cover(&as_cells, res(7), 1).unwrap();
        let direct = cover(&Geometry::point(pos(30.52, 50.45)), res(7), 1).unwrap();
        assert_eq!(parent, direct);

        let children = cover(
            &Geometry::Cells {
                resolution: res(7),
                cells: direct.cells.clone(),
            },
            res(8),
            7,
        )
        .unwrap();
        assert_eq!(children.cells.len(), 7);
        assert!(
            cover(
                &Geometry::Cells {
                    resolution: res(7),
                    cells: direct.cells,
                },
                res(9),
                48,
            )
            .is_err()
        );
    }

    #[test]
    fn cells_must_be_real_and_match_their_declared_resolution() {
        let not_a_cell = Geometry::Cells {
            resolution: res(7),
            cells: vec![CellId::try_from("1".to_owned()).unwrap()],
        };
        assert_eq!(
            cover(&not_a_cell, res(7), 10).unwrap_err().code(),
            "invalid_geometry"
        );

        let res9 = cover(&Geometry::point(pos(30.52, 50.45)), res(9), 1).unwrap();
        let mislabeled = Geometry::Cells {
            resolution: res(7),
            cells: res9.cells,
        };
        assert!(cover(&mislabeled, res(7), 10).is_err());
    }

    #[test]
    fn profile_names_the_zerox1_resolutions() {
        let profile = GridProfile::zerox1_current();
        assert_eq!(profile.broadcast.get(), 7);
        assert_eq!(profile.proximity.get(), 8);
    }
}
