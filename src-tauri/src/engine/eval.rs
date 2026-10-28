//! Fixture evaluation: transcribes every clip listed in
//! tests/audio/references.tsv and prints word error rate. Ignored by default
//! because it needs a downloaded model:
//!
//!   cargo test fixture_wer -- --ignored --nocapture
//!
//! MURMUR_MODEL_ID picks a catalogue model (default: the catalogue default),
//! MURMUR_FIXTURES overrides the clip folder.

use std::path::{Path, PathBuf};

use super::parakeet::{self, ParakeetEngine};
use super::whisper::WhisperEngine;
use super::{has_arabic_script, SpeechEngine};
use crate::audio::resample;
use crate::models::catalog::{self, Family};
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
    let opts = options_for(spec.family);
    let mut engine: Box<dyn SpeechEngine> = match spec.family {
        Family::Whisper => Box::new(WhisperEngine::new()),
        Family::Parakeet => {
            parakeet::set_runtime_path(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("vendor/onnxruntime-win-arm64-1.31.0/lib/onnxruntime.dll"),
            );
            Box::new(ParakeetEngine::new())
        }
    };
    let loading = std::time::Instant::now();
    engine.load(&spec.path, &opts).unwrap();
    println!(
        "\nmodel {id} loaded in {} ms, {} threads\n",
        loading.elapsed().as_millis(),
        opts.n_threads
    );

    let mut totals: Vec<(String, usize, usize)> = Vec::new();
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
        match totals.iter_mut().find(|(g, _, _)| *g == r.group) {
            Some(entry) => {
                entry.1 += errors;
                entry.2 += words;
            }
            None => totals.push((r.group, errors, words)),
        }
    }

    println!();
    for (group, errors, words) in &totals {
        println!(
            "{group:<8} WER {:.1}% ({errors} errors / {words} words)",
            100.0 * *errors as f64 / (*words).max(1) as f64
        );
    }
    if !missing.is_empty() {
        println!("not recorded yet: {}", missing.join(", "));
    }
    assert!(!totals.is_empty(), "no clips found in {}", dir.display());
    assert!(arabic.is_empty(), "Arabic script in: {}", arabic.join(", "));
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
