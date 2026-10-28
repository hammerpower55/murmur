//! Roman Urdu spelling normalisation (spec 6.3). Whisper spells the same
//! Urdu word many ways ("acha", "accha", "achha"); this rewrites each known
//! variant to one canonical spelling, keeping the original capitalisation.
//! The shipped table lives in roman_urdu.json and becomes editable from the
//! Dictionary page.

use std::collections::HashMap;

use super::Token;

const DEFAULT_TABLE: &str = include_str!("roman_urdu.json");

/// Variant (lowercase) to canonical spelling.
pub fn default_map() -> HashMap<String, String> {
    parse_table(DEFAULT_TABLE).unwrap_or_default()
}

/// Parses {"canonical": ["variant", ...]}. Keys starting with _ are notes.
pub fn parse_table(json: &str) -> Option<HashMap<String, String>> {
    let table: serde_json::Map<String, serde_json::Value> = serde_json::from_str(json).ok()?;
    let mut map = HashMap::new();
    for (canonical, variants) in table {
        if canonical.starts_with('_') {
            continue;
        }
        for v in variants.as_array()? {
            map.insert(v.as_str()?.to_lowercase(), canonical.clone());
        }
    }
    Some(map)
}

pub fn normalise(tokens: Vec<Token>, map: &HashMap<String, String>) -> Vec<Token> {
    tokens
        .into_iter()
        .map(|t| match t {
            Token::Word(w) => match map.get(&w.to_lowercase()) {
                Some(canonical) => Token::Word(match_case(&w, canonical)),
                None => Token::Word(w),
            },
            other => other,
        })
        .collect()
}

/// "ACHHA" to "ACHA", "Achha" to "Acha", "achha" to "acha".
fn match_case(original: &str, canonical: &str) -> String {
    let letters: Vec<char> = original.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() > 1 && letters.iter().all(|c| c.is_uppercase()) {
        return canonical.to_uppercase();
    }
    let mut chars = canonical.chars();
    match (original.chars().next(), chars.next()) {
        (Some(o), Some(first)) if o.is_uppercase() => first.to_uppercase().chain(chars).collect(),
        _ => canonical.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::format::{render, tidy};
    use crate::text::tokenize;

    fn run(text: &str) -> String {
        render(&tidy(normalise(tokenize(text), &default_map())))
    }

    #[test]
    fn shipped_table_parses_with_at_least_150_entries() {
        let map = parse_table(DEFAULT_TABLE).expect("roman_urdu.json must parse");
        assert!(map.len() >= 150, "only {} entries", map.len());
    }

    #[test]
    fn spec_examples() {
        assert_eq!(run("accha achha"), "Acha acha");
        assert_eq!(run("thik teek"), "Theek theek");
        assert_eq!(run("hy"), "Hai");
        assert_eq!(run("nahin nai"), "Nahi nahi");
        assert_eq!(run("kia"), "Kya");
        assert_eq!(run("mein mai"), "Main main");
        assert_eq!(run("kro"), "Karo");
        assert_eq!(run("kel"), "Kal");
        assert_eq!(run("bhaai"), "Bhai");
        assert_eq!(run("yar"), "Yaar");
    }

    #[test]
    fn keeps_capitalisation_and_punctuation() {
        assert_eq!(run("Accha, theek HY."), "Acha, theek HAI.");
    }

    #[test]
    fn whole_sentence() {
        assert_eq!(
            run("yar kal meeting hy, kia tum ne email dheki?"),
            "Yaar kal meeting hai, kya tum ne email dekhi?"
        );
    }

    #[test]
    fn english_words_are_never_rewritten() {
        let sentence = "Send me the deck to Sam or the team by bus, the main fir tree.";
        assert_eq!(run(sentence), sentence);
    }

    #[test]
    fn no_variant_is_a_common_english_word() {
        const ENGLISH: &[&str] = &[
            "me", "to", "or", "bus", "the", "fir", "sham", "hen", "bat", "bad", "rat", "tab",
            "mat", "a", "i", "in", "on", "an", "as", "at", "be", "by", "do", "go", "he", "if",
            "is", "it", "my", "no", "of", "so", "up", "us", "we", "hi", "pr", "bi", "sub", "par",
            "say", "sat", "was", "were", "her", "him", "his", "hum", "ham", "kin", "yes", "yeah",
        ];
        let map = default_map();
        for w in ENGLISH {
            assert!(!map.contains_key(*w), "{w} is an English word");
        }
    }

    #[test]
    fn variants_never_point_at_two_spellings() {
        let table: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(DEFAULT_TABLE).unwrap();
        let mut seen = HashMap::new();
        for (canonical, variants) in table.iter().filter(|(k, _)| !k.starts_with('_')) {
            for v in variants.as_array().unwrap() {
                let v = v.as_str().unwrap();
                assert!(
                    !table.contains_key(v),
                    "{v} is both a variant and a canonical spelling"
                );
                if let Some(prev) = seen.insert(v.to_string(), canonical.clone()) {
                    panic!("{v} maps to both {prev} and {canonical}");
                }
            }
        }
    }
}
