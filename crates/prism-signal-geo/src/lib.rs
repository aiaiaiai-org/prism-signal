// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Deterministic H3 covers for Prism Signal geometry.
//!
//! [`cover`] turns a [`Geometry`] into a sorted, deduplicated [`CellSet`] at one resolution,
//! with an explicit bound: a cover larger than `max_cells` is a [`CoverError::TooLarge`]
//! failure, never a silent truncation. The resolution comes from the caller, normally from
//! a [`GridProfile`]; this crate hard-codes none.

use std::collections::BTreeSet;

use geo::{ChamberlainDuquetteArea, Coord, Haversine, Length, LineString, Polygon};
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
/// - A polygon gives the cells whose centers fall inside it. A polygon that contains no cell
///   center, such as one smaller than a cell, gives the cells it touches instead, so a valid
///   polygon never yields an empty set.
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

/// Mean Earth radius in km, the sphere the disc is drawn on.
const EARTH_RADIUS_KM: f64 = 6371.0088;

/// The most vertices a [`disc`] may have. A bound, so a request cannot ask for unbounded work.
pub const MAX_DISC_VERTICES: usize = 360;

/// A circle of `radius_km` around `center`, as a polygon of `vertices` corners on the sphere.
///
/// The corners lie exactly on the circle, so the polygon sits inside it and the gap at the
/// middle of an edge is `radius * (1 - cos(pi / vertices))`: 0.1 km for a 12 km disc drawn with
/// 48 corners. Equal input gives equal output.
///
/// This is how a place with a footprint, rather than a point, is given a geometry a cover can
/// use. It does not cross the antimeridian or enclose a pole.
pub fn disc(center: Position, radius_km: f64, vertices: usize) -> Result<Geometry, CoverError> {
    if !radius_km.is_finite() || radius_km <= 0.0 || !(8..=MAX_DISC_VERTICES).contains(&vertices) {
        return Err(CoverError::InvalidGeometry("disc"));
    }
    let angular = radius_km / EARTH_RADIUS_KM;
    let (lat0, lon0) = (center.lat().to_radians(), center.lon().to_radians());
    let mut corners = Vec::with_capacity(vertices + 1);
    for i in 0..vertices {
        let bearing = std::f64::consts::TAU * (i as f64) / (vertices as f64);
        let lat = (lat0.sin() * angular.cos() + lat0.cos() * angular.sin() * bearing.cos()).asin();
        let lon = lon0
            + (bearing.sin() * angular.sin() * lat0.cos())
                .atan2(angular.cos() - lat0.sin() * lat.sin());
        let corner = Position::new(lon.to_degrees(), lat.to_degrees())
            .map_err(|_| CoverError::InvalidGeometry("disc leaves the map"))?;
        corners.push(corner);
    }
    corners.push(corners[0]);
    let ring = Ring::try_from(corners).map_err(|_| CoverError::InvalidGeometry("disc"))?;
    Ok(Geometry::Polygon { rings: vec![ring] })
}

/// A cell set widened by `rings` rings of neighbours, sorted and deduplicated.
///
/// A person is placed in the cell that contains them, and a cover holds the cells whose centers
/// lie inside a zone, so someone just inside the zone's edge can stand in a cell whose center is
/// just outside it. One ring of neighbours closes that gap in the safe direction: it may alert
/// someone slightly outside the zone, and never miss someone inside it.
pub fn expand(set: &CellSet, rings: u32, max_cells: usize) -> Result<CellSet, CoverError> {
    let resolution = h3_resolution(set.resolution);
    let mut cells = BTreeSet::new();
    for id in &set.cells {
        let cell = parse_cell(id)?;
        if cell.resolution() != resolution {
            return Err(CoverError::InvalidGeometry("cell resolution mismatch"));
        }
        for neighbour in cell.grid_disk::<Vec<_>>(rings) {
            cells.insert(neighbour);
            if cells.len() > max_cells {
                return Err(CoverError::TooLarge { limit: max_cells });
            }
        }
    }
    Ok(CellSet {
        resolution: set.resolution,
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
    if estimated_cells(&polygon, resolution) > (max_cells as f64) * ESTIMATE_SLACK {
        return Err(CoverError::TooLarge { limit: max_cells });
    }

    let mut cells = tile(
        &polygon,
        resolution,
        ContainmentMode::ContainsCentroid,
        max_cells,
    )?;
    if cells.is_empty() {
        // No cell center lies inside: a polygon smaller than a cell, or a sliver. Fall back
        // to every cell the polygon touches, so the zone is never silently lost.
        cells = tile(&polygon, resolution, ContainmentMode::Covers, max_cells)?;
    }
    if cells.is_empty() {
        return Err(CoverError::InvalidGeometry("polygon covers no cell"));
    }
    Ok(cells)
}

/// The estimate may exceed the real count by this factor before a polygon is refused, since
/// it works from average cell size.
const ESTIMATE_SLACK: f64 = 2.;

/// A cheap estimate of the tiler's work, taken before tiling so an oversize polygon is
/// refused without computing its cells: the larger of the number of cells the area holds
/// and the number of cell edges along the boundary.
fn estimated_cells(polygon: &Polygon, resolution: Resolution) -> f64 {
    let area_km2 = polygon.chamberlain_duquette_unsigned_area() / 1e6;
    let boundary_km = std::iter::once(polygon.exterior())
        .chain(polygon.interiors())
        .map(|ring| Haversine.length(ring))
        .sum::<f64>()
        / 1e3;
    (area_km2 / resolution.area_km2()).max(boundary_km / resolution.edge_length_km())
}

fn tile(
    polygon: &Polygon,
    resolution: Resolution,
    mode: ContainmentMode,
    max_cells: usize,
) -> Result<BTreeSet<CellIndex>, CoverError> {
    let mut tiler = TilerBuilder::new(resolution).containment_mode(mode).build();
    tiler
        .add(polygon.clone())
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
    fn continent_sized_polygon_is_refused_before_tiling() {
        // Would be hundreds of millions of cells; it must fail fast, not after tiling them.
        let zone = square(30.0, 50.0, 20.0);
        let started = std::time::Instant::now();
        let error = cover(&zone, res(12), 1_000).unwrap_err();
        assert_eq!(error, CoverError::TooLarge { limit: 1_000 });
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }

    #[test]
    fn polygon_smaller_than_a_cell_still_covers_a_cell() {
        // About 200 m across at resolution 7, where cells are about 2.5 km across.
        let zone = square(30.52, 50.45, 0.001);
        let cells = cover(&zone, res(7), 100).unwrap();
        assert!(!cells.cells.is_empty());
        let point = cover(&Geometry::point(pos(30.52, 50.45)), res(7), 1).unwrap();
        assert!(cells.cells.contains(&point.cells[0]));
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

    // ---- disc and expand ----

    fn kyiv() -> Position {
        pos(30.5238, 50.4547)
    }

    fn km(a: Position, b: Position) -> f64 {
        let (p, q) = (a.lat().to_radians(), b.lat().to_radians());
        let d = ((q - p) / 2.0).sin().powi(2)
            + p.cos() * q.cos() * ((b.lon() - a.lon()).to_radians() / 2.0).sin().powi(2);
        2.0 * EARTH_RADIUS_KM * d.sqrt().asin()
    }

    #[test]
    fn a_disc_has_every_corner_on_the_circle_and_is_closed() {
        let Geometry::Polygon { rings } = disc(kyiv(), 12.0, 48).unwrap() else {
            panic!("a disc is a polygon");
        };
        let corners = rings[0].positions();
        assert_eq!(corners.len(), 49);
        assert_eq!(corners.first(), corners.last());
        for corner in corners {
            assert!((km(kyiv(), *corner) - 12.0).abs() < 0.001);
        }
    }

    #[test]
    fn a_disc_is_deterministic_and_bounded() {
        assert_eq!(
            disc(kyiv(), 8.0, 48).unwrap(),
            disc(kyiv(), 8.0, 48).unwrap()
        );
        for (radius, vertices) in [
            (0.0, 48),
            (-1.0, 48),
            (f64::NAN, 48),
            (5.0, 7),
            (5.0, MAX_DISC_VERTICES + 1),
        ] {
            assert_eq!(
                disc(kyiv(), radius, vertices).unwrap_err().code(),
                "invalid_geometry",
                "{radius} {vertices}"
            );
        }
    }

    #[test]
    fn a_disc_covers_the_cells_around_a_place_and_grows_with_its_radius() {
        let small = cover(&disc(kyiv(), 5.0, 48).unwrap(), res(6), 500).unwrap();
        let large = cover(&disc(kyiv(), 20.0, 48).unwrap(), res(6), 500).unwrap();
        assert!(
            !small.cells.is_empty(),
            "a small disc never yields an empty cover"
        );
        assert!(large.cells.len() > small.cells.len());
        assert!(large.cells.len() < 60, "{}", large.cells.len());
        // The cell that holds the center is inside every disc's cover.
        let centre = cover(&Geometry::point(kyiv()), res(6), 1).unwrap();
        assert!(large.cells.contains(&centre.cells[0]));
    }

    #[test]
    fn expanding_by_one_ring_only_adds_and_stays_sorted() {
        let base = cover(&disc(kyiv(), 8.0, 48).unwrap(), res(6), 500).unwrap();
        let wider = expand(&base, 1, 500).unwrap();
        assert!(wider.cells.len() > base.cells.len());
        assert!(base.cells.iter().all(|c| wider.cells.contains(c)));
        let mut sorted = wider.cells.clone();
        sorted.sort();
        assert_eq!(sorted, wider.cells);
        assert_eq!(expand(&base, 0, 500).unwrap(), base);
    }

    #[test]
    fn expanding_is_bounded() {
        let base = cover(&disc(kyiv(), 20.0, 48).unwrap(), res(6), 500).unwrap();
        assert_eq!(expand(&base, 1, 10).unwrap_err().code(), "cover_too_large");
    }

    #[test]
    fn anyone_inside_a_disc_stands_in_an_expanded_cell() {
        // People at 0.99 x radius in every direction: each one's own cell is in the cover once
        // it is expanded by a ring, though the cell's center may lie outside the disc.
        let radius = 12.0;
        let set = expand(
            &cover(&disc(kyiv(), radius, 48).unwrap(), res(6), 500).unwrap(),
            1,
            500,
        )
        .unwrap();
        for bearing in (0..360).step_by(3) {
            let b = f64::from(bearing).to_radians();
            let d = radius * 0.99;
            let lat = kyiv().lat() + d * b.cos() / 111.19;
            let lon = kyiv().lon() + d * b.sin() / (111.32 * kyiv().lat().to_radians().cos());
            let own = cover(&Geometry::point(pos(lon, lat)), res(6), 1).unwrap();
            assert!(set.cells.contains(&own.cells[0]), "bearing {bearing}");
        }
    }
}
