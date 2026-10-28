//! Kaldi compatible log mel filterbank, as FunASR computes it for
//! SenseVoice (`kaldi.fbank` with a Hamming window, no dither, snip edges),
//! plus the low frame rate stacking SenseVoice's encoder expects.

use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex};

const SAMPLE_RATE: f32 = 16_000.0;
const FRAME_LEN: usize = 400; // 25 ms
const FRAME_SHIFT: usize = 160; // 10 ms
const FFT_LEN: usize = 512; // frame length rounded up to a power of two
const PREEMPHASIS: f32 = 0.97;
const LOW_FREQ: f32 = 20.0;

fn mel(freq: f32) -> f32 {
    1127.0 * (1.0 + freq / 700.0).ln()
}

pub struct Fbank {
    bins: usize,
    window: Vec<f32>,
    /// Per mel bin: first FFT bin and its weights.
    filters: Vec<(usize, Vec<f32>)>,
    fft: Arc<dyn RealToComplex<f32>>,
}

impl Fbank {
    pub fn new(bins: usize) -> Self {
        let window = (0..FRAME_LEN)
            .map(|i| {
                let a = 2.0 * std::f32::consts::PI * i as f32 / (FRAME_LEN - 1) as f32;
                0.54 - 0.46 * a.cos()
            })
            .collect();

        // Triangles evenly spaced on the mel scale between 20 Hz and Nyquist.
        // Kaldi uses FFT bins 0..FFT_LEN/2 and leaves out the Nyquist bin.
        let (mel_low, mel_high) = (mel(LOW_FREQ), mel(SAMPLE_RATE / 2.0));
        let delta = (mel_high - mel_low) / (bins + 1) as f32;
        let fft_bins = FFT_LEN / 2;
        let filters = (0..bins)
            .map(|m| {
                let left = mel_low + m as f32 * delta;
                let centre = left + delta;
                let right = centre + delta;
                let mut first = None;
                let mut weights = Vec::new();
                for k in 0..fft_bins {
                    let f = mel(k as f32 * SAMPLE_RATE / FFT_LEN as f32);
                    if f > left && f < right {
                        first.get_or_insert(k);
                        weights.push(if f <= centre {
                            (f - left) / (centre - left)
                        } else {
                            (right - f) / (right - centre)
                        });
                    } else if first.is_some() {
                        break;
                    }
                }
                (first.unwrap_or(0), weights)
            })
            .collect();

        Self {
            bins,
            window,
            filters,
            fft: RealFftPlanner::<f32>::new().plan_fft_forward(FFT_LEN),
        }
    }

    /// Frames of `bins` log energies. `samples` are 16 kHz and on the scale
    /// the model was trained with (SenseVoice wants 16 bit integer range).
    pub fn compute(&self, samples: &[f32]) -> Vec<Vec<f32>> {
        if samples.len() < FRAME_LEN {
            return Vec::new();
        }
        let frames = 1 + (samples.len() - FRAME_LEN) / FRAME_SHIFT;
        let mut input = self.fft.make_input_vec();
        let mut spectrum = self.fft.make_output_vec();
        let mut power = vec![0.0f32; FFT_LEN / 2];
        let mut out = Vec::with_capacity(frames);

        for f in 0..frames {
            let frame = &samples[f * FRAME_SHIFT..f * FRAME_SHIFT + FRAME_LEN];
            let mean = frame.iter().sum::<f32>() / FRAME_LEN as f32;
            input.iter_mut().for_each(|v| *v = 0.0);
            for (i, &s) in frame.iter().enumerate() {
                input[i] = s - mean;
            }
            for i in (1..FRAME_LEN).rev() {
                input[i] -= PREEMPHASIS * input[i - 1];
            }
            input[0] -= PREEMPHASIS * input[0];
            for (v, w) in input.iter_mut().zip(&self.window) {
                *v *= w;
            }
            if self.fft.process(&mut input, &mut spectrum).is_err() {
                continue;
            }
            for (p, c) in power.iter_mut().zip(&spectrum) {
                *p = c.norm_sqr();
            }
            let row = self
                .filters
                .iter()
                .map(|(first, weights)| {
                    let energy: f32 = weights
                        .iter()
                        .zip(&power[*first..])
                        .map(|(w, p)| w * p)
                        .sum();
                    energy.max(f32::EPSILON).ln()
                })
                .collect::<Vec<f32>>();
            debug_assert_eq!(row.len(), self.bins);
            out.push(row);
        }
        out
    }
}

/// Low frame rate stacking (FunASR `apply_lfr`): every `n` frames, join `m`
/// consecutive frames into one vector. The first frame is repeated (m-1)/2
/// times at the start, and the last frame pads the end.
pub fn stack_frames(frames: &[Vec<f32>], m: usize, n: usize) -> Vec<Vec<f32>> {
    let Some(first) = frames.first() else {
        return Vec::new();
    };
    let last = frames.last().unwrap_or(first);
    let left = (m - 1) / 2;
    let padded: Vec<&Vec<f32>> = std::iter::repeat_n(first, left).chain(frames).collect();
    let out_len = frames.len().div_ceil(n);
    (0..out_len)
        .map(|i| {
            (0..m)
                .flat_map(|j| padded.get(i * n + j).copied().unwrap_or(last).iter().copied())
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, seconds: f32, amplitude: f32) -> Vec<f32> {
        (0..(SAMPLE_RATE * seconds) as usize)
            .map(|i| amplitude * (2.0 * std::f32::consts::PI * freq * i as f32 / SAMPLE_RATE).sin())
            .collect()
    }

    #[test]
    fn frame_count_snips_edges() {
        let f = Fbank::new(80);
        assert!(f.compute(&vec![0.0; 399]).is_empty());
        assert_eq!(f.compute(&vec![0.0; 400]).len(), 1);
        // One second: 1 + (16000 - 400) / 160 = 98 frames.
        let out = f.compute(&tone(440.0, 1.0, 1000.0));
        assert_eq!(out.len(), 98);
        assert!(out.iter().all(|r| r.len() == 80));
    }

    #[test]
    fn every_filter_has_weights_and_peaks_at_one() {
        let f = Fbank::new(80);
        for (m, (_, w)) in f.filters.iter().enumerate() {
            assert!(!w.is_empty(), "filter {m}");
            let peak = w.iter().cloned().fold(0.0, f32::max);
            assert!(peak > 0.4 && peak <= 1.0, "filter {m} peak {peak}");
        }
    }

    #[test]
    fn a_tone_lights_up_the_matching_filter() {
        let f = Fbank::new(80);
        let row = &f.compute(&tone(1000.0, 0.5, 8000.0))[10];
        let loudest = crate::engine::onnx::argmax(row);
        // 1 kHz on the mel scale between 20 Hz and 8 kHz.
        let expected = ((mel(1000.0) - mel(LOW_FREQ)) / ((mel(8000.0) - mel(LOW_FREQ)) / 81.0))
            .round() as usize
            - 1;
        assert!(loudest.abs_diff(expected) <= 1, "{loudest} vs {expected}");
    }

    #[test]
    fn silence_is_the_log_floor() {
        let row = &Fbank::new(80).compute(&vec![0.0; 800])[0];
        assert!(row.iter().all(|v| (*v - f32::EPSILON.ln()).abs() < 1e-3));
    }

    #[test]
    fn stacking_matches_funasr() {
        let frames: Vec<Vec<f32>> = (0..10).map(|i| vec![i as f32]).collect();
        let out = stack_frames(&frames, 7, 6);
        // ceil(10 / 6) = 2 stacked frames, each 7 frames wide.
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], vec![0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0]);
        // Starts at padded index 6 = frame 3; runs past the end and repeats 9.
        assert_eq!(out[1], vec![3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
        let short = stack_frames(&frames[..2], 7, 6);
        assert_eq!(short, vec![vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0]]);
        assert!(stack_frames(&[], 7, 6).is_empty());
    }
}
