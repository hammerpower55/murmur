//! Holds the fix-up language model (engine/llm.rs) next to the speech engine.
//! It loads on a background thread and is never waited for: a dictation that
//! arrives while it loads, or when it failed to load, is pasted as heard.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::default_threads;
use super::llm::Llm;
use crate::text::fixup::{self, Context, Swap};

/// An installed word fixer.
#[derive(Debug, Clone, PartialEq)]
pub struct FixupModel {
    pub dir: PathBuf,
    /// Its chat prompt needs an empty think block (see `CatalogModel`).
    pub thinking: bool,
}

#[derive(Default)]
struct Inner {
    /// The installed model.
    model: Option<FixupModel>,
    llm: Option<Arc<Llm>>,
    loading: bool,
    /// Bumped on every change of model, so a stale load is thrown away.
    generation: u64,
}

#[derive(Default)]
pub struct Fixer {
    inner: Mutex<Inner>,
}

impl Fixer {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The installed model, or None once it is deleted. Unloads whatever
    /// was loaded.
    pub fn set_model(&self, model: Option<FixupModel>) {
        if let Ok(mut inner) = self.inner.lock() {
            if inner.model == model {
                return;
            }
            inner.model = model;
            inner.llm = None;
            inner.loading = false;
            inner.generation += 1;
        }
    }

    pub fn has_model(&self) -> bool {
        self.inner.lock().is_ok_and(|i| i.model.is_some())
    }

    /// Whether the installed model's chat prompt needs an empty think block.
    pub fn thinking(&self) -> bool {
        self.inner
            .lock()
            .is_ok_and(|i| i.model.as_ref().is_some_and(|m| m.thinking))
    }

    pub fn is_loaded(&self) -> bool {
        self.inner.lock().is_ok_and(|i| i.llm.is_some())
    }

    /// The loaded model, loading it first if needed and waiting up to
    /// `timeout`. None if there is no model or it did not load in time.
    pub fn model(self: &Arc<Self>, timeout: std::time::Duration) -> Option<Arc<Llm>> {
        self.wake();
        let deadline = Instant::now() + timeout;
        loop {
            {
                let inner = self.inner.lock().ok()?;
                if let Some(llm) = &inner.llm {
                    return Some(llm.clone());
                }
                if !inner.loading || Instant::now() >= deadline {
                    return None;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    /// Starts loading the model if there is one and it is not loaded.
    pub fn wake(self: &Arc<Self>) {
        let (dir, generation) = {
            let Ok(mut inner) = self.inner.lock() else {
                return;
            };
            let Some(dir) = inner.model.as_ref().map(|m| m.dir.clone()) else {
                return;
            };
            if inner.llm.is_some() || inner.loading {
                return;
            }
            inner.loading = true;
            (dir, inner.generation)
        };
        let me = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("murmur-fixup-load".into())
            .spawn(move || {
                let started = Instant::now();
                let result = Llm::load(&dir, default_threads());
                let Ok(mut inner) = me.inner.lock() else {
                    return;
                };
                if inner.generation != generation {
                    return;
                }
                inner.loading = false;
                match result {
                    Ok(llm) => {
                        log::info!("fix-up model ready in {} ms", started.elapsed().as_millis());
                        inner.llm = Some(Arc::new(llm));
                    }
                    Err(err) => log::error!("could not load the fix-up model: {err:#}"),
                }
            });
        if spawned.is_err() {
            if let Ok(mut inner) = self.inner.lock() {
                inner.loading = false;
            }
        }
    }

    /// Frees the model's memory; the next wake loads it again. Anything
    /// still using it (a chat reply) keeps its own handle until done.
    pub fn park(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            if inner.llm.take().is_some() {
                log::info!("unloaded the fix-up model after being idle");
            }
        }
    }

    /// The transcript with misheard words fixed and what was swapped, or
    /// None to keep it as heard.
    pub fn fix(&self, transcript: &str, ctx: &Context) -> Option<(String, Vec<Swap>)> {
        // Most dictations have nothing that sounds like the user's words:
        // the model is not needed then, unless every word is checked.
        if !ctx.any_word && fixup::candidates(transcript, ctx).is_empty() {
            return None;
        }
        let llm = self.inner.lock().ok()?.llm.clone();
        let Some(llm) = llm else {
            log::info!("fix-up model not loaded yet, pasting as heard");
            return None;
        };
        let started = Instant::now();
        match rescore(&llm, transcript, ctx) {
            Ok((found, notes)) => {
                log::info!(
                    "fix-up in {} ms, {} applied: {}",
                    started.elapsed().as_millis(),
                    found.len(),
                    notes.join(" | ")
                );
                if found.is_empty() {
                    return None;
                }
                let swaps = found
                    .iter()
                    .map(|r| Swap {
                        heard: transcript[r.start..r.end].to_string(),
                        meant: r.meant.clone(),
                    })
                    .collect();
                let edits: Vec<_> = found.into_iter().map(|r| (r.start, r.end, r.meant)).collect();
                Some((fixup::apply_at(transcript, &edits), swaps))
            }
            Err(err) => {
                log::error!("fix-up failed: {err:#}");
                None
            }
        }
    }
}

/// Tokens after a word that judge an alternative.
const SUFFIX_TOKENS: usize = 16;
/// A word from the context must make the text this much more likely
/// (natural log; 3 is about 20 times) to replace what was heard.
const MIN_GAIN: f32 = 3.0;
/// The same for a word from the user's dictionary.
const MIN_GAIN_DICTIONARY: f32 = 1.5;
/// "Fix any misheard word": a word the model proposes itself must make the
/// text this much more likely (e^8, about 3000 times). Lower let "weather"
/// become "water" (+6.5) and "place" become "face" (+7.1).
const MIN_GAIN_OPEN: f32 = 8.0;
/// Words less likely than this in context (natural log probability of the
/// whole word) are the ones checked in that mode.
const SUSPECT_BELOW: f32 = -5.0;
/// Most words checked, and alternatives tried for each, in that mode.
const MAX_SUSPECTS: usize = 4;
const MAX_OPEN_CANDIDATES: usize = 4;
/// Likely tokens looked through for alternatives that sound alike.
const TOP_K: usize = 2000;

/// A word the model may be asked about: plain letters, not a stopword, not
/// a name the recogniser capitalised mid sentence, not a dictionary word.
fn open_checkable(transcript: &str, start: usize, end: usize, known: &[String]) -> bool {
    let word = &transcript[start..end];
    let before = transcript[..start].trim_end();
    let sentence_start = before.is_empty() || before.ends_with(['.', '!', '?', '\n']);
    word.chars().count() >= 2
        && word.chars().all(|c| c.is_alphabetic() || c == '\'')
        && !crate::text::stopwords::is_stopword(word)
        && (sentence_start || !word.chars().next().is_some_and(char::is_uppercase))
        && !known.contains(&word.to_lowercase())
}

/// A replacement to score.
struct Try {
    /// Byte range in the transcript.
    start: usize,
    end: usize,
    /// First and last token of that range.
    tokens: (usize, usize),
    meant: String,
    /// The replacement's tokens.
    alternative: Vec<u32>,
    /// Gain it must reach.
    needed: f32,
}

/// One place in the transcript where a better word was found.
#[derive(Debug, Clone)]
pub struct Rescore {
    /// Byte range in the transcript.
    pub start: usize,
    pub end: usize,
    pub meant: String,
    pub gain: f32,
}

/// Checks each run of transcript words that sounds like one of the user's
/// words (fixup::candidates) by scoring the text both ways in context, and
/// swaps it only if the user's word makes the text much more likely. Returns
/// the swaps and a note on every candidate.
fn rescore(llm: &Llm, transcript: &str, ctx: &Context) -> anyhow::Result<(Vec<Rescore>, Vec<String>)> {
    let mut notes = Vec::new();
    if crate::text::stopwords::roman_urdu_words(transcript) >= 2 {
        notes.push("Roman Urdu, left as heard".into());
        return Ok((Vec::new(), notes));
    }
    let candidates = fixup::candidates(transcript, ctx);
    if candidates.is_empty() && !ctx.any_word {
        return Ok((Vec::new(), notes));
    }
    let lead = fixup::lead(ctx);
    let text = format!("{lead}{transcript}");
    let (ids, offsets) = llm.encode_offsets(&text)?;
    // First and last token of a transcript byte range, if the range starts
    // and ends on token edges.
    let tokens_of = |start: usize, end: usize| -> Option<(usize, usize)> {
        let (ts, te) = (start + lead.len(), end + lead.len());
        let inside: Vec<usize> = (0..ids.len())
            .filter(|&i| offsets[i].1 > ts && offsets[i].0 < te)
            .collect();
        let (&t0, &t1) = (inside.first()?, inside.last()?);
        (t0 > 0 && offsets[t1].1 <= te && offsets[t0].0 + 1 >= ts).then_some((t0, t1))
    };
    // Every word: plain words the model can be asked about, with their tokens.
    let known: Vec<String> = ctx.vocabulary.iter().map(|w| w.trim().to_lowercase()).collect();
    let open_words: Vec<(usize, usize, usize, usize)> = if ctx.any_word {
        fixup::word_spans(transcript)
            .into_iter()
            .filter(|&(s, e)| open_checkable(transcript, s, e, &known))
            .filter_map(|(s, e)| tokens_of(s, e).map(|(t0, t1)| (s, e, t0, t1)))
            .collect()
    } else {
        Vec::new()
    };
    let top_at: Vec<usize> = open_words.iter().map(|w| w.2).collect();
    let scored = llm.score(&ids, &top_at, TOP_K)?;

    let mut tries: Vec<Try> = Vec::new();
    let spaced = |start: usize| text[..start + lead.len()].ends_with(' ');
    for c in candidates {
        let Some((t0, t1)) = tokens_of(c.start, c.end) else {
            continue;
        };
        let prefix = if spaced(c.start) { " " } else { "" };
        let (tokens, _) = llm.encode_offsets(&format!("{prefix}{}", c.meant))?;
        let needed = if c.from_dictionary { MIN_GAIN_DICTIONARY } else { MIN_GAIN };
        tries.push(Try {
            start: c.start,
            end: c.end,
            tokens: (t0, t1),
            meant: c.meant,
            alternative: tokens,
            needed,
        });
    }
    // The words the model finds least likely, against the words it expected
    // there that sound alike. A word that sounds like one of the user's own
    // is left to that: the model's guess ("Maria") must not beat "Maaz".
    let own: Vec<(usize, usize)> = tries.iter().map(|t| (t.start, t.end)).collect();
    let mut suspects: Vec<(usize, usize, usize, usize, f32)> = open_words
        .iter()
        .filter(|&&(s, e, _, _)| own.iter().all(|&(os, oe)| e <= os || s >= oe))
        .map(|&(s, e, t0, t1)| (s, e, t0, t1, scored.logprobs[t0..=t1].iter().sum::<f32>()))
        .filter(|w| w.4 < SUSPECT_BELOW)
        .collect();
    suspects.sort_by(|a, b| a.4.total_cmp(&b.4));
    suspects.truncate(MAX_SUSPECTS);
    for &(s, e, t0, t1, _) in &suspects {
        let heard = &transcript[s..e];
        let top = scored.top.iter().find(|(p, _)| *p == t0).map_or(&[][..], |(_, t)| t.as_slice());
        let mut found = 0;
        for &(token, _) in top {
            let piece = llm.token_text(token);
            let meant = piece.trim();
            if spaced(s) != piece.starts_with(' ')
                || meant.chars().count() < 2
                || !meant.chars().all(|c| c.is_alphabetic() || c == '\'')
                || meant.eq_ignore_ascii_case(heard)
                || !fixup::sounds_alike(heard, meant)
            {
                continue;
            }
            tries.push(Try {
                start: s,
                end: e,
                tokens: (t0, t1),
                meant: meant.to_string(),
                alternative: vec![token],
                needed: MIN_GAIN_OPEN,
            });
            found += 1;
            if found == MAX_OPEN_CANDIDATES {
                break;
            }
        }
    }

    let mut best: Vec<Rescore> = Vec::new();
    for Try { start, end, tokens: (t0, t1), meant, mut alternative, needed } in tries {
        let last = (t1 + SUFFIX_TOKENS).min(ids.len() - 1);
        let heard_score: f32 = scored.logprobs[t0..=last].iter().sum();
        alternative.extend_from_slice(&ids[t1 + 1..=last]);
        let gain = llm.continuation(&scored, t0, &alternative)? - heard_score;
        notes.push(format!("{} => {meant}: {gain:+.1}", &transcript[start..end]));
        if gain >= needed {
            best.push(Rescore { start, end, meant, gain });
        }
    }

    // Best first, no overlaps, no more than the limit.
    best.sort_by(|a, b| b.gain.total_cmp(&a.gain));
    let mut kept: Vec<Rescore> = Vec::new();
    for r in best {
        if kept.iter().all(|k| r.end <= k.start || r.start >= k.end) {
            kept.push(r);
        }
    }
    kept.truncate(fixup::fix_limit(fixup::words(transcript).len()));
    Ok((kept, notes))
}

/// Runs the fix-up model on misheard transcripts, on traps where a context
/// word sounds alike but is wrong, and on the local history, printing every
/// change. Needs the model installed:
///
///   cargo test --release fixup_eval -- --ignored --nocapture
///
/// MURMUR_FIXUP_ID picks the model folder (default qwen3-4b-instruct-int4).
#[cfg(test)]
mod eval {
    use super::*;
    use crate::engine::onnx;

    /// The app name is only for reading the cases.
    fn ctx(_app: &str, before: &str, earlier: &str) -> Context {
        Context {
            before: before.into(),
            earlier: earlier.into(),
            vocabulary: Vec::new(),
            any_word: std::env::var_os("MURMUR_FIXUP_ANY").is_some(),
        }
    }

    fn with_words(mut ctx: Context, words: &[&str]) -> Context {
        ctx.vocabulary = words.iter().map(|w| w.to_string()).collect();
        ctx
    }

    /// (context, heard, what the speaker said)
    fn cases() -> Vec<(Context, &'static str, &'static str)> {
        vec![
            (
                ctx("Claude", "Chance cards and Community Chest cards sit in the middle of the board.", ""),
                "I want dance cards to be something else and the board to be bigger.",
                "I want chance cards to be something else and the board to be bigger.",
            ),
            (
                ctx("Notepad", "Release plan: the beta goes out in two weeks.", ""),
                "we should ship the beater to testers on friday after the security review",
                "we should ship the beta to testers on friday after the security review",
            ),
            (
                with_words(ctx("Slack", "", ""), &["Kubernetes", "Grafana"]),
                "can you check the cooper netties logs before the stand up",
                "can you check the Kubernetes logs before the stand up",
            ),
            (
                ctx("Outlook", "Thanks Maaz,", ""),
                "can you send it to mars by friday",
                "can you send it to Maaz by friday",
            ),
            (
                with_words(ctx("Outlook", "", ""), &["Maaz", "ClariTree"]),
                "I spoke to mars at clari tree yesterday about the launch",
                "I spoke to Maaz at ClariTree yesterday about the launch",
            ),
            (
                ctx("WhatsApp", "We went to the water park on Sunday.", ""),
                "what is the weather like today",
                "what is the weather like today",
            ),
            (
                ctx("Notes", "Face ID stopped working on my phone.", ""),
                "I need a new place to stay next month",
                "I need a new place to stay next month",
            ),
            (
                with_words(ctx("Chrome", "", ""), &["Murmur"]),
                "the murmur of the crowd got louder and louder",
                "the murmur of the crowd got louder and louder",
            ),
            (
                ctx("Claude", "", ""),
                "Maybe I wanna change some things on the monopoly board. I want the board to be bigger, I want the properties to be more colorful, I want the dice to have their own place, I want dance cards to be something else.",
                "Maybe I wanna change some things on the monopoly board. I want the board to be bigger, I want the properties to be more colorful, I want the dice to have their own place, I want chance cards to be something else.",
            ),
            (
                ctx("Claude", "", ""),
                "Okay, I really like the Monopoly game, but make the following changes. Make the board smaller. Make the effects larger than live. Live there should be better animations. Things should be more smooth than cohesive.",
                "Okay, I really like the Monopoly game, but make the following changes. Make the board smaller. Make the effects larger than life. Also there should be better animations. Things should be more smooth and cohesive.",
            ),
            (
                ctx("Notepad", "Release plan for Murmur 0.2", ""),
                "we should ship the beater to testers on friday after the security review",
                "we should ship the beta to testers on friday after the security review",
            ),
            (
                ctx("Slack", "", "The staging cluster keeps restarting."),
                "can you check the cooper netties logs before the stand up",
                "can you check the kubernetes logs before the stand up",
            ),
            (
                ctx("Notepad", "", ""),
                "Okay, so I need to make some changes on the game. One I need the board to be recorded. I need a multiplayer option and I need uh audio audio chat in the game.",
                "Okay, so I need to make some changes on the game. One I need the board to be recorded. I need a multiplayer option and I need uh audio audio chat in the game.",
            ),
            (
                ctx("WhatsApp", "", ""),
                "acha theek hai main kal subah office jaunga aur phir meeting karenge",
                "acha theek hai main kal subah office jaunga aur phir meeting karenge",
            ),
            (
                ctx("Chrome", "", ""),
                "What is the weather? Who are you, who am I?",
                "What is the weather? Who are you, who am I?",
            ),
            (
                ctx("Claude", "", ""),
                "It's good now, but it's still not as good as Whisperflow. The transcription with Whisperflow was immaculate.",
                "It's good now, but it's still not as good as Whisperflow. The transcription with Whisperflow was immaculate.",
            ),
        ]
    }

    #[test]
    #[ignore = "needs the downloaded fix-up model"]
    fn fixup_eval() {
        let id = std::env::var("MURMUR_FIXUP_ID").unwrap_or_else(|_| "qwen3-4b-instruct-int4".into());
        onnx::use_vendored_runtime();
        let started = Instant::now();
        let llm = Llm::load(&onnx::installed_model_dir(&id), default_threads()).unwrap();
        println!("\n{id} loaded in {} ms", started.elapsed().as_millis());

        let fix = |heard: &str, ctx: &Context| -> (String, String) {
            let (found, notes) = rescore(&llm, heard, ctx).unwrap();
            let edits: Vec<_> = found.iter().map(|r| (r.start, r.end, r.meant.clone())).collect();
            (fixup::apply_at(heard, &edits), notes.join(" | "))
        };
        let (mut fixed, mut broke, mut total_ms, mut runs) = (0, 0, 0u128, 0);
        for (ctx, heard, said) in cases() {
            let t = Instant::now();
            let (out, reply) = fix(heard, &ctx);
            let ms = t.elapsed().as_millis();
            total_ms += ms;
            runs += 1;
            let errors = |s: &str| crate::engine::eval::word_errors(said, s).0;
            let verdict = match errors(&out).cmp(&errors(heard)) {
                std::cmp::Ordering::Less => {
                    fixed += 1;
                    "BETTER"
                }
                std::cmp::Ordering::Greater => {
                    broke += 1;
                    "WORSE"
                }
                std::cmp::Ordering::Equal => "same",
            };
            println!("\n{ms} ms, {verdict}\n  heard: {heard}\n  notes: {reply}");
            if out != heard {
                println!("  out:   {out}");
            }
        }
        println!("\ncases: {fixed} better, {broke} worse, {} ms average", total_ms / runs.max(1));

        // The local history, each transcript with the one before it in the
        // same app as earlier dictation. Prints only what would change.
        let db = crate::paths::data_dir_named(&crate::paths::product_name())
            .unwrap()
            .join("history.db");
        let Ok(conn) = rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) else {
            return;
        };
        let mut stmt = conn
            .prepare("SELECT raw_text, final_text, COALESCE(app_name, '') FROM transcripts ORDER BY id")
            .unwrap();
        let rows: Vec<(String, String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        let (mut changed, mut checked) = (0, 0);
        let mut last: std::collections::HashMap<String, String> = Default::default();
        for (raw, final_text, app) in rows {
            let name = app.trim_end_matches(".exe").to_string();
            let ctx = ctx(&name, "", last.get(&app).map_or("", String::as_str));
            last.insert(app, final_text);
            if fixup::words(&raw).len() < 3 {
                continue;
            }
            checked += 1;
            let t = Instant::now();
            let (out, notes) = fix(&raw, &ctx);
            if out != raw {
                changed += 1;
                println!("\nhistory, {} ms\n  heard: {raw}\n  out:   {out}\n  notes: {notes}", t.elapsed().as_millis());
            }
        }
        println!("\nhistory: {changed} of {checked} changed");
    }
}
