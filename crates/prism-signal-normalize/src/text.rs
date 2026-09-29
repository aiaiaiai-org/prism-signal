// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Folding and tokenization of channel text.
//!
//! Channel posts mix Russian and Ukrainian, inflect place names (`к Киеву`, `на Запорожье`),
//! and carry no markup. Matching therefore works on *folded* words: lowercase, with the letters
//! that differ only between the two languages collapsed. Vocabulary files are written in natural
//! spelling and folded by this same function on load, so a curator never types a folded form.

/// Collapses spelling differences that carry no meaning for matching.
///
/// Lowercases, then maps `ё є` to `е`, `і ї ы` to `и`, `ґ` to `г`, and drops apostrophes and
/// the hard sign. `Кам'янське` and `Каменское` stay different words, as they are; `Суми` and
/// `Сумы` become the same one.
pub(crate) fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            'ё' | 'є' => out.push('е'),
            'і' | 'ї' | 'ы' => out.push('и'),
            'ґ' => out.push('г'),
            '\'' | '’' | 'ʼ' | '`' | 'ъ' => {}
            c => out.push(c),
        }
    }
    out
}

/// One word of a post.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Token {
    /// The word as written, used for reporting.
    pub original: String,
    /// The word after [`fold`].
    pub folded: String,
    /// A sentence or line break lies between this word and the previous one.
    ///
    /// Roles and lists never propagate across a break. The first token always has one.
    pub barrier_before: bool,
    /// A comma lies between this word and the previous one.
    ///
    /// A comma does not end a sentence, since it also separates items of a list. It ends one only
    /// for the reach of an all-clear word; see `Normalizer::read`.
    pub comma_before: bool,
}

impl Token {
    /// The word starts with an uppercase letter, as a proper name does.
    pub fn is_capitalized(&self) -> bool {
        self.original.chars().next().is_some_and(char::is_uppercase)
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '\'' | '’' | 'ʼ')
}

fn is_barrier(c: char) -> bool {
    matches!(c, '\n' | '.' | '!' | '?' | ';' | '…')
}

/// Splits text into words. Punctuation such as `/` and `,` separates words without breaking a
/// list; a newline or a sentence terminator also sets [`Token::barrier_before`].
pub(crate) fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut barrier = true;
    let mut comma = false;
    let mut word = String::new();
    let mut flush = |word: &mut String, barrier: &mut bool, comma: &mut bool| {
        let trimmed = word.trim_matches(|c: char| !c.is_alphanumeric());
        if !trimmed.is_empty() {
            tokens.push(Token {
                original: trimmed.to_owned(),
                folded: fold(trimmed),
                barrier_before: *barrier,
                comma_before: *comma && !*barrier,
            });
            *barrier = false;
            *comma = false;
        }
        word.clear();
    };
    for c in text.chars() {
        if is_word_char(c) {
            word.push(c);
        } else {
            flush(&mut word, &mut barrier, &mut comma);
            if is_barrier(c) {
                barrier = true;
            } else if c == ',' {
                comma = true;
            }
        }
    }
    flush(&mut word, &mut barrier, &mut comma);
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_unifies_russian_and_ukrainian_spelling() {
        assert_eq!(fold("Суми"), fold("Сумы"));
        assert_eq!(fold("Ёлка"), "елка");
        assert_eq!(fold("Кам'янське"), "камянське");
        assert_eq!(fold("Києва"), "киева");
        assert_eq!(fold("Київ"), "киив");
        assert_eq!(fold("Ґанок"), "ганок");
    }

    #[test]
    fn tokens_keep_hyphenated_names_whole() {
        let tokens = tokenize("курсом на/через Каролино-Бугаз, Оникс-М");
        let folded: Vec<_> = tokens.iter().map(|t| t.folded.as_str()).collect();
        assert_eq!(
            folded,
            ["курсом", "на", "через", "каролино-бугаз", "оникс-м"]
        );
    }

    #[test]
    fn separators_do_not_break_a_list_but_sentences_and_lines_do() {
        let tokens = tokenize("к Киеву/Ирпеню, Буче!\nдальше на Одессу. ещё");
        let breaks: Vec<_> = tokens
            .iter()
            .filter(|t| t.barrier_before)
            .map(|t| t.folded.as_str())
            .collect();
        assert_eq!(breaks, ["к", "дальше", "еще"]);
    }

    #[test]
    fn a_comma_is_recorded_without_ending_the_sentence() {
        let tokens = tokenize("минус по КАБам, еще 2 КАБа. потом Киев,Одесса");
        let commas: Vec<_> = tokens
            .iter()
            .filter(|t| t.comma_before)
            .map(|t| t.folded.as_str())
            .collect();
        assert_eq!(commas, ["еще", "одесса"]);
        assert!(
            tokens
                .iter()
                .filter(|t| t.folded == "еще")
                .all(|t| !t.barrier_before)
        );
    }

    #[test]
    fn quotes_emoji_and_dangling_hyphens_are_dropped() {
        let tokens = tokenize("«Ягодин» 🚀 - тревога");
        let folded: Vec<_> = tokens.iter().map(|t| t.folded.as_str()).collect();
        assert_eq!(folded, ["ягодин", "тревога"]);
    }

    #[test]
    fn capitalization_is_read_from_the_original_word() {
        let tokens = tokenize("на Виноградара и затоку");
        assert!(tokens[1].is_capitalized());
        assert!(!tokens[3].is_capitalized());
    }
}
