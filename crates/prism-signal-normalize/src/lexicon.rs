// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Hazard vocabulary of Russian- and Ukrainian-language air-alert channels.

use prism_signal_core::HazardKind;

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

/// Word prefix that turns an attack drone into a jet drone when it appears in the line.
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

/// Words that count objects must stand this close before the kind word.
const COUNT_LOOKBACK: usize = 2;

/// One hazard kind named in a line, with the words that named it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct KindMention {
    pub(crate) kind: HazardKind,
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

/// Whether the line reports a hazard as over.
pub(crate) fn is_clear(tokens: &[Token]) -> bool {
    tokens.iter().any(|t| has_prefix(t, CLEAR_PREFIXES))
}

/// Hazard kinds named in a line, one entry per kind, in order of first mention.
///
/// A generic missile word is dropped when the line names a specific missile, so
/// `баллистика … 2 ракеты` is one ballistic mention. An attack drone becomes a jet drone
/// when the line calls it jet-powered.
pub(crate) fn kinds(tokens: &[Token]) -> Vec<KindMention> {
    let jet = tokens.iter().any(|t| has_prefix(t, JET_PREFIXES));
    let mut mentions: Vec<KindMention> = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        let Some(mut kind) = kind_of(token) else {
            continue;
        };
        if jet && kind == HazardKind::AttackDrone {
            kind = HazardKind::JetDrone;
        }
        let count = tokens[i.saturating_sub(COUNT_LOOKBACK)..i]
            .iter()
            .rev()
            .find_map(Token::number);
        match mentions.iter_mut().find(|m| m.kind == kind) {
            Some(existing) => {
                existing.words.push(token.raw.clone());
                existing.count = existing.count.or(count);
            }
            None => mentions.push(KindMention {
                kind,
                count,
                words: vec![token.raw.clone()],
            }),
        }
    }
    let specific_missile = mentions
        .iter()
        .any(|m| m.kind == HazardKind::BallisticMissile);
    if specific_missile {
        mentions.retain(|m| m.kind != HazardKind::Missile);
    }
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
        assert!(is_clear(&tokenize(
            "минус по всем этим 4 реактивным мопедам"
        )));
        assert!(is_clear(&tokenize("Відбій тривоги")));
        assert!(!is_clear(&tokenize("2 баллистики на Киев")));
    }
}
