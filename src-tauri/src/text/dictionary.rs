//! Personal dictionary (spec 6.8): spoken phrase to written form, matched
//! case insensitively on word boundaries. "clari tree" becomes "ClariTree".

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use super::{csv, match_phrase, phrase_words, tokenize, Token};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DictEntry {
    pub spoken: String,
    pub written: String,
    /// Added by Murmur from a correction the user made (text/learn.rs).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub learned: bool,
}

#[cfg(test)]
impl DictEntry {
    pub fn new(spoken: &str, written: &str) -> Self {
        Self {
            spoken: spoken.into(),
            written: written.into(),
            learned: false,
        }
    }
}

pub fn apply(tokens: Vec<Token>, entries: &[DictEntry]) -> Vec<Token> {
    if entries.is_empty() {
        return tokens;
    }
    // Longest phrases first so "clari tree app" wins over "clari tree".
    let mut compiled: Vec<(Vec<String>, Vec<Token>)> = entries
        .iter()
        .map(|e| (phrase_words(&e.spoken), tokenize(&e.written)))
        .filter(|(words, _)| !words.is_empty())
        .collect();
    compiled.sort_by_key(|(words, _)| std::cmp::Reverse(words.len()));

    let mut out = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let hit = compiled
            .iter()
            .find_map(|(words, written)| match_phrase(&tokens, i, words).map(|end| (end, written)));
        match hit {
            Some((end, written)) => {
                out.extend(written.iter().cloned());
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
const HEADER: [&str; 2] = ["spoken", "written"];

#[allow(dead_code)] // Used by the Dictionary and Snippets pages (Phase 8).
pub fn export_csv(entries: &[DictEntry]) -> String {
    let mut out = csv::row(&HEADER);
    for e in entries {
        out.push_str(&csv::row(&[&e.spoken, &e.written]));
    }
    out
}

/// Reads "spoken,written" rows. A header row is optional.
#[allow(dead_code)] // Used by the Dictionary and Snippets pages (Phase 8).
pub fn import_csv(text: &str) -> Result<Vec<DictEntry>> {
    let mut entries = Vec::new();
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
            bail!("Row {} needs a spoken and a written column", n + 1);
        }
        entries.push(DictEntry {
            spoken: row[0].trim().to_string(),
            written: row[1].trim().to_string(),
            learned: false,
        });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::format::{render, tidy};

    fn run(text: &str, entries: &[DictEntry]) -> String {
        render(&tidy(apply(tokenize(text), entries)))
    }

    #[test]
    fn replaces_phrase_case_insensitively() {
        let d = [DictEntry::new("clari tree", "ClariTree")];
        assert_eq!(
            run("I work at Clari Tree now.", &d),
            "I work at ClariTree now."
        );
        assert_eq!(run("clari tree rocks", &d), "ClariTree rocks");
    }

    #[test]
    fn only_on_word_boundaries() {
        let d = [DictEntry::new("tree", "Tree™")];
        assert_eq!(
            run("Street trees and a tree.", &d),
            "Street trees and a Tree™."
        );
    }

    #[test]
    fn longest_phrase_wins() {
        let d = [
            DictEntry::new("clari tree", "ClariTree"),
            DictEntry::new("clari tree app", "ClariTree App"),
        ];
        assert_eq!(run("open the clari tree app", &d), "Open the ClariTree App");
    }

    #[test]
    fn written_form_keeps_its_punctuation() {
        let d = [
            DictEntry::new("node js", "Node.js"),
            DictEntry::new("see plus plus", "C++"),
        ];
        assert_eq!(
            run("I use node js and see plus plus.", &d),
            "I use Node.js and C++."
        );
    }

    #[test]
    fn empty_dictionary_is_a_no_op() {
        assert_eq!(run("nothing changes", &[]), "Nothing changes");
    }

    #[test]
    fn csv_round_trip_with_commas_and_quotes() {
        let entries = vec![
            DictEntry::new("clari tree", "ClariTree"),
            DictEntry::new("acme inc", "Acme, Inc."),
            DictEntry::new("quote", "say \"hi\""),
        ];
        let csv = export_csv(&entries);
        assert!(csv.starts_with("spoken,written\r\n"));
        assert_eq!(import_csv(&csv).unwrap(), entries);
    }

    #[test]
    fn csv_without_header_and_with_blank_lines() {
        let entries = import_csv("clari tree,ClariTree\n\nmurmur app,Murmur\n").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1], DictEntry::new("murmur app", "Murmur"));
    }

    #[test]
    fn csv_rejects_bad_rows() {
        assert!(import_csv("only one column\n").is_err());
        assert!(import_csv(",missing spoken\n").is_err());
    }
}
