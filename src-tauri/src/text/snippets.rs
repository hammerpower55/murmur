//! Snippets (spec 6.8): saying a trigger phrase pastes a longer text. The
//! text may hold {date}, {time} and {clipboard}. Expanded text is pasted
//! exactly as written, so later stages never re-case or re-space it.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use super::{csv, match_phrase, phrase_words, Token};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    pub trigger: String,
    pub text: String,
}

impl Snippet {
    #[cfg(test)]
    pub fn new(trigger: &str, text: &str) -> Self {
        Self {
            trigger: trigger.into(),
            text: text.into(),
        }
    }
}

/// Values for the placeholders, captured once per dictation.
#[derive(Debug, Clone)]
pub struct SnippetContext {
    pub date: String,
    pub time: String,
    pub clipboard: String,
}

impl SnippetContext {
    /// Local date and time now, e.g. "28 October 2026" and "3:05 PM".
    pub fn now(clipboard: String) -> Self {
        let now = chrono::Local::now();
        Self {
            date: now.format("%-d %B %Y").to_string(),
            time: now.format("%-I:%M %p").to_string(),
            clipboard,
        }
    }

    #[cfg(test)]
    pub fn fixed(date: &str, time: &str, clipboard: &str) -> Self {
        Self {
            date: date.into(),
            time: time.into(),
            clipboard: clipboard.into(),
        }
    }

    fn fill(&self, text: &str) -> String {
        text.replace("{date}", &self.date)
            .replace("{time}", &self.time)
            .replace("{clipboard}", &self.clipboard)
    }
}

pub fn expand(tokens: Vec<Token>, snippets: &[Snippet], ctx: &SnippetContext) -> Vec<Token> {
    let mut compiled: Vec<(Vec<String>, &Snippet)> = snippets
        .iter()
        .map(|s| (phrase_words(&s.trigger), s))
        .filter(|(words, _)| !words.is_empty())
        .collect();
    if compiled.is_empty() {
        return tokens;
    }
    compiled.sort_by_key(|(words, _)| std::cmp::Reverse(words.len()));

    // The whole dictation is just the trigger: replace everything, trailing
    // punctuation included.
    let words: Vec<String> = tokens
        .iter()
        .filter_map(|t| t.word().map(str::to_lowercase))
        .collect();
    let only_punctuation_besides_words = tokens
        .iter()
        .all(|t| !matches!(t, Token::Raw(_) | Token::Break(_)));
    if only_punctuation_besides_words {
        if let Some((_, s)) = compiled.iter().find(|(w, _)| *w == words) {
            return vec![Token::Raw(ctx.fill(&s.text))];
        }
    }

    let mut out = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let hit = compiled
            .iter()
            .find_map(|(words, s)| match_phrase(&tokens, i, words).map(|end| (end, s)));
        match hit {
            Some((end, s)) => {
                out.push(Token::Raw(ctx.fill(&s.text)));
                i = end;
            }
            None => {
                out.push(tokens[i].clone());
                i += 1;
            }
        }
    }
    out
}

#[allow(dead_code)] // Used by the Dictionary and Snippets pages (Phase 8).
const HEADER: [&str; 2] = ["trigger", "text"];

#[allow(dead_code)] // Used by the Dictionary and Snippets pages (Phase 8).
pub fn export_csv(snippets: &[Snippet]) -> String {
    let mut out = csv::row(&HEADER);
    for s in snippets {
        out.push_str(&csv::row(&[&s.trigger, &s.text]));
    }
    out
}

/// Reads "trigger,text" rows. A header row is optional; text may span lines.
#[allow(dead_code)] // Used by the Dictionary and Snippets pages (Phase 8).
pub fn import_csv(text: &str) -> Result<Vec<Snippet>> {
    let mut snippets = Vec::new();
    for (n, row) in csv::parse(text).into_iter().enumerate() {
        if n == 0
            && row
                .iter()
                .map(|c| c.trim().to_lowercase())
                .eq(HEADER.iter().map(|h| h.to_string()))
        {
            continue;
        }
        if row.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        if row.len() != 2 || row[0].trim().is_empty() {
            bail!("Row {} needs a trigger and a text column", n + 1);
        }
        snippets.push(Snippet {
            trigger: row[0].trim().to_string(),
            text: row[1].clone(),
        });
    }
    Ok(snippets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::format::{render, tidy};
    use crate::text::tokenize;

    fn ctx() -> SnippetContext {
        SnippetContext::fixed("28 October 2026", "3:05 PM", "https://example.com/doc")
    }

    fn run(text: &str, snippets: &[Snippet]) -> String {
        render(&tidy(expand(tokenize(text), snippets, &ctx())))
    }

    #[test]
    fn whole_dictation_equal_to_trigger_ignores_case_and_punctuation() {
        let s = [Snippet::new("my address", "12 Garden Road,\nLahore")];
        assert_eq!(run("My address.", &s), "12 Garden Road,\nLahore");
        assert_eq!(run("my ADDRESS!", &s), "12 Garden Road,\nLahore");
    }

    #[test]
    fn trigger_inside_a_sentence() {
        let s = [Snippet::new("my email", "sam@example.com")];
        assert_eq!(
            run("please write to my email today.", &s),
            "Please write to sam@example.com today."
        );
    }

    #[test]
    fn trigger_must_be_standalone_words() {
        let s = [Snippet::new("sig", "Best, Sam")];
        assert_eq!(run("the signal is weak", &s), "The signal is weak");
    }

    #[test]
    fn placeholders_are_filled() {
        let s = [Snippet::new("stamp", "Sent {date} at {time}: {clipboard}")];
        assert_eq!(
            run("stamp", &s),
            "Sent 28 October 2026 at 3:05 PM: https://example.com/doc"
        );
    }

    #[test]
    fn longest_trigger_wins() {
        let s = [
            Snippet::new("sign off", "Thanks"),
            Snippet::new("sign off formal", "Kind regards,\nSam"),
        ];
        assert_eq!(run("sign off formal", &s), "Kind regards,\nSam");
    }

    #[test]
    fn real_clock_formats() {
        let c = SnippetContext::now(String::new());
        assert!(c.date.split(' ').count() == 3, "{}", c.date);
        assert!(
            c.time.ends_with("AM") || c.time.ends_with("PM"),
            "{}",
            c.time
        );
    }

    #[test]
    fn csv_round_trip_with_multiline_text() {
        let snippets = vec![
            Snippet::new("sig", "Best,\nSam"),
            Snippet::new("addr", "12 Garden Road, Lahore"),
        ];
        assert_eq!(import_csv(&export_csv(&snippets)).unwrap(), snippets);
    }
}
