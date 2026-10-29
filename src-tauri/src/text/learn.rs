//! Learning dictionary words from the user's own corrections. After a paste
//! the field is watched for a while (inject/field.rs); when the user fixes a
//! word Murmur got wrong, the fix becomes a dictionary entry so it comes out
//! right next time.
//!
//! Only fixes that look like vocabulary are learned: names and terms with
//! capitals or digits ("clari tree" to "ClariTree", "murmur" to "Murmur"),
//! and words joined or split ("e mail" to "email"), and only when the fix
//! sounds like what was heard. A plain word swapped for another ("by" for
//! "buy") is not learned: that depends on the sentence, and a dictionary
//! entry would apply everywhere. Edits that rewrite the text are ignored.

use super::dictionary::DictEntry;
use super::fixup::sounds_alike;
use super::stopwords;

/// Longest learned phrase, in words.
const MAX_PHRASE: usize = 3;
/// More separate edits than this is a rewrite, not a correction.
const MAX_EDITS: usize = 3;

#[derive(Debug, Clone)]
struct Word {
    text: String,
    key: String,
    /// First word of the text or of a sentence.
    starts_sentence: bool,
}

fn split(text: &str) -> Vec<Word> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut sentence = true;
    let mut pending_sentence = true;
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_alphanumeric() || (c == '\'' && !current.is_empty()) {
            if current.is_empty() {
                sentence = pending_sentence;
                pending_sentence = false;
            }
            current.push(c);
            continue;
        }
        if !current.is_empty() {
            let text = current.trim_end_matches('\'').to_string();
            out.push(Word {
                key: text.to_lowercase(),
                text,
                starts_sentence: sentence,
            });
            current.clear();
        }
        if matches!(c, '.' | '!' | '?' | '\n' | '\r' | '•') {
            pending_sentence = true;
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Same,
    Changed,
    Removed,
    Added,
}

/// Aligns all of `pasted` against the best matching stretch of `field`.
/// Returns (op, pasted index, field index) steps in order.
fn align(pasted: &[Word], field: &[Word]) -> (usize, Vec<(Op, usize, usize)>) {
    let (m, n) = (pasted.len(), field.len());
    // cost[i][j]: pasted[..i] against a stretch of field ending at j.
    let mut cost = vec![vec![0usize; n + 1]; m + 1];
    for (i, row) in cost.iter_mut().enumerate() {
        row[0] = i;
    }
    for i in 1..=m {
        for j in 1..=n {
            let same = pasted[i - 1].key == field[j - 1].key;
            cost[i][j] = (cost[i - 1][j - 1] + usize::from(!same))
                .min(cost[i - 1][j] + 1)
                .min(cost[i][j - 1] + 1);
        }
    }
    let end = (0..=n).min_by_key(|&j| (cost[m][j], std::cmp::Reverse(j))).unwrap_or(0);
    let mut steps = Vec::new();
    let (mut i, mut j) = (m, end);
    while i > 0 {
        let same = j > 0 && pasted[i - 1].key == field[j - 1].key;
        if j > 0 && cost[i][j] == cost[i - 1][j - 1] + usize::from(!same) {
            steps.push((if same { Op::Same } else { Op::Changed }, i - 1, j - 1));
            i -= 1;
            j -= 1;
        } else if cost[i][j] == cost[i - 1][j] + 1 {
            steps.push((Op::Removed, i - 1, j));
            i -= 1;
        } else {
            steps.push((Op::Added, i, j - 1));
            j -= 1;
        }
    }
    steps.reverse();
    (cost[m][end], steps)
}

/// Joins words as written: one space between them.
fn join(words: &[&Word]) -> String {
    words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
}

fn looks_like_a_name(written: &[&Word]) -> bool {
    written.iter().enumerate().any(|(n, w)| {
        let caps_after_first = w.text.chars().skip(1).any(char::is_uppercase);
        let leading_cap = w.text.chars().next().is_some_and(char::is_uppercase);
        caps_after_first
            || w.text.chars().any(|c| c.is_ascii_digit())
            || (leading_cap && !(n == 0 && w.starts_sentence))
    })
}

/// The corrections the user made to `pasted`, judging by `field`: the text
/// around the cursor some time after the paste. None if the pasted text is
/// no longer there (deleted, sent, or rewritten beyond recognition).
pub fn corrections(pasted: &str, field: &str) -> Option<Vec<DictEntry>> {
    let pasted = split(pasted);
    let field = split(field);
    if pasted.len() < 2 || field.is_empty() {
        return None;
    }
    let (cost, steps) = align(&pasted, &field);
    let same = steps.iter().filter(|s| s.0 == Op::Same).count();
    // Most of the pasted text must still be there, or it is someone else's
    // text, or the user rewrote it.
    if same < 2 || same * 5 < pasted.len() * 3 {
        return None;
    }
    if cost == 0 && !has_case_change(&steps, &pasted, &field) {
        return Some(Vec::new());
    }

    // Runs of edits between unchanged words, and single words whose only
    // change is case.
    let mut edits: Vec<(Vec<&Word>, Vec<&Word>)> = Vec::new();
    let mut run: (Vec<&Word>, Vec<&Word>) = (Vec::new(), Vec::new());
    for &(op, p, f) in &steps {
        match op {
            Op::Same => {
                if !run.0.is_empty() || !run.1.is_empty() {
                    edits.push(std::mem::take(&mut run));
                }
                if pasted[p].text != field[f].text {
                    edits.push((vec![&pasted[p]], vec![&field[f]]));
                }
            }
            Op::Changed => {
                run.0.push(&pasted[p]);
                run.1.push(&field[f]);
            }
            Op::Removed => run.0.push(&pasted[p]),
            Op::Added => run.1.push(&field[f]),
        }
    }
    if !run.0.is_empty() || !run.1.is_empty() {
        edits.push(run);
    }
    if edits.len() > MAX_EDITS {
        return Some(Vec::new());
    }

    let mut learned: Vec<DictEntry> = Vec::new();
    for (heard, written) in edits {
        if heard.is_empty() || written.is_empty() || heard.len() > MAX_PHRASE || written.len() > MAX_PHRASE {
            continue;
        }
        let spoken = join(&heard).to_lowercase();
        let wrote = join(&written);
        let joined_or_split = heard.len() != written.len();
        if spoken == wrote.to_lowercase() && !looks_like_a_name(&written) {
            continue;
        }
        if !(looks_like_a_name(&written) || joined_or_split) {
            continue;
        }
        if heard.iter().all(|w| stopwords::is_stopword(&w.key)) {
            continue;
        }
        if !sounds_alike(&spoken, &wrote) {
            continue;
        }
        if learned.iter().any(|e| e.spoken == spoken) {
            continue;
        }
        learned.push(DictEntry {
            spoken,
            written: wrote,
            learned: true,
        });
    }
    Some(learned)
}

fn has_case_change(steps: &[(Op, usize, usize)], pasted: &[Word], field: &[Word]) -> bool {
    steps
        .iter()
        .any(|&(op, p, f)| op == Op::Same && pasted[p].text != field[f].text)
}

/// Adds learned entries to `dictionary`, skipping any phrase it already
/// has. Returns the ones added.
pub fn merge(dictionary: &mut Vec<DictEntry>, learned: Vec<DictEntry>) -> Vec<DictEntry> {
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let mut added = Vec::new();
    for entry in learned {
        if dictionary.iter().any(|e| norm(&e.spoken) == norm(&entry.spoken)) {
            continue;
        }
        dictionary.push(entry.clone());
        added.push(entry);
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    fn learned(pasted: &str, field: &str) -> Vec<(String, String)> {
        corrections(pasted, field)
            .unwrap_or_default()
            .into_iter()
            .map(|e| (e.spoken, e.written))
            .collect()
    }

    fn pair(s: &str, w: &str) -> Vec<(String, String)> {
        vec![(s.to_string(), w.to_string())]
    }

    #[test]
    fn learns_a_name_the_user_fixed() {
        assert_eq!(
            learned(
                "I work at Clari Tree and we ship on Fridays.",
                "Hi all,\nI work at ClariTree and we ship on Fridays. More later"
            ),
            pair("clari tree", "ClariTree")
        );
        assert_eq!(
            learned("Ping Ayesha about the murmur build tonight.", "Ping Ayesha about the Murmur build tonight."),
            pair("murmur", "Murmur")
        );
        assert_eq!(
            learned("Send it to mars by Friday please.", "Send it to Maaz by Friday please."),
            pair("mars", "Maaz")
        );
    }

    #[test]
    fn learns_joined_words() {
        assert_eq!(
            learned("Check your e mail before the call.", "Check your email before the call."),
            pair("e mail", "email")
        );
    }

    #[test]
    fn ignores_plain_word_swaps_and_style_edits() {
        // Homophones depend on the sentence; a dictionary entry would apply
        // everywhere.
        assert!(learned("I need to by some milk today.", "I need to buy some milk today.").is_empty());
        assert!(learned("Make the board bigger please.", "Make the board larger please.").is_empty());
        // Capitalising the first word of a sentence is not a name.
        assert!(learned("okay. sounds good to me.", "okay. Sounds good to me.").is_empty());
        // Deleting or adding words teaches nothing.
        assert!(learned("I think we should go now.", "We should go now.").is_empty());
        assert!(learned("Thanks for the help.", "Thanks so much for the help.").is_empty());
    }

    #[test]
    fn ignores_rewrites_and_unrelated_text() {
        assert!(learned(
            "can you send me the deck before lunch",
            "Please share the slides with me by noon."
        )
        .is_empty());
        assert!(learned("the quick brown fox", "").is_empty());
        assert_eq!(corrections("the quick brown fox", "something else entirely"), None);
        assert_eq!(corrections("the quick brown fox", "the quick brown fox"), Some(Vec::new()));
        assert!(learned("hello", "Hello").is_empty());
        // Unchanged text teaches nothing.
        assert!(learned("Meet at Gate 4 at noon.", "Meet at Gate 4 at noon.").is_empty());
    }

    #[test]
    fn finds_the_pasted_text_inside_a_longer_field() {
        let field = "Notes from Monday. The ClariTree launch moved. Ask Ayesha about the Grafana dashboards. Lunch at one.";
        assert_eq!(
            learned("Ask Ayesha about the graph fauna dashboards.", field),
            pair("graph fauna", "Grafana")
        );
    }

    #[test]
    fn merge_keeps_the_users_own_entries() {
        let mut dict = vec![DictEntry {
            spoken: "clari tree".into(),
            written: "Claritree".into(),
            learned: false,
        }];
        let added = merge(
            &mut dict,
            vec![
                DictEntry {
                    spoken: "Clari  Tree".into(),
                    written: "ClariTree".into(),
                    learned: true,
                },
                DictEntry {
                    spoken: "mars".into(),
                    written: "Maaz".into(),
                    learned: true,
                },
            ],
        );
        assert_eq!(added.len(), 1);
        assert_eq!(dict.len(), 2);
        assert_eq!(dict[0].written, "Claritree");
        assert!(dict[1].learned);
    }
}
