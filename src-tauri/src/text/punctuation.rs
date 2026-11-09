//! Spoken punctuation: "period", "comma", "question mark", "new line" and the
//! rest become the symbols. Whisper often punctuates around a spoken command
//! ("Hello, comma, world."); `format::tidy` collapses the leftovers.
//!
//! The word is only a command where it cannot be the noun: "the second
//! period of the game", "a comma", "the trailing comma" and "colon cancer"
//! stay as said.

use super::{match_phrase, Token};

/// Before one of these the word is a noun.
const NOUN_LEADS: &[&str] = &[
    "a", "an", "the", "this", "that", "these", "those", "my", "your", "our", "their", "his", "her", "its", "one",
    "each", "every", "any", "no", "which", "what", "per", "long", "short", "first", "second", "third", "fourth",
    "last", "next", "same", "whole", "entire", "certain", "given", "time", "grace", "trial", "free", "billing",
    "cooling", "waiting", "notice", "transition", "honeymoon", "rainy", "final", "oxford", "serial", "extra",
    "trailing", "missing", "add", "use", "remove", "semi", "another", "of", "or", "and", "with", "without", "called",
    "word", "using",
];
/// After one of these the word is a noun: "the period of", "colon cancer".
const NOUN_TAILS: &[&str] = &[
    "of", "when", "where", "during", "for", "is", "was", "at", "cancer", "surgery", "separated", "delimited",
    "splice", "goes", "should", "instead",
];

/// Two words back these still make a noun: "a difficult period".
const DETERMINERS: &[&str] = &[
    "a", "an", "the", "this", "that", "my", "your", "our", "their", "his", "her", "its", "each", "every", "any",
];

fn command(tokens: &[Token], i: usize, end: usize) -> bool {
    let word_at = |k: Option<usize>| k.and_then(|p| tokens.get(p)).and_then(Token::word).map(str::to_lowercase);
    // Leaving the word in is easy to spot; eating a word is not.
    let could_be_noun = tokens[i].is_word_ci("period") || tokens[i].is_word_ci("colon");
    if could_be_noun
        && word_at(i.checked_sub(1)).is_some()
        && word_at(i.checked_sub(2)).is_some_and(|w| DETERMINERS.contains(&w.as_str()))
    {
        return false;
    }
    let lead = i
        .checked_sub(1)
        .and_then(|p| tokens[p].word())
        .is_some_and(|w| NOUN_LEADS.contains(&w.to_lowercase().as_str()));
    let tail = tokens
        .get(end)
        .and_then(Token::word)
        .is_some_and(|w| NOUN_TAILS.contains(&w.to_lowercase().as_str()));
    !lead && !tail
}

fn close(s: &str) -> Token {
    Token::Close(s.into())
}

pub fn apply(tokens: Vec<Token>) -> Vec<Token> {
    let commands: Vec<(Vec<String>, Token)> = [
        ("new paragraph", Token::Break(2)),
        ("new line", Token::Break(1)),
        ("question mark", close("?")),
        ("exclamation point", close("!")),
        ("exclamation mark", close("!")),
        ("full stop", close(".")),
        ("period", close(".")),
        ("comma", close(",")),
        ("semicolon", close(";")),
        ("semi colon", close(";")),
        ("colon", close(":")),
        ("ellipsis", close("...")),
        ("dot dot dot", close("...")),
        ("open quote", Token::Open("\"".into())),
        ("close quote", close("\"")),
        ("end quote", close("\"")),
        ("unquote", close("\"")),
        ("open parenthesis", Token::Open("(".into())),
        ("close parenthesis", close(")")),
        ("open bracket", Token::Open("(".into())),
        ("close bracket", close(")")),
        ("em dash", Token::Word("\u{2014}".into())),
        ("em-dash", Token::Word("\u{2014}".into())),
        ("emdash", Token::Word("\u{2014}".into())),
        ("ampersand", Token::Word("&".into())),
        ("asterisk", Token::Word("*".into())),
    ]
    .into_iter()
    .map(|(phrase, token)| (phrase.split(' ').map(String::from).collect(), token))
    .collect();

    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let hit = commands
            .iter()
            .find_map(|(words, token)| match_phrase(&tokens, i, words).map(|end| (end, token)))
            .filter(|(end, _)| command(&tokens, i, *end));
        match hit {
            Some((end, token)) => {
                // A break swallows a comma Whisper put before it; a full
                // stop before it is a real sentence end and stays.
                if matches!(token, Token::Break(_)) {
                    while out.last().is_some_and(Token::is_comma) {
                        out.pop();
                    }
                }
                // Whisper's full stop after a closing command is noise:
                // "Is it ready? Question mark." and "Open quote, hello".
                out.push(token.clone());
                i = end;
                if matches!(token, Token::Open(_)) {
                    while tokens.get(i).is_some_and(Token::is_comma) {
                        i += 1;
                    }
                }
            }
            None => {
                out.push(tokens[i].clone());
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::format::{render, tidy};
    use crate::text::tokenize;

    fn run(text: &str) -> String {
        render(&tidy(apply(tokenize(text))))
    }

    #[test]
    fn spoken_marks_become_symbols() {
        assert_eq!(
            run("hello comma how are you question mark fine period"),
            "Hello, how are you? Fine."
        );
        assert_eq!(run("done full stop"), "Done.");
        assert_eq!(run("wow exclamation point groceries colon milk semicolon eggs"), "Wow! Groceries: milk; eggs");
    }

    #[test]
    fn whisper_punctuation_around_commands_collapses() {
        assert_eq!(run("Hello, comma, world. Period."), "Hello, world.");
        assert_eq!(run("Is it ready? Question mark."), "Is it ready?");
    }

    #[test]
    fn line_and_paragraph_breaks() {
        assert_eq!(
            run("Thanks. New line. Bye new paragraph see you"),
            "Thanks.\nBye\n\nSee you"
        );
    }

    #[test]
    fn breaks_at_the_edges_are_dropped() {
        assert_eq!(run("new line hello new paragraph"), "Hello");
    }

    #[test]
    fn quotes_and_symbols() {
        assert_eq!(run("he said open quote hello close quote"), "He said \"hello\"");
        assert_eq!(run("salt ampersand pepper"), "Salt & pepper");
    }

    #[test]
    fn the_nouns_stay() {
        for s in [
            "The second period of the game was long.",
            "Add a comma here.",
            "Remove the trailing comma.",
            "That was a long period.",
            "He has colon cancer.",
            "It needs a question mark.",
            "The period of time was short.",
            "Use a semicolon there.",
            "It was a difficult period.",
        ] {
            assert_eq!(run(s), s, "{s}");
        }
    }
}
