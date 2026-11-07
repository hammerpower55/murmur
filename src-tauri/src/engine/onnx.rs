//! ONNX Runtime setup shared by the Parakeet, Moonshine and SenseVoice
//! engines and the word fixer: one environment for the process, CPU
//! sessions, NPU sessions through Qualcomm's QNN plugin, and small helpers.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{anyhow, bail, Context, Result};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;

static RUNTIME_PATH: OnceLock<PathBuf> = OnceLock::new();
static QNN_DIR: OnceLock<PathBuf> = OnceLock::new();
static NPU: OnceLock<bool> = OnceLock::new();

/// The vendored ONNX Runtime folder whose name matches this build's
/// architecture (scripts/fetch_onnxruntime.ps1 fetches either one).
#[cfg(windows)]
// Read by debug builds (lib.rs) and by the test helper below.
#[allow(dead_code)]
pub const VENDORED_RUNTIME: &str = if cfg!(target_arch = "aarch64") {
    "onnxruntime-win-arm64-1.31.0"
} else {
    "onnxruntime-win-x64-1.31.0"
};

/// The QNN plugin's name, and the execution provider name it reports.
const QNN_EP: &str = "QNNExecutionProvider";
const QNN_PLUGIN: &str = "onnxruntime_providers_qnn.dll";
const QNN_BACKEND: &str = "QnnHtp.dll";
static ENVIRONMENT: OnceLock<std::result::Result<ort::environment::Environment, String>> =
    OnceLock::new();

/// Where onnxruntime.dll (libonnxruntime.*.dylib on macOS) lives. Set once
/// at startup, before any load. The DLL is loaded from this exact path so
/// the older copy Windows keeps in System32 is never picked up.
pub fn set_runtime_path(path: PathBuf) {
    let _ = RUNTIME_PATH.set(path);
}

/// The folder holding Qualcomm's QNN plugin (scripts/fetch_qnn.ps1). Set
/// once at startup; without it every model runs on the CPU.
pub fn set_qnn_dir(dir: PathBuf) {
    let _ = QNN_DIR.set(dir);
}

/// Whether this PC has an NPU that NPU builds of models can run on: the QNN
/// plugin is there, loads, and reports a Hexagon NPU. Checked once.
pub fn npu_available() -> bool {
    *NPU.get_or_init(|| match register_qnn() {
        Ok(found) => {
            log::info!("NPU {}", if found { "found" } else { "not found" });
            found
        }
        Err(err) => {
            log::info!("no NPU: {err:#}");
            false
        }
    })
}

fn register_qnn() -> Result<bool> {
    let dir = QNN_DIR.get().context("QNN location not set")?;
    let plugin = dir.join(QNN_PLUGIN);
    if !plugin.is_file() {
        return Ok(false);
    }
    let env = environment()?;
    env.register_ep_library(QNN_EP, &plugin)
        .map_err(|e| anyhow!("could not load {}: {e}", plugin.display()))?;
    Ok(npu_devices(env).next().is_some())
}

fn npu_devices(
    env: &ort::environment::Environment,
) -> impl Iterator<Item = ort::device::Device<'_>> + '_ {
    env.devices().filter(|d| {
        d.ep().is_ok_and(|ep| ep == QNN_EP)
            && d.hardware_device().ty() == ort::memory::DeviceType::NPU
    })
}

/// A session whose precompiled QNN graphs run on the NPU (anything else in
/// the model stays on the CPU). Sessions over the same context binaries
/// share them instead of loading each a second time.
pub fn npu_session(dir: &Path, file: &str) -> Result<Session> {
    npu_session_with(dir, file, None)
}

/// An NPU session for a model that is not precompiled: the QNN plugin
/// compiles it for this NPU the first time (float models run in fp16), and
/// the result is saved as `compiled` next to it, which later loads use.
pub fn npu_session_compiled(dir: &Path, file: &str, compiled: &str) -> Result<Session> {
    if dir.join(compiled).is_file() {
        match npu_session(dir, compiled) {
            Ok(session) => return Ok(session),
            // A cache from another NPU or QNN version: compile again.
            Err(err) => {
                log::warn!("recompiling {file}: {err:#}");
                let _ = std::fs::remove_file(dir.join(compiled));
            }
        }
    }
    npu_session_with(dir, file, Some(&dir.join(compiled)))
}

fn npu_session_with(dir: &Path, file: &str, save_compiled: Option<&Path>) -> Result<Session> {
    if !npu_available() {
        bail!("this PC has no NPU Murmur can use");
    }
    let env = environment()?;
    let qnn = QNN_DIR.get().context("QNN location not set")?;
    let options = [
        (format!("{QNN_EP}.backend_path"), qnn.join(QNN_BACKEND).display().to_string()),
        (format!("{QNN_EP}.htp_performance_mode"), "burst".to_string()),
        (format!("{QNN_EP}.enable_htp_fp16_precision"), "1".to_string()),
    ];
    let path = dir.join(file);
    // Sharing contexts between sessions conflicts with saving one.
    let share = if save_compiled.is_some() { "0" } else { "1" };
    let mut builder = Session::builder(env)
        .and_then(|b| Ok(b.with_config_entry("ep.share_ep_contexts", share)?));
    if let Some(out) = save_compiled {
        let out = out.display().to_string();
        builder = builder
            .and_then(|b| Ok(b.with_config_entry("ep.context_enable", "1")?))
            .and_then(|b| Ok(b.with_config_entry("ep.context_file_path", &out)?))
            .and_then(|b| Ok(b.with_config_entry("ep.context_embed_mode", "1")?));
    }
    builder
        .and_then(|b| Ok(b.with_devices(npu_devices(env), Some(&options))?))
        .and_then(|mut b| b.commit_from_file(&path))
        .map_err(|e| anyhow!("could not load {} on the NPU: {e}", path.display()))
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

/// Points tests at the runtimes scripts/fetch_onnxruntime.ps1 and
/// scripts/fetch_qnn.ps1 vendor.
#[cfg(test)]
pub fn use_vendored_runtime() {
    let vendor = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor");
    #[cfg(windows)]
    set_runtime_path(vendor.join(VENDORED_RUNTIME).join("lib/onnxruntime.dll"));
    #[cfg(target_os = "macos")]
    set_runtime_path(vendor.join("onnxruntime-osx-arm64-1.31.0/lib/libonnxruntime.1.31.0.dylib"));
    set_qnn_dir(vendor.join("onnxruntime-qnn-2.7.0"));
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
