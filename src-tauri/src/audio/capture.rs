//! Microphone capture. The cpal stream lives on its own thread. It is built
//! ahead of time and kept paused between dictations, because building a
//! WASAPI stream takes 250 to 500 ms while starting a built one is instant.
//! A paused stream does not capture, so the mic is idle between dictations.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};

use super::{resample, TARGET_RATE};

/// How often the capture thread drains the ring buffer while recording.
const DRAIN_INTERVAL: Duration = Duration::from_millis(10);
/// Ring buffer size in seconds of audio. Far more than one drain interval.
const RING_SECONDS: usize = 2;

/// One finished recording, 16 kHz mono.
#[derive(Debug)]
pub struct Recording {
    pub samples: Vec<f32>,
    pub device_rate: u32,
}

impl Recording {
    pub fn duration_ms(&self) -> u64 {
        self.samples.len() as u64 * 1000 / TARGET_RATE as u64
    }
}

enum Command {
    Prepare,
    Start(Sender<Result<()>>),
    Stop(Sender<Result<Recording>>),
    Cancel,
}

/// Handle to the capture thread.
pub struct Recorder {
    tx: Sender<Command>,
}

impl Recorder {
    /// Starts the capture thread and builds the stream in the background.
    pub fn spawn() -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("murmur-audio".into())
            .spawn(move || run(rx))
            .context("could not start audio thread")?;
        let _ = tx.send(Command::Prepare);
        Ok(Self { tx })
    }

    /// Starts recording. Returns once the microphone is running.
    pub fn start(&self) -> Result<()> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(Command::Start(reply))
            .map_err(|_| anyhow!("audio thread stopped"))?;
        rx.recv().map_err(|_| anyhow!("audio thread stopped"))?
    }

    /// Stops recording and returns everything recorded at 16 kHz.
    pub fn stop(&self) -> Result<Recording> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(Command::Stop(reply))
            .map_err(|_| anyhow!("audio thread stopped"))?;
        rx.recv().map_err(|_| anyhow!("audio thread stopped"))?
    }

    /// Stops recording and drops the audio.
    pub fn cancel(&self) {
        let _ = self.tx.send(Command::Cancel);
    }
}

fn run(rx: Receiver<Command>) {
    let mut mic: Option<Mic> = None;
    // Device rate samples of the recording in progress, if any.
    let mut take: Option<Vec<f32>> = None;

    loop {
        let command = if take.is_some() {
            match rx.recv_timeout(DRAIN_INTERVAL) {
                Ok(command) => Some(command),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(command) => Some(command),
                Err(_) => break,
            }
        };

        if let (Some(m), Some(samples)) = (mic.as_mut(), take.as_mut()) {
            m.drain_into(samples);
        }

        match command {
            Some(Command::Prepare) => {
                if let Err(err) = ensure_mic(&mut mic) {
                    // Not fatal: the next Start tries again and reports it.
                    log::warn!("microphone not ready: {err:#}");
                }
            }
            Some(Command::Start(reply)) => {
                let result = ensure_mic(&mut mic).and_then(|m| m.start());
                if result.is_ok() {
                    take = Some(Vec::new());
                } else {
                    // Drop it so the next attempt starts from scratch.
                    mic = None;
                }
                let _ = reply.send(result);
            }
            Some(Command::Stop(reply)) => {
                let result = match (mic.as_mut(), take.take()) {
                    (Some(m), Some(mut samples)) => {
                        m.stop_into(&mut samples);
                        resample::to_16k(&samples, m.rate).map(|samples| Recording {
                            samples,
                            device_rate: m.rate,
                        })
                    }
                    _ => Err(anyhow!("not recording")),
                };
                let _ = reply.send(result);
            }
            Some(Command::Cancel) => {
                take = None;
                if let Some(m) = mic.as_mut() {
                    m.pause();
                }
            }
            None => {}
        }
    }
}

/// Returns a usable stream, rebuilding it if there is none or the old one
/// was invalidated (default device changed, device unplugged).
fn ensure_mic(mic: &mut Option<Mic>) -> Result<&mut Mic> {
    if mic
        .as_ref()
        .is_some_and(|m| m.invalid.load(Ordering::SeqCst))
    {
        log::info!("microphone changed, reopening");
        *mic = None;
    }
    if mic.is_none() {
        *mic = Some(Mic::open()?);
    }
    mic.as_mut().ok_or_else(|| anyhow!("no microphone"))
}

struct Mic {
    stream: Stream,
    consumer: HeapCons<f32>,
    rate: u32,
    invalid: Arc<AtomicBool>,
}

impl Mic {
    fn open() -> Result<Self> {
        let started = Instant::now();
        let device = cpal::default_host()
            .default_input_device()
            .context("no microphone found")?;
        let supported = device
            .default_input_config()
            .context("could not read microphone settings")?;
        let rate = supported.sample_rate();
        let channels = supported.channels() as usize;
        let format = supported.sample_format();
        let config: StreamConfig = supported.into();

        let invalid = Arc::new(AtomicBool::new(false));
        let (producer, consumer) = HeapRb::<f32>::new(rate as usize * RING_SECONDS).split();
        let flag = invalid.clone();
        let stream = match format {
            SampleFormat::F32 => build::<f32>(&device, config, channels, producer, flag),
            SampleFormat::I16 => build::<i16>(&device, config, channels, producer, flag),
            SampleFormat::I32 => build::<i32>(&device, config, channels, producer, flag),
            SampleFormat::U16 => build::<u16>(&device, config, channels, producer, flag),
            SampleFormat::I8 => build::<i8>(&device, config, channels, producer, flag),
            SampleFormat::U8 => build::<u8>(&device, config, channels, producer, flag),
            other => Err(anyhow!("unsupported microphone format {other}")),
        }?;
        // Some hosts start streams on creation; make sure it is idle.
        stream.pause().ok();

        let name = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| "unknown".into());
        log::info!(
            "mic ready in {} ms: {name}, {rate} Hz, {channels} ch, {format}",
            started.elapsed().as_millis()
        );

        Ok(Self {
            stream,
            consumer,
            rate,
            invalid,
        })
    }

    fn start(&mut self) -> Result<()> {
        // Throw away anything left over from the previous dictation.
        self.consumer.clear();
        self.stream.play().context("could not start the microphone")
    }

    /// Pauses the device, then collects what is still in the ring buffer so
    /// nothing arrives after the final drain.
    fn stop_into(&mut self, samples: &mut Vec<f32>) {
        self.pause();
        self.drain_into(samples);
    }

    fn pause(&mut self) {
        if let Err(err) = self.stream.pause() {
            log::warn!("could not pause the microphone: {err}");
            self.invalid.store(true, Ordering::SeqCst);
        }
    }

    fn drain_into(&mut self, samples: &mut Vec<f32>) {
        let mut chunk = [0.0f32; 1024];
        loop {
            let n = self.consumer.pop_slice(&mut chunk);
            if n == 0 {
                break;
            }
            samples.extend_from_slice(&chunk[..n]);
        }
    }
}

/// Builds an input stream that downmixes to mono f32 inside the callback and
/// pushes into the ring buffer. No allocation or locking on the audio thread.
fn build<T>(
    device: &cpal::Device,
    config: StreamConfig,
    channels: usize,
    mut producer: HeapProd<f32>,
    invalid: Arc<AtomicBool>,
) -> Result<Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let scale = 1.0 / channels as f32;
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            for frame in data.chunks(channels) {
                let sum: f32 = frame.iter().map(|s| s.to_sample::<f32>()).sum();
                let _ = producer.try_push(sum * scale);
            }
        },
        move |err| match err.kind() {
            // WASAPI flags a discontinuity on the first packet after every start.
            ErrorKind::Xrun => log::debug!("microphone glitch: {err}"),
            _ => {
                log::warn!("microphone stream error: {err}");
                invalid.store(true, Ordering::SeqCst);
            }
        },
        None,
    )?;
    Ok(stream)
}
