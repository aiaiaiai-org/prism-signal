// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Hazard vocabulary of Russian- and Ukrainian-language air-alert channels.

use std::collections::BTreeSet;

use prism_signal_core::{HazardKind, Stance};

use crate::text::Token;

/// How the rest of a word may continue after a stem.
#[derive(Clone, Copy)]
enum Form {
    /// Nothing, or a case ending of a noun: `шахед`, `шахеды`, `шахедов`.
    Noun,
    /// A case or gender ending of an adjective: `крылатые`, `крилатих`.
    Adjective,
}

/// Case endings of nouns, plus the empty ending.
const NOUN_ENDINGS: &[&str] = &[
    "", "а", "я", "у", "ю", "е", "є", "и", "ы", "і", "ь", "ом", "ем", "ой", "ей", "ою", "ею", "ью",
    "ов", "ев", "ів", "ам", "ям", "ах", "ях", "ами", "ями",
];

/// Case and gender endings of adjectives.
const ADJECTIVE_ENDINGS: &[&str] = &[
    "ая", "яя", "ое", "ее", "ые", "ие", "ый", "ий", "ій", "ой", "ую", "юю", "ого", "его", "ому",
    "ему", "ым", "им", "ыми", "ими", "ых", "их", "ої", "ою", "у", "а", "е", "і",
];

impl Form {
    fn allows(self, rest: &str) -> bool {
        // `Оникс-М`, `Герань-2`: a hyphenated designation after the name.
        rest.starts_with('-')
            || match self {
                Self::Noun => NOUN_ENDINGS,
                Self::Adjective => ADJECTIVE_ENDINGS,
            }
            .contains(&rest)
    }
}

/// Whether the word is one of `stems` followed by an ending of `form`. A bare prefix test
/// would take place names and ordinary words for hazards (`Дронівка`, `ракетный`).
fn is_form_of(token: &Token, form: Form, stems: &[&str]) -> bool {
    stems.iter().any(|stem| {
        token
            .norm
            .strip_prefix(stem)
            .is_some_and(|rest| form.allows(rest))
    })
}

/// Stems naming each kind, with the form that may follow them.
const KIND_WORDS: &[(HazardKind, Form, &[&str])] = &[
    (
        HazardKind::BallisticMissile,
        Form::Noun,
        &[
            "баллист",
            "баллистик",
            "баліст",
            "балістик",
            "искандер",
            "іскандер",
            "кинжал",
            "кинджал",
        ],
    ),
    (
        HazardKind::BallisticMissile,
        Form::Adjective,
        &["баллистическ", "балістичн"],
    ),
    (
        HazardKind::Missile,
        Form::Noun,
        &[
            "ракет",
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
    (HazardKind::Missile, Form::Adjective, &["крылат", "крилат"]),
    (
        HazardKind::AttackDrone,
        Form::Noun,
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

/// Stems naming an administrative area: a place name followed by one is the area, not the
/// town.
const ADMIN_AREA_STEMS: &[&str] = &["район", "област", "громад"];

/// The abbreviation of `область`, matched as a whole word: as a prefix it would also take
/// `облетает` and `областной`.
const ADMIN_AREA_ABBREVIATION: &str = "обл";

/// Words that count objects must stand this close before the kind word.
const COUNT_LOOKBACK: usize = 2;

/// Units of time and distance. A number followed by one is not a count of objects
/// (`через 5 минут`, `за 15 км`).
const UNIT_STEMS: &[&str] = &[
    "минут",
    "хвилин",
    "секунд",
    "час",
    "годин",
    "километр",
    "кілометр",
    "метр",
];

/// Abbreviated units, matched as whole words.
const UNIT_WORDS: &[&str] = &["км", "м", "мин", "хв", "сек", "с", "ч", "год"];

/// One hazard kind named in a line with one stance, and the words that named it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct KindMention {
    pub(crate) kind: HazardKind,
    pub(crate) stance: Stance,
    /// The clause in which this kind is named with this stance. The count belongs to this
    /// clause alone.
    pub(crate) clause: usize,
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
    KIND_WORDS
        .iter()
        .find(|(_, form, stems)| is_form_of(token, *form, stems))
        .map(|(kind, _, _)| *kind)
}

/// Whether the word names an administrative area (`района`, `области`, `обл`).
pub(crate) fn is_admin_area(token: &Token) -> bool {
    token.norm == ADMIN_AREA_ABBREVIATION || is_form_of(token, Form::Noun, ADMIN_AREA_STEMS)
}

/// Whether the word is a unit of time or distance, so a number before it is no count.
fn is_unit(token: &Token) -> bool {
    UNIT_WORDS.contains(&token.norm.as_str()) || has_prefix(token, UNIT_STEMS)
}

/// Clauses whose hazard mentions are reported as over.
///
/// A clear word applies to the kinds named in its own clause. When its clause names no
/// kind (`мопеды над Николаевом - минус`), it applies to the clause right before it, if
/// that one names a kind. It never reaches forward, so `минус, 2 шахеда на Одессу` stays a
/// threat, and it never reaches past a clause that names no kind, so a trailing minus about
/// one place (`Шахеды на Киев, Одесса - минус`) does not clear the threat to another. A
/// clear that is missed leaves a threat to expire on its own TTL; a clear applied wrongly
/// would hide a live one.
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
                t.clause
                    .checked_sub(1)
                    .filter(|previous| kind_clauses.contains(previous))
            }
        })
        .collect()
}

/// The count of objects before the kind word at `at`: the first number within
/// [`COUNT_LOOKBACK`] words in the same clause that is not a measure of time or distance.
fn count_before(tokens: &[Token], at: usize) -> Option<u32> {
    let clause = tokens[at].clause;
    (at.saturating_sub(COUNT_LOOKBACK)..at)
        .rev()
        .filter(|&j| tokens[j].clause == clause)
        .filter(|&j| !tokens.get(j + 1).is_some_and(is_unit))
        .find_map(|j| tokens[j].number())
}

/// Hazard kinds named in a line, one entry per kind, stance, and clause, in order of first
/// mention.
///
/// Stance and count are decided per clause (see [`cleared_clauses`]), so a line that
/// clears one hazard and reports another yields both, and `2 шахеда на Одессу, 3 шахеда на
/// Киев` gives each place its own count. An attack drone becomes a jet drone when its
/// clause calls it jet-powered.
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
        let count = count_before(tokens, i);
        match mentions
            .iter_mut()
            .find(|m| m.kind == kind && m.stance == stance && m.clause == token.clause)
        {
            Some(existing) => {
                existing.words.push(token.raw.clone());
                existing.count = existing.count.or(count);
            }
            None => mentions.push(KindMention {
                kind,
                stance,
                clause: token.clause,
                count,
                words: vec![token.raw.clone()],
            }),
        }
    }
    mentions
}

/// Drops generic missile mentions that only restate a ballistic one.
///
/// `баллистика на Запорожье! 2 ракеты` names one threat twice, so the generic word is
/// dropped when the same clause also names a ballistic missile, or when its own clause holds
/// no place (`place_clauses`) and so has nothing to locate on its own. A generic missile in
/// a clause with its own place (`баллистика на Киев, 2 ракеты на Одессу`) stays a separate
/// mention.
pub(crate) fn absorb_generic_missiles(
    mentions: &mut Vec<KindMention>,
    place_clauses: &BTreeSet<usize>,
) {
    let ballistic: Vec<(Stance, usize)> = mentions
        .iter()
        .filter(|m| m.kind == HazardKind::BallisticMissile)
        .map(|m| (m.stance, m.clause))
        .collect();
    mentions.retain(|m| {
        if m.kind != HazardKind::Missile {
            return true;
        }
        let same_clause = ballistic.contains(&(m.stance, m.clause));
        let restatement = !place_clauses.contains(&m.clause)
            && ballistic.iter().any(|(stance, _)| *stance == m.stance);
        !(same_clause || restatement)
    });
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

    fn absorbed(line: &str, place_clauses: &[usize]) -> Vec<(HazardKind, usize)> {
        let mut mentions = kinds(&tokenize(line));
        absorb_generic_missiles(&mut mentions, &place_clauses.iter().copied().collect());
        mentions.into_iter().map(|m| (m.kind, m.clause)).collect()
    }

    #[test]
    fn specific_missile_absorbs_a_restating_generic_word() {
        // `2 ракеты` has no place of its own: it restates the ballistic threat.
        assert_eq!(
            absorbed("ещё баллистика на Запорожье с воронежа! 2 ракеты", &[0]),
            [(HazardKind::BallisticMissile, 0)]
        );
        assert_eq!(
            absorbed("баллистика и ракеты на Киев", &[0]),
            [(HazardKind::BallisticMissile, 0)]
        );
    }

    #[test]
    fn generic_missile_with_its_own_place_is_kept() {
        assert_eq!(
            absorbed("баллистика на Киев, 2 ракеты на Одессу", &[0, 1]),
            [(HazardKind::BallisticMissile, 0), (HazardKind::Missile, 1)]
        );
    }

    #[test]
    fn counts_belong_to_their_clause() {
        let mentions = kinds(&tokenize("2 шахеда на Одессу, 3 шахеда на Киев"));
        assert_eq!(
            mentions
                .iter()
                .map(|m| (m.clause, m.count))
                .collect::<Vec<_>>(),
            [(0, Some(2)), (1, Some(3))]
        );
        assert_eq!(
            summary("3 шахеда на Киев и Одессу, 2 на Львов"),
            [(HazardKind::AttackDrone, Some(3))]
        );
    }

    #[test]
    fn times_and_distances_are_not_counts() {
        assert_eq!(
            summary("через 5 минут шахеды на Киев"),
            [(HazardKind::AttackDrone, None)]
        );
        assert_eq!(summary("за 15 км шахед"), [(HazardKind::AttackDrone, None)]);
        assert_eq!(
            summary("через 5 минут 3 шахеда"),
            [(HazardKind::AttackDrone, Some(3))]
        );
    }

    #[test]
    fn plain_words_are_not_hazards() {
        assert!(summary("кабинет министров, может быть громко").is_empty());
        assert!(summary("ждём инфу от ДПСУ").is_empty());
    }

    #[test]
    fn names_and_adjectives_that_start_like_a_hazard_are_not_hazards() {
        for line in [
            "Дронівка",
            "Дронівка, Київ",
            "ракетный удар",
            "ракетная опасность",
            "Ракетне",
        ] {
            assert!(summary(line).is_empty(), "{line}");
        }
    }

    #[test]
    fn inflected_hazard_words_still_match() {
        for (line, kind) in [
            ("шахедов", HazardKind::AttackDrone),
            ("шахідів", HazardKind::AttackDrone),
            ("дронами", HazardKind::AttackDrone),
            ("Герань-2", HazardKind::AttackDrone),
            ("ракетами", HazardKind::Missile),
            ("крылатых ракет", HazardKind::Missile),
            ("баллистическая ракета", HazardKind::BallisticMissile),
            ("балістичних ракет", HazardKind::BallisticMissile),
        ] {
            assert_eq!(summary(line).first().map(|(k, _)| *k), Some(kind), "{line}");
        }
    }

    #[test]
    fn admin_area_words_match_whole_words() {
        for word in ["района", "районі", "области", "області", "обл", "громады"]
        {
            let token = &tokenize(word)[0];
            assert!(is_admin_area(token), "{word}");
        }
        for word in ["облетает", "облітає", "областной", "Одесса"] {
            let token = &tokenize(word)[0];
            assert!(!is_admin_area(token), "{word}");
        }
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
    fn kindless_clear_does_not_reach_past_a_clause_without_a_kind() {
        assert_eq!(
            stances("Шахеды на Киев, Одесса - минус"),
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
