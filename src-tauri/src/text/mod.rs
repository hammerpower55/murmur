//! Text pipeline: raw engine output to the text that gets pasted.
//!
//! Order (spec 5.2 and 6.3): trim, filler removal, dictionary, Roman Urdu
//! normalisation, snippet expansion, voice punctuation, then capitalisation
//! and spacing.
//!
//! Every stage works on tokens rather than raw strings, so no stage has to
//! worry about spacing: words, punctuation and line breaks are separate
//! tokens and `format::render` lays them out once at the end.

#[allow(dead_code)] // Used by the Dictionary and Snippets pages (Phase 8).
mod csv;
pub mod dictionary;
pub mod fillers;
pub mod format;
pub mod punctuation;
pub mod roman_urdu;
pub mod snippets;

use std::collections::HashMap;

use dictionary::DictEntry;
use snippets::{Snippet, SnippetContext};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Word(String),
    /// Punctuation that hugs the token before it: , . ? ! ; : ) ] and
    /// closing quotes.
    Close(String),
    /// Punctuation that hugs the token after it: ( [ and opening quotes.
    Open(String),
    /// One or more line breaks.
    Break(usize),
    /// Snippet output, pasted exactly as written.
    Raw(String),
}

impl Token {
    pub fn word(&self) -> Option<&str> {
        match self {
            Token::Word(w) => Some(w),
            _ => None,
        }
    }

    pub fn is_word_ci(&self, other: &str) -> bool {
        self.word().is_some_and(|w| w.eq_ignore_ascii_case(other))
    }

    /// Ends a sentence: . ? ! or a line break. An ellipsis does not, so
    /// "well... he said" keeps its lowercase.
    pub fn ends_sentence(&self) -> bool {
        match self {
            Token::Close(p) => matches!(p.as_str(), "." | "?" | "!"),
            Token::Break(_) => true,
            _ => false,
        }
    }

    pub fn is_comma(&self) -> bool {
        matches!(self, Token::Close(p) if p == ",")
    }
}

const CLOSERS: &[char] = &[
    ',', '.', '?', '!', ';', ':', ')', ']', '}', '"', '\'', '\u{201D}', '\u{2019}', '\u{2026}',
];
const OPENERS: &[char] = &['(', '[', '{', '"', '\'', '\u{201C}', '\u{2018}'];

/// Splits text into tokens. Punctuation is peeled off the ends of each
/// whitespace separated chunk, so "3.5", "o'clock", "e-mail" and
/// "example.com" stay whole while "Friday?" becomes Friday + ?.
pub fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut newlines = 0;
    let mut chunk = String::new();

    let flush = |chunk: &mut String, tokens: &mut Vec<Token>| {
        if chunk.is_empty() {
            return;
        }
        split_chunk(chunk, tokens);
        chunk.clear();
    };

    for c in text.chars() {
        if c == '\n' {
            flush(&mut chunk, &mut tokens);
            newlines += 1;
        } else if c.is_whitespace() {
            flush(&mut chunk, &mut tokens);
        } else {
            if newlines > 0 {
                if !tokens.is_empty() {
                    tokens.push(Token::Break(newlines.min(2)));
                }
                newlines = 0;
            }
            chunk.push(c);
        }
    }
    flush(&mut chunk, &mut tokens);
    tokens
}

fn split_chunk(chunk: &str, tokens: &mut Vec<Token>) {
    let chars: Vec<char> = chunk.chars().collect();
    let mut start = 0;
    let mut end = chars.len();
    while start < end {
        let c = chars[start];
        if OPENERS.contains(&c) {
            tokens.push(Token::Open(c.to_string()));
        } else if CLOSERS.contains(&c) {
            // Stray leading punctuation, as in "one ,two".
            tokens.push(Token::Close(c.to_string()));
        } else {
            break;
        }
        start += 1;
    }
    let mut trailing = Vec::new();
    while end > start && CLOSERS.contains(&chars[end - 1]) {
        trailing.push(chars[end - 1]);
        end -= 1;
    }
    if start < end {
        tokens.push(Token::Word(chars[start..end].iter().collect()));
    }
    // "..." stays one token.
    let trailing: String = trailing.iter().rev().collect();
    let mut rest = trailing.as_str();
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("...") {
            tokens.push(Token::Close("...".into()));
            rest = r;
        } else {
            let c = rest.chars().next().unwrap_or(' ');
            tokens.push(Token::Close(c.to_string()));
            rest = &rest[c.len_utf8()..];
        }
    }
}

/// Splits a phrase into lowercase words for matching (dictionary entries,
/// snippet triggers).
pub fn phrase_words(phrase: &str) -> Vec<String> {
    tokenize(phrase)
        .into_iter()
        .filter_map(|t| match t {
            Token::Word(w) => Some(w.to_lowercase()),
            _ => None,
        })
        .collect()
}

/// Finds `words` (lowercase) as a run of consecutive Word tokens starting at
/// `at`, returning the index one past the match.
pub fn match_phrase(tokens: &[Token], at: usize, words: &[String]) -> Option<usize> {
    if words.is_empty() || at + words.len() > tokens.len() {
        return None;
    }
    for (i, w) in words.iter().enumerate() {
        match tokens[at + i].word() {
            Some(t) if t.to_lowercase() == *w => {}
            _ => return None,
        }
    }
    Some(at + words.len())
}

#[derive(Debug, Clone)]
pub struct TextSettings {
    pub remove_fillers: bool,
    pub voice_punctuation: bool,
    pub add_trailing_space: bool,
    pub roman_urdu: bool,
    pub dictionary: Vec<DictEntry>,
    pub snippets: Vec<Snippet>,
    pub roman_urdu_map: HashMap<String, String>,
}

impl Default for TextSettings {
    fn default() -> Self {
        Self {
            remove_fillers: true,
            voice_punctuation: false,
            add_trailing_space: true,
            roman_urdu: true,
            dictionary: Vec::new(),
            snippets: Vec::new(),
            roman_urdu_map: roman_urdu::default_map(),
        }
    }
}

/// Runs the whole pipeline. Returns an empty string if nothing is left.
pub fn process(raw: &str, settings: &TextSettings, ctx: &SnippetContext) -> String {
    let mut tokens = tokenize(raw.trim());
    if settings.remove_fillers {
        tokens = fillers::remove(tokens);
    }
    tokens = dictionary::apply(tokens, &settings.dictionary);
    if settings.roman_urdu {
        tokens = roman_urdu::normalise(tokens, &settings.roman_urdu_map);
    }
    tokens = snippets::expand(tokens, &settings.snippets, ctx);
    if settings.voice_punctuation {
        tokens = punctuation::apply(tokens);
    }
    let text = format::render(&format::tidy(tokens));
    if text.is_empty() || !settings.add_trailing_space || text.ends_with('\n') {
        text
    } else {
        text + " "
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Token {
        Token::Word(s.into())
    }
    fn c(s: &str) -> Token {
        Token::Close(s.into())
    }

    #[test]
    fn tokenizes_words_and_punctuation() {
        assert_eq!(
            tokenize("Hello, world. Is it 3.5?"),
            vec![
                w("Hello"),
                c(","),
                w("world"),
                c("."),
                w("Is"),
                w("it"),
                w("3.5"),
                c("?")
            ]
        );
    }

    #[test]
    fn keeps_inner_punctuation_in_words() {
        assert_eq!(
            tokenize("o'clock e-mail example.com 10:30 user@mail.com"),
            vec![
                w("o'clock"),
                w("e-mail"),
                w("example.com"),
                w("10:30"),
                w("user@mail.com")
            ]
        );
    }

    #[test]
    fn splits_quotes_brackets_and_ellipsis() {
        assert_eq!(
            tokenize("(see \"this\")..."),
            vec![
                Token::Open("(".into()),
                w("see"),
                Token::Open("\"".into()),
                w("this"),
                c("\""),
                c(")"),
                c("...")
            ]
        );
    }

    #[test]
    fn line_breaks_become_break_tokens() {
        assert_eq!(
            tokenize("one\ntwo\n\n\nthree"),
            vec![
                w("one"),
                Token::Break(1),
                w("two"),
                Token::Break(2),
                w("three")
            ]
        );
    }

    #[test]
    fn phrase_matching_is_case_insensitive() {
        let tokens = tokenize("Send it to Clari Tree today");
        let words = phrase_words("clari tree");
        assert_eq!(match_phrase(&tokens, 3, &words), Some(5));
        assert_eq!(match_phrase(&tokens, 2, &words), None);
    }

    #[test]
    fn full_pipeline_with_defaults() {
        let s = TextSettings::default();
        let ctx = SnippetContext::fixed("28 October 2026", "3:05 PM", "");
        assert_eq!(
            process("  um, so the the report is   ready ", &s, &ctx),
            "The report is ready "
        );
    }

    #[test]
    fn empty_input_stays_empty() {
        let s = TextSettings::default();
        let ctx = SnippetContext::fixed("", "", "");
        assert_eq!(process("   ", &s, &ctx), "");
        assert_eq!(process("Um.", &s, &ctx), "");
    }

    #[test]
    fn trailing_space_can_be_turned_off() {
        let s = TextSettings {
            add_trailing_space: false,
            ..TextSettings::default()
        };
        let ctx = SnippetContext::fixed("", "", "");
        assert_eq!(process("hello there.", &s, &ctx), "Hello there.");
    }
}
