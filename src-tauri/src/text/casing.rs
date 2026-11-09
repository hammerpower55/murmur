//! How words are written: the pronoun I, days and months, brands and
//! acronyms, letters spelled out one by one, and contractions the engine lost
//! the apostrophe from. Runs before the dictionary, so a user's own spelling
//! of a word always wins over the table here.

use super::smart::AppKind;
use super::{is_urdu, sentence_spans, Token};

const DAYS_AND_MONTHS: &[(&str, &str)] = &[
    ("monday", "Monday"),
    ("tuesday", "Tuesday"),
    ("wednesday", "Wednesday"),
    ("thursday", "Thursday"),
    ("friday", "Friday"),
    ("saturday", "Saturday"),
    ("sunday", "Sunday"),
    ("january", "January"),
    ("february", "February"),
    ("april", "April"),
    ("june", "June"),
    ("july", "July"),
    ("august", "August"),
    ("september", "September"),
    ("october", "October"),
    ("november", "November"),
    ("december", "December"),
];

const NAMES: &[(&str, &str)] = &[
    ("iphone", "iPhone"),
    ("iphones", "iPhones"),
    ("ipad", "iPad"),
    ("ipads", "iPads"),
    ("ipod", "iPod"),
    ("imac", "iMac"),
    ("macos", "macOS"),
    ("macbook", "MacBook"),
    ("ios", "iOS"),
    ("icloud", "iCloud"),
    ("itunes", "iTunes"),
    ("airpods", "AirPods"),
    ("github", "GitHub"),
    ("gitlab", "GitLab"),
    ("youtube", "YouTube"),
    ("linkedin", "LinkedIn"),
    ("whatsapp", "WhatsApp"),
    ("tiktok", "TikTok"),
    ("paypal", "PayPal"),
    ("chatgpt", "ChatGPT"),
    ("openai", "OpenAI"),
    ("javascript", "JavaScript"),
    ("typescript", "TypeScript"),
    ("wifi", "Wi-Fi"),
    ("wi-fi", "Wi-Fi"),
    ("powerpoint", "PowerPoint"),
    ("onedrive", "OneDrive"),
    ("sharepoint", "SharePoint"),
    ("postgresql", "PostgreSQL"),
    ("mysql", "MySQL"),
    ("mongodb", "MongoDB"),
    ("nodejs", "Node.js"),
    ("hubspot", "HubSpot"),
    ("salesforce", "Salesforce"),
    ("google", "Google"),
    ("microsoft", "Microsoft"),
    ("windows", "Windows"),
    ("english", "English"),
    ("urdu", "Urdu"),
    ("hindi", "Hindi"),
    ("arabic", "Arabic"),
    ("pakistan", "Pakistan"),
    ("pakistani", "Pakistani"),
    ("karachi", "Karachi"),
    ("lahore", "Lahore"),
    ("islamabad", "Islamabad"),
    ("london", "London"),
];

/// Written in capitals. Words that are also English or Roman Urdu words
/// ("it", "us", "ram", "hr" is fine) stay out of this list.
const ACRONYMS: &[&str] = &[
    "ai", "api", "apis", "url", "urls", "ui", "ux", "css", "html", "json", "sql", "pdf", "pdfs", "usb", "cpu",
    "gpu", "npu", "ssd", "ceo", "cto", "cfo", "coo", "hr", "faq", "faqs", "eta", "usa", "llm", "llms", "sdk",
    "ide", "erp", "crm", "aws", "gcp", "vpn", "dns", "http", "https", "seo", "kpi", "kpis", "roi", "mvp", "qa",
    "nasa", "saas", "b2b", "b2c", "gb", "mb", "kb", "tb",
];

/// Lost apostrophes. "cant", "wont" and "hes" are rare enough as words;
/// "lets", "id", "were", "well" and "shed" are common words and stay out.
const CONTRACTIONS: &[(&str, &str)] = &[
    ("dont", "don't"),
    ("doesnt", "doesn't"),
    ("didnt", "didn't"),
    ("cant", "can't"),
    ("wont", "won't"),
    ("isnt", "isn't"),
    ("arent", "aren't"),
    ("wasnt", "wasn't"),
    ("werent", "weren't"),
    ("hasnt", "hasn't"),
    ("havent", "haven't"),
    ("hadnt", "hadn't"),
    ("wouldnt", "wouldn't"),
    ("couldnt", "couldn't"),
    ("shouldnt", "shouldn't"),
    ("thats", "that's"),
    ("whats", "what's"),
    ("theres", "there's"),
    ("heres", "here's"),
    ("youre", "you're"),
    ("theyre", "they're"),
    ("youve", "you've"),
    ("theyve", "they've"),
    ("weve", "we've"),
    ("youll", "you'll"),
    ("theyll", "they'll"),
    ("hes", "he's"),
    ("shes", "she's"),
    ("im", "I'm"),
    ("ive", "I've"),
];

/// Dropped g's that only ever come from the engine writing what it heard.
const DROPPED_G: &[(&str, &str)] = &[
    ("goin", "going"),
    ("doin", "doing"),
    ("nothin", "nothing"),
    ("somethin", "something"),
    ("gettin", "getting"),
    ("makin", "making"),
    ("talkin", "talking"),
    ("lookin", "looking"),
    ("comin", "coming"),
    ("tryin", "trying"),
    ("sayin", "saying"),
    ("thinkin", "thinking"),
    ("workin", "working"),
];

/// Speech spellings written out in email, where the tone is formal. Chat and
/// everything else keep the speaker's voice.
const FORMAL: &[(&str, &str)] = &[
    ("gonna", "going to"),
    ("wanna", "want to"),
    ("gotta", "have to"),
    ("kinda", "kind of"),
    ("sorta", "sort of"),
    ("dunno", "don't know"),
    ("lemme", "let me"),
    ("gimme", "give me"),
    ("cuz", "because"),
    ("cos", "because"),
];

fn lookup<'a>(table: &'a [(&str, &'a str)], key: &str) -> Option<&'a str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// All lowercase, or capitalised only because a sentence starts here. A word
/// with its own capitals ("GITHUB", "AiR") was written that way on purpose.
fn plain_case(word: &str) -> bool {
    word.chars().skip(1).all(|c| !c.is_uppercase())
}

fn follow_case(original: &str, replacement: &str) -> String {
    if original.chars().next().is_some_and(char::is_uppercase) {
        super::format::capitalise_word(replacement)
    } else {
        replacement.to_string()
    }
}

/// The pronoun: "i", "i'm", "i'll", "i've", "i'd", with either apostrophe.
fn pronoun(lower: &str) -> Option<String> {
    match lower.replace('\u{2019}', "'").as_str() {
        "i" => Some("I".into()),
        "i'm" | "i'll" | "i've" | "i'd" => Some(format!("I{}", &lower[1..])),
        _ => None,
    }
}

/// "E-R-P" to "ERP". Three or more letters, so "X-Y" and "T-shirt" stay.
fn spelled_with_hyphens(word: &str) -> Option<String> {
    let parts: Vec<&str> = word.split('-').collect();
    (parts.len() >= 3 && parts.iter().all(|p| p.len() == 1 && p.chars().all(|c| c.is_ascii_alphabetic())))
        .then(|| parts.concat().to_uppercase())
}

/// "May" and "March" only when they are clearly the month.
fn is_ambiguous_month(tokens: &[Token], i: usize) -> bool {
    let number = |t: Option<&Token>| {
        t.and_then(Token::word)
            .is_some_and(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()) || super::numbers::is_number_word(w))
    };
    let lead = i.checked_sub(1).and_then(|p| tokens[p].word()).map(str::to_lowercase);
    number(tokens.get(i + 1))
        || number(i.checked_sub(1).and_then(|p| tokens.get(p)))
        || lead.is_some_and(|w| matches!(w.as_str(), "in" | "of" | "by" | "since" | "until" | "during" | "last" | "next" | "this" | "early" | "late"))
}

/// Said before a name: "dear mr ahmed" to "Dear Mr. Ahmed". A title already
/// capitalised keeps the speaker's or engine's style ("Ms Khan").
fn titles(mut tokens: Vec<Token>) -> Vec<Token> {
    const TITLES: &[(&str, &str)] = &[("mr", "Mr."), ("mrs", "Mrs."), ("ms", "Ms."), ("dr", "Dr."), ("prof", "Prof.")];
    for i in 0..tokens.len().saturating_sub(1) {
        let Some(title) = tokens[i].word().and_then(|w| lookup(TITLES, w)) else {
            continue;
        };
        let after_number = i > 0 && tokens[i - 1].word().is_some_and(|w| w.starts_with(|c: char| c.is_ascii_digit()));
        if after_number {
            continue;
        }
        if let Token::Word(name) = &tokens[i + 1] {
            let name = super::format::capitalise_word(name);
            tokens[i] = Token::Word(title.to_string());
            tokens[i + 1] = Token::Word(name);
        }
    }
    tokens
}

/// Joins runs of three or more single capital letters: "A P I" to "API".
fn join_spelled(tokens: Vec<Token>) -> Vec<Token> {
    let single = |t: &Token| t.word().is_some_and(|w| w.len() == 1 && w.chars().all(|c| c.is_ascii_uppercase()));
    let mut out = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let run = tokens[i..].iter().take_while(|t| single(t)).count();
        if run >= 3 {
            let joined: String = tokens[i..i + run].iter().filter_map(Token::word).collect();
            out.push(Token::Word(joined));
            i += run;
        } else {
            out.push(tokens[i].clone());
            i += 1;
        }
    }
    out
}

pub fn apply(tokens: Vec<Token>, app: AppKind) -> Vec<Token> {
    let tokens = titles(join_spelled(tokens));
    let mut out = Vec::with_capacity(tokens.len());
    for span in sentence_spans(&tokens) {
        let urdu = is_urdu(&tokens[span.clone()]);
        for i in span {
            let t = &tokens[i];
            let Some(word) = t.word() else {
                out.push(t.clone());
                continue;
            };
            let lower = word.to_lowercase();
            out.extend(rewrite(&tokens, i, word, &lower, urdu, app));
        }
    }
    out
}

fn rewrite(tokens: &[Token], i: usize, word: &str, lower: &str, urdu: bool, app: AppKind) -> Vec<Token> {
    let one = |s: String| vec![Token::Word(s)];
    if let Some(p) = pronoun(lower) {
        return one(p);
    }
    if let Some(s) = spelled_with_hyphens(word) {
        return one(s);
    }
    if let Some(day) = lookup(DAYS_AND_MONTHS, lower) {
        return one(day.to_string());
    }
    if (lower == "may" || lower == "march") && word.starts_with(char::is_lowercase) && is_ambiguous_month(tokens, i) {
        return one(super::format::capitalise_word(lower));
    }
    if urdu {
        return one(word.to_string());
    }
    if plain_case(word) {
        // "valid id", "photo id": "id" alone is a common word in other senses.
        let id_lead = i
            .checked_sub(1)
            .and_then(|p| tokens[p].word())
            .is_some_and(|w| matches!(w.to_lowercase().as_str(), "valid" | "photo" | "user" | "your" | "my" | "the"));
        if lower == "id" && id_lead {
            return one("ID".to_string());
        }
        if let Some(name) = lookup(NAMES, lower) {
            return one(name.to_string());
        }
        if ACRONYMS.contains(&lower) {
            return one(lower.to_uppercase());
        }
        if let Some(full) = lookup(CONTRACTIONS, lower).or_else(|| lookup(DROPPED_G, lower)) {
            return one(follow_case(word, full));
        }
        if app == AppKind::Email {
            if let Some(full) = lookup(FORMAL, lower) {
                return follow_case(word, full)
                    .split(' ')
                    .map(|w| Token::Word(w.to_string()))
                    .collect();
            }
        }
    }
    one(word.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::format::{render, tidy};
    use crate::text::tokenize;

    fn run(s: &str) -> String {
        render(&tidy(apply(tokenize(s), AppKind::Other)))
    }

    #[test]
    fn the_pronoun_is_always_a_capital() {
        assert_eq!(run("that's fine but i'm sure i can do it and i've done it"), "That's fine but I'm sure I can do it and I've done it");
        assert_eq!(run("what i\u{2019}ll do"), "What I\u{2019}ll do");
    }

    #[test]
    fn days_months_and_names() {
        assert_eq!(run("see you on monday in january"), "See you on Monday in January");
        assert_eq!(run("open github on my iphone and check youtube"), "Open GitHub on my iPhone and check YouTube");
    }

    #[test]
    fn may_and_march_only_as_months() {
        assert_eq!(run("we may march on"), "We may march on");
        assert_eq!(run("it ships in may and ends on march 5"), "It ships in May and ends on March 5");
    }

    #[test]
    fn acronyms_and_spelled_letters() {
        assert_eq!(run("the api is down and the Url is wrong"), "The API is down and the URL is wrong");
        assert_eq!(run("the Thaiiri E-R-P needs work"), "The Thaiiri ERP needs work");
        assert_eq!(run("call the A P I now"), "Call the API now");
        assert_eq!(run("a T-shirt and X-Y axis"), "A T-shirt and X-Y axis");
    }

    #[test]
    fn lost_apostrophes_come_back() {
        assert_eq!(run("dont do it because youre late"), "Don't do it because you're late");
        assert_eq!(run("im going goin"), "I'm going going");
        assert_eq!(run("Thats it"), "That's it");
    }

    #[test]
    fn real_words_stay() {
        for s in ["It lets you see the ID.", "We were well.", "An AI model is here.", "Who shed it?"] {
            assert_eq!(run(s), s, "{s}");
        }
    }

    #[test]
    fn roman_urdu_sentences_keep_their_words() {
        let s = "Yaar ai kya kar raha hai";
        assert_eq!(run(s), s);
    }

    #[test]
    fn email_is_formal_and_other_apps_keep_the_voice() {
        assert_eq!(run("I wanna go and I'm gonna go"), "I wanna go and I'm gonna go");
        assert_eq!(
            render(&tidy(apply(tokenize("I wanna go and I'm gonna go"), AppKind::Email))),
            "I want to go and I'm going to go"
        );
    }
}
