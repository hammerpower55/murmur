//! Whisper's log mel spectrogram, as openai/whisper and Hugging Face's
//! WhisperFeatureExtractor compute it: 30 s of audio (zero padded), a
//! centred STFT with a periodic Hann window (400 samples, hop 160, reflect
//! padding), the power spectrum through librosa's Slaney mel filters, log10,
//! clamped to 8 below the loudest value, then scaled by (x + 4) / 4. The NPU
//! Whisper builds take this as their input; whisper.cpp computes its own.

use realfft::RealFftPlanner;

pub const SAMPLE_RATE: usize = 16_000;
/// Samples in Whisper's 30 s window.
pub const WINDOW_SAMPLES: usize = 30 * SAMPLE_RATE;
pub const FRAMES: usize = WINDOW_SAMPLES / HOP;
const N_FFT: usize = 400;
const HOP: usize = 160;
const BINS: usize = N_FFT / 2 + 1;

fn hz_to_mel(f: f64) -> f64 {
    // Slaney: linear below 1 kHz, logarithmic above.
    let (f_sp, min_log_hz, min_log_mel) = (200.0 / 3.0, 1000.0, 15.0);
    let logstep = 6.4f64.ln() / 27.0;
    if f >= min_log_hz {
        min_log_mel + (f / min_log_hz).ln() / logstep
    } else {
        f / f_sp
    }
}

fn mel_to_hz(m: f64) -> f64 {
    let (f_sp, min_log_hz, min_log_mel) = (200.0 / 3.0, 1000.0, 15.0);
    let logstep = 6.4f64.ln() / 27.0;
    if m >= min_log_mel {
        min_log_hz * (logstep * (m - min_log_mel)).exp()
    } else {
        f_sp * m
    }
}

/// librosa.filters.mel(sr=16000, n_fft=400, n_mels) with Slaney norm:
/// `mels` rows of BINS weights.
fn filters(mels: usize) -> Vec<Vec<f32>> {
    let fft_freqs: Vec<f64> = (0..BINS)
        .map(|i| i as f64 * (SAMPLE_RATE as f64 / 2.0) / (BINS - 1) as f64)
        .collect();
    let (lo, hi) = (hz_to_mel(0.0), hz_to_mel(SAMPLE_RATE as f64 / 2.0));
    let mel_f: Vec<f64> = (0..mels + 2)
        .map(|i| mel_to_hz(lo + (hi - lo) * i as f64 / (mels + 1) as f64))
        .collect();
    (0..mels)
        .map(|m| {
            let (left, centre, right) = (mel_f[m], mel_f[m + 1], mel_f[m + 2]);
            let norm = 2.0 / (right - left);
            fft_freqs
                .iter()
                .map(|&f| {
                    let lower = (f - left) / (centre - left);
                    let upper = (right - f) / (right - centre);
                    (lower.min(upper).max(0.0) * norm) as f32
                })
                .collect()
        })
        .collect()
}

/// [mels, FRAMES] features, row major, for up to 30 s of 16 kHz audio
/// (longer audio is cut).
pub fn log_mel(pcm: &[f32], mels: usize) -> Vec<f32> {
    let mut audio = vec![0.0f32; WINDOW_SAMPLES];
    let n = pcm.len().min(WINDOW_SAMPLES);
    audio[..n].copy_from_slice(&pcm[..n]);

    // Centre the frames: reflect N_FFT / 2 samples at each end.
    let pad = N_FFT / 2;
    let mut padded = Vec::with_capacity(WINDOW_SAMPLES + 2 * pad);
    padded.extend((1..=pad).rev().map(|i| audio[i]));
    padded.extend_from_slice(&audio);
    padded.extend((WINDOW_SAMPLES - 1 - pad..WINDOW_SAMPLES - 1).rev().map(|i| audio[i]));

    let window: Vec<f32> = (0..N_FFT)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N_FFT as f32).cos())
        .collect();
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(N_FFT);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();
    // Each triangle covers a few bins: keep only those.
    let bank: Vec<(usize, Vec<f32>)> = filters(mels)
        .into_iter()
        .map(|row| {
            let first = row.iter().position(|&w| w > 0.0).unwrap_or(0);
            let end = row.iter().rposition(|&w| w > 0.0).map_or(first, |i| i + 1);
            (first, row[first..end].to_vec())
        })
        .collect();

    // The STFT has FRAMES + 1 frames; Whisper drops the last. A frame that
    // starts past the audio sees only zeros, so it is the floor, log10 of
    // 1e-10; only frames that touch the audio are computed.
    let mut out = vec![-10.0f32; mels * FRAMES];
    let mut power = vec![0.0f32; BINS];
    let touched = (n + pad).div_ceil(HOP).min(FRAMES);
    for frame in 0..touched {
        let start = frame * HOP;
        for (i, x) in input.iter_mut().enumerate() {
            *x = padded[start + i] * window[i];
        }
        if fft.process(&mut input, &mut spectrum).is_err() {
            continue;
        }
        for (p, c) in power.iter_mut().zip(&spectrum) {
            *p = c.norm_sqr();
        }
        for (m, (first, weights)) in bank.iter().enumerate() {
            let energy: f32 = weights.iter().zip(&power[*first..]).map(|(w, p)| w * p).sum();
            out[m * FRAMES + frame] = energy.max(1e-10).log10();
        }
    }
    let top = out.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    for x in &mut out {
        *x = (x.max(top - 8.0) + 4.0) / 4.0;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slaney_scale_round_trips() {
        for f in [0.0, 300.0, 1000.0, 4000.0, 8000.0] {
            assert!((mel_to_hz(hz_to_mel(f)) - f).abs() < 1e-6, "{f}");
        }
        // Linear below 1 kHz: 1000 Hz is mel 15.
        assert!((hz_to_mel(1000.0) - 15.0).abs() < 1e-9);
    }

    #[test]
    fn filters_cover_the_band_once() {
        let bank = filters(80);
        assert_eq!((bank.len(), bank[0].len()), (80, BINS));
        // Every FFT bin above the lowest falls under at least one triangle.
        for bin in 1..BINS - 1 {
            assert!(bank.iter().any(|row| row[bin] > 0.0), "bin {bin}");
        }
        // Slaney norm: each triangle has an area of about 1 in Hz terms.
        let hz_per_bin = SAMPLE_RATE as f32 / N_FFT as f32;
        let area: f32 = bank[40].iter().sum::<f32>() * hz_per_bin;
        assert!((area - 1.0).abs() < 0.2, "{area}");
    }

    /// The plain computation: every frame, every bin.
    fn dense_log_mel(pcm: &[f32], mels: usize) -> Vec<f32> {
        let mut audio = vec![0.0f32; WINDOW_SAMPLES];
        audio[..pcm.len()].copy_from_slice(pcm);
        let pad = N_FFT / 2;
        let mut padded: Vec<f32> = (1..=pad).rev().map(|i| audio[i]).collect();
        padded.extend_from_slice(&audio);
        padded.extend((WINDOW_SAMPLES - 1 - pad..WINDOW_SAMPLES - 1).rev().map(|i| audio[i]));
        let window: Vec<f32> = (0..N_FFT)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N_FFT as f32).cos())
            .collect();
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(N_FFT);
        let (mut input, mut spectrum) = (fft.make_input_vec(), fft.make_output_vec());
        let bank = filters(mels);
        let mut out = vec![0.0f32; mels * FRAMES];
        for frame in 0..FRAMES {
            for (i, x) in input.iter_mut().enumerate() {
                *x = padded[frame * HOP + i] * window[i];
            }
            fft.process(&mut input, &mut spectrum).unwrap();
            let power: Vec<f32> = spectrum.iter().map(|c| c.norm_sqr()).collect();
            for (m, weights) in bank.iter().enumerate() {
                let energy: f32 = weights.iter().zip(&power).map(|(w, p)| w * p).sum();
                out[m * FRAMES + frame] = energy.max(1e-10).log10();
            }
        }
        let top = out.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        out.iter().map(|x| (x.max(top - 8.0) + 4.0) / 4.0).collect()
    }

    #[test]
    fn skipping_padding_matches_the_full_computation() {
        // A sweep with noise, of a length that ends mid frame.
        let pcm: Vec<f32> = (0..SAMPLE_RATE * 3 + 77)
            .map(|i| {
                let t = i as f32 / SAMPLE_RATE as f32;
                (2.0 * std::f32::consts::PI * (200.0 + 900.0 * t) * t).sin() * 0.3
                    + ((i * 7919 % 1000) as f32 / 1000.0 - 0.5) * 0.01
            })
            .collect();
        for mels in [80, 128] {
            let (fast, dense) = (log_mel(&pcm, mels), dense_log_mel(&pcm, mels));
            let worst = fast.iter().zip(&dense).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
            assert!(worst < 1e-5, "{mels} mels differ by {worst}");
        }
        assert_eq!(log_mel(&[], 128), dense_log_mel(&[], 128));
    }

    #[test]
    fn a_tone_lights_up_its_mel_band_and_silence_is_flat() {
        let tone: Vec<f32> = (0..SAMPLE_RATE)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / SAMPLE_RATE as f32).sin() * 0.5)
            .collect();
        let mel = log_mel(&tone, 80);
        assert_eq!(mel.len(), 80 * FRAMES);
        let frame = 50;
        let loudest = (0..80).max_by(|&a, &b| mel[a * FRAMES + frame].total_cmp(&mel[b * FRAMES + frame])).unwrap();
        let centre = |m: usize| mel_to_hz(hz_to_mel(0.0) + (hz_to_mel(8000.0) - hz_to_mel(0.0)) * (m + 1) as f64 / 81.0);
        assert!((centre(loudest) - 1000.0).abs() < 120.0, "band {loudest} at {} Hz", centre(loudest));
        // After the tone ends the rest is the clamped floor: top - 8, scaled.
        let floor = mel[10 * FRAMES + FRAMES - 1];
        let top = mel.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!((top - floor - 2.0).abs() < 1e-4, "top {top} floor {floor}");
    }
}
