//! Numbers said as words become what a careful writer would type, following
//! AP style: digits from 10 up, and for money, percentages, times of day,
//! measurements, decimals and arithmetic; words below 10; words at the start
//! of a sentence. Roman Urdu sentences are left alone.

use super::{is_urdu, sentence_spans, Token};

const SMALL: &[&str] = &[
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve",
    "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen",
];
const TENS: &[&str] = &["twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
const ORDINALS: &[&str] = &[
    "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth", "eleventh",
    "twelfth", "thirteenth", "fourteenth", "fifteenth", "sixteenth", "seventeenth", "eighteenth", "nineteenth",
];
const TENS_ORDINALS: &[(&str, u64)] = &[("twentieth", 20), ("thirtieth", 30)];
const MONTHS: &[&str] = &[
    "january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november",
    "december",
];
/// Always digits, even below 10.
const MEASURES: &[&str] = &[
    "gb", "mb", "kb", "tb", "km", "kg", "cm", "mm", "mph", "kph", "px", "pixels", "megabytes", "gigabytes",
    "terabytes", "kilobytes", "ghz", "mhz", "kilometers", "kilometres", "kilograms", "meters", "metres", "degrees",
    "milliseconds", "ms",
];
/// "the twenty second of March", "the thirty second floor".
const ORDINAL_NOUNS: &[&str] = &[
    "of", "century", "floor", "birthday", "anniversary", "day", "time", "street", "place", "row", "year", "week",
];
/// Before these "ten thirty" is a time of day.
const TIME_LEADS: &[&str] = &["at", "by", "around", "until", "till", "before", "after", "from"];

pub fn is_number_word(word: &str) -> bool {
    let w = word.to_lowercase();
    small(&w).is_some() || tens(&w).is_some()
}

fn small(w: &str) -> Option<u64> {
    SMALL.iter().position(|x| *x == w).map(|p| p as u64)
}

fn tens(w: &str) -> Option<u64> {
    TENS.iter().position(|x| *x == w).map(|p| (p as u64 + 2) * 10)
}

fn scale(w: &str) -> Option<u64> {
    match w {
        "thousand" => Some(1_000),
        "million" => Some(1_000_000),
        "billion" => Some(1_000_000_000),
        _ => None,
    }
}

fn ordinal(w: &str) -> Option<u64> {
    ORDINALS
        .iter()
        .position(|x| *x == w)
        .map(|p| p as u64 + 1)
        .or_else(|| TENS_ORDINALS.iter().find(|(x, _)| *x == w).map(|(_, v)| *v))
}

fn word(s: &[Token], i: usize) -> Option<String> {
    s.get(i)?.word().map(str::to_lowercase)
}

/// Commas for thousands, but not in years, and a word for exact millions.
fn render(n: u64) -> String {
    if n >= 1_000_000_000 && n.is_multiple_of(1_000_000_000) {
        return format!("{} billion", n / 1_000_000_000);
    }
    if n >= 1_000_000 && n.is_multiple_of(1_000_000) {
        return format!("{} million", n / 1_000_000);
    }
    if (1900..=2099).contains(&n) && !n.is_multiple_of(1000) {
        return n.to_string();
    }
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn with_suffix(n: u64) -> String {
    let suffix = match (n % 100, n % 10) {
        (11..=13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// A cardinal number starting at `i`: its value and the index after it.
fn cardinal(s: &[Token], mut i: usize) -> Option<(u64, usize)> {
    let (mut total, mut current) = (0u64, 0u64);
    let mut seen = false;
    if word(s, i).as_deref() == Some("a") {
        let next = word(s, i + 1)?;
        if next != "hundred" && scale(&next).is_none() {
            return None;
        }
        current = 1;
        i += 1;
    }
    while let Some(w) = word(s, i) {
        if let Some(v) = small(&w) {
            let after_tens = current % 100 >= 20 && current % 10 == 0 && (1..10).contains(&v);
            if current % 100 != 0 && !after_tens {
                break;
            }
            if v == 0 && seen {
                break;
            }
            current += v;
        } else if let Some(v) = tens(&w) {
            if current % 100 != 0 {
                break;
            }
            current += v;
        } else if w == "hundred" {
            if current >= 100 {
                break;
            }
            current = current.max(1) * 100;
        } else if let Some(v) = scale(&w) {
            total += current.max(1) * v;
            current = 0;
        } else if w == "and" && seen && current % 100 == 0 && (current >= 100 || total >= 1000) {
            let next = word(s, i + 1)?;
            if small(&next).is_none() && tens(&next).is_none() {
                break;
            }
        } else {
            break;
        }
        seen = true;
        i += 1;
    }
    seen.then_some((total + current, i))
}

/// A year said in two parts: "twenty twenty six", "nineteen ninety nine".
fn year(s: &[Token], i: usize) -> Option<(u64, usize)> {
    let (first, mid) = cardinal(s, i)?;
    if first != 19 && first != 20 {
        return None;
    }
    let (second, end) = cardinal(s, mid)?;
    (10..=99).contains(&second).then_some((first * 100 + second, end))
}

/// A decimal part: "point two five".
fn decimal(s: &[Token], i: usize) -> Option<(String, usize)> {
    if word(s, i).as_deref() != Some("point") {
        return None;
    }
    let mut digits = String::new();
    let mut j = i + 1;
    while let Some(v) = word(s, j).and_then(|w| small(&w)).filter(|v| *v < 10) {
        digits.push_str(&v.to_string());
        j += 1;
    }
    (!digits.is_empty()).then_some((digits, j))
}

fn digits_only(w: &str) -> bool {
    !w.is_empty() && w.chars().all(|c| c.is_ascii_digit())
}

fn numeric(w: &str) -> bool {
    w.chars().next().is_some_and(|c| c.is_ascii_digit())
        && w.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',')
}

fn meridiem(w: &str) -> Option<&'static str> {
    match w {
        "am" | "a.m." | "a.m" => Some("AM"),
        "pm" | "p.m." | "p.m" => Some("PM"),
        _ => None,
    }
}

/// "3pm" or "3:30pm" written as one word.
fn joined_time(w: &str) -> Option<(String, &'static str)> {
    let (time, m) = w.split_at(w.len().checked_sub(2)?);
    let m = meridiem(m)?;
    let hour = time.split(':').next()?;
    (digits_only(hour) && time.chars().all(|c| c.is_ascii_digit() || c == ':')).then(|| (time.to_string(), m))
}

fn is_month_token(t: Option<&Token>) -> bool {
    t.and_then(Token::word).is_some_and(|w| {
        let lower = w.to_lowercase();
        w.starts_with(char::is_uppercase) && MONTHS.contains(&lower.as_str())
    })
}

fn operator(s: &[Token], i: usize) -> Option<(&'static str, usize)> {
    match word(s, i)?.as_str() {
        "plus" => Some(("+", i + 1)),
        "minus" => Some(("-", i + 1)),
        "times" => Some(("\u{00D7}", i + 1)),
        "equals" => Some(("=", i + 1)),
        "divided" if word(s, i + 1).as_deref() == Some("by") => Some(("\u{00F7}", i + 2)),
        "multiplied" if word(s, i + 1).as_deref() == Some("by") => Some(("\u{00D7}", i + 2)),
        _ => None,
    }
}

/// One operand: words or digits.
fn operand(s: &[Token], i: usize) -> Option<(String, usize)> {
    let w = word(s, i)?;
    if numeric(&w) {
        return Some((w, i + 1));
    }
    if w == "a" {
        return None;
    }
    let (v, end) = cardinal(s, i)?;
    Some((render(v), end))
}

fn expression(s: &[Token], i: usize) -> Option<(Vec<Token>, usize)> {
    let (first, mut j) = operand(s, i)?;
    let mut out = vec![Token::Word(first)];
    while let Some((op, after)) = operator(s, j) {
        let Some((value, end)) = operand(s, after) else {
            break;
        };
        out.push(Token::Word(op.to_string()));
        out.push(Token::Word(value));
        j = end;
    }
    (out.len() > 1).then_some((out, j))
}

fn words_of(text: String) -> Vec<Token> {
    text.split(' ').map(|w| Token::Word(w.to_string())).collect()
}

/// What to write at `i`, if a number starts there: the tokens and where to go
/// on from.
fn at(s: &[Token], i: usize, sentence_start: bool) -> Option<(Vec<Token>, usize)> {
    let w = word(s, i)?;

    // "a hundred ideas" was already judged at its "a" and stays as said.
    let after_a = i.checked_sub(1).and_then(|p| word(s, p)).as_deref() == Some("a");
    if after_a && (w == "hundred" || scale(&w).is_some()) {
        return None;
    }

    if let Some(found) = expression(s, i) {
        return Some(found);
    }

    // Already digits, with a word that has a usual symbol or spelling.
    if let Some((time, m)) = joined_time(&w) {
        return Some((vec![Token::Word(time), Token::Word(m.into())], i + 1));
    }
    if numeric(&w) || w.contains(':') && w.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        let next = word(s, i + 1);
        return match next.as_deref() {
            Some(n) if meridiem(n).is_some() => {
                let m = meridiem(n)?;
                Some((vec![Token::Word(w), Token::Word(m.into())], i + 2))
            }
            Some("percent") => Some((vec![Token::Word(format!("{w}%"))], i + 2)),
            Some("dollars" | "dollar") => {
                // "40000 dollars" to "$40,000"; an amount already written stays.
                let amount = w.parse::<u64>().map_or_else(|_| w.clone(), render);
                Some((words_of(format!("${amount}")), i + 2))
            }
            _ => None,
        };
    }

    // "November fifteenth", "June 5", with a year after it.
    if is_month_token(s.get(i)) {
        return month_date(s, i);
    }

    // "twenty first", or an ordinal before "of March". "Second" is also a
    // unit: "a sixty second timer" is 60 seconds long, not the 62nd.
    let tens_then_ordinal = tens(&w).zip(word(s, i + 1).and_then(|n| ordinal(&n)).filter(|v| *v < 10));
    if let Some((t, o)) = tens_then_ordinal {
        let next = word(s, i + 2);
        let is_date = next.as_deref().is_some_and(|n| ORDINAL_NOUNS.contains(&n)) || is_month_token(s.get(i + 2));
        if o == 2 && !is_date {
            let unit = if next.is_some() { format!("{t}-second") } else { format!("{t} second") };
            return Some((words_of(unit), i + 2));
        }
        return Some((vec![Token::Word(with_suffix(t + o))], i + 2));
    }
    if let Some(o) = ordinal(&w) {
        if word(s, i + 1).as_deref() == Some("of") && is_month_token(s.get(i + 2)) {
            return Some((vec![Token::Word(with_suffix(o))], i + 1));
        }
        return None;
    }

    if let Some((y, end)) = year(s, i) {
        return Some((vec![Token::Word(y.to_string())], end));
    }

    let a_prefix = w == "a";
    let (value, end) = cardinal(s, i)?;

    // Decimals are always digits, with any scale word after them.
    if !a_prefix {
        if let Some((frac, after)) = decimal(s, end) {
            let number = format!("{}.{frac}", render(value));
            return Some(finish_unit(s, number, after));
        }
    }

    let next = word(s, end);

    // Times of day. "five thirty pm", "ten am", and "at five thirty".
    if (1..=12).contains(&value) && !a_prefix {
        if let Some(m) = next.as_deref().and_then(meridiem) {
            return Some((vec![Token::Word(value.to_string()), Token::Word(m.into())], end + 1));
        }
        if let Some((minutes, after)) = minutes_after(s, end) {
            if let Some(m) = word(s, after).as_deref().and_then(meridiem) {
                return Some((
                    vec![Token::Word(format!("{value}:{minutes:02}")), Token::Word(m.into())],
                    after + 1,
                ));
            }
            let lead = i.checked_sub(1).and_then(|p| word(s, p));
            if minutes >= 10 && lead.is_some_and(|l| TIME_LEADS.contains(&l.as_str())) {
                return Some((vec![Token::Word(format!("{value}:{minutes:02}"))], after));
            }
        }
    }

    // A day before its month: "fifteen November".
    if (1..=31).contains(&value) && !a_prefix && is_month_token(s.get(end)) {
        return Some((vec![Token::Word(value.to_string())], end));
    }

    match next.as_deref() {
        Some("percent") => return Some((vec![Token::Word(format!("{}%", render(value)))], end + 1)),
        Some("dollars" | "dollar") => return Some((words_of(format!("${}", render(value))), end + 1)),
        Some("euros" | "euro") => return Some((words_of(format!("\u{20AC}{}", render(value))), end + 1)),
        Some(unit) if MEASURES.contains(&unit) => {
            return Some((words_of(render(value)), end));
        }
        _ => {}
    }

    // "Twenty five people" at a sentence start takes its hyphen.
    if sentence_start && end == i + 2 && (21..100).contains(&value) && value % 10 != 0 {
        let parts: Option<Vec<&str>> = s[i..end].iter().map(Token::word).collect();
        return parts.map(|p| (vec![Token::Word(p.join("-"))], end));
    }
    // "a hundred ideas", "a thousand times" stay as said.
    if a_prefix || value < 10 || sentence_start {
        return None;
    }
    // "five o'clock" and the like are below 10 already, but not "ten o'clock".
    if next.as_deref() == Some("o'clock") {
        return None;
    }
    Some((words_of(render(value)), end))
}

fn minutes_after(s: &[Token], end: usize) -> Option<(u64, usize)> {
    if word(s, end).as_deref() == Some("oh") {
        let v = word(s, end + 1).and_then(|w| small(&w)).filter(|v| (1..10).contains(v))?;
        return Some((v, end + 2));
    }
    let (m, after) = cardinal(s, end)?;
    (m < 60).then_some((m, after))
}

fn finish_unit(s: &[Token], number: String, after: usize) -> (Vec<Token>, usize) {
    match word(s, after).as_deref() {
        Some("percent") => (vec![Token::Word(format!("{number}%"))], after + 1),
        Some("dollars" | "dollar") => (vec![Token::Word(format!("${number}"))], after + 1),
        _ => (vec![Token::Word(number)], after),
    }
}

fn month_date(s: &[Token], i: usize) -> Option<(Vec<Token>, usize)> {
    let month = s.get(i)?.clone();
    let next = word(s, i + 1)?;
    let (day, mut end) = if let Some(t) = tens(&next) {
        match word(s, i + 2).and_then(|w| ordinal(&w).or_else(|| small(&w))).filter(|v| (1..10).contains(v)) {
            Some(o) => (t + o, i + 3),
            None => (t, i + 2),
        }
    } else if let Some(o) = ordinal(&next) {
        (o, i + 2)
    } else {
        (small(&next)?, i + 2)
    };
    if !(1..=31).contains(&day) {
        return None;
    }
    let mut out = vec![month, Token::Word(day.to_string())];
    if let Some((y, after)) = year(s, end) {
        out.push(Token::Close(",".into()));
        out.push(Token::Word(y.to_string()));
        end = after;
    }
    Some((out, end))
}

/// Splits "twenty-five" and "twenty-first", so the parser sees plain words.
fn split_hyphens(tokens: Vec<Token>) -> Vec<Token> {
    let mut out = Vec::with_capacity(tokens.len());
    for t in tokens {
        let parts: Option<Vec<String>> = t.word().filter(|w| w.contains('-')).map(|w| {
            w.split('-').map(str::to_string).collect()
        });
        match parts {
            Some(parts)
                if parts.len() == 2 && {
                    let (a, b) = (parts[0].to_lowercase(), parts[1].to_lowercase());
                    tens(&a).is_some() && (small(&b).is_some() || ordinal(&b).is_some())
                } =>
            {
                out.extend(parts.into_iter().map(Token::Word));
            }
            _ => out.push(t),
        }
    }
    out
}

pub fn normalise(tokens: Vec<Token>) -> Vec<Token> {
    let tokens = split_hyphens(tokens);
    let mut out = Vec::with_capacity(tokens.len());
    for span in sentence_spans(&tokens) {
        let s = &tokens[span];
        if is_urdu(s) {
            out.extend_from_slice(s);
            continue;
        }
        let mut i = 0;
        while i < s.len() {
            // Opening a sentence, or a list item.
            let start = s[..i].iter().all(|t| !matches!(t, Token::Word(_)));
            match at(s, i, start) {
                Some((new, end)) => {
                    out.extend(new);
                    i = end;
                }
                None => {
                    out.push(s[i].clone());
                    i += 1;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::format::{render as lay_out, tidy};
    use crate::text::tokenize;

    fn run(s: &str) -> String {
        lay_out(&tidy(normalise(tokenize(s))))
    }

    #[test]
    fn digits_from_ten_up_and_words_below() {
        assert_eq!(run("I think it's forty five."), "I think it's 45.");
        assert_eq!(run("we had ten people and three dogs"), "We had 10 people and three dogs");
        assert_eq!(run("about two hundred and fifty people"), "About 250 people");
        assert_eq!(run("there were one thousand two hundred seats"), "There were 1,200 seats");
        assert_eq!(run("it cost five million overall"), "It cost 5 million overall");
        assert_eq!(run("it cost twenty million overall"), "It cost 20 million overall");
    }

    #[test]
    fn words_stay_at_a_sentence_start() {
        assert_eq!(run("Twenty people came."), "Twenty people came.");
        assert_eq!(run("Done. Twenty people came."), "Done. Twenty people came.");
        assert_eq!(run("twenty five people came"), "Twenty-five people came");
    }

    #[test]
    fn money_percent_and_measures_are_always_digits() {
        assert_eq!(run("that is five dollars"), "That is $5");
        assert_eq!(run("Fifty dollars is fine"), "$50 is fine");
        assert_eq!(run("about twenty percent"), "About 20%");
        assert_eq!(run("about 20 percent and 50 dollars"), "About 20% and $50");
        assert_eq!(run("we spent 40000 dollars"), "We spent $40,000");
        assert_eq!(run("it needs eight gb"), "It needs 8 gb");
        assert_eq!(run("two point five percent of three point one four"), "2.5% of 3.14");
    }

    #[test]
    fn times_of_day() {
        assert_eq!(run("meet at five pm"), "Meet at 5 PM");
        assert_eq!(run("meet at five thirty pm"), "Meet at 5:30 PM");
        assert_eq!(run("meet at 3pm or 3:30 p.m. today"), "Meet at 3 PM or 3:30 PM today");
        assert_eq!(run("meet at ten thirty"), "Meet at 10:30");
        assert_eq!(run("call at seven oh five am"), "Call at 7:05 AM");
    }

    #[test]
    fn dates_and_years() {
        assert_eq!(run("it ships on November fifteenth"), "It ships on November 15");
        assert_eq!(run("it ships on March third twenty twenty seven"), "It ships on March 3, 2027");
        assert_eq!(run("back in nineteen ninety nine"), "Back in 1999");
        assert_eq!(run("the third of March"), "The 3rd of March");
        assert_eq!(run("the twenty first century"), "The 21st century");
        assert_eq!(run("the twenty second of March"), "The 22nd of March");
        assert_eq!(run("this should be like a sixty second timer"), "This should be like a 60-second timer");
        assert_eq!(run("on fifteen November"), "On 15 November");
    }

    #[test]
    fn arithmetic() {
        assert_eq!(
            run("What is five plus five plus ten minus twenty minus thirty?"),
            "What is 5 + 5 + 10 - 20 - 30?"
        );
    }

    #[test]
    fn ordinary_words_stay() {
        for s in [
            "One of them came.",
            "No one else was there.",
            "I need a hundred ideas.",
            "Thousands of people came.",
            "The second time we met.",
            "Give me one second.",
            "At five o'clock we left.",
            "This one is better.",
            "Add one more plus sign.",
        ] {
            assert_eq!(run(s), s, "{s}");
        }
    }

    #[test]
    fn roman_urdu_is_left_alone() {
        let s = "Yaar twenty minute mein aa raha hoon, theek hai";
        assert_eq!(run(s), s);
    }
}
