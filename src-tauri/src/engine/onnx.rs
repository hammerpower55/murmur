//! ONNX Runtime setup shared by the Parakeet, Moonshine and SenseVoice
//! engines: one environment for the process, CPU sessions, and small helpers.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{anyhow, Result};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;

static RUNTIME_PATH: OnceLock<PathBuf> = OnceLock::new();
static ENVIRONMENT: OnceLock<std::result::Result<ort::environment::Environment, String>> =
    OnceLock::new();

/// Where onnxruntime.dll lives. Set once at startup, before any load. The
/// DLL is loaded from this exact path so the older copy Windows keeps in
/// System32 is never picked up.
pub fn set_runtime_path(path: PathBuf) {
    let _ = RUNTIME_PATH.set(path);
}

fn environment() -> Result<&'static ort::environment::Environment> {
    let env = ENVIRONMENT.get_or_init(|| {
        let path = RUNTIME_PATH
            .get()
            .ok_or_else(|| "ONNX Runtime location not set".to_string())?;
        ort::init_from(path)
            .map_err(|e| format!("could not load {}: {e}", path.display()))?
            .with_name("murmur")
            .build()
            .map_err(|e| format!("could not start ONNX Runtime: {e}"))
    });
    env.as_ref().map_err(|e| anyhow!("{e}"))
}

pub fn session(dir: &Path, file: &str, threads: usize) -> Result<Session> {
    let env = environment()?;
    let path = dir.join(file);
    Session::builder(env)
        .and_then(|b| Ok(b.with_optimization_level(GraphOptimizationLevel::Level3)?))
        .and_then(|b| Ok(b.with_intra_threads(threads)?))
        .and_then(|mut b| b.commit_from_file(&path))
        .map_err(|e| anyhow!("could not load {}: {e}", path.display()))
}

pub fn argmax(values: &[f32]) -> usize {
    values
        .iter()
        .enumerate()
        .fold((0, f32::NEG_INFINITY), |best, (i, &v)| {
            if v > best.1 {
                (i, v)
            } else {
                best
            }
        })
        .0
}

/// Points tests at the runtime scripts/fetch_onnxruntime.ps1 vendors.
#[cfg(test)]
pub fn use_vendored_runtime() {
    set_runtime_path(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("vendor/onnxruntime-win-arm64-1.31.0/lib/onnxruntime.dll"),
    );
}

/// Test helper: the installed copy of a catalogue model.
#[cfg(test)]
pub fn installed_model_dir(id: &str) -> PathBuf {
    crate::paths::data_dir_named(&crate::paths::product_name())
        .unwrap()
        .join("models")
        .join(id)
}

/// Test helper: prints a model's real input and output names and types.
#[cfg(test)]
pub fn print_io(dir: &Path, file: &str) {
    let s = session(dir, file, 2).unwrap();
    println!("== {file}");
    for i in s.inputs() {
        println!("  in  {} {:?}", i.name(), i.dtype());
    }
    for o in s.outputs() {
        println!("  out {} {:?}", o.name(), o.dtype());
    }
    if let Ok(meta) = s.metadata() {
        for key in meta.custom_keys().unwrap_or_default() {
            let value = meta.custom(&key).unwrap_or_default();
            let shown: String = value.chars().take(60).collect();
            println!("  meta {key} = {shown}");
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argmax_picks_the_largest() {
        assert_eq!(argmax(&[0.1, 3.0, -1.0, 2.9]), 1);
        assert_eq!(argmax(&[5.0]), 0);
    }
}
