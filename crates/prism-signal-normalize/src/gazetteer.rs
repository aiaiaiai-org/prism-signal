// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Place names to positions.

use std::collections::HashMap;

use prism_signal_core::Position;
use thiserror::Error;

use crate::text::{Token, skeleton, stem, tokenize};

/// Invalid gazetteer data.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("gazetteer line {line}: {reason}")]
pub struct GazetteerError {
    /// One-based line number.
    pub line: usize,
    /// What is wrong.
    pub reason: &'static str,
}

/// How significant a place is, used only to break ties between places sharing a name.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PlaceRank {
    /// Any other populated place.
    Populated,
    /// Seat of a second-level division (district).
    DistrictSeat,
    /// Seat of a first-level division (oblast).
    RegionSeat,
    /// National capital.
    Capital,
}

impl PlaceRank {
    fn from_feature(code: &str) -> Self {
        match code {
            "PPLC" => Self::Capital,
            "PPLA" => Self::RegionSeat,
            "PPLA2" => Self::DistrictSeat,
            _ => Self::Populated,
        }
    }
}

/// One named place.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    /// Stable identifier, such as `geonames:703448`.
    pub id: String,
    /// Primary name.
    pub name: String,
    /// Representative position of the place.
    pub position: Position,
    /// Tie-break rank.
    pub rank: PlaceRank,
    /// Population, zero when unknown.
    pub population: u64,
}

/// Result of looking up the words at one position of a line.
#[derive(Debug, PartialEq)]
pub enum PlaceMatch<'a> {
    /// Exactly one place fits.
    Found(&'a Place),
    /// Several places share the name and none clearly dominates.
    Ambiguous(Vec<&'a Place>),
}

/// An in-memory gazetteer with inflection-tolerant lookup.
///
/// Data is tab-separated with one place per line and `#` comments:
///
/// ```text
/// id  name  latitude  longitude  feature_code  population  alias,alias,...
/// ```
#[derive(Debug, Default)]
pub struct Gazetteer {
    places: Vec<Place>,
    index: HashMap<Vec<String>, Vec<usize>>,
    longest: usize,
}

/// Minimum stem length for a one-word name; shorter words must match whole.
const SINGLE_WORD_STEM: usize = 3;
/// Minimum stem length for each word of a multi-word name.
const MULTI_WORD_STEM: usize = 3;
/// Names longer than this many words are not indexed.
const MAX_NAME_WORDS: usize = 3;
/// A place outranks a same-rank namesake only with this many times its population.
const DOMINANCE: u64 = 10;

/// Index key of a name: the first word by stem, later words by consonant skeleton.
fn key(tokens: &[Token]) -> Vec<String> {
    let min = if tokens.len() == 1 {
        SINGLE_WORD_STEM
    } else {
        MULTI_WORD_STEM
    };
    tokens
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let base = stem(&t.norm, min);
            if i == 0 { base } else { skeleton(&base) }
        })
        .collect()
}

/// Populated places of Ukraine generated from GeoNames (CC BY 4.0) by
/// `scripts/build_gazetteer.py`.
const UKRAINE: &str = include_str!("../data/gazetteer-ua.tsv");

impl Gazetteer {
    /// The bundled gazetteer of Ukraine: populated places of at least 1,000 people and all
    /// administrative seats, from GeoNames (<https://www.geonames.org>, CC BY 4.0).
    pub fn ukraine() -> Self {
        Self::parse(UKRAINE).expect("bundled gazetteer is valid")
    }

    /// Parses gazetteer data.
    pub fn parse(data: &str) -> Result<Self, GazetteerError> {
        let mut gazetteer = Self::default();
        for (number, line) in data.lines().enumerate() {
            let line_no = number + 1;
            let line = line.trim_end_matches('\r');
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let err = |reason| GazetteerError {
                line: line_no,
                reason,
            };
            let fields: Vec<&str> = line.split('\t').collect();
            let [id, name, lat, lon, feature, population, aliases] = fields[..] else {
                return Err(err("expected 7 tab-separated fields"));
            };
            if id.is_empty() || name.is_empty() {
                return Err(err("empty id or name"));
            }
            let lat = lat.parse::<f64>().map_err(|_| err("bad latitude"))?;
            let lon = lon.parse::<f64>().map_err(|_| err("bad longitude"))?;
            let position = Position::new(lon, lat).map_err(|_| err("coordinate out of range"))?;
            let population = if population.is_empty() {
                0
            } else {
                population.parse().map_err(|_| err("bad population"))?
            };
            let place = Place {
                id: id.to_owned(),
                name: name.to_owned(),
                position,
                rank: PlaceRank::from_feature(feature),
                population,
            };
            let names = std::iter::once(name).chain(aliases.split(',').map(str::trim));
            gazetteer.insert(place, names);
        }
        Ok(gazetteer)
    }

    fn insert<'a>(&mut self, place: Place, names: impl Iterator<Item = &'a str>) {
        let index = self.places.len();
        self.places.push(place);
        for name in names {
            let tokens = tokenize(name);
            if tokens.is_empty() || tokens.len() > MAX_NAME_WORDS {
                continue;
            }
            self.longest = self.longest.max(tokens.len());
            let entry = self.index.entry(key(&tokens)).or_default();
            if !entry.contains(&index) {
                entry.push(index);
            }
        }
    }

    /// Number of places.
    pub fn len(&self) -> usize {
        self.places.len()
    }

    /// Whether the gazetteer holds no places.
    pub fn is_empty(&self) -> bool {
        self.places.is_empty()
    }

    /// Looks up the longest name starting at `tokens[0]`. Returns the match and how many
    /// words it spans. Only a capitalized first word can start a name, which keeps common
    /// nouns that double as village names out.
    pub(crate) fn lookup(&self, tokens: &[Token]) -> Option<(PlaceMatch<'_>, usize)> {
        if !tokens.first()?.is_capitalized() {
            return None;
        }
        (1..=self.longest.min(tokens.len())).rev().find_map(|len| {
            let hits = self.index.get(&key(&tokens[..len]))?;
            let places: Vec<&Place> = hits.iter().map(|&i| &self.places[i]).collect();
            Some((resolve(places), len))
        })
    }
}

fn resolve(mut places: Vec<&Place>) -> PlaceMatch<'_> {
    places.sort_by(|a, b| (b.rank, b.population, &a.id).cmp(&(a.rank, a.population, &b.id)));
    match places[..] {
        [only] => PlaceMatch::Found(only),
        [first, second, ..]
            if first.rank > second.rank
                || (first.population > 0
                    && first.population >= second.population.saturating_mul(DOMINANCE)) =>
        {
            PlaceMatch::Found(first)
        }
        _ => PlaceMatch::Ambiguous(places),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic entries: coordinates are round numbers, not real positions.
    const DATA: &str = "\
# id\tname\tlat\tlon\tfeature\tpopulation\taliases
t:1\tAlpha City\t50\t30\tPPLC\t1000000\tАльфа,Альфоград
t:2\tBeta\t46\t30\tPPLA\t500000\tБетония
t:3\tGamma Small\t48\t31\tPPL\t900\tГамма
t:4\tGamma Big\t49\t32\tPPL\t20000\tГамма
t:5\tDelta One\t47\t33\tPPL\t1000\tДельта
t:6\tDelta Two\t47\t34\tPPL\t1200\tДельта
t:7\tWhite Church\t49.5\t30.5\tPPLA2\t200000\tБелая Церковь
";

    fn found<'a>(g: &'a Gazetteer, text: &str) -> Option<&'a str> {
        match g.lookup(&tokenize(text))? {
            (PlaceMatch::Found(place), _) => Some(&place.id),
            (PlaceMatch::Ambiguous(_), _) => None,
        }
    }

    #[test]
    fn finds_inflected_and_multi_word_names() {
        let g = Gazetteer::parse(DATA).unwrap();
        assert_eq!(g.len(), 7);
        assert_eq!(found(&g, "Альфой"), Some("t:1"));
        assert_eq!(found(&g, "Бетонии"), Some("t:2"));
        assert_eq!(found(&g, "Белой Церкви"), Some("t:7"));
        assert_eq!(
            found(&g, "бетония"),
            None,
            "lowercase words never start a name"
        );
    }

    #[test]
    fn dominant_namesake_wins_and_close_ones_are_ambiguous() {
        let g = Gazetteer::parse(DATA).unwrap();
        assert_eq!(found(&g, "Гамма"), Some("t:4"));
        let (m, _) = g.lookup(&tokenize("Дельта")).unwrap();
        assert!(matches!(m, PlaceMatch::Ambiguous(ref p) if p.len() == 2));
    }

    #[test]
    fn bundled_gazetteer_parses() {
        let g = Gazetteer::ukraine();
        assert!(g.len() > 3000, "{}", g.len());
        let kyiv = found_place(&g, "Киеву").expect("Kyiv resolves");
        assert_eq!(kyiv.id, "geonames:703448");
    }

    fn found_place<'a>(g: &'a Gazetteer, text: &str) -> Option<&'a Place> {
        match g.lookup(&tokenize(text))? {
            (PlaceMatch::Found(place), _) => Some(place),
            (PlaceMatch::Ambiguous(_), _) => None,
        }
    }

    #[test]
    fn rejects_malformed_lines() {
        let error = Gazetteer::parse("t:1\tX\t50\t30\tPPL\t1\n").unwrap_err();
        assert_eq!(error.line, 1);
        assert!(Gazetteer::parse("t:1\tX\t95\t30\tPPL\t1\t\n").is_err());
    }
}
