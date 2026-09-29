// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Hazard vocabulary of Russian- and Ukrainian-language air-alert channels.

use std::collections::BTreeSet;

use prism_signal_core::{HazardKind, Stance};

use crate::text::Token;

/// Word prefixes naming each kind. A word matches when its normalized form starts with a
/// prefix.
const KIND_PREFIXES: &[(HazardKind, &[&str])] = &[
    (
        HazardKind::BallisticMissile,
        &[
            "баллист",
            "балліст",
            "баліст",
            "искандер",
            "іскандер",
            "кинжал",
            "кинджал",
        ],
    ),
    (
        HazardKind::Missile,
        &[
            "ракет",
            "крылат",
            "крилат",
            "калибр",
            "калібр",
            "циркон",
            "оникс",
            "онікс",
            "х-101",
            "х-22",
            "х-59",
            "х-69",
        ],
    ),
    (
        HazardKind::AttackDrone,
        &[
            "шахед",
            "шахід",
            "мопед",
            "бандерол",
            "бпла",
            "дрон",
            "герань",
            "герані",
        ],
    ),
];

/// Whole words naming guided bombs. Matched exactly, since `каб` begins many other words.
const GUIDED_BOMB_WORDS: &[&str] = &[
    "каб",
    "каба",
    "кабы",
    "кабов",
    "каби",
    "кабів",
    "умпб",
    "умпк",
    "фаб",
    "фабы",
    "фаби",
];

/// Word prefix that turns an attack drone into a jet drone when it appears in the same clause.
const JET_PREFIXES: &[&str] = &["реактивн"];

/// Word prefixes reporting that a hazard is over: destroyed, shot down, or all-clear.
const CLEAR_PREFIXES: &[&str] = &[
    "минус",
    "мінус",
    "отбой",
    "відбій",
    "сбит",
    "збит",
    "уничтож",
    "знищ",
];

/// Word prefixes naming an administrative area: a place name followed by one is the area,
/// not the town.
const ADMIN_AREA_PREFIXES: &[&str] = &["район", "област", "облас", "обл", "громад"];

/// Words that count objects must stand this close before the kind word.
const COUNT_LOOKBACK: usize = 2;

/// One hazard kind named in a line with one stance, and the words that named it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct KindMention {
    pub(crate) kind: HazardKind,
    pub(crate) stance: Stance,
    /// Clauses in which this kind is named with this stance.
    pub(crate) clauses: BTreeSet<usize>,
    pub(crate) count: Option<u32>,
    pub(crate) words: Vec<String>,
}

fn has_prefix(token: &Token, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| token.norm.starts_with(p))
}

fn kind_of(token: &Token) -> Option<HazardKind> {
    if GUIDED_BOMB_WORDS.contains(&token.norm.as_str()) || token.norm.starts_with("умпб") {
        return Some(HazardKind::GuidedBomb);
    }
    KIND_PREFIXES
        .iter()
        .find(|(_, prefixes)| has_prefix(token, prefixes))
        .map(|(kind, _)| *kind)
}

/// Whether the word names an administrative area (`района`, `області`, `обл`).
pub(crate) fn is_admin_area(token: &Token) -> bool {
    has_prefix(token, ADMIN_AREA_PREFIXES)
}

/// Clauses whose hazard mentions are reported as over.
///
/// A clear word applies to the kinds named in its own clause. When its clause names no
/// kind (`мопеды над Николаевом - минус`), it applies to the nearest earlier clause that
/// does. It never reaches forward, so `минус, 2 шахеда на Одессу` stays a threat.
fn cleared_clauses(tokens: &[Token]) -> BTreeSet<usize> {
    let kind_clauses: BTreeSet<usize> = tokens
        .iter()
        .filter(|t| kind_of(t).is_some())
        .map(|t| t.clause)
        .collect();
    tokens
        .iter()
        .filter(|t| has_prefix(t, CLEAR_PREFIXES))
        .filter_map(|t| {
            if kind_clauses.contains(&t.clause) {
                Some(t.clause)
            } else {
                kind_clauses.range(..t.clause).next_back().copied()
            }
        })
        .collect()
}

/// Hazard kinds named in a line, one entry per kind and stance, in order of first mention.
///
/// Stance is decided per clause (see [`cleared_clauses`]), so a line that clears one
/// hazard and reports another yields both. An attack drone becomes a jet drone when its
/// clause calls it jet-powered. A generic missile word is dropped when the line names a
/// ballistic missile with the same stance, so `баллистика … 2 ракеты` is one mention.
pub(crate) fn kinds(tokens: &[Token]) -> Vec<KindMention> {
    let cleared = cleared_clauses(tokens);
    let jet_clauses: BTreeSet<usize> = tokens
        .iter()
        .filter(|t| has_prefix(t, JET_PREFIXES))
        .map(|t| t.clause)
        .collect();
    let mut mentions: Vec<KindMention> = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        let Some(mut kind) = kind_of(token) else {
            continue;
        };
        if kind == HazardKind::AttackDrone && jet_clauses.contains(&token.clause) {
            kind = HazardKind::JetDrone;
        }
        let stance = if cleared.contains(&token.clause) {
            Stance::Clear
        } else {
            Stance::Threat
        };
        let count = tokens[i.saturating_sub(COUNT_LOOKBACK)..i]
            .iter()
            .rev()
            .filter(|t| t.clause == token.clause)
            .find_map(Token::number);
        match mentions
            .iter_mut()
            .find(|m| m.kind == kind && m.stance == stance)
        {
            Some(existing) => {
                existing.words.push(token.raw.clone());
                existing.clauses.insert(token.clause);
                existing.count = existing.count.or(count);
            }
            None => mentions.push(KindMention {
                kind,
                stance,
                clauses: BTreeSet::from([token.clause]),
                count,
                words: vec![token.raw.clone()],
            }),
        }
    }
    let ballistic: Vec<Stance> = mentions
        .iter()
        .filter(|m| m.kind == HazardKind::BallisticMissile)
        .map(|m| m.stance)
        .collect();
    mentions.retain(|m| !(m.kind == HazardKind::Missile && ballistic.contains(&m.stance)));
    mentions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::tokenize;

    fn summary(line: &str) -> Vec<(HazardKind, Option<u32>)> {
        kinds(&tokenize(line))
            .into_iter()
            .map(|m| (m.kind, m.count))
            .collect()
    }

    fn stances(line: &str) -> Vec<(HazardKind, Stance)> {
        kinds(&tokenize(line))
            .into_iter()
            .map(|m| (m.kind, m.stance))
            .collect()
    }

    #[test]
    fn reads_kind_and_count() {
        assert_eq!(
            summary("4 реактивных мопеда подлетают к Киеву"),
            [(HazardKind::JetDrone, Some(4))]
        );
        assert_eq!(
            summary("ещё 2 баллистики на Киев !"),
            [(HazardKind::BallisticMissile, Some(2))]
        );
        assert_eq!(
            summary("пуски КАБ (УМПБ-5) курсом на Затоку"),
            [(HazardKind::GuidedBomb, None)]
        );
        assert_eq!(
            summary("вылеты Цирконов/Оникс-М с курска"),
            [(HazardKind::Missile, None)]
        );
    }

    #[test]
    fn specific_missile_absorbs_generic_word() {
        assert_eq!(
            summary("ещё баллистика на Запорожье с воронежа! 2 ракеты"),
            [(HazardKind::BallisticMissile, None)]
        );
    }

    #[test]
    fn plain_words_are_not_hazards() {
        assert!(summary("кабинет министров, может быть громко").is_empty());
        assert!(summary("ждём инфу от ДПСУ").is_empty());
    }

    #[test]
    fn clear_words() {
        assert_eq!(
            stances("минус по всем этим 4 реактивным мопедам"),
            [(HazardKind::JetDrone, Stance::Clear)]
        );
        assert_eq!(
            stances("Відбій: шахеди збито"),
            [(HazardKind::AttackDrone, Stance::Clear)]
        );
        assert_eq!(
            stances("2 баллистики на Киев"),
            [(HazardKind::BallisticMissile, Stance::Threat)]
        );
    }

    #[test]
    fn clear_is_scoped_to_its_clause() {
        assert_eq!(
            stances("минус по мопеду над Одессой, 2 баллистики на Киев"),
            [
                (HazardKind::AttackDrone, Stance::Clear),
                (HazardKind::BallisticMissile, Stance::Threat),
            ]
        );
        assert_eq!(
            stances("минус по мопедам. ещё 2 мопеда на Киев"),
            [
                (HazardKind::AttackDrone, Stance::Clear),
                (HazardKind::AttackDrone, Stance::Threat),
            ]
        );
    }

    #[test]
    fn kindless_clear_clause_reaches_back_not_forward() {
        assert_eq!(
            stances("мопеды над Николаевом - минус"),
            [(HazardKind::AttackDrone, Stance::Clear)]
        );
        assert_eq!(
            stances("минус, 2 шахеда на Одессу"),
            [(HazardKind::AttackDrone, Stance::Threat)]
        );
    }

    #[test]
    fn jet_marker_is_scoped_to_its_clause() {
        assert_eq!(
            stances("реактивный мопед на Киев, 2 мопеда на Одессу")
                .into_iter()
                .map(|(k, _)| k)
                .collect::<Vec<_>>(),
            [HazardKind::JetDrone, HazardKind::AttackDrone]
        );
    }
}
