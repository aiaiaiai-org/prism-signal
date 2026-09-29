// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Rule-based normalization of channel evidence into located [`SignalObservation`]s.
//!
//! A [`Normalizer`] reads the text of one [`Evidence`] item line by line. A line becomes
//! observations when it names at least one hazard kind (see the lexicon) and at least one
//! place the [`Gazetteer`] resolves; it yields one observation per kind and place. Nothing is
//! guessed: a line with a hazard but no resolvable place, or with a place several gazetteer
//! entries share, is reported in [`Normalized::skipped`] instead.
//!
//! The rules are deterministic and hold no state. They are a heuristic reading of informal
//! text and never an authority: a model or a human may disagree, and fusion treats every
//! observation from this normalizer as uncalibrated evidence.

mod gazetteer;
mod lexicon;
mod text;

use prism_signal_core::{
    Evidence, Geometry, HazardKind, ObservationProvenance, SignalObservation, Stance, TtlSeconds,
};

pub use gazetteer::{Gazetteer, GazetteerError, Place, PlaceMatch, PlaceRank};

/// Normalizer name recorded in observation provenance.
pub const NORMALIZER: &str = concat!("prism-signal-normalize/", env!("CARGO_PKG_VERSION"));

/// How long observations stay valid. The defaults are provisional working values, not
/// measured ones; a deployment should set its own from evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NormalizeRules {
    /// Validity of a ballistic missile threat.
    pub ballistic_missile: TtlSeconds,
    /// Validity of an unspecified or cruise missile threat.
    pub missile: TtlSeconds,
    /// Validity of an attack drone threat.
    pub attack_drone: TtlSeconds,
    /// Validity of a jet drone threat.
    pub jet_drone: TtlSeconds,
    /// Validity of a guided bomb threat.
    pub guided_bomb: TtlSeconds,
    /// Validity of any clear observation.
    pub clear: TtlSeconds,
}

impl NormalizeRules {
    /// Validity of a threat observation of `kind`.
    pub fn threat_ttl(&self, kind: HazardKind) -> TtlSeconds {
        match kind {
            HazardKind::BallisticMissile => self.ballistic_missile,
            HazardKind::Missile => self.missile,
            HazardKind::AttackDrone => self.attack_drone,
            HazardKind::JetDrone => self.jet_drone,
            HazardKind::GuidedBomb => self.guided_bomb,
        }
    }
}

fn minutes(m: u32) -> TtlSeconds {
    TtlSeconds::new(m * 60).expect("positive constant")
}

impl Default for NormalizeRules {
    fn default() -> Self {
        Self {
            ballistic_missile: minutes(15),
            missile: minutes(30),
            attack_drone: minutes(60),
            jet_drone: minutes(30),
            guided_bomb: minutes(15),
            clear: minutes(10),
        }
    }
}

/// Why a line or item produced no observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SkipReason {
    /// The item was forwarded from another author; it is not this source's own report.
    Forwarded,
    /// The line names a hazard but no place the gazetteer resolves.
    Unlocated {
        /// Kinds the line names.
        kinds: Vec<HazardKind>,
    },
    /// A place name matches several gazetteer entries of similar weight.
    AmbiguousPlace {
        /// The name as written.
        name: String,
        /// Identifiers of the candidate places.
        candidates: Vec<String>,
    },
}

/// A line that yielded nothing, with the reason.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Skipped {
    /// One-based line number in the evidence text; `0` for the whole item.
    pub line: usize,
    /// Reason.
    pub reason: SkipReason,
}

/// Output of normalizing one evidence item.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Normalized {
    /// Observations in line order, then kind order, then place order.
    pub observations: Vec<SignalObservation>,
    /// Lines that named a hazard but yielded nothing.
    pub skipped: Vec<Skipped>,
}

/// Turns evidence into observations using a gazetteer and rules.
#[derive(Debug)]
pub struct Normalizer<'g> {
    gazetteer: &'g Gazetteer,
    rules: NormalizeRules,
}

struct PlaceMention<'g> {
    place: &'g Place,
    words: String,
}

impl<'g> Normalizer<'g> {
    /// Binds a gazetteer and rules.
    pub fn new(gazetteer: &'g Gazetteer, rules: NormalizeRules) -> Self {
        Self { gazetteer, rules }
    }

    /// Normalizes one evidence item.
    pub fn normalize(&self, evidence: &Evidence) -> Normalized {
        let mut out = Normalized::default();
        let Some(body) = evidence.text.as_deref() else {
            return out;
        };
        if evidence.forwarded_from.is_some() {
            out.skipped.push(Skipped {
                line: 0,
                reason: SkipReason::Forwarded,
            });
            return out;
        }
        for (index, line) in body.lines().enumerate() {
            self.normalize_line(evidence, index + 1, line, &mut out);
        }
        out
    }

    fn normalize_line(
        &self,
        evidence: &Evidence,
        line_no: usize,
        line: &str,
        out: &mut Normalized,
    ) {
        let tokens = text::tokenize(line);
        let kinds = lexicon::kinds(&tokens);
        if kinds.is_empty() {
            return;
        }
        let places = self.places(&tokens, line_no, out);
        if places.is_empty() {
            out.skipped.push(Skipped {
                line: line_no,
                reason: SkipReason::Unlocated {
                    kinds: kinds.iter().map(|k| k.kind).collect(),
                },
            });
            return;
        }
        let stance = if lexicon::is_clear(&tokens) {
            Stance::Clear
        } else {
            Stance::Threat
        };
        for mention in &kinds {
            let ttl = match stance {
                Stance::Threat => self.rules.threat_ttl(mention.kind),
                Stance::Clear => self.rules.clear,
            };
            for place in &places {
                let mut matched = mention.words.clone();
                matched.push(place.words.clone());
                out.observations.push(SignalObservation {
                    source_id: evidence.source_id.clone(),
                    kind: mention.kind,
                    stance,
                    count: mention.count,
                    observed_at: evidence.published_at,
                    geometry: Geometry::point(place.place.position),
                    ttl,
                    confidence: None,
                    provenance: ObservationProvenance {
                        evidence_id: evidence.external_id.clone(),
                        evidence_url: evidence.provenance.url.clone(),
                        normalizer: NORMALIZER.to_owned(),
                        matched,
                        place_id: Some(place.place.id.clone()),
                    },
                });
            }
        }
    }

    fn places(
        &self,
        tokens: &[text::Token],
        line_no: usize,
        out: &mut Normalized,
    ) -> Vec<PlaceMention<'g>> {
        let mut places: Vec<PlaceMention<'g>> = Vec::new();
        let mut i = 0;
        while i < tokens.len() {
            let Some((found, len)) = self.gazetteer.lookup(&tokens[i..]) else {
                i += 1;
                continue;
            };
            let words = tokens[i..i + len]
                .iter()
                .map(|t| t.raw.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            match found {
                PlaceMatch::Found(place) => {
                    if !places.iter().any(|p| p.place.id == place.id) {
                        places.push(PlaceMention { place, words });
                    }
                }
                PlaceMatch::Ambiguous(candidates) => out.skipped.push(Skipped {
                    line: line_no,
                    reason: SkipReason::AmbiguousPlace {
                        name: words,
                        candidates: candidates.iter().map(|p| p.id.clone()).collect(),
                    },
                }),
            }
            i += len;
        }
        places
    }
}
