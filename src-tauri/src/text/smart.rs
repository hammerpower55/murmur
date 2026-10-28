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

const EMAIL_APPS: &[&str] = &["outlook.exe", "olk.exe", "hxoutlook.exe", "thunderbird.exe", "mailspring.exe", "mailclient.exe"];
const CHAT_APPS: &[&str] = &[
    "slack.exe", "ms-teams.exe", "teams.exe", "whatsapp.exe", "discord.exe", "telegram.exe", "signal.exe",
    "messenger.exe", "zoom.exe",
];
const BROWSERS: &[&str] = &["chrome.exe", "msedge.exe", "firefox.exe", "brave.exe", "opera.exe", "vivaldi.exe", "arc.exe"];
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

const SCRATCH: &[&str] = &["scratch that", "strike that", "delete that", "forget that", "never mind"];
const CORRECTIONS: &[&str] = &["no wait", "or rather", "i mean", "make that", "actually", "sorry", "no", "wait", "rather"];

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
    lower(t).is_some_and(|w| matches!(w.as_str(), "am" | "pm" | "a.m" | "p.m"))
}

/// The value ending just before `end` (skipping one comma): its start index
/// and kind. "2 pm" counts as one value.
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
    Some((start, kind))
}

/// The value starting at `i` (skipping one comma): its end index and kind.
fn value_after(tokens: &[Token], mut i: usize) -> Option<(usize, usize, ValueKind)> {
    if tokens.get(i).is_some_and(Token::is_comma) {
        i += 1;
    }
    let kind = value_kind(tokens.get(i)?.word()?)?;
    let mut end = i + 1;
    if tokens.get(end).is_some_and(is_meridiem) {
        end += 1;
    }
    Some((i, end, kind))
}

/// "at 2 actually 3" to "at 3", "scratch that" drops what came before.
pub fn backtrack(tokens: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        // Scratch that: drop the sentence so far, or the previous one.
        if let Some(end) = SCRATCH.iter().find_map(|p| phrase(&tokens, i, p)) {
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

        // Value corrections: the value before the trigger is replaced by
        // the value after it, when both are the same kind.
        if let Some(after) = CORRECTIONS.iter().find_map(|p| phrase(&tokens, i, p)) {
            if let (Some((a_start, a_kind)), Some((b_start, b_end, b_kind))) =
                (value_before(&out, out.len()), value_after(&tokens, after))
            {
                if a_kind == b_kind {
                    out.truncate(a_start);
                    out.extend_from_slice(&tokens[b_start..b_end]);
                    i = b_end;
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
const LINE_PHRASES: &[&str] = &["new line", "next line", "line break"];

/// "new paragraph" always breaks; "new line" only at a pause, so "a new
/// line of products" stays as words.
pub fn spoken_breaks(tokens: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let paragraph = PARAGRAPH_PHRASES.iter().find_map(|p| phrase(&tokens, i, p));
        let line = LINE_PHRASES.iter().find_map(|p| phrase(&tokens, i, p)).filter(|&end| {
            at_pause(&tokens, i) && (end == tokens.len() || is_close(&tokens[end]) || matches!(tokens[end], Token::Break(_)))
        });
        let hit = paragraph.map(|e| (e, 2)).or(line.map(|e| (e, 1)));
        match hit {
            Some((end, n)) => {
                while out.last().is_some_and(Token::is_comma) {
                    out.pop();
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Cardinal,
    Ordinal,
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
    if let Some(end) = phrase(tokens, i, "bullet point") {
        let end = skip_pause(tokens, end);
        return Some(Marker { n: 1, family: Family::Bullet, end, clear: paused_before });
    }
    let first = lower(tokens.get(i)?)?;
    let (n, family, prefixed, at) = if PREFIXES.contains(&first.as_str()) {
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
    let paused = tokens.get(at + 1).is_some_and(|t| is_punct(t, &[",", ":", ".", ")"]));
    let digit = tokens[at].word().is_some_and(|w| w.chars().all(|c| c.is_ascii_digit()));
    Some(Marker {
        n,
        family,
        end: skip_pause(tokens, at + 1),
        clear: paused_before && (prefixed || paused || digit),
    })
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
        let Some(first) = marker_at(&tokens, i, false).filter(|m| m.n == 1) else {
            out.push(tokens[i].clone());
            i += 1;
            continue;
        };

        // Collect the items.
        let mut items: Vec<(usize, usize)> = Vec::new(); // token ranges
        let mut clear = vec![first.clear];
        let mut start = first.end;
        let mut want = 2;
        let last_end;
        loop {
            match next_marker(&tokens, start, want, first.family) {
                Some((j, m)) => {
                    items.push((start, j));
                    clear.push(m.clear);
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
        let valid = items.len() >= 2
            && counts.iter().all(|&c| (1..=30).contains(&c))
            && (clear.iter().all(|&c| c) || (items.len() >= 3 && counts.iter().all(|&c| c <= 6)));
        if !valid {
            out.push(tokens[i].clone());
            i += 1;
            continue;
        }

        // The intro: this sentence's words before the first marker.
        let intro_start = out.iter().rposition(|t| t.ends_sentence()).map_or(0, |p| p + 1);
        let mut intro: Vec<Token> = out.split_off(intro_start);
        trim_trailing(&mut intro);
        if !intro.is_empty() {
            out.extend(intro);
            out.push(Token::Close(":".into()));
            out.push(Token::Break(1));
        } else if out.last().is_some_and(|t| is_punct(t, &["."])) {
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

fn announces_a_list(sentence: &[Token]) -> bool {
    sentence.iter().any(|t| t.is_word_ci("following"))
        || sentence.iter().rev().find(|t| !matches!(t, Token::Break(_))).is_some_and(|t| is_punct(t, &[":"]))
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

        // One sentence of parallel clauses, or a colon list.
        let found = parallel_clauses(s)
            .map(|items| (before.clone(), items))
            .or_else(|| colon_list(s).map(|(intro, items)| ([before.clone(), intro].concat(), items)));
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

        // Short sentences after one that announces them.
        if announces_a_list(s) {
            let run = sents[k + 1..]
                .iter()
                .take_while(|n| short(n, ANNOUNCED_ITEM_MAX_WORDS))
                .count();
            if run >= 2 {
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
            if run >= 3 {
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
    "also", "additionally", "another thing", "moving on", "on another note", "separately", "next", "anyway",
    "by the way", "in other news", "one more thing", "apart from that", "besides that",
];
const PARAGRAPH_MIN_WORDS: usize = 60;
const PARAGRAPH_MIN_SENTENCES: usize = 4;

/// Long dictations with no breaks of their own get a paragraph break before
/// sentences that open with a transition, at least two sentences apart.
pub fn paragraphs(tokens: Vec<Token>) -> Vec<Token> {
    let sentences = tokens.iter().filter(|t| t.ends_sentence()).count();
    if words(&tokens) < PARAGRAPH_MIN_WORDS
        || sentences < PARAGRAPH_MIN_SENTENCES
        || tokens.iter().any(|t| matches!(t, Token::Break(_) | Token::Raw(_)))
    {
        return tokens;
    }
    let mut out = Vec::with_capacity(tokens.len() + 4);
    let mut since_break = 0usize;
    for (i, t) in tokens.iter().enumerate() {
        if i > 0 && at_boundary(&tokens, i) {
            since_break += 1;
            if since_break >= 2 && TRANSITIONS.iter().any(|p| phrase(&tokens, i, p).is_some()) {
                out.push(Token::Break(2));
                since_break = 0;
            }
        }
        out.push(t.clone());
    }
    out
}

// 5. Email layout

const GREETINGS: &[&str] = &["hi", "hello", "hey", "dear", "good morning", "good afternoon", "good evening"];
const SIGN_OFFS: &[&str] = &[
    "best regards", "kind regards", "warm regards", "many thanks", "all the best", "yours truly", "yours sincerely",
    "thank you", "regards", "best", "thanks", "cheers", "sincerely",
];

/// Greeting and sign off on their own lines, the way an email is laid out.
pub fn email(mut tokens: Vec<Token>) -> Vec<Token> {
    // Greeting: "Hi John," at the very start, name up to three words.
    if let Some(after) = GREETINGS.iter().find_map(|g| phrase(&tokens, 0, g)) {
        let close = (after..(after + 4).min(tokens.len())).find(|&j| is_punct(&tokens[j], &[",", ".", "!"]));
        if let Some(close) = close {
            let rest = close + 1;
            if rest < tokens.len() && words(&tokens[after..close]) <= 3 {
                let mut head: Vec<Token> = tokens[..close].to_vec();
                head.push(Token::Close(",".into()));
                head.push(Token::Break(2));
                head.extend_from_slice(&tokens[rest..]);
                tokens = head;
            }
        }
    }

    // Sign off: "Best regards, Maaz." ending the text, name up to three words.
    let end = tokens.iter().rposition(|t| !is_punct(t, &[".", "!"])).map_or(0, |p| p + 1);
    for start in (0..end).rev().take(8) {
        if !at_pause(&tokens, start) {
            continue;
        }
        let Some(after) = SIGN_OFFS.iter().find_map(|s| phrase(&tokens, start, s)) else {
            continue;
        };
        let name_from = skip_pause(&tokens, after).min(end);
        let name = &tokens[name_from..end];
        let name_words = words(name);
        if start == 0 || !(1..=3).contains(&name_words) || name.iter().any(|t| t.word().is_none()) {
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
        body.extend_from_slice(name);
        return body;
    }
    tokens
}

// 6. Chat layout

/// Short chat messages read more naturally without a closing full stop.
pub fn chat(mut tokens: Vec<Token>) -> Vec<Token> {
    let sentences = tokens.iter().filter(|t| t.ends_sentence()).count();
    let multiline = tokens.iter().any(|t| matches!(t, Token::Break(_) | Token::Raw(_)));
    if sentences <= 2 && !multiline && tokens.last().is_some_and(|t| is_punct(t, &["."])) {
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

    #[test]
    fn long_rambles_break_at_transitions() {
        let text = "The release went out this morning and the first reports look good. Most people \
                    updated within an hour. Support has seen a few questions about the new hotkey. \
                    Also, the design team wants to review the onboarding screens before Friday. \
                    They have a few ideas for the welcome page. Anyway, let me know if you want \
                    to join the review call on Thursday afternoon.";
        let out = run(paragraphs, text);
        assert_eq!(out.matches("\n\n").count(), 2, "{out}");
        assert!(out.contains("\n\nAlso, the design team"));
        assert!(out.contains("\n\nAnyway, let me know"));
    }

    #[test]
    fn short_text_is_never_split() {
        let s = "Thanks. Also, see you tomorrow. Anyway, bye. Next week works.";
        assert_eq!(run(paragraphs, s), s);
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
    }
}
