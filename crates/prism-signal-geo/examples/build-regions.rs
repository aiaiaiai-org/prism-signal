// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Builds `crates/prism-signal-bridge/data/regions.v1.json` from the OCHA COD-AB first-level
//! boundaries of Ukraine (`ukr_admin1.geojson`, CC BY-IGO, source: State Scientific Production
//! Enterprise "Kartographia"), from <https://data.humdata.org/dataset/cod-ab-ukr>.
//!
//! ```text
//! cargo run -p prism-signal-geo --example build-regions -- ukr_admin1.geojson > crates/prism-signal-bridge/data/regions.v1.json
//! ```
//!
//! Each region becomes the resolution 6 cells whose centers lie inside any of its parts, holes
//! left out (Kyiv city is not Kyiv oblast; Kotsiubynske is), compacted, with a center and a
//! reach for readers that need a point. The output is deterministic.

use std::collections::BTreeSet;
use std::io::Write;

use h3o::{CellIndex, LatLng};
use prism_signal_core::{CellResolution, Geometry, Position, Ring};
use prism_signal_geo::{cover, parse_cell};
use serde_json::{Value, json};

const RESOLUTION: u8 = 6;
const MAX_CELLS: usize = 200_000;

fn ring(coordinates: &Value) -> Ring {
    let positions: Vec<Position> = coordinates
        .as_array()
        .expect("a ring is a list")
        .iter()
        .map(|point| {
            let lon = point[0].as_f64().expect("lon");
            let lat = point[1].as_f64().expect("lat");
            Position::new(lon, lat).expect("a position on the map")
        })
        .collect();
    Ring::try_from(positions).expect("a closed ring")
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: build-regions <ukr_admin1.geojson>");
    let text = std::fs::read_to_string(&path).expect("readable boundaries");
    let doc: Value = serde_json::from_str(&text).expect("GeoJSON");
    let resolution = CellResolution::try_from(RESOLUTION).expect("valid resolution");
    let mut regions = Vec::new();
    for feature in doc["features"].as_array().expect("features") {
        let properties = &feature["properties"];
        let code = properties["adm1_pcode"].as_str().expect("pcode").to_owned();
        let name = properties["adm1_name1"]
            .as_str()
            .expect("Ukrainian name")
            .to_owned();
        let geometry = &feature["geometry"];
        let parts: Vec<&Value> = match geometry["type"].as_str() {
            Some("Polygon") => vec![&geometry["coordinates"]],
            Some("MultiPolygon") => geometry["coordinates"]
                .as_array()
                .expect("parts")
                .iter()
                .collect(),
            other => panic!("{code}: unexpected geometry {other:?}"),
        };
        let mut cells: BTreeSet<CellIndex> = BTreeSet::new();
        for part in parts {
            let rings = part.as_array().expect("rings").iter().map(ring).collect();
            let set = cover(&Geometry::Polygon { rings }, resolution, MAX_CELLS).expect("cover");
            for id in &set.cells {
                cells.insert(parse_cell(id).expect("a cell"));
            }
        }
        let n = cells.len() as f64;
        let area: f64 = cells.iter().map(|cell| cell.area_km2()).sum();
        let (lat, lon) = cells.iter().fold((0.0, 0.0), |(lat, lon), cell| {
            let center = LatLng::from(*cell);
            (lat + center.lat(), lon + center.lng())
        });
        let mut packed: Vec<CellIndex> = cells.iter().copied().collect();
        CellIndex::compact(&mut packed).expect("cells of one resolution");
        let compacted: BTreeSet<String> = packed.iter().map(ToString::to_string).collect();
        eprintln!(
            "{code} {name:<28} {:>6} cells, {:>5} compacted, {area:>8.0} km2",
            cells.len(),
            compacted.len()
        );
        regions.push(json!({
            "id": format!("ua-adm1:{code}"),
            "name": name,
            "lat": (lat / n * 10_000.0).round() / 10_000.0,
            "lon": (lon / n * 10_000.0).round() / 10_000.0,
            "reach_km": (area / std::f64::consts::PI).sqrt().round(),
            "resolution": RESOLUTION,
            "cells": compacted,
        }));
    }
    regions.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    let document = json!({
        "version": "regions.v1",
        "attribution": "Boundaries from OCHA COD-AB Ukraine (https://data.humdata.org/dataset/cod-ab-ukr), source: State Scientific Production Enterprise \"Kartographia\", licensed CC BY-IGO 3.0. Covered with H3 resolution 6 cells by aiaiaiai.",
        "regions": regions,
    });
    let mut out = std::io::stdout().lock();
    serde_json::to_writer(&mut out, &document).expect("written");
    writeln!(out).expect("written");
}
