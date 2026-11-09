//! Fits a dictation to the text around the cursor, the way Wispr Flow
//! documents: lowercase mid sentence unless it is a proper noun, a capital
//! after a sentence ends, spaces where they are needed, and no closing full
//! stop when the sentence carries on after the cursor. With nothing readable
//! around the cursor, a dictation is treated as a new sentence.

use super::stopwords::is_stopword;

/// What the focused field holds on each side of the cursor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Around {
    pub before: String,
    pub after: String,
}

/// Words that are safe to lowercase in the middle of a sentence, beyond the
/// stopwords. Anything else might be a name, so it keeps its capital.
const COMMON: &[&str] = &[
    "you", "your", "yours", "you're", "you've", "you'll", "with", "what", "what's", "when", "where", "why", "who",
    "which", "while", "will", "would", "without", "yes", "yeah", "okay", "ok", "thanks", "thank", "send", "add",
    "call", "check", "tell", "ask", "use", "show", "open", "close", "start", "stop", "change", "remove", "delete",
    "create", "write", "read", "look", "want", "think", "know", "feel", "try", "keep", "put", "take", "give", "help",
    "work", "works", "working", "maybe", "probably", "sometimes", "today", "tomorrow", "yesterday", "tonight",
    "good", "great", "better", "best", "new", "old", "big", "small", "first", "second", "third", "last", "next",
    "people", "time", "way", "day", "week", "month", "year", "team", "project", "report", "meeting", "email",
    "message", "file", "page", "app", "code", "test", "update", "fix", "bug", "issue", "problem", "question",
    "answer", "idea", "plan", "because", "when", "well", "right", "sure", "sorry", "hello", "hi", "hey", "we've",
    "we'd", "they'll", "they've", "she'll", "he'll", "that'll", "it'll", "who's", "where's", "how's",
];

fn lowers_mid_sentence(word: &str) -> bool {
    let bare: String = word.trim_end_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    if word.chars().skip(1).any(char::is_uppercase) {
        return false;
    }
    if bare == "i" || bare.starts_with("i'") {
        return false;
    }
    is_stopword(&bare) || COMMON.contains(&bare.as_str())
}

fn lowercase_first(text: &str) -> String {
    let word = text.split_whitespace().next().unwrap_or("");
    if !lowers_mid_sentence(word) {
        return text.to_string();
    }
    let mut chars = text.chars();
    match chars.next() {
        Some(first) if first.is_uppercase() => first.to_lowercase().chain(chars).collect(),
        _ => text.to_string(),
    }
}

/// Where the dictation starts, from the last thing before the cursor.
#[derive(PartialEq)]
enum Start {
    /// Empty field or a new line: as is, with no space.
    Fresh,
    /// After a full stop, ? or !: a capital and a space.
    NewSentence,
    /// After "(" or an opening quote: lowercase, no space.
    Opened,
    /// Anything else: lowercase and a space.
    MidSentence,
}

fn start_after(before: &str) -> Start {
    let trimmed = before.trim_end_matches([' ', '\t', '\u{00A0}']);
    match trimmed.chars().next_back() {
        None | Some('\n' | '\r') => Start::Fresh,
        Some('.' | '?' | '!' | '\u{2026}') => Start::NewSentence,
        Some('(' | '[' | '{' | '\u{201C}' | '\u{2018}') => Start::Opened,
        // A straight quote that opens: preceded by a space or nothing.
        Some('"') if opens_quote(trimmed) => Start::Opened,
        Some(_) => Start::MidSentence,
    }
}

fn opens_quote(trimmed: &str) -> bool {
    let quotes = trimmed.chars().filter(|c| *c == '"').count();
    quotes % 2 == 1
}

fn is_list(text: &str) -> bool {
    text.contains('\n')
        && (text.starts_with("- ")
            || text.split_whitespace().next().is_some_and(|w| {
                w.len() >= 2 && w.ends_with('.') && w[..w.len() - 1].chars().all(|c| c.is_alphanumeric())
            }))
}

/// `text` is the finished dictation with no trailing space.
pub fn fit(text: &str, around: &Around, trailing_space: bool) -> String {
    let start = start_after(&around.before);
    let spaced = around.before.ends_with(char::is_whitespace);
    let mut out = String::with_capacity(text.len() + 2);

    if is_list(text) {
        // A list starts on its own line.
        if start != Start::Fresh {
            out.push('\n');
        }
        out.push_str(text);
    } else {
        if matches!(start, Start::NewSentence | Start::MidSentence) && !spaced {
            out.push(' ');
        }
        if matches!(start, Start::MidSentence | Start::Opened) {
            out.push_str(&lowercase_first(text));
        } else {
            out.push_str(text);
        }
    }

    // What follows the cursor decides the end.
    let next = around.after.chars().next();
    let continues = around
        .after
        .trim_start_matches([' ', '\t'])
        .chars()
        .next()
        .is_some_and(|c| c.is_lowercase() || c.is_ascii_digit() || matches!(c, ',' | ';' | ':' | ')' | ']' | '}' | '.' | '?' | '!'));
    if continues && out.ends_with('.') && !out.contains('\n') {
        out.pop();
    }
    match next {
        // The cursor sits at the end of a line or the field.
        None | Some('\n' | '\r') => {
            if trailing_space && !out.ends_with('\n') {
                out.push(' ');
            }
        }
        Some(c) if c.is_whitespace() => {}
        Some(',' | ';' | ':' | ')' | ']' | '}' | '.' | '?' | '!' | '\'' | '"' | '\u{201D}' | '\u{2019}') => {}
        Some(_) => out.push(' '),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fit_in(text: &str, before: &str, after: &str) -> String {
        fit(text, &Around { before: before.into(), after: after.into() }, true)
    }

    #[test]
    fn an_empty_field_changes_nothing() {
        assert_eq!(fit_in("Hello there.", "", ""), "Hello there. ");
        assert_eq!(fit("Hello there.", &Around::default(), false), "Hello there.");
    }

    #[test]
    fn a_new_sentence_keeps_its_capital_and_gets_a_space() {
        assert_eq!(fit_in("The report is late.", "That is done.", ""), " The report is late. ");
        assert_eq!(fit_in("The report is late.", "That is done. ", ""), "The report is late. ");
        assert_eq!(fit_in("The report is late.", "Line one\n", ""), "The report is late. ");
    }

    #[test]
    fn mid_sentence_lowercases_except_names() {
        assert_eq!(fit_in("The report is late.", "I told him that", ""), " the report is late. ");
        assert_eq!(fit_in("The report is late.", "Hello, ", ""), "the report is late. ");
        assert_eq!(fit_in("Sara is late.", "I told him that ", ""), "Sara is late. ");
        assert_eq!(fit_in("I am late.", "He said ", ""), "I am late. ");
        assert_eq!(fit_in("iPhone sales rose.", "He said ", ""), "iPhone sales rose. ");
        assert_eq!(fit_in("NASA called.", "He said ", ""), "NASA called. ");
    }

    #[test]
    fn opened_brackets_and_quotes_hug() {
        assert_eq!(fit_in("See the notes.", "the plan (", ")"), "see the notes");
        assert_eq!(fit_in("Hello there.", "she said \"", "\""), "hello there.");
    }

    #[test]
    fn text_after_the_cursor_drops_the_full_stop() {
        assert_eq!(fit_in("The report.", "Send ", "by Friday"), "the report ");
        assert_eq!(fit_in("The report.", "Send ", " by Friday"), "the report");
        assert_eq!(fit_in("The report.", "Send ", ", please"), "the report");
        assert_eq!(fit_in("Is it done?", "Send ", " by Friday"), "is it done?");
    }

    #[test]
    fn a_capital_after_the_cursor_keeps_the_full_stop() {
        assert_eq!(fit_in("Thanks.", "", "Next sentence"), "Thanks. ");
        assert_eq!(fit_in("Thanks.", "", " Next sentence"), "Thanks.");
    }

    #[test]
    fn lists_start_on_their_own_line() {
        assert_eq!(fit_in("1. Milk\n2. Eggs", "Buy:", ""), "\n1. Milk\n2. Eggs ");
        assert_eq!(fit_in("- Milk\n- Eggs", "Line\n", ""), "- Milk\n- Eggs ");
    }

    #[test]
    fn no_trailing_space_when_turned_off() {
        assert_eq!(fit("Done.", &Around { before: "ok.".into(), after: String::new() }, false), " Done.");
    }
}
