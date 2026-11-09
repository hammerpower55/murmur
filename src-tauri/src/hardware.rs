//! What this PC is, for showing how fast each model will run on it. The
//! catalogue's speeds were measured on a Snapdragon X Plus (10 cores); other
//! chips scale them by a factor, one for CPU engines and one for Whisper,
//! which runs on the GPU through Metal on a Mac. The factors are estimates
//! from public benchmarks of these chips, so the page calls them estimates
//! until a model has been used here and its own speed is known.

use std::sync::OnceLock;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    /// The chip as people know it ("Snapdragon X Plus", "Apple M2 Pro").
    pub name: String,
    pub cores: usize,
    pub memory_mb: u64,
    /// Time on this PC over time on the reference Snapdragon X Plus.
    #[serde(skip)]
    pub cpu_factor: f32,
    #[serde(skip)]
    pub whisper_factor: f32,
}

/// This PC, looked up once.
pub fn device() -> &'static Device {
    static DEVICE: OnceLock<Device> = OnceLock::new();
    DEVICE.get_or_init(|| {
        let cores = std::thread::available_parallelism().map_or(8, |n| n.get());
        let device = describe(&chip_brand().unwrap_or_default(), cores, memory_mb());
        log::info!(
            "device: {} ({} cores, {} MB), CPU x{:.2}, Whisper x{:.2}",
            device.name,
            device.cores,
            device.memory_mb,
            device.cpu_factor,
            device.whisper_factor
        );
        device
    })
}

/// A device from the CPU brand string the system reports.
pub fn describe(brand: &str, cores: usize, memory_mb: u64) -> Device {
    let b = brand.to_lowercase();
    let (name, cpu, whisper): (String, f32, f32) = if let Some(apple) = apple_chip(&b) {
        apple
    } else if b.contains("snapdragon") || b.contains("qualcomm") || b.contains("oryon") {
        if b.contains("x2") {
            ("Snapdragon X2 Elite".into(), 0.6, 0.6)
        } else if b.contains("x elite") || b.contains("x1e") {
            ("Snapdragon X Elite".into(), 0.8, 0.8)
        } else if b.contains("x plus") || b.contains("x1p") {
            // The reference is the 10 core X Plus; the 8 core one is slower.
            let f = if cores >= 10 { 1.0 } else { 1.2 };
            ("Snapdragon X Plus".into(), f, f)
        } else if b.contains("8cx") || b.contains("7c") || b.contains("sq") {
            ("Snapdragon 8cx".into(), 2.5, 2.5)
        } else {
            ("Snapdragon".into(), 1.0, 1.0)
        }
    } else {
        // Anything else: scale by cores against the reference's 8.
        let f = (8.0 / cores.max(2) as f32).clamp(0.6, 2.5);
        let name = brand.split_whitespace().collect::<Vec<_>>().join(" ");
        (if name.is_empty() { "This PC".into() } else { name }, f, f)
    };
    Device {
        name,
        cores,
        memory_mb,
        cpu_factor: cpu,
        whisper_factor: whisper,
    }
}

/// "apple m2 pro" and the like: name, CPU factor, Whisper (Metal) factor.
fn apple_chip(b: &str) -> Option<(String, f32, f32)> {
    let rest = b.strip_prefix("apple m")?;
    let generation: u32 = rest.chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()?;
    let tier = if rest.contains("ultra") {
        "Ultra"
    } else if rest.contains("max") {
        "Max"
    } else if rest.contains("pro") {
        "Pro"
    } else {
        ""
    };
    // Base chips: CPU close to the X Plus for M1, faster each generation;
    // whisper.cpp on the GPU is several times faster than on any CPU here.
    let (cpu, metal) = match generation {
        1 => (1.1, 0.35),
        2 => (0.95, 0.3),
        3 => (0.85, 0.25),
        4 => (0.7, 0.2),
        _ => (0.6, 0.17),
    };
    let (cpu_tier, gpu_tier) = match tier {
        "Ultra" => (0.5, 0.25),
        "Max" => (0.65, 0.4),
        "Pro" => (0.75, 0.6),
        _ => (1.0, 1.0),
    };
    let name = format!("Apple M{generation}{}", if tier.is_empty() { String::new() } else { format!(" {tier}") });
    Some((name, cpu * cpu_tier, metal * gpu_tier))
}

#[cfg(windows)]
fn chip_brand() -> Option<String> {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let mut buf = [0u16; 256];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: the buffer and its byte length are valid for the call.
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0"),
            w!("ProcessorNameString"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    }
    .ok()
    .ok()?;
    let chars = (len as usize / 2).saturating_sub(1).min(buf.len());
    Some(String::from_utf16_lossy(&buf[..chars]).trim().to_string())
}

#[cfg(windows)]
fn memory_mb() -> u64 {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: dwLength is set as the call requires.
    if unsafe { GlobalMemoryStatusEx(&mut status) }.is_ok() {
        status.ullTotalPhys / (1024 * 1024)
    } else {
        0
    }
}

#[cfg(target_os = "macos")]
fn sysctl_bytes(name: &str) -> Option<Vec<u8>> {
    let name = std::ffi::CString::new(name).ok()?;
    let mut len: libc::size_t = 0;
    // SAFETY: a null buffer asks for the size; the second call fills `buf`.
    unsafe {
        if libc::sysctlbyname(name.as_ptr(), std::ptr::null_mut(), &mut len, std::ptr::null_mut(), 0) != 0 {
            return None;
        }
        let mut buf = vec![0u8; len];
        if libc::sysctlbyname(name.as_ptr(), buf.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0) != 0 {
            return None;
        }
        buf.truncate(len);
        Some(buf)
    }
}

#[cfg(target_os = "macos")]
fn chip_brand() -> Option<String> {
    let bytes = sysctl_bytes("machdep.cpu.brand_string")?;
    Some(String::from_utf8_lossy(&bytes).trim_end_matches('\0').trim().to_string())
}

#[cfg(target_os = "macos")]
fn memory_mb() -> u64 {
    sysctl_bytes("hw.memsize")
        .and_then(|b| b.get(..8).map(|b| u64::from_ne_bytes(b.try_into().unwrap_or([0; 8]))))
        .map_or(0, |bytes| bytes / (1024 * 1024))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn chip_brand() -> Option<String> {
    None
}

#[cfg(not(any(windows, target_os = "macos")))]
fn memory_mb() -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapdragon_chips_are_named_and_scaled() {
        let plus = describe("Snapdragon(R) X Plus - X1P64100 - Qualcomm(R) Oryon(TM) CPU", 10, 16_000);
        assert_eq!((plus.name.as_str(), plus.cpu_factor), ("Snapdragon X Plus", 1.0));
        assert!(describe("Snapdragon(R) X Plus - X1P42100", 8, 16_000).cpu_factor > 1.0);
        let elite = describe("Snapdragon(R) X Elite - X1E78100 - Qualcomm(R) Oryon(TM) CPU", 12, 32_000);
        assert_eq!(elite.name, "Snapdragon X Elite");
        assert!(elite.cpu_factor < plus.cpu_factor);
        assert_eq!(describe("Snapdragon (TM) 8cx Gen 3 @ 3.0 GHz", 8, 16_000).name, "Snapdragon 8cx");
    }

    #[test]
    fn apple_chips_run_whisper_on_the_gpu() {
        let m1 = describe("Apple M1", 8, 8_192);
        assert_eq!(m1.name, "Apple M1");
        assert!(m1.whisper_factor < 0.5 && m1.cpu_factor > 1.0);
        let m3_pro = describe("Apple M3 Pro", 12, 18_432);
        assert_eq!(m3_pro.name, "Apple M3 Pro");
        assert!(m3_pro.cpu_factor < describe("Apple M3", 8, 8_192).cpu_factor);
        assert_eq!(describe("Apple M4 Max", 16, 65_536).name, "Apple M4 Max");
    }

    #[test]
    fn unknown_chips_scale_by_cores() {
        let d = describe("Some ARM CPU", 4, 8_000);
        assert_eq!(d.name, "Some ARM CPU");
        assert!((d.cpu_factor - 2.0).abs() < 1e-6);
        assert_eq!(describe("", 8, 0).name, "This PC");
    }

    #[test]
    fn this_device_is_detected() {
        let d = device();
        assert!(!d.name.is_empty() && d.cores > 0);
        println!("{d:?}");
    }
}
