//! Sentence level punctuation the engine missed: sentences it ran together,
//! question marks, the commas a writer would use, and a closing full stop.
//! English rules, skipped for Roman Urdu sentences; only the question words
//! there are used.

use super::stopwords::is_stopword;
use super::{is_urdu, sentence_spans, Token};

fn lower(t: &Token) -> Option<String> {
    t.word().map(str::to_lowercase)
}

fn words_in(tokens: &[Token]) -> Vec<String> {
    tokens.iter().filter_map(lower).collect()
}

const AUX: &[&str] = &[
    "is", "are", "am", "was", "were", "do", "does", "did", "can", "could", "would", "should", "will", "shall", "has",
    "have", "had", "isn't", "aren't", "wasn't", "weren't", "don't", "doesn't", "didn't", "can't", "couldn't",
    "wouldn't", "shouldn't", "won't", "haven't", "hasn't",
];
const WH: &[&str] = &["what", "why", "how", "when", "where", "who"];
const WH_CONTRACTED: &[&str] = &["what's", "where's", "who's", "how's", "when's", "why's"];
const PERSON: &[&str] = &["i", "you", "we", "they", "he", "she"];
const SUBJECT: &[&str] = &[
    "i", "you", "we", "they", "he", "she", "it", "there", "this", "that", "these", "those", "the", "a", "an",
    "anyone", "someone", "anybody", "everyone", "everything", "anything", "my", "your", "our", "their", "his", "her",
];
const URDU_QUESTION: &[&str] = &[
    "kya", "kia", "kyun", "kyon", "kaise", "kab", "kahan", "kaun", "kitna", "kitni", "kitne", "kis", "kisne",
];

/// "Why does this crash", "Can you send it", "Do we need this", "What time".
fn is_question_shape(w: &[String]) -> bool {
    let at = |i: usize| w.get(i).map(String::as_str).unwrap_or("");
    let (first, second) = (at(0), at(1));
    if w.len() < 2 {
        return false;
    }
    if WH.contains(&first) && AUX.contains(&second) {
        // "When is the" yes, "How do you do" yes, but "Who is" needs a rest.
        return w.len() >= 2;
    }
    if WH_CONTRACTED.contains(&first) {
        return !(first == "what's" && second == "more");
    }
    if matches!((first, second), ("how", "come" | "about" | "many" | "much" | "long" | "old" | "far" | "often") | ("what", "about" | "if" | "time") | ("why", "not") | ("which", "one")) {
        return true;
    }
    // "What kind of car did you buy", "How big is it", "Where else can we go".
    if (WH.contains(&first) || first == "which") && !SUBJECT.contains(&second) {
        let inverted = (1..=5).any(|k| {
            AUX.contains(&at(k)) && (PERSON.contains(&at(k + 1)) || matches!(at(k + 1), "it" | "there" | "this" | "that"))
        });
        return inverted;
    }
    if !AUX.contains(&first) {
        return false;
    }
    // "Do it now" and "Have the report ready" give orders.
    if matches!(first, "do" | "don't" | "have") {
        return PERSON.contains(&second);
    }
    SUBJECT.contains(&second)
}

const TAGS: &[&[&str]] = &[
    &["right"],
    &["correct"],
    &["isn't", "it"],
    &["aren't", "you"],
    &["aren't", "they"],
    &["don't", "you"],
    &["don't", "they"],
    &["doesn't", "it"],
    &["didn't", "you"],
    &["didn't", "it"],
    &["won't", "you"],
    &["wasn't", "it"],
    &["can't", "you"],
    &["no"],
];

fn split_clauses(tokens: &[Token]) -> Vec<Vec<String>> {
    tokens.split(Token::is_comma).map(words_in).filter(|c| !c.is_empty()).collect()
}

/// Whether a sentence asks something.
fn asks(sentence: &[Token]) -> bool {
    let clauses = split_clauses(sentence);
    let (Some(first), Some(last)) = (clauses.first(), clauses.last()) else {
        return false;
    };
    if URDU_QUESTION.contains(&first[0].as_str()) {
        return true;
    }
    if is_urdu(sentence) {
        return false;
    }
    if clauses.len() >= 2 && TAGS.iter().any(|t| last.len() == t.len() && last.iter().zip(t.iter()).all(|(a, b)| a == b)) {
        return true;
    }
    // "Hey John can you review it." The greeting is not part of the question.
    let greeted = first.len() >= 4 && matches!(first[0].as_str(), "hi" | "hello" | "hey");
    if clauses.len() == 1 && greeted && is_question_shape(&first[2..]) {
        return true;
    }
    // "Okay, what is the weather." and "I see, what do you mean."
    is_question_shape(last) || (clauses.len() == 1 && is_question_shape(first))
}

/// Ends that mean the sentence did not really end there.
const BROKEN_END: &[&str] = &["the", "a", "an", "and", "or", "but", "of", "to", "my", "your", "our", "their"];
/// A short fragment on one of these, then the same words again, is a restart:
/// "We should. We should really fix this."
const FRAGMENT_END: &[&str] = &["should", "will", "can", "could", "would", "must", "want", "need", "is", "was", "are", "were", "have"];
const FRAGMENT_MAX_WORDS: usize = 4;
const WHEN_WORDS: &[&str] = &["on", "in", "at", "by", "until", "before", "after", "from", "since"];

/// Starts with a day, a month or a number: "sent on. Monday morning."
fn starts_with_a_when(s: &[Token]) -> bool {
    s.iter().find_map(Token::word).is_some_and(|w| {
        let lower = w.to_lowercase();
        lower.chars().next().is_some_and(|c| c.is_ascii_digit())
            || [
                "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday", "january", "february",
                "march", "april", "may", "june", "july", "august", "september", "october", "november", "december",
                "today", "tomorrow", "yesterday",
            ]
            .contains(&lower.as_str())
    })
}

/// Full stops the engine put in the middle of a sentence, usually at a breath.
pub fn merge_broken(tokens: Vec<Token>) -> Vec<Token> {
    let spans = sentence_spans(&tokens);
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut lower_next = false;
    for (k, span) in spans.iter().enumerate() {
        let s = &tokens[span.clone()];
        let next = spans.get(k + 1).map(|n| &tokens[n.clone()]);
        let w = words_in(s);
        let full_stop = matches!(s.last(), Some(Token::Close(p)) if p == ".");
        let next_words = next.map(words_in).unwrap_or_default();

        let restart = full_stop
            && (2..=FRAGMENT_MAX_WORDS).contains(&w.len())
            && w.last().is_some_and(|l| FRAGMENT_END.contains(&l.as_str()))
            && next_words.len() > w.len()
            && next_words[..w.len()] == w[..];
        if restart {
            continue;
        }

        let broken_word = w.last().is_some_and(|l| BROKEN_END.contains(&l.as_str()));
        let broken_when = w.last().is_some_and(|l| WHEN_WORDS.contains(&l.as_str())) && next.is_some_and(starts_with_a_when);
        let merge = full_stop && w.len() >= 2 && next.is_some() && (broken_word || broken_when);

        let mut sentence = s.to_vec();
        if merge {
            sentence.pop();
        }
        if std::mem::take(&mut lower_next) {
            if let Some(Token::Word(first)) = sentence.iter_mut().find(|t| t.word().is_some()) {
                if first.chars().skip(1).all(|c| !c.is_uppercase()) {
                    *first = first.to_lowercase();
                }
            }
        }
        out.extend(sentence);
        lower_next = merge && broken_word;
    }
    out
}

/// Full stops the engine should have written as question marks.
pub fn question_marks(tokens: Vec<Token>) -> Vec<Token> {
    let mut out = Vec::with_capacity(tokens.len());
    for span in sentence_spans(&tokens) {
        let s = &tokens[span];
        let asking = asks(s);
        match s.last() {
            Some(Token::Close(p)) if p == "." && asking => {
                out.extend_from_slice(&s[..s.len() - 1]);
                out.push(Token::Close("?".into()));
            }
            Some(Token::Break(n)) if asking && s.len() >= 2 && !s[s.len() - 2].ends_sentence() => {
                out.extend_from_slice(&s[..s.len() - 1]);
                out.push(Token::Close("?".into()));
                out.push(Token::Break(*n));
            }
            Some(Token::Word(_)) if asking => {
                out.extend_from_slice(s);
                out.push(Token::Close("?".into()));
            }
            _ => out.extend_from_slice(s),
        }
    }
    out
}

/// Capitalised words that only ever start a sentence: an engine that writes
/// "so many Like why does" finished a segment there and lost the full stop.
const STARTERS: &[&str] = &[
    "like", "why", "what", "how", "when", "where", "who", "this", "that", "these", "those", "it", "we", "they", "you",
    "he", "she", "there", "here", "my", "our", "your", "but", "and", "so", "because", "also", "then", "if", "make",
    "can", "could", "would", "should", "do", "does", "did", "is", "are", "was", "were", "have", "has", "let's", "the",
    "it's", "that's", "there's",
];

pub fn split_at_capitals(tokens: Vec<Token>) -> Vec<Token> {
    let mut out = Vec::with_capacity(tokens.len());
    for (i, t) in tokens.iter().enumerate() {
        if i > 0 {
            if let Token::Word(w) = t {
                let capital = w.starts_with(char::is_uppercase) && w.chars().skip(1).all(|c| !c.is_uppercase());
                let after_lower = tokens[i - 1].word().is_some_and(|p| p.starts_with(char::is_lowercase));
                // "read The Hobbit" keeps its title.
                let title = tokens.get(i + 1).and_then(Token::word).is_some_and(|n| n.starts_with(char::is_uppercase));
                if capital && after_lower && !title && STARTERS.contains(&w.to_lowercase().as_str()) {
                    out.push(Token::Close(".".into()));
                }
            }
        }
        out.push(t.clone());
    }
    out
}

/// Verbs and words that take a question after them, so "tell me what do you
/// think" stays one sentence.
const TAKES_A_CLAUSE: &[&str] = &[
    "know", "knows", "knew", "see", "saw", "tell", "told", "ask", "asked", "asking", "wonder", "wondering",
    "understand", "understood", "show", "explain", "remember", "forget", "decide", "figure", "about", "of", "on",
    "in", "at", "to", "for", "and", "or", "but", "that", "is", "was", "exactly", "like", "idea", "sure", "out",
    "matter", "me", "him", "her", "them", "us", "you", "i", "so", "because", "if", "then", "when", "why", "what",
    "how", "where", "who",
];
/// Sentences this long with no punctuation at all are the engine's failure.
const RUN_ON_WORDS: usize = 18;
/// Openings of a new clause. Only trusted inside a run on.
const CLAUSE_OPENINGS: &[&[&str]] = &[
    &["i", "don't"],
    &["i'm"],
    &["i", "want"],
    &["i", "need"],
    &["i", "think"],
    &["we", "need"],
    &["we", "should"],
    &["we're"],
    &["it's"],
    &["that's"],
    &["there's"],
    &["they're"],
    &["you", "can"],
    &["you", "should"],
    &["also"],
];
/// Never a clause boundary after one of these: "the thing I want".
const JOINS_NEXT: &[&str] = &["sure", "think", "thing", "things", "way", "time", "know", "said", "say", "guess", "hope", "feel", "believe", "mean", "wish", "glad", "sorry", "afraid", "because", "stuff", "reason", "place", "one", "person", "people"];

fn matches_at(words: &[String], i: usize, phrase: &[&str]) -> bool {
    phrase.iter().enumerate().all(|(k, p)| words.get(i + k).is_some_and(|w| w == p))
}

/// Full stops in sentences the engine ran together.
pub fn split_run_ons(tokens: Vec<Token>) -> Vec<Token> {
    let mut out = Vec::with_capacity(tokens.len());
    for span in sentence_spans(&tokens) {
        let s = &tokens[span];
        if is_urdu(s) {
            out.extend_from_slice(s);
            continue;
        }
        let positions: Vec<usize> = s.iter().enumerate().filter(|(_, t)| t.word().is_some()).map(|(i, _)| i).collect();
        let w = words_in(s);
        let unpunctuated = w.len() >= RUN_ON_WORDS && !s.iter().any(Token::is_comma);
        let mut splits = Vec::new();
        for j in 2..w.len().saturating_sub(1) {
            // Only between words with nothing already separating them.
            if positions[j] != positions[j - 1] + 1 {
                continue;
            }
            let prev = w[j - 1].as_str();
            let question = WH.contains(&w[j].as_str())
                && w.get(j + 1).is_some_and(|a| AUX.contains(&a.as_str()))
                && w.get(j + 2).is_some_and(|x| SUBJECT.contains(&x.as_str()))
                && !TAKES_A_CLAUSE.contains(&prev);
            let clause = unpunctuated
                && !is_stopword(prev)
                && !JOINS_NEXT.contains(&prev)
                && j >= 4
                && j + 3 <= w.len()
                && CLAUSE_OPENINGS.iter().any(|p| matches_at(&w, j, p));
            if question || clause {
                splits.push(positions[j]);
            }
        }
        for (i, t) in s.iter().enumerate() {
            if splits.contains(&i) {
                out.push(Token::Close(".".into()));
            }
            out.push(t.clone());
        }
    }
    out
}

/// Words that open a sentence and take a comma, with what may follow them
/// for it to count. "However long it takes" has none; "However, we" does.
const CLAUSE_START: &[&str] = &[
    "i", "you", "we", "they", "he", "she", "it", "there", "this", "that", "these", "those", "the", "a", "an", "my",
    "our", "your", "their", "his", "her", "let's", "i'm", "you're", "we're", "they're", "it's", "that's", "there's",
    "i'll", "you'll", "we'll", "they'll", "i've", "you've", "we've", "they've", "i'd", "please", "if", "when",
    "don't", "can", "could", "should", "would", "will", "what", "how", "why", "where",
];
/// Take a comma before any clause start.
const OPENERS: &[&str] = &[
    "however", "therefore", "meanwhile", "unfortunately", "fortunately", "honestly", "anyway", "additionally",
    "finally", "otherwise", "instead", "personally", "obviously", "okay", "ok", "yes", "yeah", "sure", "well",
    "alright", "no", "nevertheless", "furthermore", "moreover", "consequently", "luckily", "sadly", "frankly",
    "basically", "actually", "firstly", "secondly", "thirdly", "lastly", "separately", "oh",
];
/// Take a comma before anything: "Also add a timer."
const ANY_FOLLOWER: &[&str] = &["also", "wow"];
const OPENING_PHRASES: &[&[&str]] = &[
    &["first", "of", "all"],
    &["first", "off"],
    &["on", "another", "note"],
    &["on", "a", "different", "note"],
    &["apart", "from", "that"],
    &["besides", "that"],
    &["having", "said", "that"],
    &["that", "said"],
    &["in", "other", "news"],
    &["for", "example"],
    &["for", "instance"],
    &["by", "the", "way"],
    &["in", "fact"],
    &["of", "course"],
    &["as", "a", "result"],
    &["in", "addition"],
    &["on", "the", "other", "hand"],
    &["at", "the", "end", "of", "the", "day"],
    &["to", "be", "honest"],
    &["in", "other", "words"],
];
/// "not only ... but", "nothing but", "anything but" need no comma.
const BEFORE_BUT: &[&str] = &["nothing", "all", "anything", "last", "everything", "not", "none", "anyone", "everyone", "no"];
const BEFORE_SO: &[&str] = &["do", "think", "hope", "guess", "even", "and", "or", "not", "say", "said", "is", "was", "ever", "just", "like", "more", "and"];

/// What opens the main clause after "if you have time".
const MAIN_CLAUSE: &[&str] = &[
    "please", "then", "i", "we", "let's", "let", "ask", "call", "send", "tell", "email", "check", "give",
];
/// After these the condition is still going: "if you think we should".
const CONTINUES_CONDITION: &[&str] = &[
    "think", "know", "say", "said", "feel", "believe", "hope", "see", "mean", "that", "and", "or", "to", "when", "if",
    "tell", "ask", "asked", "told", "because", "so", "but", "what", "how", "why", "who",
];

/// Commas a careful writer would add, where the words make them certain.
pub fn commas(tokens: Vec<Token>) -> Vec<Token> {
    let mut out = Vec::with_capacity(tokens.len() + 4);
    for span in sentence_spans(&tokens) {
        let s = &tokens[span];
        if is_urdu(s) {
            out.extend_from_slice(s);
        } else {
            out.extend(sentence_commas(s));
        }
    }
    out
}

fn sentence_commas(s: &[Token]) -> Vec<Token> {
    let positions: Vec<usize> = s.iter().enumerate().filter(|(_, t)| t.word().is_some()).map(|(i, _)| i).collect();
    let w = words_in(s);
    let mut after: Vec<usize> = Vec::new(); // token index a comma follows
    let bare = |i: usize| s.get(i + 1).is_some_and(|t| t.word().is_some());

    if let Some(first) = w.first() {
        if let Some(next) = w.get(1) {
            let opener = OPENERS.contains(&first.as_str()) && CLAUSE_START.contains(&next.as_str());
            if (opener || ANY_FOLLOWER.contains(&first.as_str())) && bare(positions[0]) {
                after.push(positions[0]);
            }
        }
        for phrase in OPENING_PHRASES {
            if matches_at(&w, 0, phrase) && w.len() > phrase.len() && bare(positions[phrase.len() - 1]) {
                after.push(positions[phrase.len() - 1]);
            }
        }
        // "Thanks John." and "Hi John how are you?"
        if matches!(first.as_str(), "thanks" | "hi" | "hello" | "hey") && bare(positions[0]) {
            let name = s.get(positions[0] + 1).and_then(Token::word).filter(|n| n.starts_with(char::is_uppercase));
            if first == "thanks" && name.is_some() && w.len() == 2 {
                after.push(positions[0]);
            }
            if first != "thanks" && name.is_some() && w.len() >= 4 {
                let next = s.get(positions[0] + 2).and_then(Token::word);
                if next.is_some_and(|n| n.starts_with(char::is_lowercase)) {
                    after.push(positions[0] + 1);
                }
            }
        }
    }

    // "If you have time please review it", "When the boss is here I will go".
    let opens_condition = w.first().is_some_and(|f| matches!(f.as_str(), "if" | "when" | "once"));
    if opens_condition {
        let main_clause = (3..w.len().saturating_sub(1)).find(|&j| {
            positions[j] == positions[j - 1] + 1
                && MAIN_CLAUSE.contains(&w[j].as_str())
                && !CONTINUES_CONDITION.contains(&w[j - 1].as_str())
                && !AUX.contains(&w[j - 1].as_str())
        });
        if let Some(j) = main_clause {
            after.push(positions[j - 1]);
        }
    }

    for j in 3..w.len().saturating_sub(1) {
        if positions[j] != positions[j - 1] + 1 {
            continue;
        }
        let prev = w[j - 1].as_str();
        let next = w[j + 1].as_str();
        let splits_clause = match w[j].as_str() {
            "but" => CLAUSE_START.contains(&next) && !BEFORE_BUT.contains(&prev),
            // "so we can" gives a purpose, "so I slept" a result.
            "so" => {
                CLAUSE_START.contains(&next)
                    && next != "that"
                    && !BEFORE_SO.contains(&prev)
                    && !matches!(w.get(j + 2).map(String::as_str), Some("can" | "could" | "will" | "would" | "might" | "may"))
            }
            "especially" => !matches!(prev, "and" | "or" | "but" | "is" | "are" | "was" | "were" | "the" | "a" | "an" | "of" | "not" | "more"),
            _ => false,
        };
        if splits_clause {
            after.push(positions[j - 1]);
        }
    }

    let mut out = Vec::with_capacity(s.len() + after.len());
    for (i, t) in s.iter().enumerate() {
        out.push(t.clone());
        if after.contains(&i) {
            out.push(Token::Close(",".into()));
        }
    }
    out
}

/// Words below this count may be fragments ("Okay", "Sounds good").
const FULL_STOP_MIN_WORDS: usize = 3;

/// The speaker trailed off on one of these, so the sentence is not finished.
const TRAILS_OFF: &[&str] = &[
    "the", "a", "an", "and", "or", "but", "if", "of", "to", "my", "your", "our", "their", "because", "with", "every",
    "each", "very", "than",
];

/// A closing full stop, unless the speaker trailed off.
pub fn final_stop(mut tokens: Vec<Token>) -> Vec<Token> {
    let last_word_ok = match tokens.last() {
        Some(Token::Word(w)) => {
            let bare_end = !w.ends_with('.') && w.chars().last().is_some_and(char::is_alphanumeric);
            let trailing = TRAILS_OFF.contains(&w.to_lowercase().as_str());
            bare_end && !trailing && !w.contains("://") && !w.contains('@')
        }
        _ => false,
    };
    if last_word_ok {
        let start = tokens.iter().rposition(Token::ends_sentence).map_or(0, |p| p + 1);
        if words_in(&tokens[start..]).len() >= FULL_STOP_MIN_WORDS {
            tokens.push(Token::Close(".".into()));
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::format::{render, tidy};
    use crate::text::tokenize;

    fn run(f: fn(Vec<Token>) -> Vec<Token>, s: &str) -> String {
        render(&tidy(f(tokenize(s))))
    }

    #[test]
    fn questions_get_their_mark() {
        for (a, b) in [
            ("Why does every space have an illustration.", "Why does every space have an illustration?"),
            ("what is the weather", "What is the weather?"),
            ("Can you send me the deck.", "Can you send me the deck?"),
            ("Okay, what do you think.", "Okay, what do you think?"),
            ("How many people came.", "How many people came?"),
            ("We can ship it, right.", "We can ship it, right?"),
            ("Kya kar rahe ho.", "Kya kar rahe ho?"),
        ] {
            assert_eq!(run(question_marks, a), b, "{a}");
        }
    }

    #[test]
    fn statements_stay_statements() {
        for s in [
            "What I want is a bigger board.",
            "When I get home I will call.",
            "How lovely.",
            "What a wonderful world.",
            "Do it now.",
            "Have the report ready.",
            "Don't do that.",
            "Why it matters is another thing.",
            "Is.",
            "Can you send it, I need it today.",
            "Do din ke baad aana.",
        ] {
            assert_eq!(run(question_marks, s), s, "{s}");
        }
    }

    #[test]
    fn breath_stops_merge() {
        assert_eq!(run(merge_broken, "The invoice was sent on. Monday morning."), "The invoice was sent on Monday morning.");
        assert_eq!(run(merge_broken, "Send it to the. Client today."), "Send it to the client today.");
        assert_eq!(run(merge_broken, "We should. We should really fix this bug."), "We should really fix this bug.");
        for s in [
            "Turn it on. Then leave.",
            "I know. I know you did.",
            "Maybe two seconds, but definitely not. Three seconds.",
            "We should. Maybe tomorrow.",
        ] {
            assert_eq!(run(merge_broken, s), s, "{s}");
        }
    }

    #[test]
    fn segments_with_a_lost_full_stop_split() {
        assert_eq!(
            run(split_at_capitals, "There's still so many Like why does it."),
            "There's still so many. Like why does it."
        );
        for s in ["I read The Hobbit today.", "I met Mark And Spencer.", "Ask Will about it.", "So many Sara people."] {
            assert_eq!(run(split_at_capitals, s), s, "{s}");
        }
    }

    #[test]
    fn run_ons_split() {
        assert_eq!(run(split_run_ons, "What is the weather what am I doing?"), "What is the weather. What am I doing?");
        let long = "That's much better but i'm sure you can improve it further to make it even better especially the walking animation also change sticks bak tith i don't want him to have bak tith";
        assert_eq!(
            run(split_run_ons, long),
            "That's much better but i'm sure you can improve it further to make it even better especially the walking animation. Also change sticks bak tith. I don't want him to have bak tith"
        );
    }

    #[test]
    fn a_clause_is_not_a_run_on() {
        for s in [
            "Tell me what do you think about it.",
            "I know what you want.",
            "Short sentence I don't want this.",
            "The thing I don't want is for this very long sentence with many many words to get split apart at all.",
        ] {
            assert_eq!(run(split_run_ons, s), s, "{s}");
        }
    }

    #[test]
    fn commas_where_they_are_certain() {
        for (a, b) in [
            ("However we need more time.", "However, we need more time."),
            ("Also I don't like the dice.", "Also, I don't like the dice."),
            ("Also add a timer.", "Also, add a timer."),
            ("Yes I can.", "Yes, I can."),
            ("For example we could wait.", "For example, we could wait."),
            ("Things might not go your way but that's okay.", "Things might not go your way, but that's okay."),
            ("I was very tired so I slept.", "I was very tired, so I slept."),
            ("It's better now especially the animation.", "It's better now, especially the animation."),
            ("Thanks John.", "Thanks, John."),
            ("Hi John how are you?", "Hi John, how are you?"),
        ] {
            assert_eq!(run(commas, a), b, "{a}");
        }
    }

    #[test]
    fn no_comma_where_none_belongs() {
        for s in [
            "No one came.",
            "No problem at all.",
            "However long it takes.",
            "Well done.",
            "We were simple but elegant.",
            "Nothing but trouble came.",
            "Work hard so we can ship.",
            "Make room on the right so that you can see.",
            "Thanks for the update.",
            "Okay.",
            "Yaar main so raha tha but mujhe pata hai.",
        ] {
            assert_eq!(run(commas, s), s, "{s}");
        }
    }

    #[test]
    fn closing_full_stop() {
        assert_eq!(run(final_stop, "I need to know"), "I need to know.");
        assert_eq!(run(final_stop, "Sounds good"), "Sounds good");
        assert_eq!(run(final_stop, "Add more models that are on the"), "Add more models that are on the");
        assert_eq!(run(final_stop, "Is it done?"), "Is it done?");
    }
}
