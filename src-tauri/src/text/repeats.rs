//! Repeated speech. Two different things look alike here:
//!
//! - `loops`: Whisper sometimes falls into repeating one phrase until the end
//!   of the window ("main shaam ko baat nahi, main shaam ko baat nahi, ...").
//!   That is the model failing, in any language, so three or more copies
//!   become one.
//! - `stutters`: a speaker saying something twice ("I think I think we
//!   should", "audio audio chat") or giving up on a sentence and starting it
//!   again. English only: Roman Urdu repeats words on purpose ("jaldi jaldi").
//!
//! Emphasis stays ("very very good", "bye bye", "I know, I know.").

use super::{is_urdu, sentence_spans, Token};

/// Longest phrase looked for in a Whisper loop.
const MAX_LOOP_WORDS: usize = 16;
/// Longest phrase a speaker is assumed to repeat.
const MAX_STUTTER_WORDS: usize = 6;
/// Most words a false start runs before the speaker begins again.
const MAX_FALSE_START_WORDS: usize = 6;

/// Words people double on purpose.
const EMPHASIS: &[&str] = &[
    "very", "really", "so", "no", "yes", "yeah", "bye", "ha", "haha", "hey", "hi", "hello", "okay", "ok", "that",
    "had", "now", "well", "never", "many", "much", "more", "too", "there", "again", "please", "go", "wow", "oh",
    "ah", "yay", "knock", "tick", "tock", "night", "quite", "super", "long", "far", "big", "little", "hot", "cold",
    "wait", "come", "run", "right", "sure", "thanks", "sorry", "nice", "good", "great", "stop", "hurry", "boom",
    "blah", "yada", "chop", "choo", "tut", "bla", "cha",
    // Correct when doubled: "I gave her her keys", "what he does does not".
    "her", "does", "did", "do",
];

/// Function words people stutter on even around a comma ("the, the deck").
const COMMA_STUTTER: &[&str] = &[
    "the", "a", "an", "i", "to", "and", "of", "in", "is", "it", "we", "you", "my", "for", "on", "at", "but", "he",
    "she", "they", "this", "with", "be", "are", "was", "can", "will", "just", "or", "if", "me", "our", "your",
    "their", "his", "her", "its", "we're", "i'm", "it's",
];

/// A sentence given up on stops on one of these, which cannot end a clause:
/// "I want to the, I want to go". Not "and" ("we need chairs and we need
/// tables"), "you" ("I told you I told him") or "on" ("I turned it on I
/// turned it off"): those are two real clauses.
const UNFINISHED: &[&str] = &[
    "the", "a", "an", "of", "my", "your", "our", "their", "you're", "i'm", "we're", "they're",
];

fn word_positions(tokens: &[Token]) -> Vec<(usize, String)> {
    tokens
        .iter()
        .enumerate()
        .filter_map(|(i, t)| t.word().map(|w| (i, w.to_lowercase())))
        .collect()
}

fn same(w: &[(usize, String)], a: usize, b: usize, n: usize) -> bool {
    (0..n).all(|k| w[a + k].1 == w[b + k].1)
}

/// Three or more copies of a phrase (five of a single word) become one.
pub fn loops(mut tokens: Vec<Token>) -> Vec<Token> {
    while let Some((keep_to, drop_to)) = find_loop(&tokens) {
        tokens.drain(keep_to + 1..=drop_to);
    }
    tokens
}

/// The last token to keep and the last to drop.
fn find_loop(tokens: &[Token]) -> Option<(usize, usize)> {
    let w = word_positions(tokens);
    for start in 0..w.len() {
        for n in 1..=MAX_LOOP_WORDS {
            let min = if n == 1 { 5 } else { 3 };
            if start + n * min > w.len() {
                break;
            }
            if n == 1 && EMPHASIS.contains(&w[start].1.as_str()) {
                continue;
            }
            if n > 1 && (1..n).all(|k| w[start + k].1 == w[start].1) {
                continue;
            }
            let mut reps = 1;
            while start + n * (reps + 1) <= w.len() && same(&w, start, start + n * reps, n) {
                reps += 1;
            }
            if reps < min {
                continue;
            }
            // The window often ends part way through one more copy.
            let end = start + n * reps;
            let mut tail = 0;
            while tail < n && end + tail < w.len() {
                let (want, got) = (&w[start + tail].1, &w[end + tail].1);
                let last = end + tail + 1 == w.len();
                if want == got || (last && want.starts_with(got.as_str())) {
                    tail += 1;
                } else {
                    break;
                }
            }
            // The loop may have been found part way through a copy: keep on
            // to the next pause, so the copy kept is a whole one.
            let drop_to = w[end + tail - 1].0;
            let mut keep_to = w[start + n - 1].0;
            for k in 0..n {
                if tokens.get(keep_to + 1).is_some_and(|t| matches!(t, Token::Close(_))) {
                    break;
                }
                match w.get(start + n + k) {
                    Some(next) if next.0 < drop_to => keep_to = next.0,
                    _ => break,
                }
            }
            return Some((keep_to, drop_to));
        }
    }
    None
}

/// Collapses a speaker's repeats and false starts, one sentence at a time.
pub fn stutters(tokens: Vec<Token>) -> Vec<Token> {
    let mut out = Vec::with_capacity(tokens.len());
    for span in sentence_spans(&tokens) {
        let sentence = &tokens[span];
        if is_urdu(sentence) {
            out.extend_from_slice(sentence);
        } else {
            out.extend(clean_sentence(sentence.to_vec()));
        }
    }
    out
}

/// "I w- I want" and "my bro- my brother": a word cut off with a hyphen goes
/// when one of the next two words starts the same way. "pre- and post-" stays.
fn drop_cut_words(s: &mut Vec<Token>) {
    let mut i = 0;
    while i < s.len() {
        let stem = s[i].word().and_then(|w| w.strip_suffix('-')).map(str::to_lowercase);
        let cut = stem.is_some_and(|stem| {
            !stem.is_empty()
                && s[i + 1..]
                    .iter()
                    .filter_map(Token::word)
                    .take(2)
                    .any(|w| w.to_lowercase().starts_with(&stem))
        });
        if cut {
            s.remove(i);
        } else {
            i += 1;
        }
    }
}

fn clean_sentence(mut s: Vec<Token>) -> Vec<Token> {
    drop_cut_words(&mut s);
    while let Some(range) = find_repeat(&s).or_else(|| find_false_start(&s)) {
        s.drain(range);
    }
    s
}

/// What is between two neighbouring words: nothing, a comma, or something
/// else (None).
fn comma_between(tokens: &[Token], a: usize, b: usize) -> Option<bool> {
    match b - a {
        1 => Some(false),
        2 if tokens[a + 1].is_comma() => Some(true),
        _ => None,
    }
}

/// The tokens of the second copy of a repeated phrase, with the comma before.
fn find_repeat(s: &[Token]) -> Option<std::ops::Range<usize>> {
    let w = word_positions(s);
    for i in 0..w.len() {
        for n in (1..=MAX_STUTTER_WORDS).rev() {
            if i + 2 * n > w.len() || !same(&w, i, i + n, n) {
                continue;
            }
            // Each copy must run without punctuation other than commas, and
            // nothing but a comma may separate them.
            let tight = (i..i + 2 * n - 1)
                .filter(|&k| k != i + n - 1)
                .all(|k| comma_between(s, w[k].0, w[k + 1].0).is_some());
            let Some(comma) = comma_between(s, w[i + n - 1].0, w[i + n].0) else {
                continue;
            };
            if !tight {
                continue;
            }
            let phrase = &w[i..i + n];
            if phrase.iter().all(|(_, x)| EMPHASIS.contains(&x.as_str())) {
                continue;
            }
            // "Room 5 5", and "five plus five plus ten" is arithmetic.
            if phrase.iter().any(|(_, x)| x.chars().all(|c| c.is_ascii_digit()) || super::numbers::is_number_word(x)) {
                continue;
            }
            if n == 1 && comma && !COMMA_STUTTER.contains(&phrase[0].1.as_str()) {
                continue;
            }
            // "I know, I know." is emphasis, unlike "I know, I know he said".
            let last = w[i + 2 * n - 1].0;
            if n > 1 && comma && s.get(last + 1).is_none_or(Token::ends_sentence) {
                continue;
            }
            return Some(w[i + n - 1].0 + 1..last + 1);
        }
    }
    None
}

/// "I want to the, I want to go": a run that stops on an unfinished word and
/// starts again with the same two words. Returns the tokens given up on.
fn find_false_start(s: &[Token]) -> Option<std::ops::Range<usize>> {
    let w = word_positions(s);
    for i in 0..w.len() {
        // Two words straight after each other are a repeat, not a false start.
        for k in 3..=MAX_FALSE_START_WORDS {
            if i + k + 2 > w.len() || !same(&w, i, i + k, 2) {
                continue;
            }
            if !UNFINISHED.contains(&w[i + k - 1].1.as_str()) {
                continue;
            }
            // Only for a run the speaker clearly restarted: no other commas.
            let inner = s[w[i].0..w[i + k - 1].0].iter().any(|t| matches!(t, Token::Close(_)));
            if !inner {
                return Some(w[i].0..w[i + k].0);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::format::{render, tidy};
    use crate::text::tokenize;

    fn loop_fix(s: &str) -> String {
        render(&tidy(loops(tokenize(s))))
    }

    fn fix(s: &str) -> String {
        render(&tidy(stutters(tokenize(s))))
    }

    #[test]
    fn whisper_loops_collapse_in_any_language() {
        assert_eq!(
            loop_fix("Koi baat nahi, main shaam ko baat nahi, main shaam ko baat nahi, main shaam ko baat nahi, main shaam ko baat nah"),
            "Koi baat nahi, main shaam ko baat nahi"
        );
        assert_eq!(loop_fix("Thank you. Thank you. Thank you. Thank you."), "Thank you.");
    }

    #[test]
    fn two_copies_are_not_a_loop() {
        let s = "We tried again, we tried again.";
        assert_eq!(loop_fix(s), s);
        assert_eq!(loop_fix("Ha ha ha ha ha ha."), "Ha ha ha ha ha ha.");
    }

    #[test]
    fn stutters_collapse() {
        assert_eq!(fix("I think I think we should go."), "I think we should go.");
        assert_eq!(fix("I need audio audio chat."), "I need audio chat.");
        assert_eq!(fix("Send the, the deck."), "Send the deck.");
        assert_eq!(fix("In the in the morning."), "In the morning.");
        assert_eq!(fix("We should, we should leave now."), "We should leave now.");
    }

    #[test]
    fn false_starts_are_dropped() {
        assert_eq!(fix("I want to the I want to go home."), "I want to go home.");
        assert_eq!(fix("Maybe because you're maybe because you won."), "Maybe because you won.");
        assert_eq!(fix("I w- I want to go home."), "I want to go home.");
        assert_eq!(fix("My bro- my brother lives here."), "My brother lives here.");
    }

    #[test]
    fn two_real_clauses_are_not_a_false_start() {
        for s in [
            "We need chairs and we need tables.",
            "I told you I told him.",
            "I turned it on I turned it off.",
            "I want this, I want that.",
            "Pre- and post-launch work.",
            "I gave her her keys back.",
            "What he does does not matter.",
        ] {
            assert_eq!(fix(s), s, "{s}");
        }
    }

    #[test]
    fn emphasis_stays() {
        for s in [
            "That was very very good.",
            "Okay bye bye.",
            "I know, I know.",
            "Hello, hello.",
            "I know that that is true.",
            "He had had enough.",
            "Room 5 5 stays.",
            "What is five plus five plus ten?",
        ] {
            assert_eq!(fix(s), s, "{s}");
        }
    }

    #[test]
    fn roman_urdu_repeats_stay() {
        for s in ["Jaldi jaldi bhej do yaar.", "Kabhi kabhi hota hai."] {
            assert_eq!(fix(s), s, "{s}");
        }
    }
}
