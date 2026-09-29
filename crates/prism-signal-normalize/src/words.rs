// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Word patterns shared by the lexicon and the gazetteer.

use crate::LoadError;
use crate::text::fold;

/// One folded word to look for: an exact form, or a stem followed by a bounded ending.
#[derive(Clone, Debug)]
pub(crate) struct WordPattern {
    text: String,
    exact: bool,
    max_suffix: usize,
}

impl WordPattern {
    pub fn exact(text: &str) -> Result<Self, LoadError> {
        Self::build(text, true, 0)
    }

    /// A stem that accepts at most `max_suffix` further characters, which is how an inflected
    /// name (`Киев` → `Киеву`, `Киевом`) matches without also matching `Киевщина`.
    pub fn stem(text: &str, max_suffix: usize) -> Result<Self, LoadError> {
        Self::build(text, false, max_suffix)
    }

    fn build(text: &str, exact: bool, max_suffix: usize) -> Result<Self, LoadError> {
        let text = fold(text);
        if text.is_empty() {
            return Err(LoadError::EmptyPattern);
        }
        Ok(Self {
            text,
            exact,
            max_suffix,
        })
    }

    pub fn matches(&self, word: &str) -> bool {
        if self.exact {
            return word == self.text;
        }
        word.strip_prefix(self.text.as_str())
            .is_some_and(|rest| rest.chars().count() <= self.max_suffix)
    }
}

/// A set of single-word patterns; a word matches if any pattern does.
#[derive(Clone, Debug, Default)]
pub(crate) struct WordSet(Vec<WordPattern>);

impl WordSet {
    /// Builds a set from exact forms and unbounded prefixes.
    pub fn new(forms: &[String], stems: &[String]) -> Result<Self, LoadError> {
        let mut patterns = Vec::with_capacity(forms.len() + stems.len());
        for form in forms {
            patterns.push(WordPattern::exact(form)?);
        }
        for stem in stems {
            patterns.push(WordPattern::stem(stem, usize::MAX)?);
        }
        Ok(Self(patterns))
    }

    /// Builds a set from exact words only.
    pub fn of_forms(forms: &[String]) -> Result<Self, LoadError> {
        Self::new(forms, &[])
    }

    pub fn matches(&self, word: &str) -> bool {
        self.0.iter().any(|pattern| pattern.matches(word))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stem_accepts_a_bounded_ending_only() {
        let stem = WordPattern::stem("киев", 2).unwrap();
        assert!(stem.matches("киев"));
        assert!(stem.matches("киеву"));
        assert!(stem.matches("киевом"));
        assert!(!stem.matches("киевщина"));
        assert!(!stem.matches("кие"));
    }

    #[test]
    fn exact_form_matches_the_whole_word() {
        let exact = WordPattern::exact("каб").unwrap();
        assert!(exact.matches("каб"));
        assert!(!exact.matches("кабель"));
        assert!(!exact.matches("кабинет"));
    }

    #[test]
    fn patterns_are_folded_on_build() {
        let stem = WordPattern::stem("Київ", 2).unwrap();
        assert!(stem.matches("киива"));
        assert!(matches!(
            WordPattern::exact("'"),
            Err(LoadError::EmptyPattern)
        ));
    }
}
