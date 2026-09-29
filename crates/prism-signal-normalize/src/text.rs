// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Tokenization and inflection folding for Russian and Ukrainian channel text.

/// One word of a line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Token {
    /// The word as written.
    pub(crate) raw: String,
    /// Lowercase, `ё` folded to `е`, apostrophes removed.
    pub(crate) norm: String,
}

impl Token {
    /// Whether the word starts with an uppercase letter, as proper names do.
    pub(crate) fn is_capitalized(&self) -> bool {
        self.raw.chars().next().is_some_and(char::is_uppercase)
    }

    /// The word as a small count, if it is one.
    pub(crate) fn number(&self) -> Option<u32> {
        self.norm
            .parse::<u32>()
            .ok()
            .filter(|n| (1..=1000).contains(n))
    }
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '’' | 'ʼ' | '`')
}

/// Combining acute and grave accents, used to mark stress in dictionaries and some names.
fn is_stress_mark(c: char) -> bool {
    matches!(c, '\u{0300}' | '\u{0301}')
}

/// Folds case, `ё`, stress marks, and apostrophes so spellings of one name compare equal.
pub(crate) fn normalize_word(word: &str) -> String {
    word.chars()
        .filter(|c| !is_apostrophe(*c) && !is_stress_mark(*c))
        .flat_map(char::to_lowercase)
        .map(|c| if c == 'ё' { 'е' } else { c })
        .collect()
}

/// Splits a line into words: letters, digits, inner hyphens, and apostrophes. Slashes and
/// all other punctuation separate words.
pub(crate) fn tokenize(line: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut flush = |current: &mut String| {
        let word = current.trim_matches('-');
        if !word.is_empty() {
            tokens.push(Token {
                raw: word.to_owned(),
                norm: normalize_word(word),
            });
        }
        current.clear();
    };
    for c in line.chars() {
        if c.is_alphanumeric() || c == '-' || is_apostrophe(c) || is_stress_mark(c) {
            current.push(c);
        } else {
            flush(&mut current);
        }
    }
    flush(&mut current);
    tokens
}

/// Case endings stripped before comparing names, longest first. `-ов`/`-ів` are absent on
/// purpose: in place names (`Фастов`, `Харьков`) they belong to the name, not the case.
const ENDINGS: &[&str] = &[
    "ами", "ями", "ого", "его", "ому", "ему", "ой", "ей", "ою", "ею", "ом", "ем", "ах", "ях", "ая",
    "яя", "ое", "ее", "ые", "ие", "ий", "ый", "а", "я", "о", "е", "у", "ю", "ы", "и", "і", "ь",
    "й",
];

/// A word with at most one case ending removed, so `Киеву`, `Одессы`, and `Гостомеля` meet
/// `Киев`, `Одесса`, and `Гостомель`. Stems shorter than `min_len` letters are left whole,
/// which keeps short words from colliding.
pub(crate) fn stem(word: &str, min_len: usize) -> String {
    for ending in ENDINGS {
        if let Some(base) = word.strip_suffix(ending) {
            if base.chars().count() >= min_len {
                return base.to_owned();
            }
        }
    }
    word.to_owned()
}

/// A stem with its vowels removed, so fleeting vowels (`Церковь` / `Церкви`) do not break a
/// match. Too loose for a whole name; used only for the later words of a multi-word name.
pub(crate) fn skeleton(stem: &str) -> String {
    stem.chars()
        .filter(|c| !"аеёиоуыэюяіїє".contains(*c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_splits_on_slash_and_keeps_hyphenated_names() {
        let words: Vec<_> = tokenize("в сторону Каролино-Бугаза/Овидиополя - громко!")
            .into_iter()
            .map(|t| t.raw)
            .collect();
        assert_eq!(
            words,
            ["в", "сторону", "Каролино-Бугаза", "Овидиополя", "громко"]
        );
    }

    #[test]
    fn normalization_folds_case_yo_and_apostrophes() {
        assert_eq!(normalize_word("Ванёк"), "ванек");
        assert_eq!(normalize_word("Кам’янське"), "камянське");
        assert_eq!(normalize_word("Кам'янське"), "камянське");
        assert_eq!(normalize_word("Ові\u{301}діополь"), "овідіополь");
    }

    #[test]
    fn inflected_forms_share_a_stem() {
        for (a, b) in [
            ("киеву", "киев"),
            ("одессы", "одесса"),
            ("гостомеля", "гостомель"),
            ("днепром", "днепр"),
            ("затоку", "затока"),
            ("фастова", "фастов"),
            ("харькову", "харьков"),
            ("николаеве", "николаев"),
            ("запорожья", "запорожье"),
        ] {
            assert_eq!(stem(a, 4), stem(b, 4), "{a} / {b}");
        }
        assert_eq!(stem("рима", 4), "рима");
        assert_eq!(skeleton(&stem("церкви", 3)), skeleton(&stem("церковь", 3)));
    }
}
