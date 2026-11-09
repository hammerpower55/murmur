//! Addresses said out loud: "example dot com" to example.com and "sara at
//! gmail dot com" to sara@gmail.com. A chain only forms when it ends on a
//! known domain ending, so "dot" in ordinary speech stays a word.

use super::stopwords::is_stopword;
use super::Token;

const ENDINGS: &[&str] = &[
    "com", "org", "net", "io", "ai", "co", "dev", "app", "edu", "gov", "pk", "uk", "me", "info", "xyz", "tv", "ca",
    "de",
];

/// Words before "at" that are never the name in an address.
const NOT_A_NAME: &[&str] = &[
    "look", "looking", "visit", "go", "went", "email", "emailed", "mail", "site", "website", "you", "them", "met",
    "meet", "work", "works", "working", "worked", "reach", "contact", "write", "wrote", "up", "address", "him",
    "her", "us", "me", "page", "found", "find", "see", "seen", "check", "here",
];

fn plain(word: &str) -> bool {
    !word.is_empty() && word.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Domain chain from `i`: its text and the index after it.
fn chain(tokens: &[Token], i: usize) -> Option<(String, usize)> {
    let first = tokens.get(i)?.word()?;
    if !plain(first) {
        return None;
    }
    let lower = first.to_lowercase();
    if lower != "www" && is_stopword(&lower) {
        return None;
    }
    let mut parts = vec![lower];
    let mut j = i + 1;
    while tokens.get(j).is_some_and(|t| t.is_word_ci("dot")) {
        let Some(part) = tokens.get(j + 1).and_then(Token::word).filter(|w| plain(w)) else {
            break;
        };
        parts.push(part.to_lowercase());
        j += 2;
    }
    let ends_well = parts.len() >= 2 && parts.last().is_some_and(|p| ENDINGS.contains(&p.as_str()));
    ends_well.then(|| (parts.join("."), j))
}

/// A word already holding a domain: "gmail.com".
fn is_domain(word: &str) -> bool {
    let lower = word.to_lowercase();
    lower.rsplit_once('.').is_some_and(|(head, end)| !head.is_empty() && ENDINGS.contains(&end))
}

pub fn apply(tokens: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let found = chain(&tokens, i).or_else(|| {
            let word = tokens[i].word().filter(|w| is_domain(w))?;
            Some((word.to_lowercase(), i + 1))
        });
        let Some((domain, end)) = found else {
            out.push(tokens[i].clone());
            i += 1;
            continue;
        };
        // "sara at gmail dot com" takes the name along.
        let n = out.len();
        let name = (n >= 2 && out[n - 1].is_word_ci("at"))
            .then(|| out[n - 2].word())
            .flatten()
            .map(str::to_lowercase)
            .filter(|w| plain(w) && !is_stopword(w) && !NOT_A_NAME.contains(&w.as_str()));
        match name {
            Some(name) => {
                out.truncate(n - 2);
                out.push(Token::Word(format!("{name}@{domain}")));
            }
            None => out.push(Token::Word(domain)),
        }
        i = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::format::{render, tidy};
    use crate::text::tokenize;

    fn run(s: &str) -> String {
        render(&tidy(apply(tokenize(s))))
    }

    #[test]
    fn spoken_addresses() {
        assert_eq!(run("Go to example dot com today."), "Go to example.com today.");
        assert_eq!(run("open www dot murmur dot co dot uk"), "Open www.murmur.co.uk");
        assert_eq!(run("Email sara at gmail dot com please."), "Email sara@gmail.com please.");
        assert_eq!(run("Write to ali.khan at claritree.net now."), "Write to ali.khan@claritree.net now.");
    }

    #[test]
    fn ordinary_speech_stays() {
        for s in [
            "Put a dot on the map.",
            "We met at Google.",
            "The dot is red.",
            "Look at example.com today.",
            "Meet me at the mall dot com.",
        ] {
            let got = run(s);
            assert!(!got.contains('@'), "{got}");
        }
        assert_eq!(run("Put a dot on the map."), "Put a dot on the map.");
    }
}
