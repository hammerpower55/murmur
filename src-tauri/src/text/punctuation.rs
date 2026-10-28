//! Voice punctuation commands (spec 6.8, off by default): "new line",
//! "new paragraph", "comma", "period", "full stop" and "question mark"
//! become the symbols. Whisper often punctuates around a spoken command
//! ("Hello, comma, world."); `format::tidy` collapses the leftovers.

use super::{match_phrase, Token};

pub fn apply(tokens: Vec<Token>) -> Vec<Token> {
    let commands: Vec<(Vec<String>, Token)> = [
        ("new paragraph", Token::Break(2)),
        ("new line", Token::Break(1)),
        ("question mark", Token::Close("?".into())),
        ("full stop", Token::Close(".".into())),
        ("period", Token::Close(".".into())),
        ("comma", Token::Close(",".into())),
    ]
    .into_iter()
    .map(|(phrase, token)| (phrase.split(' ').map(String::from).collect(), token))
    .collect();

    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let hit = commands
            .iter()
            .find_map(|(words, token)| match_phrase(&tokens, i, words).map(|end| (end, token)));
        match hit {
            Some((end, token)) => {
                // A break swallows a comma Whisper put before it; a full
                // stop before it is a real sentence end and stays.
                if matches!(token, Token::Break(_)) {
                    while out.last().is_some_and(Token::is_comma) {
                        out.pop();
                    }
                }
                out.push(token.clone());
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
}
