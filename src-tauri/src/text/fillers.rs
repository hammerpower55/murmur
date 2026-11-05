//! Filler word removal (spec 6.8). Rule based and deterministic.
//!
//! - Hesitations (um, uh, er, ah, hmm, mm and stretched forms) go anywhere.
//! - "like" goes between commas or at sentence start.
//! - "you know", "I mean", "sort of", "kind of" go when followed by a comma
//!   or the end of a sentence, unless they are part of a question or noun
//!   phrase ("do you know", "what kind of").
//! - "basically", "actually" and "so" go at sentence start. "so" only when it
//!   introduces a clause ("So we went"), never as an intensifier ("So sorry",
//!   "So far").
//! - Stuttered repeats collapse ("the the" to "the"), but only for English
//!   function words: Roman Urdu repeats words on purpose ("jaldi jaldi").

use super::{match_phrase, Token};

pub fn remove(tokens: Vec<Token>) -> Vec<Token> {
    let mut t = tokens;
    remove_hesitations(&mut t);
    remove_hedges(&mut t);
    remove_like(&mut t);
    remove_openers(&mut t);
    remove_stutters(&mut t);
    t
}

/// um, uh, uhm, er, erm, ah, hmm, mm, including stretched forms like
/// "ummm" or "hmmm". Squeezing repeated letters makes "umbrella" or
/// "summer" safe: they never squeeze down to a bare hesitation.
pub fn is_hesitation(word: &str) -> bool {
    let w = word.to_lowercase();
    let mut squeezed = String::new();
    for c in w.chars() {
        if !squeezed.ends_with(c) {
            squeezed.push(c);
        }
    }
    match squeezed.as_str() {
        "um" | "uh" | "uhm" | "er" | "erm" | "ah" | "hm" => true,
        // "mm" and longer, but not a lone "m".
        "m" => w.len() >= 2,
        _ => false,
    }
}

/// Removes tokens[start..end] and the commas that framed them, so
/// "I, um, think" becomes "I think" and "Um, so" becomes "so".
fn cut(tokens: &mut Vec<Token>, start: usize, end: usize) {
    let after_comma = tokens.get(end).is_some_and(Token::is_comma);
    let end = if after_comma { end + 1 } else { end };
    let next_ends = tokens
        .get(end)
        .is_none_or(|t| t.ends_sentence() || matches!(t, Token::Close(_)));
    let before_comma = start > 0 && tokens[start - 1].is_comma();
    let start = if before_comma && (after_comma || next_ends) {
        start - 1
    } else {
        start
    };
    tokens.drain(start..end);
}

fn at_sentence_start(tokens: &[Token], i: usize) -> bool {
    tokens[..i]
        .iter()
        .rev()
        .find(|t| !matches!(t, Token::Open(_)))
        .is_none_or(Token::ends_sentence)
}

fn remove_hesitations(tokens: &mut Vec<Token>) {
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i].word().is_some_and(is_hesitation) {
            cut(tokens, i, i + 1);
            i = i.saturating_sub(1);
        } else {
            i += 1;
        }
    }
}

const HEDGES: &[&[&str]] = &[
    &["you", "know"],
    &["i", "mean"],
    &["sort", "of"],
    &["kind", "of"],
];

/// Words that turn a hedge into real content: "do you know", "what I mean",
/// "this kind of", "the sort of".
const HEDGE_GUARDS: &[&str] = &[
    "do", "does", "did", "don't", "doesn't", "didn't", "what", "how", "if", "whether", "the", "a",
    "this", "that", "which", "what's", "any", "some",
];

fn remove_hedges(tokens: &mut Vec<Token>) {
    let hedges: Vec<Vec<String>> = HEDGES
        .iter()
        .map(|h| h.iter().map(|w| w.to_string()).collect())
        .collect();
    let mut i = 0;
    while i < tokens.len() {
        let hit = hedges.iter().find_map(|h| match_phrase(tokens, i, h));
        if let Some(end) = hit {
            let followed_ok = tokens
                .get(end)
                .is_none_or(|t| t.is_comma() || t.ends_sentence());
            let guarded = i > 0
                && tokens[i - 1]
                    .word()
                    .is_some_and(|w| HEDGE_GUARDS.contains(&w.to_lowercase().as_str()));
            if followed_ok && !guarded {
                cut(tokens, i, end);
                i = i.saturating_sub(1);
                continue;
            }
        }
        i += 1;
    }
}

fn remove_like(tokens: &mut Vec<Token>) {
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i].is_word_ci("like") {
            let between_commas =
                i > 0 && tokens[i - 1].is_comma() && tokens.get(i + 1).is_some_and(Token::is_comma);
            if between_commas || at_sentence_start(tokens, i) {
                cut(tokens, i, i + 1);
                i = i.saturating_sub(1);
                continue;
            }
        }
        i += 1;
    }
}

/// "so" is only a filler when a clause follows it.
const SO_FOLLOWERS: &[&str] = &[
    "i",
    "we",
    "you",
    "they",
    "he",
    "she",
    "it",
    "the",
    "this",
    "there",
    "let's",
    "i'm",
    "we're",
    "you're",
    "they're",
    "it's",
    "that's",
    "there's",
    "if",
    "now",
    "then",
    "anyway",
    "yeah",
    "okay",
    "ok",
    "basically",
    "actually",
    "my",
    "our",
    "your",
];

fn remove_openers(tokens: &mut Vec<Token>) {
    let mut i = 0;
    while i < tokens.len() {
        let removable = at_sentence_start(tokens, i)
            && match tokens[i].word().map(str::to_lowercase).as_deref() {
                Some("basically") | Some("actually") => true,
                Some("so") => match tokens.get(i + 1) {
                    Some(t) if t.is_comma() => true,
                    Some(Token::Word(next)) => SO_FOLLOWERS.contains(&next.to_lowercase().as_str()),
                    _ => false,
                },
                _ => false,
            };
        if removable {
            // Stay at i: the next word may be another opener.
            cut(tokens, i, i + 1);
        } else {
            i += 1;
        }
    }
}

/// English function words that people stutter on. Content words and Roman
/// Urdu are never collapsed, and "that that" / "had had" can be correct.
const STUTTER_WORDS: &[&str] = &[
    "the", "a", "an", "i", "to", "and", "of", "in", "is", "it", "we", "you", "my", "for", "on",
    "at", "but", "he", "she", "they", "this", "with", "be", "are", "was", "can", "will", "just",
    "or", "if", "me", "our", "your", "their", "his", "her", "its", "so", "we're", "i'm", "it's",
];

fn remove_stutters(tokens: &mut Vec<Token>) {
    let mut i = 1;
    while i < tokens.len() {
        let repeat = match (tokens[i - 1].word(), tokens[i].word()) {
            (Some(a), Some(b)) => {
                a.eq_ignore_ascii_case(b) && STUTTER_WORDS.contains(&a.to_lowercase().as_str())
            }
            _ => false,
        };
        if repeat {
            tokens.remove(i);
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::format::{render, tidy};
    use crate::text::tokenize;

    fn clean(s: &str) -> String {
        render(&tidy(remove(tokenize(s))))
    }

    // Hesitations.
    #[test]
    fn t01_um_at_start() {
        assert_eq!(clean("Um, I think so."), "I think so.");
    }
    #[test]
    fn t02_uh_mid_sentence() {
        assert_eq!(clean("I uh need it"), "I need it");
    }
    #[test]
    fn t03_between_commas() {
        assert_eq!(clean("We need, um, three apples."), "We need three apples.");
    }
    #[test]
    fn t04_before_period() {
        assert_eq!(clean("That is all, uh."), "That is all.");
    }
    #[test]
    fn t05_stretched_forms() {
        assert_eq!(clean("Ummm, hmmm, okay"), "Okay");
        assert_eq!(clean("Uhh fine"), "Fine");
    }
    #[test]
    fn t06_er_erm_ah_mm() {
        assert_eq!(clean("Er, erm, ah, mm, yes."), "Yes.");
    }
    #[test]
    fn t07_only_fillers_leaves_nothing() {
        assert_eq!(clean("Um, uh, hmm."), "");
    }
    #[test]
    fn t08_hesitation_case_insensitive() {
        assert_eq!(clean("UM so UH we go"), "We go");
    }
    #[test]
    fn t09_words_containing_fillers_survive() {
        assert_eq!(
            clean("Umbrella, error, ahead, hummus, summer."),
            "Umbrella, error, ahead, hummus, summer."
        );
    }

    // like.
    #[test]
    fn t10_like_between_commas() {
        assert_eq!(clean("It was, like, huge."), "It was huge.");
    }
    #[test]
    fn t11_like_at_sentence_start() {
        assert_eq!(clean("Like, I said no."), "I said no.");
    }
    #[test]
    fn t12_like_as_verb_stays() {
        assert_eq!(clean("I like this plan."), "I like this plan.");
    }
    #[test]
    fn t13_like_as_comparison_stays() {
        assert_eq!(clean("It looks like rain."), "It looks like rain.");
    }
    #[test]
    fn t14_like_after_second_sentence() {
        assert_eq!(clean("Fine. Like, whatever."), "Fine. Whatever.");
    }

    // you know, I mean, sort of, kind of.
    #[test]
    fn t15_you_know_before_comma() {
        assert_eq!(clean("You know, it's fine."), "It's fine.");
    }
    #[test]
    fn t16_you_know_at_end() {
        assert_eq!(clean("It's fine, you know."), "It's fine.");
    }
    #[test]
    fn t17_do_you_know_stays() {
        assert_eq!(clean("Do you know, he left?"), "Do you know, he left?");
    }
    #[test]
    fn t18_you_know_followed_by_word_stays() {
        assert_eq!(clean("You know the answer."), "You know the answer.");
    }
    #[test]
    fn t19_i_mean_before_comma() {
        assert_eq!(clean("I mean, really?"), "Really?");
    }
    #[test]
    fn t20_i_mean_it_stays() {
        assert_eq!(clean("I mean it."), "I mean it.");
    }
    #[test]
    fn t21_what_i_mean_stays() {
        assert_eq!(clean("That is what I mean."), "That is what I mean.");
    }
    #[test]
    fn t22_sort_of_at_end() {
        assert_eq!(clean("It worked, sort of."), "It worked.");
    }
    #[test]
    fn t23_kind_of_before_comma() {
        assert_eq!(clean("It was kind of, slow."), "It was slow.");
    }
    #[test]
    fn t24_kind_of_noun_phrase_stays() {
        assert_eq!(clean("This kind of thing."), "This kind of thing.");
        assert_eq!(clean("What kind of?"), "What kind of?");
    }
    #[test]
    fn t25_kind_of_adverb_stays() {
        assert_eq!(clean("It is kind of slow."), "It is kind of slow.");
    }

    // Sentence openers.
    #[test]
    fn t26_basically_at_start() {
        assert_eq!(clean("Basically, we need more time."), "We need more time.");
    }
    #[test]
    fn t27_actually_at_start() {
        assert_eq!(clean("Actually the build passed."), "The build passed.");
    }
    #[test]
    fn t28_actually_mid_sentence_stays() {
        assert_eq!(clean("It actually works."), "It actually works.");
    }
    #[test]
    fn t29_so_introducing_a_clause() {
        assert_eq!(clean("So we went home."), "We went home.");
        assert_eq!(clean("So, the plan is set."), "The plan is set.");
    }
    #[test]
    fn t30_so_as_intensifier_stays() {
        assert_eq!(clean("So sorry about that."), "So sorry about that.");
        assert_eq!(clean("So far so good."), "So far so good.");
    }
    #[test]
    fn t31_stacked_openers() {
        assert_eq!(clean("So basically, actually we ship."), "We ship.");
    }
    #[test]
    fn t32_opener_in_second_sentence() {
        assert_eq!(clean("Done. So we can merge."), "Done. We can merge.");
    }
    #[test]
    fn t33_so_mid_sentence_stays() {
        assert_eq!(clean("I was tired so I slept."), "I was tired so I slept.");
    }

    // Stutters.
    #[test]
    fn t34_the_the() {
        assert_eq!(clean("Send the the deck."), "Send the deck.");
    }
    #[test]
    fn t35_triple_stutter_any_case() {
        assert_eq!(clean("I I i think so."), "I think so.");
    }
    #[test]
    fn t36_that_that_stays() {
        assert_eq!(
            clean("I know that that is true."),
            "I know that that is true."
        );
    }
    #[test]
    fn t37_roman_urdu_repeats_stay() {
        assert_eq!(
            clean("Jaldi jaldi bhej do, kabhi kabhi hota hai."),
            "Jaldi jaldi bhej do, kabhi kabhi hota hai."
        );
    }

    // Mixed and untouched.
    #[test]
    fn t38_everything_at_once() {
        assert_eq!(
            clean("Um, so, like, the the report is, uh, ready, you know."),
            "The report is ready."
        );
    }
    #[test]
    fn t39_roman_urdu_sentence_untouched() {
        assert_eq!(
            clean("Yaar kal meeting hai, can you send me the deck before lunch?"),
            "Yaar kal meeting hai, can you send me the deck before lunch?"
        );
    }
    #[test]
    fn t40_clean_text_untouched() {
        assert_eq!(
            clean("Please send the quarterly report by Friday."),
            "Please send the quarterly report by Friday."
        );
    }
}
