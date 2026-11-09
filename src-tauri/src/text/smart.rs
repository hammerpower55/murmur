//! Smart formatting: the structure people mean but do not say. Modelled on
//! what Wispr Flow documents (lists from "one, two" or "first, second",
//! Backtrack corrections, spoken line breaks, email and chat layout), done
//! offline with deterministic rules instead of a cloud model.
//!
//! Every rule is conservative: when a pattern is ambiguous the words are
//! left as spoken, because pasting something the user did not say is worse
//! than missing a format.

use super::format::capitalise_word;
use super::Token;

/// What kind of app the text is going into, from its exe and window title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppKind {
    Email,
    Chat,
    #[default]
    Other,
}

// Windows executables, then macOS app bundles (inject/focus.rs).
const EMAIL_APPS: &[&str] = &[
    "outlook.exe", "olk.exe", "hxoutlook.exe", "thunderbird.exe", "mailspring.exe", "mailclient.exe",
    "mail.app", "microsoft outlook.app", "thunderbird.app", "mailspring.app", "spark.app", "airmail 5.app",
];
const CHAT_APPS: &[&str] = &[
    "slack.exe", "ms-teams.exe", "teams.exe", "whatsapp.exe", "discord.exe", "telegram.exe", "signal.exe",
    "messenger.exe", "zoom.exe",
    "slack.app", "microsoft teams.app", "whatsapp.app", "discord.app", "telegram.app", "signal.app",
    "messenger.app", "messages.app", "zoom.us.app",
];
const BROWSERS: &[&str] = &[
    "chrome.exe", "msedge.exe", "firefox.exe", "brave.exe", "opera.exe", "vivaldi.exe", "arc.exe",
    "google chrome.app", "safari.app", "microsoft edge.app", "firefox.app", "brave browser.app", "opera.app",
    "vivaldi.app", "arc.app",
];
const EMAIL_SITES: &[&str] = &["gmail", "outlook", "proton mail", "yahoo mail", "fastmail"];
const CHAT_SITES: &[&str] = &["whatsapp", "slack", "discord", "messenger", "telegram", "microsoft teams"];

impl AppKind {
    pub fn detect(exe: &str, title: &str) -> Self {
        let exe = exe.to_ascii_lowercase();
        let title = title.to_lowercase();
        if EMAIL_APPS.contains(&exe.as_str()) {
            return AppKind::Email;
        }
        if CHAT_APPS.contains(&exe.as_str()) {
            return AppKind::Chat;
        }
        if BROWSERS.contains(&exe.as_str()) {
            if EMAIL_SITES.iter().any(|s| title.contains(s)) {
                return AppKind::Email;
            }
            if CHAT_SITES.iter().any(|s| title.contains(s)) {
                return AppKind::Chat;
            }
        }
        AppKind::Other
    }
}

// Helpers

fn lower(t: &Token) -> Option<String> {
    t.word().map(str::to_lowercase)
}

fn is_word(tokens: &[Token], i: usize, w: &str) -> bool {
    tokens.get(i).is_some_and(|t| t.is_word_ci(w))
}

fn is_close(t: &Token) -> bool {
    matches!(t, Token::Close(_))
}

fn is_punct(t: &Token, marks: &[&str]) -> bool {
    matches!(t, Token::Close(p) if marks.contains(&p.as_str()))
}

/// A sentence or line ends just before `i` (or `i` is the start).
fn at_boundary(tokens: &[Token], i: usize) -> bool {
    i == 0 || tokens[i - 1].ends_sentence()
}

/// At a spoken pause: the start, after any punctuation, or after a break.
fn at_pause(tokens: &[Token], i: usize) -> bool {
    i == 0 || is_close(&tokens[i - 1]) || matches!(tokens[i - 1], Token::Break(_))
}

fn words(tokens: &[Token]) -> usize {
    tokens.iter().filter(|t| t.word().is_some()).count()
}

/// Removes trailing commas, semicolons, colons and full stops.
fn trim_trailing(tokens: &mut Vec<Token>) {
    while tokens
        .last()
        .is_some_and(|t| is_punct(t, &[",", ";", ":", "."]) || t.is_word_ci("and"))
    {
        tokens.pop();
    }
}

fn trim_leading(tokens: &[Token]) -> &[Token] {
    let start = tokens
        .iter()
        .position(|t| !is_punct(t, &[",", ";", ":", "."]))
        .unwrap_or(tokens.len());
    &tokens[start..]
}

fn capitalise_first(tokens: &mut [Token]) {
    if let Some(Token::Word(w)) = tokens.iter_mut().find(|t| t.word().is_some()) {
        *w = capitalise_word(w);
    }
}

/// Matches a phrase of words (lowercase) at `i`; returns the index after it.
fn phrase(tokens: &[Token], i: usize, phrase: &str) -> Option<usize> {
    let mut j = i;
    for w in phrase.split(' ') {
        if !is_word(tokens, j, w) {
            return None;
        }
        j += 1;
    }
    Some(j)
}

// 1. Backtrack

/// Always mean "drop that", except before a word that makes them literal.
const SCRATCH: &[&str] = &["scratch that", "strike that"];
const SCRATCH_LITERAL: &[&str] = &["itch", "off", "out", "card", "deal", "pad", "from"];
/// Only a command standing alone between pauses: "don't forget that we have a
/// meeting" and "please delete that file" keep every word.
const SOFT_SCRATCH: &[&str] = &["never mind", "nevermind", "delete that", "forget that", "forget it", "ignore that"];
const NEVER_MIND_LEADS: &[&str] = &["i", "you", "we", "they", "he", "she", "don't", "wouldn't", "just", "and", "to"];
/// Before these "scratch that" is the verb: "try not to scratch that bite".
const SCRATCH_VERB_LEADS: &[&str] = &[
    "to", "not", "don't", "didn't", "won't", "can't", "can", "could", "will", "would", "should", "must", "i", "you",
    "we", "they", "please", "let's", "might", "may", "did", "cats", "dogs", "he", "she",
];
/// "never mind let's just cook": a new clause starts straight after.
const AFTER_NEVER_MIND: &[&str] = &["let's", "i", "we", "i'll", "we'll", "it's", "just"];
/// Longest first, so "no wait" is not read as "no".
const CORRECTIONS: &[&str] = &[
    "actually make that", "sorry i mean", "no i mean", "actually i mean", "no wait", "wait no", "no sorry", "sorry no", "actually no",
    "no actually", "wait actually", "actually wait", "or rather", "i mean", "i meant", "make that", "actually",
    "sorry", "oops", "no", "wait", "rather",
];
/// Corrections too clear to need a pause before them.
const CLEAR_CORRECTIONS: &[&str] = &[
    "actually make that", "sorry i mean", "no i mean", "actually i mean", "no wait", "wait no", "no sorry", "sorry no", "actually no",
    "no actually", "wait actually", "actually wait", "or rather", "i meant", "make that",
];
/// Longest run of words a correction may look back over.
const ANCHOR_WINDOW: usize = 8;
/// A restatement anchored only on a word like "the" must be this short and
/// end there: "the beach", not "the beach is nicer", which is a new thought.
const ANCHOR_SHORT_WORDS: usize = 3;

const NUMBER_WORDS: &[&str] = &[
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve",
    "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen", "twenty", "thirty", "forty",
    "fifty", "sixty", "seventy", "eighty", "ninety", "hundred",
];
const WEEKDAYS: &[&str] = &["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];
const MONTHS: &[&str] = &[
    "january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november",
    "december",
];
const RELATIVE_DAYS: &[&str] = &["today", "tomorrow", "tonight", "yesterday"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    Number,
    Weekday,
    Month,
    RelativeDay,
}

fn value_kind(word: &str) -> Option<ValueKind> {
    let w = word.to_lowercase();
    let w = w.trim_end_matches(['s', '\'']);
    let digits = word.chars().next().is_some_and(|c| c.is_ascii_digit())
        && word.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '.' | '%' | '$'));
    if digits || NUMBER_WORDS.contains(&w) || word.starts_with('$') {
        Some(ValueKind::Number)
    } else if WEEKDAYS.contains(&w) {
        Some(ValueKind::Weekday)
    } else if MONTHS.contains(&w) {
        Some(ValueKind::Month)
    } else if RELATIVE_DAYS.contains(&w) {
        Some(ValueKind::RelativeDay)
    } else {
        None
    }
}

fn is_meridiem(t: &Token) -> bool {
    lower(t).is_some_and(|w| matches!(w.as_str(), "am" | "pm" | "a.m" | "p.m" | "a.m." | "p.m."))
}

fn is_number_word(t: &Token) -> bool {
    lower(t).is_some_and(|w| NUMBER_WORDS.contains(&w.as_str()) || w == "thousand" || w == "million")
}

/// The value ending just before `end` (skipping one comma): its start index
/// and kind. "2 pm" counts as one value, and so does "two hundred".
fn value_before(tokens: &[Token], end: usize) -> Option<(usize, ValueKind)> {
    let mut i = end;
    if i > 0 && tokens[i - 1].is_comma() {
        i -= 1;
    }
    let mut start = i.checked_sub(1)?;
    if is_meridiem(&tokens[start]) && start > 0 {
        start -= 1;
    }
    let kind = value_kind(tokens[start].word()?)?;
    if kind == ValueKind::Number {
        while start > 0 && is_number_word(&tokens[start]) && is_number_word(&tokens[start - 1]) {
            start -= 1;
        }
    }
    Some((start, kind))
}

/// The value starting at `i` (skipping one comma): its end index and kind.
fn value_after(tokens: &[Token], mut i: usize) -> Option<(usize, usize, ValueKind)> {
    if tokens.get(i).is_some_and(Token::is_comma) {
        i += 1;
    }
    let kind = value_kind(tokens.get(i)?.word()?)?;
    let mut end = i + 1;
    if kind == ValueKind::Number {
        while tokens.get(end).is_some_and(is_number_word) {
            end += 1;
        }
    }
    if tokens.get(end).is_some_and(is_meridiem) {
        end += 1;
    }
    Some((i, end, kind))
}

/// A value correction at the trigger: where to cut the output, and the
/// replacement's range. "two hundred dollars, no, three hundred dollars"
/// carries its unit along when both sides say it.
fn value_correction(out: &[Token], tokens: &[Token], after: usize, no_trigger: bool) -> Option<(usize, usize, usize)> {
    let (b_start, b_end, b_kind) = value_after(tokens, after)?;
    // "no one else", "no two ways about it".
    if no_trigger && tokens.get(b_end).and_then(Token::word).is_some() {
        let first = lower(&tokens[b_start])?;
        if first == "one" || first == "two" {
            return None;
        }
    }
    if let Some((a_start, a_kind)) = value_before(out, out.len()) {
        if a_kind == b_kind {
            return Some((a_start, b_start, b_end));
        }
    }
    // The unit form.
    let end = if out.last().is_some_and(Token::is_comma) { out.len() - 1 } else { out.len() };
    let unit = out.get(end.checked_sub(1)?)?.word()?;
    let same_unit = tokens.get(b_end).is_some_and(|t| t.is_word_ci(unit));
    let (a_start, a_kind) = value_before(out, end - 1)?;
    (same_unit && a_kind == b_kind).then_some((a_start, b_start, b_end + 1))
}

fn is_capitalised(t: &Token) -> bool {
    t.word().is_some_and(|w| w.starts_with(char::is_uppercase) && w != "I" && !w.contains('\''))
}

/// A restatement after a correction word: what to keep of the output, and
/// where the words that replace the rest start. Either the new words open
/// the same way as some earlier ones ("send it to John, no wait, send it to
/// Sara") or a name replaces a name ("tell John, I mean Sara, about it").
fn restatement(out: &[Token], tokens: &[Token], after: usize, paused: bool) -> Option<(usize, usize)> {
    let mut from = after;
    while tokens.get(from).is_some_and(Token::is_comma) {
        from += 1;
    }
    let first = lower(tokens.get(from)?)?;
    let start = out.iter().rposition(Token::ends_sentence).map_or(0, |p| p + 1);

    // Words looked back over, nearest first.
    let earlier: Vec<usize> = (start..out.len())
        .rev()
        .filter(|&p| out[p].word().is_some())
        .take(ANCHOR_WINDOW)
        .collect();
    if let Some(&p) = earlier.iter().find(|&&p| out[p].is_word_ci(&first)) {
        let mut same = 1;
        while out.get(p + same).zip(tokens.get(from + same)).is_some_and(|(a, b)| {
            a.word().is_some() && b.word().is_some_and(|w| a.is_word_ci(w))
        }) {
            same += 1;
        }
        let new_words = (from..tokens.len()).take_while(|&j| !tokens[j].ends_sentence() && !tokens[j].is_comma()).count();
        let weak = same == 1 && super::stopwords::is_stopword(&first);
        if (!weak && paused) || (same >= 2) || (weak && new_words <= ANCHOR_SHORT_WORDS) {
            return Some((p, from));
        }
    }

    // A name replaces a name, when the sentence carries on sensibly after it.
    if !paused || !is_capitalised(&tokens[from]) || super::stopwords::is_stopword(&first) {
        return None;
    }
    let end = if out.last().is_some_and(Token::is_comma) { out.len() - 1 } else { out.len() };
    let last = end.checked_sub(1)?;
    if last <= start || !is_capitalised(&out[last]) {
        return None;
    }
    let mut name_end = from + 1;
    while tokens.get(name_end).is_some_and(is_capitalised) {
        name_end += 1;
    }
    let carries_on = match tokens.get(name_end) {
        None | Some(Token::Close(_) | Token::Break(_)) => true,
        Some(t) => lower(t).is_some_and(|w| matches!(w.as_str(), "about" | "to" | "at" | "in" | "on" | "for" | "with" | "and" | "from")),
    };
    if !carries_on {
        return None;
    }
    let mut cut = last;
    while cut > start + 1 && is_capitalised(&out[cut - 1]) {
        cut -= 1;
    }
    Some((cut, from))
}

/// "scratch that" and friends at `i`: the index after the phrase.
fn scratch_at(tokens: &[Token], i: usize, out: &[Token]) -> Option<usize> {
    let pause_after = |end: usize| {
        tokens.get(end).is_none_or(|t| is_close(t) || matches!(t, Token::Break(_)))
    };
    let lead = i.checked_sub(1).and_then(|p| lower(&tokens[p]));
    if let Some(end) = SCRATCH.iter().find_map(|p| phrase(tokens, i, p)) {
        let literal = tokens.get(end).and_then(lower).is_some_and(|w| SCRATCH_LITERAL.contains(&w.as_str()));
        let verb = lead.as_ref().is_some_and(|w| SCRATCH_VERB_LEADS.contains(&w.as_str()));
        return (!literal && !verb).then_some(end);
    }
    let end = SOFT_SCRATCH.iter().find_map(|p| phrase(tokens, i, p))?;
    if !out.iter().any(|t| t.word().is_some()) {
        return None;
    }
    if tokens[i].is_word_ci("never") {
        // "let's order pizza never mind let's just cook", "see you never mind".
        let lead_ok = lead.is_some_and(|w| !NEVER_MIND_LEADS.contains(&w.as_str()));
        let clause_next = tokens.get(end).and_then(lower).is_some_and(|w| AFTER_NEVER_MIND.contains(&w.as_str()));
        if lead_ok && (clause_next || pause_after(end)) {
            return Some(end);
        }
    }
    (at_pause(tokens, i) && pause_after(end)).then_some(end)
}

/// "at 2 actually 3" to "at 3", "scratch that" drops what came before.
pub fn backtrack(tokens: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        // Scratch that: drop the sentence so far, or the previous one.
        if let Some(end) = scratch_at(&tokens, i, &out) {
            while out.last().is_some_and(|t| is_punct(t, &[","])) {
                out.pop();
            }
            let sentence_start = |out: &Vec<Token>| {
                out.iter()
                    .rposition(|t| t.ends_sentence())
                    .map_or(0, |p| p + 1)
            };
            let mut cut = sentence_start(&out);
            if cut == out.len() {
                // Nothing yet in this sentence: drop the previous sentence.
                out.truncate(cut.saturating_sub(1));
                cut = sentence_start(&out);
            }
            out.truncate(cut);
            i = end;
            while tokens.get(i).is_some_and(|t| is_punct(t, &[",", ".", "!"])) {
                i += 1;
            }
            continue;
        }

        // Corrections: the value before the trigger is replaced by the value
        // after it, when both are the same kind, or the speaker restates.
        if let Some((trigger, after)) = CORRECTIONS.iter().find_map(|p| phrase(&tokens, i, p).map(|e| (*p, e))) {
            let no_trigger = trigger == "no";
            if let Some((a_start, b_start, b_end)) = value_correction(&out, &tokens, after, no_trigger) {
                out.truncate(a_start);
                out.extend_from_slice(&tokens[b_start..b_end]);
                i = b_end;
                continue;
            }
            let paused = at_pause(&tokens, i) || CLEAR_CORRECTIONS.contains(&trigger);
            if paused || trigger == "actually" {
                if let Some((keep, from)) = restatement(&out, &tokens, after, paused) {
                    let was_name = out.get(keep).is_some_and(is_capitalised) && !out[keep].is_word_ci(tokens[from].word().unwrap_or(""));
                    out.truncate(keep);
                    let mut j = from;
                    if was_name {
                        // The commas that framed the correction go with it.
                        while tokens.get(j).is_some_and(is_capitalised) {
                            out.push(tokens[j].clone());
                            j += 1;
                        }
                        if tokens.get(j).is_some_and(Token::is_comma) {
                            j += 1;
                        }
                    }
                    i = j;
                    continue;
                }
            }
        }
        out.push(tokens[i].clone());
        i += 1;
    }
    out
}

// 2. Spoken line breaks

const PARAGRAPH_PHRASES: &[&str] = &["start a new paragraph", "new paragraph", "next paragraph"];
const LINE_PHRASES: &[&str] = &["new line", "next line", "line break", "skip a line"];

/// "new paragraph" breaks unless it is a noun ("write a new paragraph"); "new
/// line" only at a pause, so "a new line of products" stays as words.
pub fn spoken_breaks(tokens: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let noun = i > 0 && lower(&tokens[i - 1]).is_some_and(|w| matches!(w.as_str(), "a" | "the" | "this" | "that" | "each" | "every" | "one" | "another"));
        let paragraph = PARAGRAPH_PHRASES.iter().find_map(|p| phrase(&tokens, i, p)).filter(|_| !noun);
        let line = LINE_PHRASES.iter().find_map(|p| phrase(&tokens, i, p)).filter(|&end| {
            at_pause(&tokens, i) && (end == tokens.len() || is_close(&tokens[end]) || matches!(tokens[end], Token::Break(_)))
        });
        let hit = paragraph.map(|e| (e, 2)).or(line.map(|e| (e, 1)));
        match hit {
            Some((end, n)) => {
                let mut had_comma = false;
                while out.last().is_some_and(Token::is_comma) {
                    out.pop();
                    had_comma = true;
                }
                // "The sky was clear, new line": the pause ended a sentence.
                // "Hi John, new line" is too short to be one.
                let from = out.iter().rposition(Token::ends_sentence).map_or(0, |p| p + 1);
                if had_comma && words(&out[from..]) >= 3 {
                    out.push(Token::Close(".".into()));
                }
                out.push(Token::Break(n));
                i = end;
                while tokens.get(i).is_some_and(|t| is_punct(t, &[",", ".", ":", ";"])) {
                    i += 1;
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

// 3. Lists

const ORDINALS: &[&[&str]] = &[
    &["first", "firstly"],
    &["second", "secondly"],
    &["third", "thirdly"],
    &["fourth", "fourthly"],
    &["fifth", "fifthly"],
    &["sixth"],
    &["seventh"],
    &["eighth"],
    &["ninth"],
    &["tenth"],
];
const PREFIXES: &[&str] = &["number", "point", "step", "item"];
const FINAL_MARKERS: &[&str] = &["finally", "lastly"];

/// Before these a number or ordinal is not a marker: "a second", "the one".
const NOT_BEFORE_MARKER: &[&str] = &[
    "a", "an", "the", "this", "that", "my", "your", "our", "per", "every", "each", "one", "in", "at", "on", "by",
    "for", "to", "of", "these", "those", "about", "around", "only", "just", "than",
];
/// After these "one" and "two" are counting, not a list: "One of them",
/// "One thing I noticed", "Two people".
const NOT_AFTER_MARKER: &[&str] = &[
    "of", "more", "thing", "things", "time", "times", "day", "days", "person", "people", "way", "ways", "reason",
    "reasons", "another", "or", "and", "other", "last", "big", "small", "little", "or", "guy", "guys", "minute",
    "minutes", "second", "seconds", "hour", "hours", "week", "weeks", "month", "months", "year", "years",
];
/// After these an ordinal describes a noun instead of opening an item.
const ORDINAL_NOUNS: &[&str] = &[
    "time", "times", "week", "day", "floor", "hand", "half", "place", "one", "round", "guess", "thought", "nature",
    "language", "class", "grade", "chance", "opinion", "look", "thing", "year", "month", "quarter", "date", "person",
    "try", "attempt", "page", "line", "version", "draft",
];
const LETTERS: &str = "abcdefghij";
/// Items in an unpunctuated "first X second Y" pair.
const ORDINAL_PAIR_MAX_WORDS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Cardinal,
    Ordinal,
    Letter,
    Bullet,
}

#[derive(Debug, Clone, Copy)]
struct Marker {
    /// 1 based position; 0 for "finally" (whatever comes next).
    n: usize,
    family: Family,
    /// First token of the item.
    end: usize,
    /// Followed by a pause, or spoken with a prefix like "number one".
    clear: bool,
    /// Opens a sentence without a pause: "One I ran into the stairs." Only
    /// counts with others like it.
    soft: bool,
}

fn cardinal(word: &str) -> Option<usize> {
    let w = word.to_lowercase();
    if let Ok(n) = w.parse::<usize>() {
        return (1..=10).contains(&n).then_some(n);
    }
    NUMBER_WORDS[1..=10].iter().position(|x| *x == w).map(|p| p + 1)
}

/// A list marker at `i`. The first marker of a list must follow a pause;
/// later ones (`loose`) may not, as in "one milk two eggs", but then only
/// count as clear after a pause or an "and".
fn marker_at(tokens: &[Token], i: usize, loose: bool) -> Option<Marker> {
    let paused_before = at_pause(tokens, i) || (i > 0 && tokens[i - 1].is_word_ci("and"));
    if !loose && !at_pause(tokens, i) {
        return None;
    }
    if i > 0 && lower(&tokens[i - 1]).is_some_and(|w| NOT_BEFORE_MARKER.contains(&w.as_str())) {
        return None;
    }
    let bullet = ["bullet point", "next bullet", "bullet"].iter().find_map(|p| phrase(tokens, i, p));
    if let Some(end) = bullet {
        let end = skip_pause(tokens, end);
        return Some(Marker { n: 1, family: Family::Bullet, end, clear: paused_before, soft: false });
    }
    let first = lower(tokens.get(i)?)?;
    // "A. my homework. B. Feed the dog."
    if first.chars().count() == 1 {
        let n = first.chars().next().and_then(|c| LETTERS.find(c)).map(|p| p + 1);
        let pause = tokens.get(i + 1).is_some_and(|t| is_punct(t, &[".", ")", ":"]));
        if let (Some(n), true) = (n, pause) {
            let end = skip_pause(tokens, i + 1);
            return Some(Marker { n, family: Family::Letter, end, clear: paused_before, soft: false });
        }
    }
    // "The first is the budget." "The second one is the timeline."
    if first == "the" {
        let ord = lower(tokens.get(i + 1)?)?;
        if let Some(p) = ORDINALS.iter().position(|o| o.contains(&ord.as_str())) {
            let mut j = i + 2;
            if tokens.get(j).is_some_and(|t| t.is_word_ci("one") || t.is_word_ci("thing")) {
                j += 1;
            }
            if tokens.get(j).is_some_and(|t| t.is_word_ci("is")) {
                return Some(Marker { n: p + 1, family: Family::Ordinal, end: skip_pause(tokens, j + 1), clear: paused_before, soft: false });
            }
        }
        return None;
    }
    if let Some(end) = phrase(tokens, i, "last but not least") {
        return Some(Marker { n: 0, family: Family::Ordinal, end: skip_pause(tokens, end), clear: paused_before, soft: false });
    }
    let (n, family, prefixed, mut at) = if PREFIXES.contains(&first.as_str()) {
        let n = cardinal(tokens.get(i + 1)?.word()?)?;
        (n, Family::Cardinal, true, i + 1)
    } else if let Some(n) = cardinal(&first) {
        (n, Family::Cardinal, false, i)
    } else if let Some(p) = ORDINALS.iter().position(|o| o.contains(&first.as_str())) {
        (p + 1, Family::Ordinal, false, i)
    } else if FINAL_MARKERS.contains(&first.as_str()) {
        (0, Family::Ordinal, false, i)
    } else {
        return None;
    };
    // "First of all", "first off".
    if family == Family::Ordinal {
        if phrase(tokens, at + 1, "of all").is_some() {
            at += 2;
        } else if phrase(tokens, at + 1, "off").is_some() {
            at += 1;
        }
    }
    let paused = tokens.get(at + 1).is_some_and(|t| is_punct(t, &[",", ":", ".", ")"]));
    let digit = tokens[at].word().is_some_and(|w| w.chars().all(|c| c.is_ascii_digit()));
    let counting = tokens.get(at + 1).and_then(lower).is_some_and(|w| NOT_AFTER_MARKER.contains(&w.as_str()));
    // "First time we met", "second hand", "third floor".
    let ordinal_noun = tokens.get(at + 1).and_then(lower).is_some_and(|w| ORDINAL_NOUNS.contains(&w.as_str()));
    if family == Family::Ordinal && ordinal_noun {
        return None;
    }
    let opens_sentence = at_boundary(tokens, i) && tokens.get(at + 1).is_some_and(|t| t.word().is_some());
    Some(Marker {
        n,
        family,
        end: skip_pause(tokens, at + 1),
        clear: paused_before && (prefixed || paused || digit),
        soft: opens_sentence && !counting,
    })
}

// Announcing a list

/// What people list. Used with a number or a word like "some", so "I have
/// three kids" announces nothing.
const LIST_NOUNS: &[&str] = &[
    "things", "items", "points", "changes", "tasks", "steps", "options", "reasons", "questions", "issues", "problems",
    "ideas", "features", "requests", "updates", "fixes", "goals", "priorities", "bugs", "notes", "errors",
    "improvements", "tweaks", "suggestions", "comments", "concerns", "topics", "parts", "ways", "stuff", "groceries",
    "ingredients", "requirements", "rules", "edits",
];
const COUNT_WORDS: &[&str] = &["two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "2", "3", "4", "5", "6", "7", "8", "9", "10", "these", "those"];
const SOME_WORDS: &[&str] = &["some", "few", "couple", "several", "multiple", "many", "various", "bunch", "more"];
/// Longer than this, a sentence mentioning "things" is just talking about them.
const ANNOUNCE_MAX_WORDS: usize = 10;
/// Items after a soft announcement. Anything longer reads as prose.
const SHORT_ITEM_MAX_WORDS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Announces {
    No,
    /// "the action items", "here's what you need": only counts when what
    /// follows is short enough to be items and nothing else.
    IfShort,
    /// "some changes": says more may follow, which only counts when what
    /// follows looks like items.
    Maybe,
    /// "three things", "the following", "a list", or a closing colon.
    Yes,
}

fn announcement(sentence: &[Token]) -> Announces {
    let w: Vec<String> = sentence.iter().filter_map(lower).collect();
    let closes_with_colon = sentence.iter().rev().find(|t| !matches!(t, Token::Break(_))).is_some_and(|t| is_punct(t, &[":"]));
    // "the following day" is not a list; "the following changes" and a
    // sentence ending on "following" are.
    let following = w.iter().enumerate().any(|(i, x)| {
        x == "following" && w.get(i + 1).is_none_or(|n| !matches!(n.as_str(), "day" | "week" | "month" | "year" | "morning" | "night" | "evening"))
    });
    let as_follows = w.windows(2).any(|p| p[0] == "as" && p[1] == "follows");
    let list = w.iter().any(|x| x == "list");
    if closes_with_colon || following || as_follows || list {
        return Announces::Yes;
    }
    let names_a_list = |q: usize| w[q + 1..].iter().take(3).any(|x| LIST_NOUNS.contains(&x.as_str()));
    if (0..w.len()).any(|q| COUNT_WORDS.contains(&w[q].as_str()) && names_a_list(q)) {
        return Announces::Yes;
    }
    if (0..w.len()).any(|q| SOME_WORDS.contains(&w[q].as_str()) && names_a_list(q)) {
        return Announces::Maybe;
    }
    let here = matches!(
        (w.first().map(String::as_str), w.get(1).map(String::as_str), w.get(2).map(String::as_str)),
        (Some("here"), Some("are"), _) | (Some("here's"), Some("what"), _) | (Some("here"), Some("is"), Some("what"))
    );
    if here || (w.len() <= ANNOUNCE_MAX_WORDS && w.iter().any(|x| LIST_NOUNS.contains(&x.as_str()))) {
        return Announces::IfShort;
    }
    Announces::No
}

/// Whether a sentence can sit before a list as its intro. A short label
/// ("Agenda.") can; "We launched yesterday." cannot.
fn introduces(sentence: &[Token]) -> bool {
    let here = first_word(sentence).is_some_and(|w| w == "here" || w == "here's") && words(sentence) <= ANNOUNCE_MAX_WORDS;
    announcement(sentence) != Announces::No || words(sentence) <= 3 || here
}

fn skip_pause(tokens: &[Token], mut i: usize) -> usize {
    while tokens.get(i).is_some_and(|t| is_punct(t, &[",", ":", ".", ")"])) {
        i += 1;
    }
    i
}

/// Where the next marker in sequence starts, searching from `from`.
fn next_marker(tokens: &[Token], from: usize, want: usize, family: Family) -> Option<(usize, Marker)> {
    let limit = (from + 60).min(tokens.len());
    (from..limit).find_map(|j| {
        let m = marker_at(tokens, j, true)?;
        let fits = match family {
            Family::Bullet => m.family == Family::Bullet,
            _ => m.family == family && m.n == want || m.n == 0 && want >= 3,
        };
        (fits && j > from).then_some((j, m))
    })
}

/// "I need three things, one milk, two eggs, three bread" to an intro line
/// and a numbered list. Needs two or more markers in order from one, each
/// followed by a pause, or three or more short items.
pub fn lists(tokens: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len() + 8);
    let mut i = 0;
    while i < tokens.len() {
        // A list usually starts at a pause. Unpunctuated, "my plan first X
        // second Y" can start anywhere for ordinals, and for "one, two" once
        // the sentence has said a list is coming.
        let sentence_so_far = out.iter().rposition(|t| t.ends_sentence()).map_or(0, |p| p + 1);
        let found = marker_at(&tokens, i, false).filter(|m| m.n == 1).or_else(|| {
            let m = marker_at(&tokens, i, true).filter(|m| m.n == 1)?;
            let said_so = announcement(&out[sentence_so_far..]) != Announces::No;
            (m.family == Family::Ordinal || (m.family == Family::Cardinal && said_so)).then_some(m)
        });
        let Some(first) = found else {
            out.push(tokens[i].clone());
            i += 1;
            continue;
        };

        // Collect the items.
        let mut items: Vec<(usize, usize)> = Vec::new(); // token ranges
        let mut clear = vec![first.clear];
        let mut soft = vec![first.soft];
        let mut start = first.end;
        let mut want = 2;
        let last_end;
        loop {
            match next_marker(&tokens, start, want, first.family) {
                Some((j, m)) => {
                    items.push((start, j));
                    clear.push(m.clear);
                    soft.push(m.soft);
                    start = m.end;
                    want += 1;
                    if m.n == 0 {
                        // "finally" closes the list.
                        last_end = item_end(&tokens, start);
                        items.push((start, last_end));
                        break;
                    }
                }
                None => {
                    last_end = item_end(&tokens, start);
                    items.push((start, last_end));
                    break;
                }
            }
        }

        let counts: Vec<usize> = items.iter().map(|&(a, b)| words(&tokens[a..b])).collect();
        let intro_start = out.iter().rposition(|t| t.ends_sentence()).map_or(0, |p| p + 1);
        let last_sentence = {
            let end = out.len().saturating_sub(1);
            let from = out[..end].iter().rposition(|t| t.ends_sentence()).map_or(0, |p| p + 1);
            &out[from..]
        };
        let announced = if intro_start < out.len() {
            announcement(&out[intro_start..]) != Announces::No
        } else {
            announcement(last_sentence) != Announces::No
        };
        let firm = clear.iter().zip(&soft).all(|(c, s)| *c || *s);
        // "First pack the bags second lock the door."
        let ordinal_pair = first.family == Family::Ordinal
            && first.soft
            && items.len() == 2
            && counts.iter().all(|&c| c <= ORDINAL_PAIR_MAX_WORDS);
        let valid = items.len() >= 2
            && counts.iter().all(|&c| (1..=30).contains(&c))
            && (clear.iter().all(|&c| c)
                || ordinal_pair
                || (firm && (items.len() >= 3 || announced))
                || (items.len() >= 3 && counts.iter().all(|&c| c <= 6)));
        if !valid {
            out.push(tokens[i].clone());
            i += 1;
            continue;
        }

        // The intro: this sentence's words before the first marker.
        let colon_fits = intro_start == out.len() && introduces(last_sentence);
        let mut intro: Vec<Token> = out.split_off(intro_start);
        trim_trailing(&mut intro);
        if !intro.is_empty() {
            out.extend(intro);
            out.push(Token::Close(":".into()));
            out.push(Token::Break(1));
        } else if colon_fits && out.last().is_some_and(|t| is_punct(t, &["."])) {
            // "I need three things. One, milk." The sentence before
            // introduces the list.
            out.pop();
            out.push(Token::Close(":".into()));
            out.push(Token::Break(1));
        } else if !out.is_empty() {
            while out.last().is_some_and(|t| matches!(t, Token::Break(_))) {
                out.pop();
            }
            out.push(Token::Break(2));
        }

        for (k, &(a, b)) in items.iter().enumerate() {
            let mut item: Vec<Token> = trim_leading(&tokens[a..b]).to_vec();
            trim_trailing(&mut item);
            capitalise_first(&mut item);
            if k > 0 {
                out.push(Token::Break(1));
            }
            out.push(Token::Raw(match first.family {
                Family::Bullet => "-".into(),
                Family::Letter => format!("{}.", (b'A' + k as u8) as char),
                _ => format!("{}.", k + 1),
            }));
            out.extend(item);
        }

        i = last_end;
        // Sentence end that closed the last item.
        while tokens.get(i).is_some_and(|t| is_punct(t, &[",", ".", ";"])) {
            i += 1;
        }
        if i < tokens.len() && !matches!(tokens[i], Token::Break(_)) {
            out.push(Token::Break(2));
        }
    }
    out
}

/// The last item runs to the end of its sentence.
fn item_end(tokens: &[Token], start: usize) -> usize {
    (start..tokens.len())
        .find(|&j| tokens[j].ends_sentence() && j > start)
        .unwrap_or(tokens.len())
}

// 3b. Lists people say without numbers

/// Longest item in a list said without numbers. Anything longer reads as
/// prose, not a list.
const IMPLICIT_ITEM_MAX_WORDS: usize = 15;
const ANNOUNCED_ITEM_MAX_WORDS: usize = 20;
const LEAD_WORDS: usize = 2;

/// Splits into sentences, each keeping its closing punctuation.
fn sentences(tokens: &[Token]) -> Vec<Vec<Token>> {
    let mut out = vec![Vec::new()];
    for t in tokens {
        if let Some(s) = out.last_mut() {
            s.push(t.clone());
        }
        if t.ends_sentence() {
            out.push(Vec::new());
        }
    }
    out.retain(|s| words(s) > 0);
    out
}

/// The first `LEAD_WORDS` words, lowercase, as the item's opening.
fn lead(tokens: &[Token]) -> Option<Vec<String>> {
    let lead: Vec<String> = tokens.iter().filter_map(lower).take(LEAD_WORDS).collect();
    (lead.len() == LEAD_WORDS).then_some(lead)
}

fn strip_connectives(mut part: &[Token]) -> &[Token] {
    part = trim_leading(part);
    while part
        .first()
        .and_then(lower)
        .is_some_and(|w| matches!(w.as_str(), "and" | "also" | "then" | "plus"))
    {
        part = trim_leading(&part[1..]);
    }
    part
}

/// "I need a bigger board, I need multiplayer, I need audio chat": three or
/// more comma separated parts, all but at most one opening the same way.
fn parallel_clauses(sentence: &[Token]) -> Option<Vec<Vec<Token>>> {
    let parts: Vec<Vec<Token>> = sentence
        .split(|t| t.is_comma() || is_punct(t, &[";"]))
        .map(|p| strip_connectives(p).to_vec())
        .filter(|p| words(p) > 0)
        .collect();
    if parts.len() < 3 || parts.iter().any(|p| words(p) > IMPLICIT_ITEM_MAX_WORDS) {
        return None;
    }
    let leads: Vec<Option<Vec<String>>> = parts.iter().map(|p| lead(p)).collect();
    let shared = leads.iter().flatten().max_by_key(|l| leads.iter().filter(|m| m.as_ref() == Some(l)).count())?;
    let same = leads.iter().filter(|l| l.as_ref() == Some(shared)).count();
    (same >= 3 && same + 1 >= parts.len()).then_some(parts)
}

/// "I need three things: milk, eggs and bread": three or more short items
/// after a colon. Returns the intro and the items.
fn colon_list(sentence: &[Token]) -> Option<(Vec<Token>, Vec<Vec<Token>>)> {
    let colon = sentence.iter().position(|t| is_punct(t, &[":"]))?;
    let intro = sentence[..colon].to_vec();
    let mut items: Vec<Vec<Token>> = Vec::new();
    for part in sentence[colon + 1..].split(|t| t.is_comma()) {
        // "eggs and bread" at the end is two items.
        let part = strip_connectives(part);
        match part.iter().position(|t| t.is_word_ci("and")) {
            Some(a) if part.len() > a + 1 => {
                items.push(part[..a].to_vec());
                items.push(part[a + 1..].to_vec());
            }
            _ => items.push(part.to_vec()),
        }
    }
    items.retain(|p| words(p) > 0);
    (words(&intro) > 0 && items.len() >= 3 && items.iter().all(|p| words(p) <= 5)).then_some((intro, items))
}

/// Verbs that open an instruction, so "add X, add Y and make Z" reads as a
/// list of things to do.
const IMPERATIVES: &[&str] = &[
    "add", "make", "remove", "change", "fix", "update", "use", "move", "send", "call", "get", "set", "put", "delete",
    "create", "check", "write", "finish", "buy", "book", "ask", "tell", "email", "review", "schedule", "clean",
    "test", "run", "build", "ship", "open", "close", "turn", "show", "hide", "give", "take", "keep", "bring", "pay",
    "reduce", "increase", "improve", "replace", "rename", "print", "sign", "submit", "reply", "pick", "feed", "wash",
];
/// Openings that say "here is one more item" when they repeat.
const LIST_LEADS: &[&[&str]] = &[
    &["we", "should"],
    &["we", "need"],
    &["we", "must"],
    &["i", "need"],
    &["i", "want"],
    &["you", "should"],
    &["you", "need"],
    &["make", "sure"],
    &["let's", "not"],
];
/// A longer item in an instruction series reads as prose.
const SERIES_ITEM_MAX_WORDS: usize = 14;
/// Openers short enough to label what follows ("Some notes.").
const LABEL_MAX_WORDS: usize = 4;

fn first_word(tokens: &[Token]) -> Option<String> {
    tokens.iter().find_map(lower)
}

fn imperative(tokens: &[Token]) -> bool {
    first_word(tokens).is_some_and(|w| IMPERATIVES.contains(&w.as_str()))
}

/// All instructions, or all opening on the same word.
fn uniform(items: &[Vec<Token>]) -> bool {
    let first = items.first().and_then(|i| first_word(i));
    items.iter().all(|i| imperative(i)) || items.iter().all(|i| first_word(i) == first)
}

/// "Add X, add Y and make Z": comma separated items, the last possibly joined
/// by "and", when they are instructions or open the same way.
fn series(sentence: &[Token]) -> Option<Vec<Vec<Token>>> {
    let mut parts: Vec<Vec<Token>> = sentence
        .split(|t| t.is_comma() || is_punct(t, &[";"]))
        .map(|p| strip_connectives(p).to_vec())
        .filter(|p| words(p) > 0)
        .collect();
    if parts.len() < 2 {
        return None;
    }
    if let Some(last) = parts.last().cloned() {
        let joined = last.iter().position(|t| t.is_word_ci("and")).filter(|&a| a > 0 && a + 1 < last.len());
        if let Some(a) = joined {
            let (head, tail) = (last[..a].to_vec(), last[a + 1..].to_vec());
            let same = first_word(&head) == first_word(&tail);
            if imperative(&tail) || same {
                parts.pop();
                parts.push(head);
                parts.push(tail);
            }
        }
    }
    (parts.len() >= 3 && parts.iter().all(|p| words(p) <= SERIES_ITEM_MAX_WORDS) && uniform(&parts)).then_some(parts)
}

/// "I need three things, milk, eggs and bread": the first comma part says how
/// many, the rest are short items. Returns the intro and the items.
fn counted_series(sentence: &[Token]) -> Option<(Vec<Token>, Vec<Vec<Token>>)> {
    let comma = sentence.iter().position(Token::is_comma)?;
    let intro = sentence[..comma].to_vec();
    if announcement(&intro) != Announces::Yes {
        return None;
    }
    let mut items: Vec<Vec<Token>> = Vec::new();
    for part in sentence[comma + 1..].split(Token::is_comma) {
        let part = strip_connectives(part);
        match part.iter().position(|t| t.is_word_ci("and")) {
            Some(a) if a > 0 && part.len() > a + 1 => {
                items.push(part[..a].to_vec());
                items.push(part[a + 1..].to_vec());
            }
            _ => items.push(part.to_vec()),
        }
    }
    items.retain(|p| words(p) > 0);
    (items.len() >= 2 && items.iter().all(|p| words(p) <= 5)).then_some((intro, items))
}

fn push_bullets(out: &mut Vec<Token>, items: &[Vec<Token>]) {
    for (k, item) in items.iter().enumerate() {
        let mut item = trim_leading(item).to_vec();
        trim_trailing(&mut item);
        while item.last().is_some_and(|t| is_punct(t, &["?", "!"])) {
            item.pop();
        }
        capitalise_first(&mut item);
        if k > 0 {
            out.push(Token::Break(1));
        }
        out.push(Token::Raw("-".into()));
        out.extend(item);
    }
}

/// Ends the text before a list with a colon and a line break.
fn push_intro(out: &mut Vec<Token>, before: &[Token]) {
    let mut intro = before.to_vec();
    trim_trailing(&mut intro);
    if intro.is_empty() {
        return;
    }
    out.extend(intro);
    out.push(Token::Close(":".into()));
    out.push(Token::Break(1));
}

/// Lists said the way people talk, without numbers: a sentence of parallel
/// clauses, a colon followed by short items, a run of three or more short
/// sentences that open the same way, or short sentences after one that
/// announces them ("make the following changes"). Items keep their words.
pub fn implicit_lists(tokens: Vec<Token>) -> Vec<Token> {
    // A list already made from numbers or bullets is left alone.
    if tokens.iter().any(|t| matches!(t, Token::Raw(_) | Token::Break(_))) {
        return tokens;
    }
    let sents = sentences(&tokens);
    let short = |s: &Vec<Token>, max: usize| words(s) <= max && !s.last().is_some_and(|t| is_punct(t, &["?"]));

    for (k, s) in sents.iter().enumerate() {
        let before: Vec<Token> = sents[..k].concat();
        let after: Vec<Token> = sents[k + 1..].concat();
        let announced = k > 0 && announcement(&sents[k - 1]) != Announces::No;

        // One sentence of parallel clauses or instructions, after a sentence
        // that says items are coming ("I can't sleep, I can't eat" alone is
        // prose). Or a list with its intro in the same sentence.
        let found = announced
            .then(|| parallel_clauses(s).or_else(|| series(s)))
            .flatten()
            .map(|items| (before.clone(), items))
            .or_else(|| {
                colon_list(s)
                    .or_else(|| counted_series(s))
                    .map(|(intro, items)| ([before.clone(), intro].concat(), items))
            });
        if let Some((intro, items)) = found {
            let mut out = Vec::new();
            push_intro(&mut out, &intro);
            push_bullets(&mut out, &items);
            if words(&after) > 0 {
                out.push(Token::Break(2));
                out.extend(after);
            }
            return out;
        }

        // "Here is what I need: the report is late. The client called."
        // The words after a colon are the first item, the short sentences
        // after it the rest, three or more in all.
        if let Some(c) = s.iter().position(|t| is_punct(t, &[":"])) {
            let (head, first) = (&s[..c], &s[c + 1..]);
            let run = sents[k + 1..]
                .iter()
                .take_while(|n| short(n, ANNOUNCED_ITEM_MAX_WORDS))
                .count();
            if words(head) > 0 && (1..=ANNOUNCED_ITEM_MAX_WORDS).contains(&words(first)) && run >= 2 {
                let mut out = Vec::new();
                push_intro(&mut out, &[before, head.to_vec()].concat());
                let items: Vec<Vec<Token>> = std::iter::once(first.to_vec())
                    .chain(sents[k + 1..k + 1 + run].iter().cloned())
                    .collect();
                push_bullets(&mut out, &items);
                let rest: Vec<Token> = sents[k + 1 + run..].concat();
                if words(&rest) > 0 {
                    out.push(Token::Break(2));
                    out.extend(rest);
                }
                return out;
            }
        }

        // Short sentences after one that announces them. "Some changes" only
        // counts when they look like items.
        let announces = announcement(s);
        if announces != Announces::No {
            let run = sents[k + 1..]
                .iter()
                .take_while(|n| short(n, ANNOUNCED_ITEM_MAX_WORDS))
                .count();
            let items = &sents[k + 1..k + 1 + run];
            let fits = match announces {
                Announces::Yes => true,
                Announces::Maybe => uniform(items),
                _ => items.iter().all(|i| words(i) <= SHORT_ITEM_MAX_WORDS),
            };
            if run >= 2 && fits {
                let mut out = Vec::new();
                push_intro(&mut out, &[before, s.clone()].concat());
                push_bullets(&mut out, &sents[k + 1..k + 1 + run]);
                let rest: Vec<Token> = sents[k + 1 + run..].concat();
                if words(&rest) > 0 {
                    out.push(Token::Break(2));
                    out.extend(rest);
                }
                return out;
            }
        }

        // Three or more short sentences in a row opening the same way.
        if let Some(l) = lead(s).filter(|_| short(s, IMPLICIT_ITEM_MAX_WORDS)) {
            let run = sents[k..]
                .iter()
                .take_while(|n| short(n, IMPLICIT_ITEM_MAX_WORDS) && lead(n).as_ref() == Some(&l))
                .count();
            // "I went home. I went out. I went to sleep." is a story: it
            // needs a label before it or an opening that lists.
            let labelled = k > 0 && words(&sents[k - 1]) <= LABEL_MAX_WORDS;
            let lists = LIST_LEADS.iter().any(|p| l.iter().map(String::as_str).eq(p.iter().copied()));
            let distinct = (k..k + run).all(|a| (a + 1..k + run).all(|b| sents[a] != sents[b]));
            if run >= 3 && distinct && (labelled || lists) {
                let mut out = Vec::new();
                push_intro(&mut out, &before);
                push_bullets(&mut out, &sents[k..k + run]);
                let rest: Vec<Token> = sents[k + run..].concat();
                if words(&rest) > 0 {
                    out.push(Token::Break(2));
                    out.extend(rest);
                }
                return out;
            }
        }
    }
    tokens
}

// 4. Paragraphs

const TRANSITIONS: &[&str] = &[
    "also", "additionally", "another thing", "moving on", "on another note", "on a different note", "separately",
    "next", "anyway", "by the way", "in other news", "one more thing", "one last thing", "the other thing",
    "apart from that", "besides that", "as for", "regarding", "changing topics", "speaking of", "finally", "lastly",
    "in addition", "meanwhile", "let me know",
];
const PARAGRAPH_MIN_WORDS: usize = 60;
const PARAGRAPH_MIN_SENTENCES: usize = 4;
/// Longer than this, a paragraph is split where the topic changes most.
const PARAGRAPH_MAX_WORDS: usize = 70;
/// Email reads in shorter blocks.
const EMAIL_PARAGRAPH_MIN_WORDS: usize = 40;
const EMAIL_PARAGRAPH_MIN_SENTENCES: usize = 3;
const EMAIL_PARAGRAPH_MAX_WORDS: usize = 50;
/// Sentences on each side compared when looking for a change of topic.
const TOPIC_WINDOW: usize = 2;

/// Shared content words between two runs of sentences, 0 to 1.
fn cohesion(a: &[Vec<Token>], b: &[Vec<Token>]) -> f32 {
    let content = |run: &[Vec<Token>]| -> std::collections::HashSet<String> {
        let text: Vec<&str> = run.iter().flatten().filter_map(Token::word).collect();
        super::stopwords::countable_words(&text.join(" ")).into_iter().collect()
    };
    let (x, y) = (content(a), content(b));
    if x.is_empty() || y.is_empty() {
        return 0.0;
    }
    x.intersection(&y).count() as f32 / ((x.len() * y.len()) as f32).sqrt()
}

/// Sentence indexes in `from..to` to break at, so no paragraph runs past
/// `max` words: at the point where neighbouring sentences share the least
/// (TextTiling), nearest the middle on a tie.
fn topic_breaks(sents: &[Vec<Token>], from: usize, to: usize, max: usize, out: &mut Vec<usize>) {
    let total: usize = sents[from..to].iter().map(|s| words(s)).sum();
    if total <= max || to - from < TOPIC_WINDOW * 2 {
        return;
    }
    let middle = (from + to) as f32 / 2.0;
    let best = (from + TOPIC_WINDOW..=to - TOPIC_WINDOW).min_by(|&a, &b| {
        let score = |k: usize| cohesion(&sents[k - TOPIC_WINDOW..k], &sents[k..k + TOPIC_WINDOW]);
        score(a)
            .total_cmp(&score(b))
            .then((a as f32 - middle).abs().total_cmp(&(b as f32 - middle).abs()))
    });
    if let Some(k) = best {
        out.push(k);
        topic_breaks(sents, from, k, max, out);
        topic_breaks(sents, k, to, max, out);
    }
}

/// Long dictations with no breaks of their own get a paragraph break before
/// sentences that open with a transition, at least two sentences apart, and
/// wherever a paragraph would still run long, where the topic changes most.
pub fn paragraphs(tokens: Vec<Token>, app: AppKind) -> Vec<Token> {
    let (min_words, min_sentences, max_words) = if app == AppKind::Email {
        (EMAIL_PARAGRAPH_MIN_WORDS, EMAIL_PARAGRAPH_MIN_SENTENCES, EMAIL_PARAGRAPH_MAX_WORDS)
    } else {
        (PARAGRAPH_MIN_WORDS, PARAGRAPH_MIN_SENTENCES, PARAGRAPH_MAX_WORDS)
    };
    let sents = sentences(&tokens);
    if words(&tokens) < min_words
        || sents.len() < min_sentences
        || tokens.iter().any(|t| matches!(t, Token::Break(_) | Token::Raw(_)))
    {
        return tokens;
    }
    let mut breaks = Vec::new();
    let mut since_break = 0usize;
    for (k, s) in sents.iter().enumerate().skip(1) {
        since_break += 1;
        if since_break >= 2 && TRANSITIONS.iter().any(|p| phrase(s, 0, p).is_some()) {
            breaks.push(k);
            since_break = 0;
        }
    }
    let mut bounds = vec![0];
    bounds.extend_from_slice(&breaks);
    bounds.push(sents.len());
    for pair in bounds.windows(2) {
        topic_breaks(&sents, pair[0], pair[1], max_words, &mut breaks);
    }
    let mut out = Vec::with_capacity(tokens.len() + breaks.len());
    for (k, s) in sents.iter().enumerate() {
        if breaks.contains(&k) {
            out.push(Token::Break(2));
        }
        out.extend_from_slice(s);
    }
    out
}

// 5. Email layout

const GREETINGS: &[&str] = &["hi", "hello", "hey", "dear", "good morning", "good afternoon", "good evening"];
const SIGN_OFFS: &[&str] = &[
    "best regards", "kind regards", "warm regards", "many thanks", "all the best", "yours truly", "yours sincerely",
    "yours faithfully", "thank you so much", "thanks so much", "thanks a lot", "thanks again", "with thanks",
    "take care", "talk soon", "thank you", "regards", "best", "thanks", "cheers", "sincerely", "warmly",
    "respectfully",
];
/// Addressed to a group: "Hi team,".
const GROUP_NAMES: &[&str] = &["there", "team", "everyone", "everybody", "all", "guys", "folks"];
const GREETING_NAME_MAX_WORDS: usize = 3;

/// After a greeting, the sentence going on instead of a name.
const NOT_GREETING_NAMES: &[&str] = &[
    "thanks", "thank", "just", "quick", "sorry", "good", "you", "wanted", "hope", "will", "would", "what", "when",
    "where", "why", "who", "with",
];

/// A name in a greeting. Stopwords are the sentence going on ("Hello how are
/// you"), but a lowercase name is fine ("hi john,").
fn greeting_name(t: &Token) -> bool {
    lower(t).is_some_and(|w| {
        GROUP_NAMES.contains(&w.as_str())
            || (!super::stopwords::is_stopword(&w) && !NOT_GREETING_NAMES.contains(&w.as_str()))
    })
}

fn capitalise_name(tokens: &[Token]) -> Vec<Token> {
    tokens
        .iter()
        .map(|t| match t {
            Token::Word(w) if !GROUP_NAMES.contains(&w.to_lowercase().as_str()) => Token::Word(capitalise_word(w)),
            other => other.clone(),
        })
        .collect()
}

/// Greeting and sign off on their own lines, the way an email is laid out.
pub fn email(mut tokens: Vec<Token>) -> Vec<Token> {
    // Greeting: "Hi John," at the very start, name up to three words, with
    // or without a pause after it.
    let mut greeted = false;
    if let Some(after) = GREETINGS.iter().find_map(|g| phrase(&tokens, 0, g)) {
        // "Hi, John, thanks": the comma after "Hi" goes.
        let comma_first = tokens.get(after).is_some_and(Token::is_comma)
            && tokens.get(after + 1).is_some_and(is_capitalised);
        let from = if comma_first { after + 1 } else { after };
        let mut end = from;
        while end - from < GREETING_NAME_MAX_WORDS && tokens.get(end).is_some_and(greeting_name) {
            // Unpunctuated: a name stops where lowercase words carry on.
            if end > from && !tokens[end].word().is_some_and(|w| w.starts_with(char::is_uppercase)) {
                break;
            }
            end += 1;
        }
        let closed = tokens.get(end).is_some_and(|t| is_punct(t, &[",", ".", "!"]));
        let named = end > from;
        let rest = if closed { end + 1 } else { end };
        if rest < tokens.len() && (closed || named) {
            let mut head: Vec<Token> = tokens[..after].to_vec();
            head.extend(capitalise_name(&tokens[from..end]));
            head.push(Token::Close(",".into()));
            head.push(Token::Break(2));
            head.extend_from_slice(&tokens[rest..]);
            tokens = head;
            greeted = true;
        }
    }

    // A sign off with no name, after a greeting: its own paragraph.
    if greeted {
        let last_start = tokens[..tokens.len().saturating_sub(1)].iter().rposition(Token::ends_sentence).map_or(0, |p| p + 1);
        let end = tokens.iter().rposition(|t| !is_punct(t, &[".", "!"])).map_or(0, |p| p + 1);
        let bare = SIGN_OFFS.iter().any(|s| phrase(&tokens, last_start, s) == Some(end));
        if bare && last_start > 0 && !matches!(tokens[last_start - 1], Token::Break(_)) {
            tokens.insert(last_start, Token::Break(2));
            return tokens;
        }
    }

    // Sign off: "Best regards, Maaz." ending the text, name up to three words.
    let end = tokens.iter().rposition(|t| !is_punct(t, &[".", "!"])).map_or(0, |p| p + 1);
    for start in (0..end).rev().take(8) {
        // After a greeting this is clearly an email, so "thanks maaz" with no
        // pause before it signs off too.
        if !at_pause(&tokens, start) && !greeted {
            continue;
        }
        let Some(after) = SIGN_OFFS.iter().find_map(|s| phrase(&tokens, start, s)) else {
            continue;
        };
        // "regards" inside "best regards": the longer one starts earlier.
        let inside_longer = start > 0 && SIGN_OFFS.iter().any(|s| phrase(&tokens, start - 1, s).is_some_and(|e| e >= after));
        if inside_longer {
            continue;
        }
        let name_from = skip_pause(&tokens, after).min(end);
        let name = &tokens[name_from..end];
        let name_words = words(name);
        let named = name.iter().all(|t| {
            t.word().is_some_and(|w| {
                w.starts_with(char::is_uppercase) || (greeted && !super::stopwords::is_stopword(w) && !NOT_GREETING_NAMES.contains(&w))
            })
        });
        if start == 0 || !(1..=3).contains(&name_words) || !named {
            continue;
        }
        let mut body: Vec<Token> = tokens[..start].to_vec();
        while body.last().is_some_and(Token::is_comma) {
            body.pop();
        }
        if !body.last().is_some_and(Token::ends_sentence) {
            body.push(Token::Close(".".into()));
        }
        body.push(Token::Break(2));
        body.extend_from_slice(&tokens[start..after]);
        body.push(Token::Close(",".into()));
        body.push(Token::Break(1));
        body.extend(capitalise_name(name));
        return body;
    }
    tokens
}

// 6. Chat layout

/// Short chat messages read more naturally without a closing full stop.
pub fn chat(mut tokens: Vec<Token>) -> Vec<Token> {
    let sentences = tokens.iter().filter(|t| t.ends_sentence()).count();
    let multiline = tokens.iter().any(|t| matches!(t, Token::Break(_) | Token::Raw(_)));
    if sentences <= 2 && !multiline && tokens.last().is_some_and(|t| is_punct(t, &[".", "...", "\u{2026}"])) {
        tokens.pop();
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::format::{render, tidy};
    use crate::text::tokenize;

    fn run(f: fn(Vec<Token>) -> Vec<Token>, text: &str) -> String {
        render(&tidy(f(tokenize(text))))
    }

    // Backtrack

    #[test]
    fn corrects_a_number() {
        assert_eq!(run(backtrack, "Let's do coffee at 2 actually 3."), "Let's do coffee at 3.");
        assert_eq!(run(backtrack, "Send it at 2 pm, I mean 3 pm."), "Send it at 3 pm.");
        assert_eq!(run(backtrack, "We need five, no, six chairs."), "We need six chairs.");
    }

    #[test]
    fn corrects_a_day_or_month() {
        assert_eq!(run(backtrack, "Let's meet on Tuesday, no, Wednesday."), "Let's meet on Wednesday.");
        assert_eq!(run(backtrack, "The launch is in May, sorry, June."), "The launch is in June.");
        assert_eq!(run(backtrack, "Call me today, actually tomorrow."), "Call me tomorrow.");
    }

    #[test]
    fn leaves_real_uses_alone() {
        for s in [
            "I actually enjoyed the movie.",
            "No, I mean it.",
            "Sorry, I was late.",
            "Wait for me at the door.",
            "I have 2 actually.",
            "It was Tuesday, actually a good day.",
        ] {
            assert_eq!(run(backtrack, s), s, "{s}");
        }
    }

    #[test]
    fn scratch_that_drops_the_sentence_so_far() {
        assert_eq!(
            run(backtrack, "Send the deck on Monday scratch that send it on Tuesday."),
            "Send it on Tuesday."
        );
        assert_eq!(run(backtrack, "The total is ten. Scratch that. It is eleven."), "It is eleven.");
        assert_eq!(run(backtrack, "Hello there, never mind. See you soon."), "See you soon.");
        assert_eq!(run(backtrack, "Scratch that."), "");
    }

    // Spoken breaks

    #[test]
    fn paragraph_commands_always_break() {
        assert_eq!(
            run(spoken_breaks, "Thanks for the update new paragraph see you soon"),
            "Thanks for the update\n\nSee you soon"
        );
        assert_eq!(run(spoken_breaks, "Done. Start a new paragraph. Next topic."), "Done.\n\nNext topic.");
    }

    #[test]
    fn line_commands_only_at_a_pause() {
        assert_eq!(run(spoken_breaks, "Hi John, new line, thanks again."), "Hi John\nThanks again.");
        assert_eq!(run(spoken_breaks, "Milk. Next line. Eggs."), "Milk.\nEggs.");
        assert_eq!(
            run(spoken_breaks, "We launched a new line of products."),
            "We launched a new line of products."
        );
    }

    // Lists

    #[test]
    fn numbered_list_with_an_intro() {
        assert_eq!(
            run(lists, "I need to buy three things. One, milk. Two, eggs. Three, bread."),
            "I need to buy three things:\n1. Milk\n2. Eggs\n3. Bread"
        );
        assert_eq!(
            run(lists, "My plan for today: one, finish the report, two, send the presentation, and three, call John."),
            "My plan for today:\n1. Finish the report\n2. Send the presentation\n3. Call John"
        );
    }

    #[test]
    fn ordinal_list_with_finally() {
        assert_eq!(
            run(lists, "First, open the app. Second, pick a model. Finally, hold the hotkey and speak."),
            "1. Open the app\n2. Pick a model\n3. Hold the hotkey and speak"
        );
    }

    #[test]
    fn digits_and_spoken_prefixes() {
        assert_eq!(
            run(lists, "Agenda. 1. Budget. 2. Hiring. 3. Launch date."),
            "Agenda:\n1. Budget\n2. Hiring\n3. Launch date"
        );
        assert_eq!(
            run(lists, "Number one, the budget. Number two, the timeline."),
            "1. The budget\n2. The timeline"
        );
        assert_eq!(
            run(lists, "Steps: step one install it, step two open it."),
            "Steps:\n1. Install it\n2. Open it"
        );
    }

    #[test]
    fn short_unpunctuated_items_need_three() {
        assert_eq!(
            run(lists, "Shopping list, one milk two eggs three bread"),
            "Shopping list:\n1. Milk\n2. Eggs\n3. Bread"
        );
        // Only two short items and no pauses: leave it.
        assert_eq!(run(lists, "I have one dog two cats"), "I have one dog two cats");
    }

    #[test]
    fn ordinary_counting_is_not_a_list() {
        for s in [
            "One thing I noticed is that two people left early.",
            "I need one more day.",
            "Two of them came, three stayed home.",
            "The first time we met was the second week of May.",
        ] {
            assert_eq!(run(lists, s), s, "{s}");
        }
    }

    #[test]
    fn text_after_the_list_gets_its_own_paragraph() {
        assert_eq!(
            run(lists, "Two things. One, the budget. Two, the timeline. Let me know what you think."),
            "Two things:\n1. The budget\n2. The timeline\n\nLet me know what you think."
        );
    }

    #[test]
    fn bullet_points() {
        assert_eq!(
            run(lists, "Notes. Bullet point fix the login. Bullet point update the docs."),
            "Notes:\n- Fix the login\n- Update the docs"
        );
    }

    // Lists without numbers

    #[test]
    fn parallel_clauses_become_bullets() {
        assert_eq!(
            run(
                implicit_lists,
                "I need to make some changes in the monopoly game. I need a bigger board, I need multiplayer, \
                 I need audio chat, and I think I need an Urdu version of the game."
            ),
            "I need to make some changes in the monopoly game:\n- I need a bigger board\n- I need multiplayer\n\
             - I need audio chat\n- I think I need an Urdu version of the game"
        );
    }

    #[test]
    fn announced_lists_become_bullets() {
        assert_eq!(
            run(
                implicit_lists,
                "I really like the game, but make the following changes. Make the board smaller. \
                 Add better animations. Things should feel smoother."
            ),
            "I really like the game, but make the following changes:\n- Make the board smaller\n\
             - Add better animations\n- Things should feel smoother"
        );
        assert_eq!(
            run(implicit_lists, "Here is what I need: the report is late. The client called. Send it today."),
            "Here is what I need:\n- The report is late\n- The client called\n- Send it today"
        );
    }

    #[test]
    fn repeated_openings_across_sentences() {
        assert_eq!(
            run(
                implicit_lists,
                "Some notes. We should fix the login. We should update the docs. We should ship on Friday. Thanks."
            ),
            "Some notes:\n- We should fix the login\n- We should update the docs\n- We should ship on Friday\n\nThanks."
        );
    }

    #[test]
    fn colon_lists_of_short_items() {
        assert_eq!(
            run(implicit_lists, "I need three things: milk, eggs and bread."),
            "I need three things:\n- Milk\n- Eggs\n- Bread"
        );
    }

    #[test]
    fn ordinary_prose_stays_prose() {
        for s in [
            "I like apples, oranges, and pears.",
            "I think so, I guess, maybe we can.",
            "We should go. We should probably eat first. Then a film.",
            "Make the board smaller and the effects larger.",
            "The time is: 3 pm.",
            "I need a bigger board, I need multiplayer.",
            "Can you check the following? Is it ready? Did they pay?",
        ] {
            assert_eq!(run(implicit_lists, s), s, "{s}");
        }
    }

    // Paragraphs

    fn other_paragraphs(tokens: Vec<Token>) -> Vec<Token> {
        paragraphs(tokens, AppKind::Other)
    }

    #[test]
    fn long_rambles_break_at_transitions() {
        let text = "The release went out this morning and the first reports look good. Most people \
                    updated within an hour. Support has seen a few questions about the new hotkey. \
                    Also, the design team wants to review the onboarding screens before Friday. \
                    They have a few ideas for the welcome page. Anyway, let me know if you want \
                    to join the review call on Thursday afternoon.";
        let out = run(other_paragraphs, text);
        assert_eq!(out.matches("\n\n").count(), 2, "{out}");
        assert!(out.contains("\n\nAlso, the design team"));
        assert!(out.contains("\n\nAnyway, let me know"));
    }

    #[test]
    fn short_text_is_never_split() {
        let s = "Thanks. Also, see you tomorrow. Anyway, bye. Next week works.";
        assert_eq!(run(other_paragraphs, s), s);
    }

    #[test]
    fn long_text_with_no_transitions_splits_where_the_topic_changes() {
        let text = "The kitchen renovation is running late because the tiles arrived broken. The tiles \
                    need replacing and the kitchen fitter wants another week. The fitter says the kitchen \
                    tiles should arrive on Monday. Separate from that my car failed its inspection this \
                    morning. The car needs new brakes and the garage quoted a high price for brakes. \
                    The garage can fix the car before the weekend if I approve the brakes today.";
        let out = run(other_paragraphs, text);
        assert_eq!(out.matches("\n\n").count(), 1, "{out}");
        assert!(out.contains("\n\nSeparate from that my car"), "{out}");
    }

    // Email

    #[test]
    fn email_greeting_and_sign_off() {
        assert_eq!(
            run(email, "Hi John, thanks for the update. I'll send the deck tomorrow. Best regards, Maaz."),
            "Hi John,\n\nThanks for the update. I'll send the deck tomorrow.\n\nBest regards,\nMaaz"
        );
        assert_eq!(
            run(email, "Dear Ms Khan. Please find the invoice attached. Thanks, Ali Raza"),
            "Dear Ms Khan,\n\nPlease find the invoice attached.\n\nThanks,\nAli Raza"
        );
    }

    #[test]
    fn email_without_greeting_or_sign_off_is_unchanged() {
        let s = "Please send the invoice by Friday. Thanks.";
        assert_eq!(run(email, s), s);
        let s = "Hi.";
        assert_eq!(run(email, s), s);
    }

    // Chat

    #[test]
    fn chat_drops_the_final_full_stop_on_short_messages() {
        assert_eq!(run(chat, "Sounds good, see you at 3."), "Sounds good, see you at 3");
        assert_eq!(run(chat, "On my way. Five minutes."), "On my way. Five minutes");
        assert_eq!(run(chat, "Are you coming?"), "Are you coming?");
        let long = "One. Two. Three.";
        assert_eq!(run(chat, long), long);
    }

    #[test]
    fn app_kinds() {
        assert_eq!(AppKind::detect("OUTLOOK.EXE", "Inbox"), AppKind::Email);
        assert_eq!(AppKind::detect("chrome.exe", "Inbox (3) - me@x.com - Gmail"), AppKind::Email);
        assert_eq!(AppKind::detect("slack.exe", "general"), AppKind::Chat);
        assert_eq!(AppKind::detect("msedge.exe", "WhatsApp"), AppKind::Chat);
        assert_eq!(AppKind::detect("notepad.exe", "Gmail notes.txt"), AppKind::Other);
        assert_eq!(AppKind::detect("Code.exe", "slack.rs"), AppKind::Other);
        assert_eq!(AppKind::detect("Slack.app", "general"), AppKind::Chat);
        assert_eq!(AppKind::detect("Google Chrome.app", "Inbox - Gmail"), AppKind::Email);
        assert_eq!(AppKind::detect("Mail.app", ""), AppKind::Email);
    }
}
