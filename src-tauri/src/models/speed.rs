//! How fast a model runs on this PC: the catalogue's time for a 5 s clip on
//! the reference Snapdragon X Plus, scaled for this chip, until the model
//! has been used here a few times; then the median of its own dictations.

use std::collections::HashMap;

use super::catalog::{CatalogModel, Family};
use crate::hardware::Device;

/// Dictations needed before a model's own speed replaces the estimate.
const MEASURED_AFTER: usize = 3;
/// Slower than this for a 5 s clip, the card says so.
pub const SLOW_MS: u32 = 8_000;

/// Estimated milliseconds for a 5 s clip on `device`.
pub fn estimate(model: &CatalogModel, device: &Device) -> u32 {
    let factor = if model.npu {
        // Same NPU on every Snapdragon X it runs on.
        1.0
    } else if model.family == Family::Whisper {
        device.whisper_factor
    } else {
        device.cpu_factor
    };
    (model.clip_ms as f32 * factor).round() as u32
}

/// One to five dots for a 5 s clip taking `ms`.
pub fn dots(ms: u32) -> u8 {
    match ms {
        0..=400 => 5,
        401..=1_500 => 4,
        1_501..=5_000 => 3,
        5_001..=10_000 => 2,
        _ => 1,
    }
}

/// Each model's median time for a 5 s clip from its dictations on this PC
/// (`(model id, inference ms, audio ms)`, newest first), for models with
/// enough of them. Whisper always encodes 30 s, so its time barely depends
/// on the clip's length; the others scale with it.
pub fn measured(rows: &[(String, u64, u64)], family: impl Fn(&str) -> Option<Family>) -> HashMap<String, u32> {
    let mut times: HashMap<&str, Vec<u32>> = HashMap::new();
    for (id, inference_ms, audio_ms) in rows {
        let Some(family) = family(id) else {
            continue;
        };
        if *audio_ms < 1_000 || *inference_ms == 0 {
            continue;
        }
        let ms = if family == Family::Whisper {
            *inference_ms as f64
        } else {
            *inference_ms as f64 * 5_000.0 / *audio_ms as f64
        };
        let list = times.entry(id.as_str()).or_default();
        if list.len() < 20 {
            list.push(ms.round() as u32);
        }
    }
    times
        .into_iter()
        .filter(|(_, t)| t.len() >= MEASURED_AFTER)
        .map(|(id, mut t)| {
            t.sort_unstable();
            (id.to_string(), t[t.len() / 2])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::describe;
    use crate::models::catalog;

    #[test]
    fn dots_follow_the_time_for_a_clip() {
        assert_eq!(dots(150), 5);
        assert_eq!(dots(500), 4);
        assert_eq!(dots(4_500), 3);
        assert_eq!(dots(6_000), 2);
        assert_eq!(dots(12_000), 1);
    }

    #[test]
    fn a_mac_runs_whisper_much_faster_than_the_reference() {
        let turbo = catalog::find("whisper-large-v3-turbo-q8").unwrap();
        let parakeet = catalog::find("parakeet-tdt-0.6b-v2").unwrap();
        let plus = describe("Snapdragon(R) X Plus - X1P64100 - Qualcomm(R) Oryon(TM) CPU", 10, 16_000);
        let m2 = describe("Apple M2", 8, 16_384);
        assert_eq!(estimate(&turbo, &plus), turbo.clip_ms);
        assert!(estimate(&turbo, &m2) < turbo.clip_ms / 2);
        assert!(dots(estimate(&turbo, &m2)) > dots(estimate(&turbo, &plus)));
        // CPU engines stay close.
        assert_eq!(dots(estimate(&parakeet, &m2)), 5);
    }

    #[test]
    fn measured_speed_needs_a_few_dictations_and_scales_short_clips() {
        let family = |id: &str| match id {
            "w" => Some(Family::Whisper),
            "p" => Some(Family::Parakeet),
            _ => None,
        };
        let rows = vec![
            ("w".to_string(), 2_000, 3_000),
            ("w".to_string(), 3_000, 9_000),
            ("w".to_string(), 2_500, 5_000),
            ("p".to_string(), 100, 10_000),
            ("p".to_string(), 50, 2_500),
            ("p".to_string(), 999, 500),
            ("gone".to_string(), 1, 5_000),
        ];
        let m = measured(&rows, family);
        assert_eq!(m.get("w"), Some(&2_500));
        // Two usable Parakeet dictations (the 0.5 s one is skipped): not enough.
        assert_eq!(m.get("p"), None);
        assert!(!m.contains_key("gone"));
    }
}
