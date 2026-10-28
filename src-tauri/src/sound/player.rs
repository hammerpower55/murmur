//! UI sounds (spec 6.5) through rodio on the default output device. The
//! output stays open on its own thread so a cue starts at once instead of
//! paying 100 ms or more to open a device on every key press.

use std::io::Cursor;
use std::sync::mpsc::{self, Receiver, Sender};

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

pub struct SoundPlayer {
    tx: Option<Sender<Cue>>,
}

impl SoundPlayer {
    /// `volume` is 0 to 100. Muted players start no thread and play nothing.
    pub fn spawn(volume: u8, muted: bool) -> Self {
        if muted || volume == 0 {
            return Self { tx: None };
        }
        let gain = volume.min(100) as f32 / 100.0;
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("murmur-sound".into())
            .spawn(move || run(rx, gain));
        if let Err(err) = spawned {
            log::warn!("sounds disabled: {err}");
            return Self { tx: None };
        }
        Self { tx: Some(tx) }
    }

    /// Queues a cue. Returns immediately; playback starts within a few ms.
    pub fn play(&self, cue: Cue) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(cue);
        }
    }
}

fn run(rx: Receiver<Cue>, gain: f32) {
    let mut sink = match DeviceSinkBuilder::open_default_sink() {
        Ok(sink) => sink,
        Err(err) => {
            log::warn!("no sound output: {err}");
            // Keep draining so senders never block or error.
            while rx.recv().is_ok() {}
            return;
        }
    };
    sink.log_on_drop(false);

    while let Ok(cue) = rx.recv() {
        match Decoder::new(Cursor::new(cue.bytes())) {
            Ok(source) => sink.mixer().add(source.amplify(gain)),
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
        let p = SoundPlayer::spawn(60, true);
        assert!(p.tx.is_none());
        p.play(Cue::Start);
    }
}
