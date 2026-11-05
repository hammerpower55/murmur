//! Fixing misheard words using the user's own words, before the rest of the
//! pipeline runs. The speech recogniser sometimes writes a word that sounds
//! like the right one: "mars" for a name, "beater" for beta, "dance cards"
//! when the field above says Chance cards.
//!
//! Only words the user uses are ever offered as replacements: dictionary
//! entries (including learned ones) and words already in the text before the
//! cursor or in recent dictation. A run of transcript words that sounds like
//! one of them becomes a candidate (`candidates`, pure and cheap). The local
//! language model (engine/llm.rs, run by engine/fixer.rs) then scores the
//! text both ways in context and the swap is made only when the user's word
//! makes it much more likely. Earlier versions let the model propose any
//! word or rewrite the text: they missed real errors and invented new ones
//! ("weather" to "water"), so open ended fixes are not attempted.

use serde::{Deserialize, Serialize};

/// Text before the cursor used as context, in words from the end.
const BEFORE_WORDS: usize = 80;
/// Earlier dictation used as context, in words from the end.
const EARLIER_WORDS: usize = 60;

/// What the fix-up may use besides the transcript.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// Text already in the field before the cursor.
    pub before: String,
    /// Recent dictation into the same app.
    pub earlier: String,
    /// Written forms from the dictionary.
    pub vocabulary: Vec<String>,
    /// Also let the model propose its own word for any word it finds
    /// unlikely ("Fix any misheard word").
    pub any_word: bool,
}

/// The last `n` words of `text`, on one line.
fn tail_words(text: &str, n: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    words[words.len().saturating_sub(n)..].join(" ")
}

/// The context written out as plain text to put before the transcript when
/// scoring it: the text before the cursor, else earlier dictation.
pub fn lead(ctx: &Context) -> String {
    let before = tail_words(&ctx.before, BEFORE_WORDS);
    let text = if before.is_empty() {
        tail_words(&ctx.earlier, EARLIER_WORDS)
    } else {
        before
    };
    if text.is_empty() {
        String::new()
    } else {
        format!("{text}\n\n")
    }
}

/// One misheard stretch and the user's word that replaced it, kept with the
/// transcript so History can show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Swap {
    pub heard: String,
    pub meant: String,
}

/// A run of transcript words that sounds like a word the user uses.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// Byte range in the transcript.
    pub start: usize,
    pub end: usize,
    pub meant: String,
    /// From the dictionary rather than the text around the cursor.
    pub from_dictionary: bool,
}

/// Most candidates scored per dictation.
const MAX_CANDIDATES: usize = 8;

/// Words in the context worth offering as replacements: three letters or
/// more, not stopwords. Lower case unless written with a capital mid
/// sentence somewhere.
pub fn context_words(ctx: &Context) -> Vec<String> {
    let text = format!(
        "{}
{}",
        tail_words(&ctx.earlier, EARLIER_WORDS),
        tail_words(&ctx.before, BEFORE_WORDS)
    );
    let mut out: Vec<String> = Vec::new();
    let mut sentence_start = true;
    let mut last_end = 0;
    for (s, e) in word_spans(&text) {
        if text[last_end..s].contains(['.', '!', '?', '\n', '\r']) {
            sentence_start = true;
        }
        last_end = e;
        let word = &text[s..e];
        let starts = std::mem::replace(&mut sentence_start, false);
        if word.chars().count() < 3
            || !word.chars().all(char::is_alphabetic)
            || super::stopwords::is_stopword(word)
        {
            continue;
        }
        let capital = word.chars().next().is_some_and(char::is_uppercase) && !starts;
        let shown = if capital { word.to_string() } else { word.to_lowercase() };
        match out.iter_mut().find(|w| w.eq_ignore_ascii_case(word)) {
            Some(existing) if capital => *existing = shown,
            Some(_) => {}
            None => out.push(shown),
        }
    }
    out
}

/// Runs of one to three transcript words that sound like a dictionary word
/// or a word from the context. Only these are ever scored, so a dictation
/// with none never runs the model.
pub fn candidates(transcript: &str, ctx: &Context) -> Vec<Candidate> {
    let mut offers: Vec<(String, bool)> = ctx
        .vocabulary
        .iter()
        .map(|w| w.trim())
        .filter(|w| !w.is_empty())
        .map(|w| (w.to_string(), true))
        .collect();
    for w in context_words(ctx) {
        if !offers.iter().any(|(o, _)| o.eq_ignore_ascii_case(&w)) {
            offers.push((w, false));
        }
    }
    let spans = word_spans(transcript);
    let plain = |s: usize, e: usize| transcript[s..e].chars().all(|c| c.is_alphabetic() || c == '\'');
    let mut found: Vec<Candidate> = Vec::new();
    for i in 0..spans.len() {
        for j in i..spans.len().min(i + 3) {
            if !plain(spans[j].0, spans[j].1) {
                break;
            }
            let (start, end) = (spans[i].0, spans[j].1);
            let heard = &transcript[start..end];
            let heard_words = words(heard);
            let small = |w: Option<&String>| w.is_some_and(|w| super::stopwords::is_stopword(w));
            // A stretch that starts or ends on a small word ("board to be")
            // would swallow words that were heard right.
            let edge = heard_words.len() > 1 && (small(heard_words.first()) || small(heard_words.last()));
            if edge
                || heard_words.iter().all(|w| super::stopwords::is_stopword(w))
                || heard_words.iter().any(|w| super::fillers::is_hesitation(w))
                || offers.iter().any(|(o, _)| words(o) == heard_words)
            {
                continue;
            }
            for (meant, from_dictionary) in &offers {
                let meant_words = words(meant);
                // Keeping one of the heard words and dropping the rest is not
                // a mishearing ("private season" once became "private").
                let drops = heard_words.len() > 1 && meant_words.iter().any(|m| heard_words.contains(m));
                if !drops && meant_words.len() <= heard_words.len() + 1 && sounds_alike(heard, meant) {
                    found.push(Candidate {
                        start,
                        end,
                        meant: meant.clone(),
                        from_dictionary: *from_dictionary,
                    });
                }
            }
        }
    }
    // Dictionary words first, then the longest stretch of transcript.
    found.sort_by_key(|c| (!c.from_dictionary, std::cmp::Reverse(c.end - c.start)));
    found.truncate(MAX_CANDIDATES);
    found
}

/// Most fixes allowed in a transcript of `words` words: about one per six
/// words, never more than three.
pub fn fix_limit(words: usize) -> usize {
    (words / 6).clamp(1, 3)
}

/// The transcript with each (start, end, meant) byte range replaced.
/// Ranges must not overlap.
pub fn apply_at(transcript: &str, edits: &[(usize, usize, String)]) -> String {
    let mut edits = edits.to_vec();
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut text = transcript.to_string();
    for (start, end, meant) in edits {
        if start <= end && end <= text.len() && text.is_char_boundary(start) && text.is_char_boundary(end) {
            let meant = cased(&text[start..end], &meant);
            text.replace_range(start..end, &meant);
        }
    }
    text
}

/// `meant` with a capital first letter if `original` had one and `meant`
/// has no capitals of its own.
fn cased(original: &str, meant: &str) -> String {
    if meant.chars().any(char::is_uppercase) || !original.chars().next().is_some_and(char::is_uppercase) {
        return meant.to_string();
    }
    let mut chars = meant.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\''
}

pub fn word_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (is_word_char(c), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                spans.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        spans.push((s, text.len()));
    }
    spans
}

/// Lower case words, punctuation dropped.
pub fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !is_word_char(c))
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

/// True if two phrases could be heard as each other: close in spelling, or
/// the same consonants in the same order.
pub fn sounds_alike(a: &str, b: &str) -> bool {
    let letters = |s: &str| -> Vec<char> {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    };
    let (a, b) = (letters(a), letters(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let longest = a.len().max(b.len());
    let distance = edit_distance(&a, &b);
    if distance * 2 < longest {
        return true;
    }
    // Same consonants in the same order, and not too far apart in spelling:
    // short keys like "nt" match too much on their own.
    let key = skeleton(&a);
    key == skeleton(&b)
        && key.len() >= 2
        && (distance * 2 <= longest || (key.len() >= 3 && distance * 3 <= longest * 2))
}

/// Consonant classes in order, repeats collapsed: a rough sound key. Silent
/// starts (write, knight), "gh" and an r after a vowel that no vowel follows
/// (beater sounds like beta in many accents) are dropped.
fn skeleton(s: &[char]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < s.len() {
        let (c, next) = (s[i], s.get(i + 1).copied());
        let silent_start = i == 0
            && ((c == 'w' && next == Some('r'))
                || (matches!(c, 'k' | 'g' | 'p') && next == Some('n')));
        if silent_start || (c == 'g' && next == Some('h')) {
            i += if c == 'g' { 2 } else { 1 };
            continue;
        }
        let vowel = |c: Option<char>| matches!(c, Some('a' | 'e' | 'i' | 'o' | 'u'));
        let soft_r = c == 'r' && i > 0 && vowel(Some(s[i - 1])) && !vowel(next);
        i += 1;
        if soft_r {
            continue;
        }
        let class = match c {
            'a' | 'e' | 'i' | 'o' | 'u' if i == 1 => 'a',
            'a' | 'e' | 'i' | 'o' | 'u' | 'y' | 'h' | 'w' => continue,
            'c' if matches!(next, Some('e' | 'i' | 'y')) => 's',
            'c' | 'k' | 'q' | 'g' => 'k',
            'b' | 'p' => 'p',
            'd' | 't' => 't',
            'f' | 'v' => 'f',
            's' | 'z' | 'x' => 's',
            'm' | 'n' => 'n',
            other => other,
        };
        if !out.ends_with(class) {
            out.push(class);
        }
    }
    out
}

/// Levenshtein distance over any comparable items.
pub fn edit_distance<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if x == y {
                diagonal
            } else {
                1 + diagonal.min(above).min(row[j])
            };
            diagonal = above;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lead_keeps_the_end_of_long_context() {
        let before = (0..200).map(|n| n.to_string()).collect::<Vec<_>>().join(" ");
        let ctx = Context {
            before,
            earlier: "ignored while there is text before the cursor".into(),
            ..Context::default()
        };
        let text = lead(&ctx);
        assert!(text.starts_with("120 121 "));
        assert!(text.ends_with(" 199\n\n"));
        let earlier = Context {
            earlier: "Hello  there".into(),
            ..Context::default()
        };
        assert_eq!(lead(&earlier), "Hello there\n\n");
        assert_eq!(lead(&Context::default()), "");
    }

    #[test]
    fn candidates_never_drop_heard_words_or_touch_fillers() {
        // From the history: "private season" became "private", "Um" became "Im".
        let ctx = Context {
            earlier: "Private beta. Im on it.".into(),
            ..Context::default()
        };
        let found = candidates("You know, private season and every. Um larger board", &ctx);
        assert!(found.iter().all(|c| c.meant != "Private" && c.meant != "Im"), "{found:?}");
    }

    #[test]
    fn apply_at_replaces_ranges_and_keeps_a_leading_capital() {
        let heard = "Beater testers saw dance cards.";
        let edits = vec![(0, 6, "beta".to_string()), (19, 24, "chance".to_string())];
        assert_eq!(apply_at(heard, &edits), "Beta testers saw chance cards.");
        assert_eq!(apply_at("send it to mars", &[(11, 15, "Maaz".into())]), "send it to Maaz");
        // Out of range edits are skipped.
        assert_eq!(apply_at("short", &[(3, 99, "x".into())]), "short");
    }

    #[test]
    fn sound_alike_check() {
        assert!(sounds_alike("dance", "chance"));
        assert!(sounds_alike("live", "life"));
        assert!(sounds_alike("cooper netties", "Kubernetes"));
        assert!(sounds_alike("night", "knight"));
        assert!(sounds_alike("than cohesive", "and cohesive"));
        assert!(sounds_alike("beater", "beta"));
        assert!(!sounds_alike("bigger", "larger"));
        assert!(!sounds_alike("good", "great"));
        assert!(!sounds_alike("board", "table"));
        assert!(!sounds_alike("need", "made"));
        assert!(!sounds_alike("weather", "third"));
        assert!(!sounds_alike("you", "huh"));
        assert!(!sounds_alike("away", "or"));
        assert!(!sounds_alike("won", "may"));
    }

    #[test]
    fn candidates_come_only_from_the_users_words() {
        let ctx = Context {
            before: "Thanks Maaz. Water the plants.
Chance cards sit in the middle.".into(),
            vocabulary: vec!["Kubernetes".into()],
            ..Context::default()
        };
        let words = context_words(&ctx);
        assert!(words.contains(&"Maaz".to_string()));
        assert!(words.contains(&"water".to_string()));
        assert!(words.contains(&"chance".to_string()));
        assert!(!words.iter().any(|w| w == "the" || w == "in"));

        let found = |t: &str| -> Vec<(String, String)> {
            candidates(t, &ctx)
                .into_iter()
                .map(|c| (t[c.start..c.end].to_string(), c.meant))
                .collect()
        };
        // The dictionary first; the model decides between the rest.
        assert_eq!(found("send it to mars by friday")[0], ("mars".into(), "Maaz".into()));
        assert_eq!(found("check the cooper netties logs")[0], ("cooper netties".into(), "Kubernetes".into()));
        assert!(found("I want dance cards").contains(&("dance".into(), "chance".into())));
        assert!(!found("the plants to be watered").iter().any(|(h, _)| h.contains(' ')));
        // Nothing sounds like the user's words: the model never runs.
        assert!(found("make the board bigger").is_empty());
        assert!(candidates("hello there friend", &Context::default()).is_empty());
    }

    #[test]
    fn edit_distance_counts_changes() {
        let w = |s: &str| words(s);
        assert_eq!(edit_distance(&w("a b c"), &w("a b c")), 0);
        assert_eq!(edit_distance(&w("a b c"), &w("a x c")), 1);
        assert_eq!(edit_distance(&w("Hello, world."), &w("hello world")), 0);
        assert_eq!(edit_distance(&['a', 'b'], &['b']), 1);
    }
}
