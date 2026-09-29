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
    continuation: ContinuationDoc,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContinuationDoc {
    leads: Vec<String>,
    marks: Vec<String>,
    #[serde(default, rename = "note")]
    _note: Option<String>,
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
    /// Words that, directly before a listed word, void it (`до отбоя`).
    #[serde(default)]
    negated_by: Vec<String>,
    /// Words that, within two words after a listed word, void it (`отбоя пока нет`).
    #[serde(default)]
    negated_after: Vec<String>,
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
    cleared_negators: WordSet,
    cleared_negators_after: WordSet,
    cues: Vec<(PlaceRole, WordSet)>,
    conjunctions: WordSet,
    fillers: WordSet,
    continuation_leads: WordSet,
    continuation_marks: WordSet,
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
            cleared_negators: WordSet::of_forms(&doc.cleared.negated_by)?,
            cleared_negators_after: WordSet::of_forms(&doc.cleared.negated_after)?,
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
            continuation_leads: WordSet::of_forms(&doc.continuation.leads)?,
            continuation_marks: WordSet::new(&[], &doc.continuation.marks)?,
        })
    }

    /// The hazard kind a folded word names, if any.
    pub fn kind(&self, word: &str) -> Option<HazardKind> {
        self.kinds
            .iter()
            .find(|(_, set)| set.matches(word))
            .map(|(kind, _)| *kind)
    }

    /// Whether the word at `tokens[at]` says a reported threat is over.
    ///
    /// A negator directly before it (`до отбоя`) or within two words after it (`отбоя пока нет`)
    /// voids it: an all-clear that is still awaited is the opposite of an all-clear.
    pub fn is_cleared_at(&self, tokens: &[Token], at: usize) -> bool {
        let Some(token) = tokens.get(at) else {
            return false;
        };
        if !self.cleared.matches(&token.folded) {
            return false;
        }
        let negated_before = !token.barrier_before
            && at > 0
            && self.cleared_negators.matches(&tokens[at - 1].folded);
        let negated_after = tokens
            .iter()
            .skip(at + 1)
            .take(2)
            .take_while(|next| !next.barrier_before)
            .any(|next| self.cleared_negators_after.matches(&next.folded));
        !(negated_before || negated_after)
    }

    /// Whether a multi-word all-clear such as `больше не было` starts at `tokens[at]`.
    ///
    /// Phrases are weaker than the single words of [`Lexicon::is_cleared_at`], because `больше
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

    /// Whether a folded word, opening a sentence, says it refers back to the previous threat
    /// (`эти летят на Кривой Рог`, `ещё 1 подлетает к Киеву`).
    pub fn is_continuation_lead(&self, word: &str) -> bool {
        self.continuation_leads.matches(word)
    }

    /// Whether a folded word is the channel's idiom for expected noise, as `громко` in `может
    /// быть громко в Николаеве`. It refers to the threat just reported wherever it stands.
    pub fn is_continuation_mark(&self, word: &str) -> bool {
        self.continuation_marks.matches(word)
    }

    /// Whether a folded word joins two list items, as `и` in `Киеву и Одессе`.
    pub fn is_conjunction(&self, word: &str) -> bool {
        self.conjunctions.matches(word)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::tokenize;

    fn cleared_at(text: &str, at: usize) -> bool {
        Lexicon::embedded()
            .unwrap()
            .is_cleared_at(&tokenize(text), at)
    }

    #[test]
    fn an_all_clear_word_is_read_and_a_negator_voids_it() {
        assert!(cleared_at("минус по мопедам", 0));
        assert!(cleared_at("на сейчас минуса", 2));
        assert!(cleared_at("угроза пока отбой", 2));
        assert!(!cleared_at("актуальна до отбоя тревоги", 2));
        assert!(!cleared_at("актуальна без отбоя", 2));
        assert!(!cleared_at("пока нет отбоя", 2));
        assert!(!cleared_at("отбоя пока нет", 0));
        assert!(!cleared_at("відбою ще немає", 0));
        assert!(!cleared_at("актуальна до відбою", 2));
    }

    #[test]
    fn a_negator_in_another_sentence_does_not_void_it() {
        assert!(cleared_at("тревога до утра.\nотбой", 3));
    }

    #[test]
    fn interceptions_and_lost_tracks_are_not_all_clears() {
        for word in ["сбито", "сбития", "збито", "знищено", "не фиксируется"]
        {
            for i in 0..2 {
                assert!(!cleared_at(word, i), "{word}");
            }
        }
    }

    #[test]
    fn a_shipped_lexicon_has_no_all_clear_phrases() {
        let lexicon = Lexicon::embedded().unwrap();
        for text in [
            "больше не было",
            "ракета не фиксируется",
            "без дальнейшей фиксации",
        ] {
            let tokens = tokenize(text);
            assert!(
                !(0..tokens.len()).any(|i| lexicon.starts_cleared_phrase(&tokens, i)),
                "{text}"
            );
        }
    }
}
