// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! The place vocabulary: which words in running text name which located place.

use std::collections::HashSet;

use serde::Deserialize;

use crate::LoadError;
use crate::text::{Token, fold};
use crate::words::WordPattern;

/// A populated place a report can point at.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    /// Stable identifier, `geonames:<id>`.
    pub id: String,
    /// Display name in Ukrainian.
    pub name: String,
    /// Latitude in degrees, WGS84.
    pub lat: f64,
    /// Longitude in degrees, WGS84.
    pub lon: f64,
    /// Radius in kilometres around the centre within which a person counts as being there.
    pub reach_km: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GazetteerDoc {
    version: String,
    attribution: String,
    places: Vec<PlaceDoc>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlaceDoc {
    id: String,
    name: String,
    lat: f64,
    lon: f64,
    reach_km: u32,
    #[serde(rename = "match")]
    matching: MatchDoc,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatchDoc {
    #[serde(default)]
    stems: Vec<String>,
    #[serde(default)]
    forms: Vec<String>,
    #[serde(default, rename = "not")]
    excluded: Vec<String>,
    #[serde(default = "default_max_suffix")]
    max_suffix: usize,
    #[serde(default)]
    capitalized: bool,
}

fn default_max_suffix() -> usize {
    2
}

#[derive(Debug)]
struct Entry {
    place: Place,
    /// Each pattern is one or more consecutive words; a multi-word name such as `Кривий Ріг`
    /// has several words.
    patterns: Vec<Vec<WordPattern>>,
    /// Folded single words that must not match this place even though a stem does, such as
    /// `Николаевка` for `Николаев`.
    excluded: HashSet<String>,
    /// The name is also an ordinary word (`Маяки`, `Ровно`, `Изюм`), so it only counts when the
    /// first word is capitalised as a proper name is.
    capitalized: bool,
}

/// The set of places a normalizer can recognise.
#[derive(Debug)]
pub struct Gazetteer {
    attribution: String,
    entries: Vec<Entry>,
}

impl Gazetteer {
    /// The gazetteer shipped with this crate.
    pub fn embedded() -> Result<Self, LoadError> {
        Self::from_json(include_str!("../data/gazetteer.v1.json"))
    }

    /// Parses a `gazetteer.v1` document.
    pub fn from_json(json: &str) -> Result<Self, LoadError> {
        let doc: GazetteerDoc = serde_json::from_str(json).map_err(|source| LoadError::Json {
            what: "gazetteer",
            source,
        })?;
        if doc.version != "gazetteer.v1" {
            return Err(LoadError::Version {
                what: "gazetteer",
                found: doc.version,
            });
        }
        let mut entries = Vec::with_capacity(doc.places.len());
        for place in doc.places {
            let MatchDoc {
                stems,
                forms,
                excluded,
                max_suffix,
                capitalized,
            } = place.matching;
            let mut patterns = Vec::new();
            for stem in &stems {
                patterns.push(words(stem, |w| WordPattern::stem(w, max_suffix))?);
            }
            for form in &forms {
                patterns.push(words(form, WordPattern::exact)?);
            }
            entries.push(Entry {
                place: Place {
                    id: place.id,
                    name: place.name,
                    lat: place.lat,
                    lon: place.lon,
                    reach_km: place.reach_km,
                },
                patterns,
                excluded: excluded.iter().map(|word| fold(word)).collect(),
                capitalized,
            });
        }
        Ok(Self {
            attribution: doc.attribution,
            entries,
        })
    }

    /// Licence and provenance statement for the data this gazetteer was built from.
    pub fn attribution(&self) -> &str {
        &self.attribution
    }

    /// Number of places.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the gazetteer has no places.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterates the places in file order.
    pub fn places(&self) -> impl Iterator<Item = &Place> {
        self.entries.iter().map(|entry| &entry.place)
    }

    /// Finds the place named at `tokens[at]`, returning it with the number of words it spans.
    ///
    /// The longest match wins; among equal lengths the earlier entry wins, so the result is
    /// deterministic.
    pub(crate) fn match_at(&self, tokens: &[Token], at: usize) -> Option<(&Place, usize)> {
        let mut best: Option<(&Place, usize)> = None;
        for entry in &self.entries {
            for pattern in &entry.patterns {
                let len = pattern.len();
                if best.is_some_and(|(_, best_len)| best_len >= len) {
                    continue;
                }
                let Some(window) = tokens.get(at..at + len) else {
                    continue;
                };
                let contiguous = window.iter().skip(1).all(|token| !token.barrier_before);
                let words_match = window
                    .iter()
                    .zip(pattern)
                    .all(|(token, word)| word.matches(&token.folded));
                let vetoed = len == 1 && entry.excluded.contains(&window[0].folded);
                let unnamed = entry.capitalized && !window[0].is_capitalized();
                if contiguous && words_match && !vetoed && !unnamed {
                    best = Some((&entry.place, len));
                }
            }
        }
        best
    }
}

fn words(
    spelled: &str,
    build: impl Fn(&str) -> Result<WordPattern, LoadError>,
) -> Result<Vec<WordPattern>, LoadError> {
    let pattern: Vec<_> = spelled
        .split_whitespace()
        .map(build)
        .collect::<Result<_, _>>()?;
    if pattern.is_empty() {
        return Err(LoadError::EmptyPattern);
    }
    Ok(pattern)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::tokenize;

    fn find(text: &str) -> Option<(String, usize)> {
        let gazetteer = Gazetteer::embedded().unwrap();
        let tokens = tokenize(text);
        (0..tokens.len()).find_map(|i| {
            gazetteer
                .match_at(&tokens, i)
                .map(|(place, len)| (place.name.clone(), len))
        })
    }

    fn name(text: &str) -> Option<String> {
        find(text).map(|(name, _)| name)
    }

    #[test]
    fn embedded_gazetteer_loads_with_attribution() {
        let gazetteer = Gazetteer::embedded().unwrap();
        assert!(gazetteer.len() >= 80);
        assert!(gazetteer.attribution().contains("GeoNames"));
        assert!(gazetteer.places().all(|p| p.id.starts_with("geonames:")));
        assert!(gazetteer.places().all(|p| (44.0..53.0).contains(&p.lat)));
        assert!(gazetteer.places().all(|p| (22.0..41.0).contains(&p.lon)));
    }

    #[test]
    fn inflections_in_both_languages_resolve_to_one_place() {
        for text in ["Киев", "Киеву", "Києвом", "Київ", "у Києві", "над Киевом"]
        {
            assert_eq!(name(text).as_deref(), Some("Київ"), "{text}");
        }
        for text in ["Запорожье", "Запорожья", "Запоріжжя", "Запоріжжі"]
        {
            assert_eq!(name(text).as_deref(), Some("Запоріжжя"), "{text}");
        }
        assert_eq!(name("на Одессу").as_deref(), Some("Одеса"));
        assert_eq!(name("в Одесі").as_deref(), Some("Одеса"));
    }

    #[test]
    fn multi_word_and_hyphenated_names_match() {
        assert_eq!(find("на Кривой Рог"), Some(("Кривий Ріг".to_owned(), 2)));
        assert_eq!(find("до Кривого Рогу"), Some(("Кривий Ріг".to_owned(), 2)));
        assert_eq!(find("Белой Церкви"), Some(("Біла Церква".to_owned(), 2)));
        assert_eq!(name("к Каролино-Бугаза").as_deref(), Some("Кароліно-Бугаз"));
        assert_eq!(
            name("Ивано-Франковска").as_deref(),
            Some("Івано-Франківськ")
        );
    }

    #[test]
    fn a_name_split_by_a_sentence_break_does_not_match() {
        assert_eq!(name("Кривой. Рог"), None);
    }

    #[test]
    fn regions_and_homonyms_are_not_mistaken_for_cities() {
        for text in [
            "Николаевщина",
            "Одесская область",
            "Киевщине",
            "Каменск-Уральский",
            "Николаевка",
            "сумма",
            "кабель",
            "Первомайское",
        ] {
            assert_eq!(name(text), None, "{text}");
        }
    }

    #[test]
    fn exact_form_places_do_not_match_lookalikes() {
        assert_eq!(name("Сумы").as_deref(), Some("Суми"));
        assert_eq!(name("Сумах").as_deref(), Some("Суми"));
        assert_eq!(name("сумки"), None);
        assert_eq!(name("Бучу").as_deref(), Some("Буча"));
        assert_eq!(name("бучу"), None);
        assert_eq!(name("Бучный"), None);
    }

    #[test]
    fn names_that_are_also_words_need_a_capital_letter() {
        assert_eq!(name("на Маяки").as_deref(), Some("Маяки"));
        assert_eq!(name("на маяки"), None);
        assert_eq!(name("Изюма").as_deref(), Some("Ізюм"));
        assert_eq!(name("изюма"), None);
        assert_eq!(name("на Ровно").as_deref(), Some("Рівне"));
        assert_eq!(name("ровно 2 мопеда"), None);
        assert_eq!(name("авангард"), None);
    }

    #[test]
    fn malformed_documents_are_rejected() {
        assert!(matches!(
            Gazetteer::from_json("{"),
            Err(LoadError::Json { .. })
        ));
        assert!(matches!(
            Gazetteer::from_json(r#"{"version":"gazetteer.v9","attribution":"","places":[]}"#),
            Err(LoadError::Version { .. })
        ));
        let empty_pattern = r#"{"version":"gazetteer.v1","attribution":"","places":[
            {"id":"x","name":"X","lat":1,"lon":1,"reach_km":1,"match":{"stems":["'"]}}]}"#;
        assert!(matches!(
            Gazetteer::from_json(empty_pattern),
            Err(LoadError::EmptyPattern)
        ));
    }
}
