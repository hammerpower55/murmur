//! Last pipeline stage: tidy punctuation left behind by earlier stages,
//! capitalise sentences, and lay tokens out with single spaces.

use super::Token;

const SENTENCE_PUNCT: &[&str] = &[",", ".", "?", "!", ";", ":", "...", "\u{2026}"];

fn is_sentence_punct(t: &Token) -> bool {
    matches!(t, Token::Close(p) if SENTENCE_PUNCT.contains(&p.as_str()))
}

/// Removes punctuation that no longer belongs after words were dropped,
/// then capitalises the first word of every sentence.
pub fn tidy(tokens: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    for t in tokens {
        let prev = out.last();
        let at_start = prev.is_none_or(|p| matches!(p, Token::Break(_) | Token::Open(_)));
        match &t {
            // Nothing to punctuate yet.
            _ if is_sentence_punct(&t) && at_start => continue,
            Token::Break(_) if prev.is_none() => continue,
            // ", ," and ". ," collapse; ", ." keeps the stronger mark.
            Token::Close(p) if p == "," && prev.is_some_and(is_sentence_punct) => continue,
            _ if is_sentence_punct(&t) && prev.is_some_and(Token::is_comma) => {
                out.pop();
            }
            Token::Close(p) if prev.is_some_and(|q| matches!(q, Token::Close(x) if x == p)) => {
                continue
            }
            // "ready? ." keeps the first sentence end.
            Token::Close(_)
                if t.ends_sentence()
                    && prev.is_some_and(|q| matches!(q, Token::Close(_)) && q.ends_sentence()) =>
            {
                continue
            }
            _ => {}
        }
        out.push(t);
    }
    // A trailing comma or line break has nothing after it.
    while out
        .last()
        .is_some_and(|t| t.is_comma() || matches!(t, Token::Break(_)))
    {
        out.pop();
    }
    capitalise(&mut out);
    out
}

fn capitalise(tokens: &mut [Token]) {
    let mut sentence_start = true;
    for t in tokens.iter_mut() {
        match t {
            Token::Word(w) => {
                if sentence_start {
                    *w = capitalise_word(w);
                }
                sentence_start = false;
            }
            Token::Raw(r) => {
                sentence_start = r.trim_end().ends_with(['.', '?', '!', '\n']);
            }
            Token::Open(_) => {}
            other => {
                if other.ends_sentence() {
                    sentence_start = true;
                } else if !matches!(other, Token::Close(p) if p == "\"" || p == "\u{201D}" || p == ")")
                {
                    sentence_start = false;
                }
            }
        }
    }
}

/// Uppercases the first letter, leaving handles, links and numbers alone.
fn capitalise_word(w: &str) -> String {
    if w.contains('@') || w.contains("://") || w.starts_with("www.") {
        return w.to_string();
    }
    let mut chars = w.chars();
    match chars.next() {
        Some(first) if first.is_lowercase() => first.to_uppercase().chain(chars).collect(),
        _ => w.to_string(),
    }
}

/// Lays tokens out: one space between words, none before closing
/// punctuation or after opening punctuation, line breaks as newlines.
pub fn render(tokens: &[Token]) -> String {
    let mut out = String::new();
    let mut prev: Option<&Token> = None;
    for t in tokens {
        let no_space = matches!(
            (prev, t),
            (None, _)
                | (_, Token::Close(_) | Token::Break(_))
                | (Some(Token::Open(_) | Token::Break(_)), _)
        );
        if !no_space {
            out.push(' ');
        }
        match t {
            Token::Word(s) | Token::Close(s) | Token::Open(s) | Token::Raw(s) => out.push_str(s),
            Token::Break(n) => out.push_str(&"\n".repeat(*n)),
        }
        prev = Some(t);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::tokenize;

    fn fmt(s: &str) -> String {
        render(&tidy(tokenize(s)))
    }

    #[test]
    fn capitalises_each_sentence() {
        assert_eq!(
            fmt("hello there. how are you? fine! ok"),
            "Hello there. How are you? Fine! Ok"
        );
    }

    #[test]
    fn single_space_after_punctuation_and_none_before() {
        assert_eq!(fmt("one ,two.three   four ."), "One, two.three four.");
        assert_eq!(fmt("wait , what ?"), "Wait, what?");
    }

    #[test]
    fn collapses_whitespace() {
        assert_eq!(fmt("a    b\t c"), "A b c");
    }

    #[test]
    fn drops_orphaned_commas() {
        assert_eq!(fmt(", so , , we go ,"), "So, we go");
        assert_eq!(fmt("we go , ."), "We go.");
        assert_eq!(fmt("done. , next"), "Done. Next");
    }

    #[test]
    fn drops_leading_punctuation_and_duplicate_marks() {
        assert_eq!(fmt(". . hello.."), "Hello.");
        assert_eq!(fmt("really?? yes"), "Really? Yes");
    }

    #[test]
    fn keeps_ellipsis_and_quotes() {
        assert_eq!(
            fmt("well... he said \"hi\" (twice)"),
            "Well... he said \"hi\" (twice)"
        );
        assert_eq!(fmt("\"stop.\" then go"), "\"Stop.\" Then go");
    }

    #[test]
    fn capitalises_after_line_breaks() {
        assert_eq!(fmt("first\nsecond\n\nthird"), "First\nSecond\n\nThird");
    }

    #[test]
    fn leaves_links_handles_and_numbers_alone() {
        assert_eq!(fmt("www.example.com is up"), "www.example.com is up");
        assert_eq!(fmt("user@mail.com wrote"), "user@mail.com wrote");
        assert_eq!(fmt("3pm works"), "3pm works");
    }

    #[test]
    fn does_not_lowercase_anything() {
        assert_eq!(fmt("NASA and iPhone. ok"), "NASA and iPhone. Ok");
    }

    #[test]
    fn raw_tokens_are_untouched() {
        let tokens = vec![
            Token::Word("see".into()),
            Token::Raw("my  Address,\nLine 2.".into()),
            Token::Word("thanks".into()),
        ];
        assert_eq!(render(&tidy(tokens)), "See my  Address,\nLine 2. Thanks");
    }
}
