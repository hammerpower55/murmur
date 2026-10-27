use anyhow::Result;
use rubato::audioadapter::Adapter;
use rubato::audioadapter_buffers::direct::SequentialSlice;
use rubato::{Fft, FixedSync, Resampler};

use super::TARGET_RATE;

/// Resamples a whole mono clip to 16 kHz. Runs once per dictation, after
/// capture stops, so the fixed ratio FFT resampler is the right fit.
pub fn to_16k(input: &[f32], rate: u32) -> Result<Vec<f32>> {
    if rate == TARGET_RATE || input.is_empty() {
        return Ok(input.to_vec());
    }
    let mut resampler = Fft::<f32>::new(
        rate as usize,
        TARGET_RATE as usize,
        1024,
        1,
        FixedSync::Input,
    )?;
    let adapter = SequentialSlice::new(input, 1, input.len())?;
    let output = resampler.process_all(&adapter, input.len(), None)?;
    let frames = output.frames();
    let mut samples = output.take_data();
    samples.truncate(frames);
    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, seconds: f32, freq: f32) -> Vec<f32> {
        let n = (rate as f32 * seconds) as usize;
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin() * 0.5)
            .collect()
    }

    #[test]
    fn length_matches_duration_for_common_rates() {
        for rate in [8_000, 22_050, 44_100, 48_000, 96_000] {
            let out = to_16k(&sine(rate, 2.0, 440.0), rate).unwrap();
            let expected = 32_000i64;
            assert!(
                (out.len() as i64 - expected).abs() <= 1,
                "rate {rate}: {} samples",
                out.len()
            );
        }
    }

    #[test]
    fn preserves_a_tone_below_nyquist() {
        let out = to_16k(&sine(48_000, 1.0, 440.0), 48_000).unwrap();
        // Skip the edges, compare against the ideal 16 kHz sine.
        let expected = sine(16_000, 1.0, 440.0);
        let worst = (2000..14_000)
            .map(|i| (out[i] - expected[i]).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 0.01, "max error {worst}");
    }

    #[test]
    fn passes_16k_through_untouched() {
        let input = sine(16_000, 0.5, 300.0);
        assert_eq!(to_16k(&input, 16_000).unwrap(), input);
    }

    #[test]
    fn empty_input_is_empty() {
        assert!(to_16k(&[], 48_000).unwrap().is_empty());
    }
}
