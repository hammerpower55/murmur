//! Microphone level for the overlay waveform: RMS and peak over each 50 ms
//! of audio, mapped from decibels onto 0 to 1 so quiet speech still moves
//! the bars. -50 dBFS (room noise) is 0, -10 dBFS (loud speech) is 1.

use serde::Serialize;

pub const WINDOW_MS: u32 = 50;
const FLOOR_DB: f32 = -50.0;
const CEILING_DB: f32 = -10.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Level {
    pub rms: f32,
    pub peak: f32,
}

pub struct LevelMeter {
    window: usize,
    count: usize,
    sum_sq: f64,
    peak: f32,
}

impl LevelMeter {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            window: (sample_rate * WINDOW_MS / 1000).max(1) as usize,
            count: 0,
            sum_sq: 0.0,
            peak: 0.0,
        }
    }

    /// Feeds samples in; returns one level for every full 50 ms window.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Level> {
        let mut out = Vec::new();
        for &s in samples {
            self.sum_sq += (s as f64) * (s as f64);
            self.peak = self.peak.max(s.abs());
            self.count += 1;
            if self.count == self.window {
                let rms = (self.sum_sq / self.count as f64).sqrt() as f32;
                out.push(Level {
                    rms: to_display(rms),
                    peak: to_display(self.peak),
                });
                self.count = 0;
                self.sum_sq = 0.0;
                self.peak = 0.0;
            }
        }
        out
    }
}

fn to_display(amplitude: f32) -> f32 {
    if amplitude <= 1e-6 {
        return 0.0;
    }
    let db = 20.0 * amplitude.log10();
    ((db - FLOOR_DB) / (CEILING_DB - FLOOR_DB)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(amplitude: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (i as f32 * 0.2).sin() * amplitude).collect()
    }

    #[test]
    fn one_level_per_50_ms_window() {
        let mut m = LevelMeter::new(48_000);
        assert!(m.push(&vec![0.0; 2399]).is_empty());
        assert_eq!(m.push(&[0.0]).len(), 1);
        assert_eq!(m.push(&vec![0.0; 2400 * 3 + 10]).len(), 3);
    }

    #[test]
    fn silence_is_zero() {
        let mut m = LevelMeter::new(16_000);
        let l = m.push(&vec![0.0; 800])[0];
        assert_eq!((l.rms, l.peak), (0.0, 0.0));
    }

    #[test]
    fn loud_speech_saturates() {
        let mut m = LevelMeter::new(16_000);
        let l = m.push(&sine(0.9, 800))[0];
        assert_eq!(l.rms, 1.0);
        assert_eq!(l.peak, 1.0);
    }

    #[test]
    fn scale_is_logarithmic() {
        // A sine's RMS is amplitude / sqrt(2): -30 dBFS RMS sits at 0.5.
        let amplitude = 10f32.powf(-30.0 / 20.0) * std::f32::consts::SQRT_2;
        let mut m = LevelMeter::new(16_000);
        let l = m.push(&sine(amplitude, 1600))[0];
        assert!((l.rms - 0.5).abs() < 0.02, "{}", l.rms);
        assert!(l.peak > l.rms);
    }

    #[test]
    fn room_noise_reads_as_near_zero() {
        let mut m = LevelMeter::new(16_000);
        let l = m.push(&sine(0.002, 800))[0];
        assert!(l.rms < 0.05, "{}", l.rms);
    }
}
