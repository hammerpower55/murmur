//! Fixture evaluation: transcribes every clip listed in
//! tests/audio/references.tsv and prints word error rate. Ignored by default
//! because it needs a downloaded model:
//!
//!   cargo test fixture_wer -- --ignored --nocapture
//!
//! MURMUR_MODEL_ID picks a catalogue model (default: the catalogue default),
//! MURMUR_FIXTURES overrides the clip folder.

use std::path::{Path, PathBuf};

use super::onnx;
use super::whisper::WhisperEngine;
use super::{has_arabic_script, SpeechEngine};
use crate::audio::resample;
use crate::models::catalog;
use crate::models::manager::options_for;
use crate::models::registry::Registry;

/// Lowercase words with punctuation removed. Apostrophes inside words stay,
/// so "let's" is one word.
pub fn normalize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(|w| w.trim_matches('\'').to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Word level edit distance: (substitutions + deletions + insertions, words
/// in the reference).
pub fn word_errors(reference: &str, hypothesis: &str) -> (usize, usize) {
    let r = normalize(reference);
    let h = normalize(hypothesis);
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    for (i, rw) in r.iter().enumerate() {
        let mut row = vec![i + 1; h.len() + 1];
        for (j, hw) in h.iter().enumerate() {
            let substitute = prev[j] + usize::from(rw != hw);
            row[j + 1] = substitute.min(prev[j + 1] + 1).min(row[j] + 1);
        }
        prev = row;
    }
    (prev[h.len()], r.len())
}

fn read_wav(path: &Path) -> (Vec<f32>, u32) {
    let mut reader = hound::WavReader::open(path).unwrap();
    let spec = reader.spec();
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap()).collect(),
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.unwrap() as f32 / scale)
                .collect()
        }
    };
    let channels = spec.channels as usize;
    let mono = interleaved
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();
    (mono, spec.sample_rate)
}

struct Reference {
    file: String,
    group: String,
    text: String,
}

fn references(dir: &Path) -> Vec<Reference> {
    std::fs::read_to_string(dir.join("references.tsv"))
        .unwrap()
        .lines()
        // Windows editors may save a UTF-8 byte order mark.
        .map(|l| l.trim_start_matches('\u{feff}'))
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut cols = l.splitn(3, '\t');
            Reference {
                file: cols.next().unwrap().trim().to_string(),
                group: cols.next().unwrap().trim().to_string(),
                text: cols.next().unwrap().trim().to_string(),
            }
        })
        .collect()
}

#[test]
#[ignore = "needs the downloaded model and recorded clips"]
fn fixture_wer() {
    let dir = std::env::var_os("MURMUR_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/audio"));
    let id =
        std::env::var("MURMUR_MODEL_ID").unwrap_or_else(|_| catalog::default_model().unwrap().id);
    let models_dir = crate::paths::data_dir_named(&crate::paths::product_name())
        .unwrap()
        .join("models");
    let spec = Registry::new(models_dir).resolve(&id).unwrap();
    let opts = options_for(spec.family, &id, true);
    onnx::use_vendored_runtime();
    let mut engine = super::host::engine_for(&spec).unwrap();
    let loading = std::time::Instant::now();
    engine.load(&spec.path, &opts).unwrap();
    println!(
        "\nmodel {id} loaded in {} ms, {} threads\n",
        loading.elapsed().as_millis(),
        opts.n_threads
    );

    // Raw engine errors, and errors after the text pipeline (what is pasted).
    let mut totals: Vec<(String, usize, usize, usize)> = Vec::new();
    let text_settings = crate::text::TextSettings {
        add_trailing_space: false,
        ..crate::text::TextSettings::default()
    };
    let ctx = crate::text::snippets::SnippetContext::fixed("", "", "");
    let mut missing = Vec::new();
    let mut arabic = Vec::new();
    for r in references(&dir) {
        let path = dir.join(&r.file);
        if !path.exists() {
            missing.push(r.file);
            continue;
        }
        let (audio, rate) = read_wav(&path);
        let audio = resample::to_16k(&audio, rate).unwrap();
        let seconds = audio.len() as f64 / 16_000.0;
        let t = engine.transcribe(&audio, Some("en")).unwrap();
        let (errors, words) = word_errors(&r.text, &t.text);
        let cleaned = crate::text::process(&t.text, &text_settings, &ctx);
        let (clean_errors, _) = word_errors(&r.text, &cleaned);
        if has_arabic_script(&t.text) {
            arabic.push(r.file.clone());
        }
        println!(
            "{:<12} {:<8} {:>5.1} s audio  {:>5} ms  {:>4.2}x  WER {:>5.1}%",
            r.file,
            r.group,
            seconds,
            t.inference_ms,
            t.inference_ms as f64 / 1000.0 / seconds,
            100.0 * errors as f64 / words.max(1) as f64
        );
        println!("    ref: {}\n    got: {}", r.text, t.text);
        if cleaned != t.text {
            println!("  paste: {cleaned}");
        }
        match totals.iter_mut().find(|(g, _, _, _)| *g == r.group) {
            Some(entry) => {
                entry.1 += errors;
                entry.2 += words;
                entry.3 += clean_errors;
            }
            None => totals.push((r.group, errors, words, clean_errors)),
        }
    }

    println!();
    for (group, errors, words, clean) in &totals {
        println!(
            "{group:<8} WER {:.1}% ({errors} errors / {words} words), after clean up {:.1}%",
            100.0 * *errors as f64 / (*words).max(1) as f64,
            100.0 * *clean as f64 / (*words).max(1) as f64
        );
    }
    if !missing.is_empty() {
        println!("not recorded yet: {}", missing.join(", "));
    }
    assert!(!totals.is_empty(), "no clips found in {}", dir.display());
    assert!(arabic.is_empty(), "Arabic script in: {}", arabic.join(", "));
}

/// Compares ways of getting Roman Urdu out of Whisper on the mixed clips:
/// language and prompt combinations, printing the raw text for each.
///   cargo test roman_urdu_strategies -- --ignored --nocapture
#[test]
#[ignore = "needs the downloaded Whisper model and recorded clips"]
fn roman_urdu_strategies() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/audio");
    let id = std::env::var("MURMUR_MODEL_ID").unwrap_or_else(|_| "whisper-large-v3-turbo-q8".into());
    let models_dir = crate::paths::data_dir_named(&crate::paths::product_name())
        .unwrap()
        .join("models");
    let spec = Registry::new(models_dir).resolve(&id).unwrap();
    let clips: Vec<Reference> = references(&dir).into_iter().filter(|r| r.group != "english").collect();
    let audio: Vec<Vec<f32>> = clips
        .iter()
        .map(|r| {
            let (a, rate) = read_wav(&dir.join(&r.file));
            resample::to_16k(&a, rate).unwrap()
        })
        .collect();
    let short = "Yaar kal meeting hai, main tumhe bata deta hoon. Acha theek hai, mujhe lagta hai ke hum kal tak kar lenge. Bhai jaldi bhej do, client wait kar raha hai.";
    let long = "Assalam o alaikum. Aaj ka din bohat busy tha, main abhi ghar pohncha hoon. \
                Acha theek hai, mujhe lagta hai ke hum kal tak yeh kaam khatam kar lenge. \
                Tum ne meri email dekhi? Bhai please jaldi bhej do, client wait kar raha hai. \
                Koi baat nahi, main shaam ko call karta hoon aur sab samjha deta hoon.";
    let strategies: Vec<(&str, Option<&str>, Option<String>)> = vec![
        ("en + ROMAN_URDU_PROMPT (shipped)", Some("en"), Some(super::whisper::ROMAN_URDU_PROMPT.into())),
        ("en + short Roman Urdu prompt", Some("en"), Some(short.into())),
        ("en + long Roman Urdu prompt", Some("en"), Some(long.into())),
        ("hi + long Roman Urdu prompt", Some("hi"), Some(long.into())),
        ("ur + long Roman Urdu prompt", Some("ur"), Some(long.into())),
    ];
    for (name, lang, prompt) in strategies {
        let opts = super::EngineOptions {
            initial_prompt: prompt,
            ..super::EngineOptions::default()
        };
        let mut engine = WhisperEngine::new();
        engine.load(&spec.path, &opts).unwrap();
        println!("\n== {name}");
        let (mut errors, mut total, mut ms) = (0, 0, 0);
        for (r, a) in clips.iter().zip(&audio) {
            let t = engine.transcribe(a, lang).unwrap();
            let (e, w) = word_errors(&r.text, &t.text);
            errors += e;
            total += w;
            ms += t.inference_ms;
            let arabic = if has_arabic_script(&t.text) { "  (Arabic script)" } else { "" };
            println!("  {}  {:>3}%  {}{arabic}", r.file, 100 * e / w.max(1), t.text);
        }
        println!("  WER {:.1}%, {} ms per clip", 100.0 * errors as f64 / total.max(1) as f64, ms / clips.len() as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_drops_case_and_punctuation() {
        assert_eq!(
            normalize("Yaar, kal MEETING hai! Let's go."),
            vec!["yaar", "kal", "meeting", "hai", "let's", "go"]
        );
    }

    #[test]
    fn identical_text_has_no_errors() {
        assert_eq!(word_errors("send the deck", "Send the deck."), (0, 3));
    }

    #[test]
    fn counts_substitution_deletion_and_insertion() {
        assert_eq!(word_errors("a b c d", "a x c d"), (1, 4));
        assert_eq!(word_errors("a b c d", "a c d"), (1, 4));
        assert_eq!(word_errors("a b c d", "a b c d e"), (1, 4));
        assert_eq!(word_errors("a b", ""), (2, 2));
    }
}
