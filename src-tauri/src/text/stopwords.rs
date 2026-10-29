//! Stopwords (spec 6.7): words too common to count for "most used word".
//! The lists are parsed once, on first use.

use std::collections::HashSet;
use std::sync::OnceLock;

const LISTS: &str = include_str!("stopwords.json");

fn stopwords() -> &'static HashSet<String> {
    static SET: OnceLock<HashSet<String>> = OnceLock::new();
    SET.get_or_init(|| {
        let json: serde_json::Value = serde_json::from_str(LISTS).unwrap_or_default();
        ["english", "urdu"]
            .iter()
            .filter_map(|key| json.get(key)?.as_array())
            .flatten()
            .filter_map(|w| w.as_str().map(str::to_lowercase))
            .collect()
    })
}

/// Lowercase words worth counting: no stopwords, no numbers, at least two
/// letters. Apostrophes inside words stay ("o'clock").
pub fn countable_words(text: &str) -> Vec<String> {
    let set = stopwords();
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(|w| w.trim_matches('\''))
        .filter(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 2)
        .filter(|w| !set.contains(*w))
        .map(String::from)
        .collect()
}

/// Number of words in `text` that are Roman Urdu and not also English.
pub fn roman_urdu_words(text: &str) -> usize {
    static URDU: OnceLock<HashSet<String>> = OnceLock::new();
    let urdu = URDU.get_or_init(|| {
        let json: serde_json::Value = serde_json::from_str(LISTS).unwrap_or_default();
        let list = |key: &str| -> HashSet<String> {
            json.get(key)
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .filter_map(|w| w.as_str().map(str::to_lowercase))
                .collect()
        };
        let english = list("english");
        list("urdu").into_iter().filter(|w| !english.contains(w)).collect()
    });
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| urdu.contains(*w))
        .count()
}

/// True for a word too common to mean anything on its own.
pub fn is_stopword(word: &str) -> bool {
    stopwords().contains(&word.to_lowercase())
}

/// Words in the final text, as stored in transcripts.word_count.
pub fn word_count(text: &str) -> usize {
    text.split_whitespace()
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_have_the_spec_sizes() {
        let json: serde_json::Value = serde_json::from_str(LISTS).unwrap();
        assert_eq!(json["english"].as_array().unwrap().len(), 200);
        assert_eq!(json["urdu"].as_array().unwrap().len(), 100);
    }

    #[test]
    fn drops_stopwords_numbers_and_single_letters() {
        assert_eq!(
            countable_words("The quarterly report is ready, a 2nd draft at 3 pm."),
            vec!["quarterly", "report", "ready", "2nd", "draft", "pm"]
        );
    }

    #[test]
    fn drops_roman_urdu_stopwords() {
        assert_eq!(
            countable_words("Yaar kal meeting hai, deck bhej do"),
            vec!["kal", "meeting", "deck", "bhej"]
        );
    }

    #[test]
    fn keeps_inner_apostrophes() {
        assert_eq!(countable_words("Three o'clock"), vec!["three", "o'clock"]);
    }

    #[test]
    fn counts_words_not_punctuation() {
        assert_eq!(word_count("Hello, world. - How are you? "), 5);
        assert_eq!(word_count(""), 0);
    }
}
