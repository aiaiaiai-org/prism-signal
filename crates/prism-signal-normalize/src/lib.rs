// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Reads hazard reports out of channel text and places them on a gazetteer.
//!
//! [`Normalizer::read`] takes one [`Evidence`] item and returns zero or more [`Reading`]s: what
//! kind of threat a sentence reports, whether it reports it or calls it off, and which known
//! places it points at, each with a [`PlaceRole`]. The role is the point. `1 мопед подлетает к
//! Киеву со стороны Гостомеля` names two places, but only Kyiv is in danger; `пуски с курской`
//! names where a launch came from, not where it is going.
//!
//! Reading is deterministic and stateless: the same text and the same vocabulary always give the
//! same result, and nothing is remembered between posts. Deciding whether a reading becomes an
//! alert, for whom, and for how long belongs to later stages (fusion, the hub's issuer policy).
//! This crate never sees who is subscribed or where they are.
//!
//! Precision is never fabricated. A word the gazetteer does not know is reported in
//! [`Reading::unresolved`] instead of being guessed at, and a region such as `Николаевщина` is
//! never collapsed onto its capital.

mod gazetteer;
mod lexicon;
mod text;
mod words;

use std::collections::BTreeSet;

use prism_signal_core::{Evidence, ExternalId, SourceId, Timestamp};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use gazetteer::{Gazetteer, Place};

use lexicon::Lexicon;
use text::{Token, tokenize};

/// Longest post, in characters, that is read as a live report.
///
/// Live alerts are one or two short lines. Long posts are news items and daily summaries that
/// recount attacks already over, which is the noise this stage exists to drop.
pub const DEFAULT_MAX_TEXT_CHARS: usize = 500;

/// A vocabulary document could not be loaded.
#[derive(Debug, Error)]
pub enum LoadError {
    /// The document is not valid JSON of the expected shape.
    #[error("invalid {what} document: {source}")]
    Json {
        /// Which document.
        what: &'static str,
        /// Parser error.
        source: serde_json::Error,
    },
    /// The document declares a version this crate does not read.
    #[error("unsupported {what} version `{found}`")]
    Version {
        /// Which document.
        what: &'static str,
        /// Declared version.
        found: String,
    },
    /// A pattern is empty once folded.
    #[error("empty word pattern")]
    EmptyPattern,
}

/// What a report is about.
///
/// Ordered from most specific to least, which is also the order readings of one sentence are
/// emitted in.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HazardKind {
    /// Attack drone.
    Drone,
    /// Guided aerial bomb.
    GuidedBomb,
    /// Cruise missile.
    CruiseMissile,
    /// Ballistic or aeroballistic missile.
    BallisticMissile,
    /// A missile the text does not further specify.
    Missile,
}

/// Whether a reading reports a threat or calls it off.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// The sentence reports a threat.
    Threat,
    /// The sentence calls a threat off, as `минус по ракетам` does.
    Cleared,
}

/// How a sentence relates a place to the threat.
///
/// Ordered by strength: when a run of cue words disagrees, the strongest role wins, so an
/// ambiguous `курсом на/через Авангард` is treated as a target rather than a pass-by.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceRole {
    /// Named without a cue that ties it to the threat, or as context only.
    Mention,
    /// Where the threat came from: `с Херсона`, `со стороны Гостомеля`.
    Origin,
    /// Somewhere the threat is passing or near: `через Бровары`, `южнее Каменского`.
    Via,
    /// Where the threat is heading or hovering: `на Киев`, `над Днепром`, `в сторону Фастова`.
    Target,
}

/// A known place a sentence points at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaceMention {
    /// Gazetteer identifier.
    pub place_id: String,
    /// Display name in Ukrainian.
    pub name: String,
    /// Latitude in degrees.
    pub lat: f64,
    /// Longitude in degrees.
    pub lon: f64,
    /// Radius in kilometres around the centre that the place stands for.
    pub reach_km: u32,
    /// How the sentence relates the place to the threat.
    pub role: PlaceRole,
}

/// One hazard reading of one sentence of a post.
///
/// A post with several sentences or several kinds gives several readings. A reading with no
/// `kind` is a bare all-clear such as `больше пусков не было`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    /// Source of the post.
    pub source_id: SourceId,
    /// Post identifier inside the source.
    pub external_id: ExternalId,
    /// When the source published the post.
    pub published_at: Timestamp,
    /// Public URL of the post, for linking back to it.
    pub url: String,
    /// The post was forwarded from elsewhere rather than written by the source.
    pub forwarded: bool,
    /// What the sentence is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<HazardKind>,
    /// The sentence names no kind of its own; it was taken from the nearest earlier sentence of
    /// the same post, as in `1 КАБ ... ⏎ эти летят на Кривой Рог`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub kind_inherited: bool,
    /// Whether the sentence reports or calls off the threat.
    pub phase: Phase,
    /// Known places the sentence names, with roles, in reading order.
    pub places: Vec<PlaceMention>,
    /// Capitalised words that follow a target or via cue but are not in the gazetteer, as
    /// written. Candidates for extending the gazetteer; never guessed at.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved: Vec<String>,
}

impl Reading {
    /// The places the threat is at or near: [`PlaceRole::Target`] and [`PlaceRole::Via`].
    pub fn affected(&self) -> impl Iterator<Item = &PlaceMention> {
        self.places
            .iter()
            .filter(|place| matches!(place.role, PlaceRole::Target | PlaceRole::Via))
    }

    /// Whether this reading could move someone to act: it reports a threat of a known kind and
    /// says where.
    ///
    /// This is a property of the text, not a decision. Whether to alert anyone remains a hub
    /// policy applied to fused assessments.
    pub fn actionable(&self) -> bool {
        self.phase == Phase::Threat && self.kind.is_some() && self.affected().next().is_some()
    }
}

/// Reads hazard reports from post text.
#[derive(Debug)]
pub struct Normalizer {
    gazetteer: Gazetteer,
    lexicon: Lexicon,
    max_text_chars: usize,
}

impl Normalizer {
    /// A normalizer with the vocabulary shipped in this crate.
    pub fn embedded() -> Result<Self, LoadError> {
        Ok(Self {
            gazetteer: Gazetteer::embedded()?,
            lexicon: Lexicon::embedded()?,
            max_text_chars: DEFAULT_MAX_TEXT_CHARS,
        })
    }

    /// A normalizer over a caller-supplied gazetteer and a `lexicon.v1` document.
    ///
    /// For vocabulary that differs from the shipped one, such as a test of a rule the shipped
    /// data does not use.
    pub fn from_vocabulary(gazetteer: Gazetteer, lexicon_json: &str) -> Result<Self, LoadError> {
        Ok(Self {
            gazetteer,
            lexicon: Lexicon::from_json(lexicon_json)?,
            max_text_chars: DEFAULT_MAX_TEXT_CHARS,
        })
    }

    /// A normalizer over a caller-supplied gazetteer and the shipped lexicon.
    pub fn with_gazetteer(gazetteer: Gazetteer) -> Result<Self, LoadError> {
        Ok(Self {
            gazetteer,
            lexicon: Lexicon::embedded()?,
            max_text_chars: DEFAULT_MAX_TEXT_CHARS,
        })
    }

    /// Sets the longest post that is read; longer posts yield no readings.
    #[must_use]
    pub fn max_text_chars(mut self, max: usize) -> Self {
        self.max_text_chars = max;
        self
    }

    /// The gazetteer in use.
    pub fn gazetteer(&self) -> &Gazetteer {
        &self.gazetteer
    }

    /// Reads one post. Media-only posts, posts longer than the limit, and posts with nothing
    /// about a hazard give no readings.
    pub fn read(&self, evidence: &Evidence) -> Vec<Reading> {
        let Some(text) = evidence.text.as_deref() else {
            return Vec::new();
        };
        if text.chars().count() > self.max_text_chars {
            return Vec::new();
        }
        let tokens = tokenize(text);
        let mut clauses = self.clauses(&tokens);
        carry_kinds_forward(&mut clauses);
        clauses
            .into_iter()
            .flat_map(|clause| clause.into_readings(evidence))
            .collect()
    }

    fn clauses(&self, tokens: &[Token]) -> Vec<Clause> {
        // What each word turned out to be, so a later word can inherit from an earlier one.
        let mut role_at: Vec<Option<PlaceRole>> = vec![None; tokens.len()];
        let mut done = Vec::new();
        let mut clause = Clause::default();
        let mut words_in_clause = 0_usize;
        let mut i = 0;
        while i < tokens.len() {
            let token = &tokens[i];
            if token.barrier_before && i > 0 {
                done.push(std::mem::take(&mut clause));
            }
            if token.barrier_before {
                words_in_clause = 0;
                clause.leads_with_count = token.folded.chars().all(|c| c.is_ascii_digit());
            }
            // The opening two words, so `все остальные летят на Днепр` refers back like
            // `остальные летят на Днепр` does.
            if words_in_clause < 2 && self.lexicon.is_continuation_lead(&token.folded) {
                clause.leads_with_continuation = true;
            }
            words_in_clause += 1;
            if self.lexicon.is_continuation_mark(&token.folded) {
                clause.marked_continuation = true;
            }
            let context = self.context(tokens, &role_at, i);
            if self.lexicon.starts_cleared_phrase(tokens, i) {
                clause.cleared_phrase = true;
            }

            if let Some((place, len)) = self.gazetteer.match_at(tokens, i) {
                let role = context.unwrap_or(PlaceRole::Mention);
                clause.mention(place, role);
                role_at[i..i + len].fill(Some(role));
                i += len;
                continue;
            }
            if let Some(kind) = self.lexicon.kind(&token.folded) {
                clause.kinds.insert(kind);
            } else if self.lexicon.is_cleared_at(tokens, i) {
                clause.cleared_word = true;
            } else if matches!(context, Some(PlaceRole::Target | PlaceRole::Via))
                && self.lexicon.cue(&token.folded).is_none()
                && !self.lexicon.is_conjunction(&token.folded)
                && token.is_capitalized()
                && token.folded.chars().count() >= 4
            {
                clause.unresolve(&token.original);
                role_at[i] = context;
            }
            i += 1;
        }
        done.push(clause);
        done
    }

    /// The role the word at `at` would take if it named a place.
    ///
    /// A run of cue words directly before it decides, filler words such as `центром` in `над
    /// центром Николаева` not breaking the run. Failing that, a word in a list
    /// (`Киеву/Ирпеню, Буче`) inherits the role of the item before it. Anything else is no
    /// context at all, so a place named in passing stays a plain mention.
    fn context(
        &self,
        tokens: &[Token],
        role_at: &[Option<PlaceRole>],
        at: usize,
    ) -> Option<PlaceRole> {
        let mut run: Option<PlaceRole> = None;
        let mut j = at;
        while j > 0 && !tokens[j].barrier_before {
            let word = &tokens[j - 1].folded;
            match self.lexicon.cue(word) {
                Some(role) => run = run.max(Some(role)),
                None if self.lexicon.is_filler(word) => {}
                None => break,
            }
            j -= 1;
        }
        if run.is_some() {
            return run;
        }
        let mut j = at;
        while j > 0 && !tokens[j].barrier_before {
            j -= 1;
            if self.lexicon.is_conjunction(&tokens[j].folded) {
                continue;
            }
            return role_at[j];
        }
        None
    }
}

/// What one sentence said, before it is turned into readings.
#[derive(Default)]
struct Clause {
    kinds: BTreeSet<HazardKind>,
    /// A strong all-clear word (`минус`, `отбой`) was seen.
    cleared_word: bool,
    /// A weaker all-clear phrase (`больше не`) was seen.
    cleared_phrase: bool,
    /// The kinds were carried over from an earlier sentence.
    inherited: bool,
    /// The sentence opens with a number, as an item of a list does.
    leads_with_count: bool,
    /// The sentence opens with a word that refers back (`эти`, `ещё`).
    leads_with_continuation: bool,
    /// The sentence contains the channel's noise idiom (`громко`).
    marked_continuation: bool,
    places: Vec<PlaceMention>,
    unresolved: Vec<String>,
}

/// Kinds a later sentence may take, and where they came from.
struct Carried {
    kinds: BTreeSet<HazardKind>,
    /// The sentence they came from names no place, so it reads as the header of a list
    /// (`общая по мопедам:`).
    from_header: bool,
}

/// Lets a sentence with a place but no kind of its own take the kind of the nearest earlier
/// sentence of the same post that reported a threat, but only on an explicit signal that it
/// continues that threat.
///
/// Two signals count:
///
/// - the sentence refers back: it opens (in its first two words) with `эти`, `остальные`, `ещё` and the like, or carries
///   the channel's noise idiom (`может быть громко в Николаеве`);
/// - the sentence is an item of a list: the earlier sentence was a header with no place of its
///   own (`общая по мопедам:`) and this one opens with a number (`1 под Киевом`).
///
/// A place with a cue is not enough. `1 мопед на Киев ⏎ ПВО в Киеве работает` must not turn the
/// second sentence into a drone report. A sentence that calls a threat off ends the carry-over.
fn carry_kinds_forward(clauses: &mut [Clause]) {
    let mut carried: Option<Carried> = None;
    for clause in clauses {
        if !clause.kinds.is_empty() {
            carried = (!clause.cleared()).then(|| Carried {
                kinds: clause.kinds.clone(),
                from_header: clause.places.is_empty(),
            });
        } else if !clause.cleared() && clause.affects_a_place() {
            if let Some(source) = &carried {
                let refers_back = clause.leads_with_continuation || clause.marked_continuation;
                let list_item = source.from_header && clause.leads_with_count;
                if refers_back || list_item {
                    clause.kinds = source.kinds.clone();
                    clause.inherited = true;
                }
            }
        }
    }
}

impl Clause {
    fn cleared(&self) -> bool {
        self.cleared_word || self.cleared_phrase
    }

    fn affects_a_place(&self) -> bool {
        self.places
            .iter()
            .any(|place| matches!(place.role, PlaceRole::Target | PlaceRole::Via))
    }

    fn mention(&mut self, place: &Place, role: PlaceRole) {
        if let Some(existing) = self.places.iter_mut().find(|m| m.place_id == place.id) {
            existing.role = existing.role.max(role);
            return;
        }
        self.places.push(PlaceMention {
            place_id: place.id.clone(),
            name: place.name.clone(),
            lat: place.lat,
            lon: place.lon,
            reach_km: place.reach_km,
            role,
        });
    }

    fn unresolve(&mut self, word: &str) {
        if !self.unresolved.iter().any(|seen| seen == word) {
            self.unresolved.push(word.to_owned());
        }
    }

    fn into_readings(mut self, evidence: &Evidence) -> Vec<Reading> {
        // A generic `ракета` next to `баллистика` is the same threat named twice.
        if self.kinds.contains(&HazardKind::BallisticMissile)
            || self.kinds.contains(&HazardKind::CruiseMissile)
        {
            self.kinds.remove(&HazardKind::Missile);
        }
        let phase = if self.cleared() {
            Phase::Cleared
        } else {
            Phase::Threat
        };
        let kinds: Vec<Option<HazardKind>> = if self.kinds.is_empty() {
            // Without a kind only a bare all-clear word says anything. A phrase such as `больше
            // не` turns up in ordinary speech and must not clear threats by itself.
            if self.cleared_word {
                vec![None]
            } else {
                Vec::new()
            }
        } else {
            self.kinds.iter().copied().map(Some).collect()
        };
        kinds
            .into_iter()
            .map(|kind| Reading {
                source_id: evidence.source_id.clone(),
                external_id: evidence.external_id.clone(),
                published_at: evidence.published_at,
                url: evidence.provenance.url.clone(),
                forwarded: evidence.forwarded_from.is_some(),
                kind,
                kind_inherited: self.inherited,
                phase,
                places: self.places.clone(),
                unresolved: self.unresolved.clone(),
            })
            .collect()
    }
}
