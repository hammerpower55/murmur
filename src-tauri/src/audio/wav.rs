//! Minimal 16 kHz mono 16 bit WAV writer. Microphone audio reaches disk only
//! through here: when the user turns on "Keep audio clips", or as test
//! fixtures in debug builds.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

use super::TARGET_RATE;

pub fn write_16k_mono(path: &Path, samples: &[f32]) -> Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let byte_rate = TARGET_RATE * 2;
    let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&TARGET_RATE.to_le_bytes());
    bytes.extend_from_slice(&byte_rate.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes()); // block align
    bytes.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    let mut file = std::fs::File::create(path)
        .with_context(|| format!("could not create {}", path.display()))?;
    file.write_all(&bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_hound() {
        let dir = std::env::temp_dir().join(format!("murmur-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        let samples: Vec<f32> = (0..1600).map(|i| (i as f32 / 50.0).sin() * 0.5).collect();
        write_16k_mono(&path, &samples).unwrap();

        let mut reader = hound::WavReader::open(&path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.sample_rate, 16_000);
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.bits_per_sample, 16);
        let read: Vec<f32> = reader
            .samples::<i16>()
            .map(|s| s.unwrap() as f32 / i16::MAX as f32)
            .collect();
        assert_eq!(read.len(), samples.len());
        assert!(read.iter().zip(&samples).all(|(a, b)| (a - b).abs() < 1e-3));
        std::fs::remove_dir_all(&dir).ok();
    }
}
