//! `murmur --check-model <id> [clip.wav]`: downloads a catalogue model if it
//! is missing, loads it with the same code and runtime the app uses, runs it
//! once, prints what happened and exits (0 when it worked). Speech models
//! transcribe the clip (without one, only loading is checked); word fixer
//! models score a sentence with a misheard dictionary word. For checking a built app on a machine with no microphone
//! or screen, such as CI, and for getting a readable error from a user.
//!
//!   Murmur.app/Contents/MacOS/murmur --check-model parakeet-tdt-0.6b-v2 clip.wav

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use anyhow::{bail, Context, Result};

use crate::audio::resample;
use crate::engine::{default_threads, fixer, host, llm::Llm};
use crate::models::catalog::{self, Family};
use crate::models::download;
use crate::models::manager::options_for;
use crate::models::registry::Registry;
use crate::text::fixup;

pub const FLAG: &str = "--check-model";

/// The model id and clip given after `--check-model`, if it was passed.
pub fn requested() -> Option<(String, Option<PathBuf>)> {
    parse(std::env::args().skip(1))
}

fn parse(mut args: impl Iterator<Item = String>) -> Option<(String, Option<PathBuf>)> {
    args.by_ref().find(|a| a == FLAG)?;
    let id = args.next().unwrap_or_default();
    let clip = args.next().filter(|a| !a.starts_with("--")).map(PathBuf::from);
    Some((id, clip))
}

/// Runs the check and returns the process exit code.
pub fn run(models_dir: &Path, id: &str, clip: Option<&Path>) -> i32 {
    match check(models_dir, id, clip) {
        Ok(()) => {
            println!("ok: {id}");
            0
        }
        Err(err) => {
            println!("failed: {id}: {err:#}");
            1
        }
    }
}

fn check(models_dir: &Path, id: &str, clip: Option<&Path>) -> Result<()> {
    let model = catalog::find(id).with_context(|| format!("{id} is not in the catalogue"))?;
    let dir = models_dir.join(id);
    let started = Instant::now();
    let mut shown = None;
    download::download_model(&model, &dir, &AtomicBool::new(false), |done, total| {
        let percent = done * 100 / total.max(1);
        if shown.is_none_or(|s| percent >= s + 25 || (percent == 100 && s < 100)) {
            shown = Some(percent);
            println!("download {percent}%");
        }
    })?;
    println!("downloaded in {} s", started.elapsed().as_secs());

    let loading = Instant::now();
    if model.family == Family::Fixup {
        let llm = Llm::load(&dir, default_threads())?;
        println!("loaded in {} ms", loading.elapsed().as_millis());
        let heard = "can you send it to mars by friday";
        let ctx = fixup::Context {
            before: String::new(),
            earlier: String::new(),
            vocabulary: vec!["Maaz".into()],
            any_word: false,
        };
        let scoring = Instant::now();
        let (found, notes) = fixer::rescore(&llm, heard, &ctx, fixer::MIN_GAIN_OPEN)?;
        let edits: Vec<_> = found.iter().map(|r| (r.start, r.end, r.meant.clone())).collect();
        println!("scored in {} ms", scoring.elapsed().as_millis());
        println!("heard: {heard}");
        println!("fixed: {}", fixup::apply_at(heard, &edits));
        println!("notes: {}", notes.join(" | "));
        return Ok(());
    }

    let spec = Registry::new(models_dir.to_path_buf()).resolve(id)?;
    let opts = options_for(spec.family, id, true);
    let mut engine = host::engine_for(&spec)?;
    engine.load(&spec.path, &opts)?;
    if !engine.is_loaded() {
        bail!("{} engine did not load {id}", engine.id());
    }
    println!("loaded with the {} engine in {} ms", engine.id(), loading.elapsed().as_millis());
    let Some(clip) = clip else {
        println!("no clip given, so only loading was checked");
        return Ok(());
    };
    let (audio, rate) = read_wav(clip)?;
    let audio = resample::to_16k(&audio, rate)?;
    let t = engine.transcribe(&audio, Some("en"))?;
    println!("transcribed {:.1} s of audio in {} ms", audio.len() as f64 / 16_000.0, t.inference_ms);
    println!("text: {}", t.text);
    if t.text.trim().is_empty() {
        bail!("the transcript is empty");
    }
    Ok(())
}

/// Any PCM or float WAV, mixed down to mono.
fn read_wav(path: &Path) -> Result<(Vec<f32>, u32)> {
    let mut reader =
        hound::WavReader::open(path).with_context(|| format!("could not open {}", path.display()))?;
    let spec = reader.spec();
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<_, _>>()?
        }
    };
    let channels = usize::from(spec.channels.max(1));
    let mono = interleaved
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();
    Ok((mono, spec.sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> impl Iterator<Item = String> {
        list.iter().map(|s| s.to_string()).collect::<Vec<_>>().into_iter()
    }

    #[test]
    fn reads_the_model_and_clip() {
        assert_eq!(parse(args(&["--hidden"])), None);
        assert_eq!(
            parse(args(&[FLAG, "moonshine-tiny", "a.wav"])),
            Some(("moonshine-tiny".into(), Some(PathBuf::from("a.wav"))))
        );
        assert_eq!(parse(args(&[FLAG, "qwen3-1.7b-int4"])), Some(("qwen3-1.7b-int4".into(), None)));
        assert_eq!(parse(args(&[FLAG, "x", "--hidden"])), Some(("x".into(), None)));
    }
}
