//! Dictation state machine. Pure logic: time is passed in so every transition
//! can be unit tested without a clock, a keyboard or a microphone.

use std::time::{Duration, Instant};

/// A press shorter than this counts as a tap rather than push to talk.
pub const TAP_MAX: Duration = Duration::from_millis(250);
/// A second press within this window after a tap switches to hands free.
pub const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(400);
/// How long the Cancelled state is shown before returning to Idle.
pub const CANCELLED_HOLD: Duration = Duration::from_millis(300);
/// How long the Error state is shown before returning to Idle.
pub const ERROR_HOLD: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Recording,
    Processing,
    Cancelled,
    Error,
}

/// How the current recording ends. Only meaningful while Recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Key is held; releasing it stops.
    PushToTalk,
    /// Key was tapped; a second press within the window means hands free.
    AwaitingSecondTap,
    /// Recording until the hotkey is pressed again.
    HandsFree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    HotkeyDown,
    HotkeyUp,
    /// The toggle only hotkey: starts or stops, never push to talk.
    ToggleHotkey,
    Escape,
    /// Processing produced text.
    Finished,
    /// Processing found nothing worth keeping (too short or silent).
    Discarded,
    /// Capture or processing failed.
    Failed,
}

/// What the caller must do after a transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    None,
    StartCapture,
    /// Stop capture and process what was recorded.
    StopCapture,
    /// Stop capture and throw the audio away.
    CancelCapture,
}

#[derive(Debug)]
pub struct Machine {
    state: State,
    mode: Mode,
    pressed_at: Option<Instant>,
    deadline: Option<Instant>,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    pub fn new() -> Self {
        Self {
            state: State::Idle,
            mode: Mode::PushToTalk,
            pressed_at: None,
            deadline: None,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn mode(&self) -> Option<Mode> {
        (self.state == State::Recording).then_some(self.mode)
    }

    /// When `tick` next needs to run, if ever.
    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Fire any expired timer. Callers must tick before every `handle`.
    pub fn tick(&mut self, now: Instant) -> Effect {
        match self.deadline {
            Some(deadline) if now >= deadline => {}
            _ => return Effect::None,
        }
        self.deadline = None;
        match self.state {
            // A single short tap with no follow up is treated as accidental.
            State::Recording if self.mode == Mode::AwaitingSecondTap => {
                self.enter_hold(State::Cancelled, now);
                Effect::CancelCapture
            }
            State::Cancelled | State::Error => {
                self.state = State::Idle;
                Effect::None
            }
            _ => Effect::None,
        }
    }

    pub fn handle(&mut self, input: Input, now: Instant) -> Effect {
        use Input::*;
        use State::*;

        match (self.state, input) {
            (Idle | Cancelled | Error, HotkeyDown) => {
                self.start(Mode::PushToTalk, now);
                Effect::StartCapture
            }
            (Idle | Cancelled | Error, ToggleHotkey) => {
                self.start(Mode::HandsFree, now);
                Effect::StartCapture
            }

            (Recording, HotkeyDown) => match self.mode {
                Mode::AwaitingSecondTap => {
                    self.mode = Mode::HandsFree;
                    self.deadline = None;
                    Effect::None
                }
                Mode::HandsFree => self.stop(),
                Mode::PushToTalk => Effect::None,
            },
            (Recording, HotkeyUp) => match self.mode {
                Mode::PushToTalk => {
                    let held = self.pressed_at.map_or(Duration::MAX, |at| now - at);
                    if held < TAP_MAX {
                        self.mode = Mode::AwaitingSecondTap;
                        self.deadline = Some(now + DOUBLE_TAP_WINDOW);
                        Effect::None
                    } else {
                        self.stop()
                    }
                }
                Mode::AwaitingSecondTap | Mode::HandsFree => Effect::None,
            },
            (Recording, ToggleHotkey) => self.stop(),
            (Recording, Escape) => {
                self.enter_hold(Cancelled, now);
                Effect::CancelCapture
            }
            (Recording, Failed) => {
                self.enter_hold(Error, now);
                Effect::CancelCapture
            }

            (Processing, Finished) => {
                self.state = Idle;
                Effect::None
            }
            (Processing, Discarded) => {
                self.enter_hold(Cancelled, now);
                Effect::None
            }
            (Processing, Failed) => {
                self.enter_hold(Error, now);
                Effect::None
            }

            _ => Effect::None,
        }
    }

    /// A hands free hotkey that contains the main one (Ctrl+Shift+Win over
    /// Ctrl+Win) can only complete after or together with it, so the main
    /// hotkey has just started a push to talk recording. That recording
    /// becomes the hands free one instead of being stopped. False if there is
    /// no such recording.
    pub fn go_hands_free(&mut self) -> bool {
        let held = self.state == State::Recording && self.mode != Mode::HandsFree;
        if held {
            self.mode = Mode::HandsFree;
            self.deadline = None;
        }
        held
    }

    fn start(&mut self, mode: Mode, now: Instant) {
        self.state = State::Recording;
        self.mode = mode;
        self.pressed_at = Some(now);
        self.deadline = None;
    }

    fn stop(&mut self) -> Effect {
        self.state = State::Processing;
        self.deadline = None;
        Effect::StopCapture
    }

    fn enter_hold(&mut self, state: State, now: Instant) {
        let hold = if state == State::Error {
            ERROR_HOLD
        } else {
            CANCELLED_HOLD
        };
        self.state = state;
        self.deadline = Some(now + hold);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn a_longer_hotkey_turns_push_to_talk_hands_free() {
        let t0 = Instant::now();
        let mut m = Machine::new();
        assert_eq!(m.handle(Input::HotkeyDown, t0), Effect::StartCapture);
        assert!(m.go_hands_free());
        assert_eq!(m.mode(), Some(Mode::HandsFree));
        // Letting go of the keys keeps recording; pressing them again stops.
        assert_eq!(m.handle(Input::HotkeyUp, t0 + ms(800)), Effect::None);
        assert_eq!(m.handle(Input::HotkeyDown, t0 + ms(9000)), Effect::StopCapture);
        // Only a recording held by the main hotkey changes.
        assert!(!m.go_hands_free());
        let mut idle = Machine::new();
        assert!(!idle.go_hands_free());
    }

    fn recording(mode: Mode) -> (Machine, Instant) {
        let t0 = Instant::now();
        let mut m = Machine::new();
        match mode {
            Mode::PushToTalk => {
                m.handle(Input::HotkeyDown, t0);
            }
            Mode::AwaitingSecondTap => {
                m.handle(Input::HotkeyDown, t0);
                m.handle(Input::HotkeyUp, t0 + ms(100));
            }
            Mode::HandsFree => {
                m.handle(Input::HotkeyDown, t0);
                m.handle(Input::HotkeyUp, t0 + ms(100));
                m.handle(Input::HotkeyDown, t0 + ms(200));
            }
        }
        assert_eq!(m.mode(), Some(mode));
        (m, t0)
    }

    fn processing() -> (Machine, Instant) {
        let (mut m, t0) = recording(Mode::PushToTalk);
        assert_eq!(
            m.handle(Input::HotkeyUp, t0 + ms(2000)),
            Effect::StopCapture
        );
        (m, t0 + ms(2000))
    }

    #[test]
    fn starts_idle() {
        let m = Machine::new();
        assert_eq!(m.state(), State::Idle);
        assert_eq!(m.mode(), None);
        assert_eq!(m.deadline(), None);
    }

    #[test]
    fn idle_hotkey_down_starts_push_to_talk() {
        let mut m = Machine::new();
        assert_eq!(
            m.handle(Input::HotkeyDown, Instant::now()),
            Effect::StartCapture
        );
        assert_eq!(m.state(), State::Recording);
        assert_eq!(m.mode(), Some(Mode::PushToTalk));
    }

    #[test]
    fn idle_toggle_starts_hands_free() {
        let mut m = Machine::new();
        assert_eq!(
            m.handle(Input::ToggleHotkey, Instant::now()),
            Effect::StartCapture
        );
        assert_eq!(m.mode(), Some(Mode::HandsFree));
    }

    #[test]
    fn idle_ignores_everything_else() {
        for input in [
            Input::HotkeyUp,
            Input::Escape,
            Input::Finished,
            Input::Discarded,
            Input::Failed,
        ] {
            let mut m = Machine::new();
            assert_eq!(m.handle(input, Instant::now()), Effect::None, "{input:?}");
            assert_eq!(m.state(), State::Idle);
        }
    }

    #[test]
    fn long_hold_release_stops_and_processes() {
        let (m, _) = processing();
        assert_eq!(m.state(), State::Processing);
        assert_eq!(m.deadline(), None);
    }

    #[test]
    fn release_exactly_at_tap_max_is_push_to_talk() {
        let (mut m, t0) = recording(Mode::PushToTalk);
        assert_eq!(m.handle(Input::HotkeyUp, t0 + TAP_MAX), Effect::StopCapture);
    }

    #[test]
    fn short_tap_waits_for_second_tap() {
        let (m, t0) = recording(Mode::AwaitingSecondTap);
        assert_eq!(m.state(), State::Recording);
        assert_eq!(m.deadline(), Some(t0 + ms(100) + DOUBLE_TAP_WINDOW));
    }

    #[test]
    fn second_tap_switches_to_hands_free() {
        let (m, _) = recording(Mode::HandsFree);
        assert_eq!(m.state(), State::Recording);
        assert_eq!(m.deadline(), None);
    }

    #[test]
    fn single_tap_times_out_to_cancelled() {
        let (mut m, t0) = recording(Mode::AwaitingSecondTap);
        let expiry = t0 + ms(100) + DOUBLE_TAP_WINDOW;
        assert_eq!(m.tick(expiry - ms(1)), Effect::None);
        assert_eq!(m.state(), State::Recording);
        assert_eq!(m.tick(expiry), Effect::CancelCapture);
        assert_eq!(m.state(), State::Cancelled);
    }

    #[test]
    fn hands_free_ignores_key_up() {
        let (mut m, t0) = recording(Mode::HandsFree);
        assert_eq!(m.handle(Input::HotkeyUp, t0 + ms(5000)), Effect::None);
        assert_eq!(m.mode(), Some(Mode::HandsFree));
    }

    #[test]
    fn hands_free_next_press_stops() {
        let (mut m, t0) = recording(Mode::HandsFree);
        assert_eq!(
            m.handle(Input::HotkeyDown, t0 + ms(5000)),
            Effect::StopCapture
        );
        assert_eq!(m.state(), State::Processing);
    }

    #[test]
    fn push_to_talk_ignores_repeat_down() {
        let (mut m, t0) = recording(Mode::PushToTalk);
        assert_eq!(m.handle(Input::HotkeyDown, t0 + ms(500)), Effect::None);
        assert_eq!(m.mode(), Some(Mode::PushToTalk));
    }

    #[test]
    fn toggle_stops_any_recording() {
        for mode in [Mode::PushToTalk, Mode::AwaitingSecondTap, Mode::HandsFree] {
            let (mut m, t0) = recording(mode);
            assert_eq!(
                m.handle(Input::ToggleHotkey, t0 + ms(300)),
                Effect::StopCapture
            );
            assert_eq!(m.state(), State::Processing, "{mode:?}");
        }
    }

    #[test]
    fn escape_cancels_any_recording() {
        for mode in [Mode::PushToTalk, Mode::AwaitingSecondTap, Mode::HandsFree] {
            let (mut m, t0) = recording(mode);
            assert_eq!(m.handle(Input::Escape, t0 + ms(300)), Effect::CancelCapture);
            assert_eq!(m.state(), State::Cancelled, "{mode:?}");
            assert_eq!(m.deadline(), Some(t0 + ms(300) + CANCELLED_HOLD));
        }
    }

    #[test]
    fn capture_failure_while_recording_goes_to_error() {
        let (mut m, t0) = recording(Mode::PushToTalk);
        assert_eq!(m.handle(Input::Failed, t0), Effect::CancelCapture);
        assert_eq!(m.state(), State::Error);
        assert_eq!(m.deadline(), Some(t0 + ERROR_HOLD));
    }

    #[test]
    fn processing_finished_returns_to_idle() {
        let (mut m, t) = processing();
        assert_eq!(m.handle(Input::Finished, t), Effect::None);
        assert_eq!(m.state(), State::Idle);
    }

    #[test]
    fn processing_discarded_goes_to_cancelled() {
        let (mut m, t) = processing();
        m.handle(Input::Discarded, t);
        assert_eq!(m.state(), State::Cancelled);
        assert_eq!(m.deadline(), Some(t + CANCELLED_HOLD));
    }

    #[test]
    fn processing_failed_goes_to_error() {
        let (mut m, t) = processing();
        m.handle(Input::Failed, t);
        assert_eq!(m.state(), State::Error);
        assert_eq!(m.deadline(), Some(t + ERROR_HOLD));
    }

    #[test]
    fn processing_ignores_hotkeys_and_escape() {
        for input in [
            Input::HotkeyDown,
            Input::HotkeyUp,
            Input::ToggleHotkey,
            Input::Escape,
        ] {
            let (mut m, t) = processing();
            assert_eq!(m.handle(input, t), Effect::None, "{input:?}");
            assert_eq!(m.state(), State::Processing);
        }
    }

    #[test]
    fn cancelled_returns_to_idle_after_hold() {
        let (mut m, t) = processing();
        m.handle(Input::Discarded, t);
        assert_eq!(m.tick(t + CANCELLED_HOLD - ms(1)), Effect::None);
        assert_eq!(m.state(), State::Cancelled);
        assert_eq!(m.tick(t + CANCELLED_HOLD), Effect::None);
        assert_eq!(m.state(), State::Idle);
        assert_eq!(m.deadline(), None);
    }

    #[test]
    fn error_returns_to_idle_after_hold() {
        let (mut m, t) = processing();
        m.handle(Input::Failed, t);
        m.tick(t + ERROR_HOLD);
        assert_eq!(m.state(), State::Idle);
    }

    #[test]
    fn hotkey_during_cancelled_or_error_starts_new_recording() {
        for failure in [Input::Discarded, Input::Failed] {
            let (mut m, t) = processing();
            m.handle(failure, t);
            assert_eq!(
                m.handle(Input::HotkeyDown, t + ms(10)),
                Effect::StartCapture
            );
            assert_eq!(m.mode(), Some(Mode::PushToTalk));
            assert_eq!(m.deadline(), None);
        }
    }

    #[test]
    fn toggle_during_cancelled_starts_hands_free() {
        let (mut m, t) = processing();
        m.handle(Input::Discarded, t);
        assert_eq!(m.handle(Input::ToggleHotkey, t), Effect::StartCapture);
        assert_eq!(m.mode(), Some(Mode::HandsFree));
    }

    #[test]
    fn tick_without_deadline_does_nothing() {
        let (mut m, t0) = recording(Mode::PushToTalk);
        assert_eq!(m.tick(t0 + ms(60_000)), Effect::None);
        assert_eq!(m.state(), State::Recording);
    }
}
