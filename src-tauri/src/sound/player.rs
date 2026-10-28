//! UI sounds (spec 6.5) through rodio on the default output device. The
//! output stays open on its own thread so a cue starts at once instead of
//! paying 100 ms or more to open a device on every key press.

use std::io::Cursor;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

use rodio::{Decoder, DeviceSinkBuilder, Source};

const START: &[u8] = include_bytes!("../../sounds/start.wav");
const STOP: &[u8] = include_bytes!("../../sounds/stop.wav");
const CANCEL: &[u8] = include_bytes!("../../sounds/cancel.wav");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    Start,
    Stop,
    Cancel,
}

impl Cue {
    fn bytes(self) -> &'static [u8] {
        match self {
            Cue::Start => START,
            Cue::Stop => STOP,
            Cue::Cancel => CANCEL,
        }
    }
}

enum Message {
    /// A cue and its gain, 0 to 1.
    Play(Cue, f32),
    /// Open the output device now so the next cue starts at once.
    Warm,
}

/// Cheap to clone; every clone drives the same output.
#[derive(Clone)]
pub struct SoundPlayer {
    tx: Option<Sender<Message>>,
    /// Volume 0 to 100, 0 when sounds are off.
    volume: Arc<AtomicU32>,
}

/// Effective volume: 0 when muted.
fn level(volume: u32, enabled: bool) -> u32 {
    if enabled {
        volume.min(100)
    } else {
        0
    }
}

impl SoundPlayer {
    /// `volume` is 0 to 100. The output device opens only once sounds are on.
    pub fn spawn(volume: u32, enabled: bool) -> Self {
        let volume = Arc::new(AtomicU32::new(level(volume, enabled)));
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("murmur-sound".into())
            .spawn(move || run(rx));
        if let Err(err) = spawned {
            log::warn!("sounds disabled: {err}");
            return Self { tx: None, volume };
        }
        let player = Self {
            tx: Some(tx),
            volume,
        };
        player.warm();
        player
    }

    pub fn set(&self, volume: u32, enabled: bool) {
        self.volume.store(level(volume, enabled), Ordering::SeqCst);
        self.warm();
    }

    fn warm(&self) {
        if self.volume.load(Ordering::SeqCst) > 0 {
            if let Some(tx) = &self.tx {
                let _ = tx.send(Message::Warm);
            }
        }
    }

    /// Queues a cue. Returns immediately; playback starts within a few ms.
    pub fn play(&self, cue: Cue) {
        if self.volume.load(Ordering::SeqCst) == 0 {
            return;
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(Message::Play(cue, self.gain()));
        }
    }

    fn gain(&self) -> f32 {
        self.volume.load(Ordering::SeqCst) as f32 / 100.0
    }
}

fn run(rx: Receiver<Message>) {
    let mut sink: Option<rodio::MixerDeviceSink> = None;
    let mut failed = false;
    while let Ok(message) = rx.recv() {
        if sink.is_none() && !failed {
            match DeviceSinkBuilder::open_default_sink() {
                Ok(mut s) => {
                    s.log_on_drop(false);
                    sink = Some(s);
                }
                Err(err) => {
                    // Keep draining so senders never block or error.
                    log::warn!("no sound output: {err}");
                    failed = true;
                }
            }
        }
        let (Message::Play(cue, gain), Some(s)) = (message, sink.as_ref()) else {
            continue;
        };
        match Decoder::new(Cursor::new(cue.bytes())) {
            Ok(source) => s.mixer().add(source.amplify(gain)),
            Err(err) => log::warn!("could not play {cue:?}: {err}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_sounds_decode_with_the_spec_lengths() {
        for (cue, ms) in [(Cue::Start, 140), (Cue::Stop, 140), (Cue::Cancel, 90)] {
            let source = Decoder::new(Cursor::new(cue.bytes())).unwrap();
            let duration = source.total_duration().unwrap().as_millis();
            assert!((duration as i64 - ms).abs() <= 1, "{cue:?}: {duration} ms");
        }
    }

    #[test]
    fn muted_player_is_silent_and_never_fails() {
        let p = SoundPlayer::spawn(60, false);
        assert_eq!(p.gain(), 0.0);
        p.play(Cue::Start);
        p.set(80, true);
        assert!((p.gain() - 0.8).abs() < 1e-6);
        p.set(150, true);
        assert_eq!(p.gain(), 1.0);
        p.set(80, false);
        assert_eq!(p.gain(), 0.0);
    }
}
