//! Text pipeline: raw engine output to the text that gets pasted.
//!
//! Order (spec 5.2 and 6.3, plus smart formatting): trim, Backtrack
//! corrections, filler removal, dictionary, Roman Urdu normalisation,
//! snippet expansion, spoken line breaks, voice punctuation, lists,
//! paragraphs, email or chat layout, then capitalisation and spacing.
//!
//! Every stage works on tokens rather than raw strings, so no stage has to
//! worry about spacing: words, punctuation and line breaks are separate
//! tokens and `format::render` lays them out once at the end.

mod csv;
pub mod dictionary;
pub mod fillers;
pub mod format;
pub mod punctuation;
pub mod roman_urdu;
pub mod smart;
pub mod snippets;
pub mod stopwords;
pub mod store;

use std::collections::HashMap;

use dictionary::DictEntry;
use smart::AppKind;
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
    /// Lists, Backtrack corrections, spoken breaks, paragraphs, and email
    /// or chat layout (smart.rs).
    pub smart_formatting: bool,
    /// Where the text is going; shapes email and chat layout.
    pub app: AppKind,
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
            smart_formatting: true,
            app: AppKind::Other,
            dictionary: Vec::new(),
            snippets: Vec::new(),
            roman_urdu_map: roman_urdu::default_map(),
        }
    }
}

/// Runs the whole pipeline. Returns an empty string if nothing is left.
pub fn process(raw: &str, settings: &TextSettings, ctx: &SnippetContext) -> String {
    let smart_on = settings.smart_formatting;
    let mut tokens = tokenize(raw.trim());
    // Before filler removal, which would drop the "actually" in "2 actually 3".
    if smart_on {
        tokens = smart::backtrack(tokens);
    }
    if settings.remove_fillers {
        tokens = fillers::remove(tokens);
    }
    tokens = dictionary::apply(tokens, &settings.dictionary);
    if settings.roman_urdu {
        tokens = roman_urdu::normalise(tokens, &settings.roman_urdu_map);
    }
    tokens = snippets::expand(tokens, &settings.snippets, ctx);
    if smart_on || settings.voice_punctuation {
        tokens = smart::spoken_breaks(tokens);
    }
    if settings.voice_punctuation {
        tokens = punctuation::apply(tokens);
    }
    if smart_on {
        tokens = smart::lists(tokens);
        tokens = smart::implicit_lists(tokens);
        tokens = smart::paragraphs(tokens);
        tokens = match settings.app {
            AppKind::Email => smart::email(tokens),
            AppKind::Chat => smart::chat(tokens),
            AppKind::Other => tokens,
        };
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
    fn smart_formatting_end_to_end() {
        let s = TextSettings {
            add_trailing_space: false,
            ..TextSettings::default()
        };
        let ctx = SnippetContext::fixed("", "", "");
        assert_eq!(
            process("Um, so I need three things. One, milk. Two, uh, eggs. Three, bread.", &s, &ctx),
            "I need three things:\n1. Milk\n2. Eggs\n3. Bread"
        );
        assert_eq!(
            process("Let's do coffee at 2 actually 3.", &s, &ctx),
            "Let's do coffee at 3."
        );
        assert_eq!(
            process("Thanks for the update. New paragraph. See you Friday.", &s, &ctx),
            "Thanks for the update.\n\nSee you Friday."
        );
    }

    #[test]
    fn smart_formatting_follows_the_app() {
        let ctx = SnippetContext::fixed("", "", "");
        let chat = TextSettings {
            add_trailing_space: false,
            app: AppKind::Chat,
            ..TextSettings::default()
        };
        assert_eq!(process("Sounds good, see you at 3.", &chat, &ctx), "Sounds good, see you at 3");
        let mail = TextSettings {
            app: AppKind::Email,
            ..chat.clone()
        };
        assert_eq!(
            process("Hi Sara, the report is attached. Best, Maaz.", &mail, &ctx),
            "Hi Sara,\n\nThe report is attached.\n\nBest,\nMaaz"
        );
    }

    /// Runs every raw transcript in the local history through the pipeline
    /// with and without smart formatting and prints the ones that change,
    /// to catch rules firing on real speech. Reads only, prints locally.
    ///   cargo test smart_formatting_on_history -- --ignored --nocapture
    #[test]
    #[ignore = "reads the local history database"]
    fn smart_formatting_on_history() {
        let path = crate::paths::data_dir_named(&crate::paths::product_name())
            .unwrap()
            .join(crate::db::FILE);
        let conn = rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let mut stmt = conn.prepare("SELECT raw_text, app_name, app_title FROM transcripts").unwrap();
        let rows: Vec<(String, Option<String>, Option<String>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let ctx = SnippetContext::fixed("", "", "");
        let mut changed = 0;
        for (raw, exe, title) in &rows {
            let app = AppKind::detect(exe.as_deref().unwrap_or(""), title.as_deref().unwrap_or(""));
            let on = TextSettings { app, add_trailing_space: false, ..TextSettings::default() };
            let off = TextSettings { smart_formatting: false, ..on.clone() };
            let (a, b) = (process(raw, &off, &ctx), process(raw, &on, &ctx));
            if a != b {
                changed += 1;
                println!("--- {app:?}\n  before: {a:?}\n  after:  {b:?}");
            }
        }
        println!("\n{changed} of {} transcripts changed", rows.len());
    }

    #[test]
    fn smart_formatting_can_be_turned_off() {
        let s = TextSettings {
            add_trailing_space: false,
            smart_formatting: false,
            ..TextSettings::default()
        };
        let ctx = SnippetContext::fixed("", "", "");
        assert_eq!(
            process("Two things. One, the budget. Two, the timeline.", &s, &ctx),
            "Two things. One, the budget. Two, the timeline."
        );
        assert_eq!(process("At 2 actually 3.", &s, &ctx), "At 2 actually 3.");
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
