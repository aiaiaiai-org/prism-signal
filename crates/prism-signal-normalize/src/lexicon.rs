// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! The hazard vocabulary: what a report is about and how it points at a place.

use serde::Deserialize;

use crate::text::Token;
use crate::words::{WordPattern, WordSet};
use crate::{HazardKind, LoadError, PlaceRole};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LexiconDoc {
    version: String,
    kinds: KindsDoc,
    cleared: WordListDoc,
    cues: CuesDoc,
    conjunctions: Vec<String>,
    fillers: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KindsDoc {
    drone: WordListDoc,
    guided_bomb: WordListDoc,
    cruise_missile: WordListDoc,
    ballistic_missile: WordListDoc,
    missile: WordListDoc,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WordListDoc {
    #[serde(default)]
    stems: Vec<String>,
    #[serde(default)]
    forms: Vec<String>,
    #[serde(default)]
    phrases: Vec<String>,
    #[serde(default, rename = "note")]
    _note: Option<String>,
}

impl WordListDoc {
    fn set(&self) -> Result<WordSet, LoadError> {
        WordSet::new(&self.forms, &self.stems)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CuesDoc {
    target: Vec<String>,
    via: Vec<String>,
    origin: Vec<String>,
}

/// Compiled vocabulary for reading hazard reports.
#[derive(Debug)]
pub(crate) struct Lexicon {
    kinds: Vec<(HazardKind, WordSet)>,
    cleared: WordSet,
    cleared_phrases: Vec<Vec<WordPattern>>,
    cues: Vec<(PlaceRole, WordSet)>,
    conjunctions: WordSet,
    fillers: WordSet,
}

impl Lexicon {
    pub fn embedded() -> Result<Self, LoadError> {
        Self::from_json(include_str!("../data/lexicon.v1.json"))
    }

    pub fn from_json(json: &str) -> Result<Self, LoadError> {
        let doc: LexiconDoc = serde_json::from_str(json).map_err(|source| LoadError::Json {
            what: "lexicon",
            source,
        })?;
        if doc.version != "lexicon.v1" {
            return Err(LoadError::Version {
                what: "lexicon",
                found: doc.version,
            });
        }
        Ok(Self {
            kinds: vec![
                (HazardKind::Drone, doc.kinds.drone.set()?),
                (HazardKind::GuidedBomb, doc.kinds.guided_bomb.set()?),
                (HazardKind::CruiseMissile, doc.kinds.cruise_missile.set()?),
                (
                    HazardKind::BallisticMissile,
                    doc.kinds.ballistic_missile.set()?,
                ),
                (HazardKind::Missile, doc.kinds.missile.set()?),
            ],
            cleared: doc.cleared.set()?,
            cleared_phrases: doc
                .cleared
                .phrases
                .iter()
                .map(|phrase| {
                    let words: Vec<_> = phrase
                        .split_whitespace()
                        .map(WordPattern::exact)
                        .collect::<Result<_, _>>()?;
                    if words.is_empty() {
                        return Err(LoadError::EmptyPattern);
                    }
                    Ok(words)
                })
                .collect::<Result<_, _>>()?,
            cues: vec![
                (PlaceRole::Target, WordSet::of_forms(&doc.cues.target)?),
                (PlaceRole::Via, WordSet::of_forms(&doc.cues.via)?),
                (PlaceRole::Origin, WordSet::of_forms(&doc.cues.origin)?),
            ],
            conjunctions: WordSet::of_forms(&doc.conjunctions)?,
            fillers: WordSet::of_forms(&doc.fillers)?,
        })
    }

    /// The hazard kind a folded word names, if any.
    pub fn kind(&self, word: &str) -> Option<HazardKind> {
        self.kinds
            .iter()
            .find(|(_, set)| set.matches(word))
            .map(|(kind, _)| *kind)
    }

    /// Whether a folded word says a reported threat is over.
    pub fn is_cleared(&self, word: &str) -> bool {
        self.cleared.matches(word)
    }

    /// Whether a multi-word all-clear such as `больше не было` starts at `tokens[at]`.
    ///
    /// Phrases are weaker than the single words of [`Lexicon::is_cleared`], because `больше
    /// не` also occurs in ordinary speech. Callers must not let a phrase stand alone.
    pub fn starts_cleared_phrase(&self, tokens: &[Token], at: usize) -> bool {
        self.cleared_phrases.iter().any(|phrase| {
            tokens.get(at..at + phrase.len()).is_some_and(|window| {
                window.iter().skip(1).all(|token| !token.barrier_before)
                    && window
                        .iter()
                        .zip(phrase)
                        .all(|(token, word)| word.matches(&token.folded))
            })
        })
    }

    /// The strongest role a folded word gives to the place that follows it.
    pub fn cue(&self, word: &str) -> Option<PlaceRole> {
        self.cues
            .iter()
            .filter(|(_, set)| set.matches(word))
            .map(|(role, _)| *role)
            .max()
    }

    /// Whether a folded word may sit between a cue and a place without breaking the link, as
    /// `центром` does in `над центром Николаева`.
    pub fn is_filler(&self, word: &str) -> bool {
        self.fillers.matches(word)
    }

    /// Whether a folded word joins two list items, as `и` in `Киеву и Одессе`.
    pub fn is_conjunction(&self, word: &str) -> bool {
        self.conjunctions.matches(word)
    }
}
