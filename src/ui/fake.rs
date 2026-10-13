//! A Spotify session that never leaves the computer, for the tests of the engine and of
//! the window: the fake player records what the engine asks of it and plays back the
//! events the test feeds it.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use super::engine::{Backend, Connector, EngineHandle, Problem};
use super::viewmodel::ViewModel;
use crate::player::{PlaybackEvent, PlayerError};
use crate::state::PlaybackEffect;

/// How long a test waits for the engine before it gives up.
const PATIENCE: Duration = Duration::from_secs(5);

/// What the engine asked of the fake player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Apply(PlaybackEffect),
    Preload(String),
}

/// Hands out the outcomes it was given, one per connection; then never connects again.
pub struct FakeConnector {
    outcomes: Mutex<VecDeque<Result<FakeBackend, Problem>>>,
}

impl FakeConnector {
    /// Connects at once as `username`, to a player that `FakeControls` drives.
    pub fn connected(username: &str) -> (Self, FakeControls) {
        let (backend, controls) = FakeBackend::new(username);
        let connector = Self {
            outcomes: Mutex::new(VecDeque::from([Ok(backend)])),
        };
        (connector, controls)
    }
}

impl Connector for FakeConnector {
    type Backend = FakeBackend;

    async fn connect(&self) -> Result<FakeBackend, Problem> {
        let outcome = self.outcomes.lock().expect("outcomes").pop_front();
        match outcome {
            Some(outcome) => outcome,
            None => std::future::pending().await,
        }
    }
}

pub struct FakeBackend {
    username: String,
    calls: Arc<Mutex<Vec<Call>>>,
    events: mpsc::UnboundedReceiver<PlaybackEvent>,
    position: Arc<Mutex<Duration>>,
    dropped: Arc<AtomicBool>,
}

impl FakeBackend {
    fn new(username: &str) -> (Self, FakeControls) {
        let (sender, events) = mpsc::unbounded_channel();
        let controls = FakeControls {
            calls: Arc::default(),
            events: sender,
            position: Arc::default(),
            dropped: Arc::default(),
        };
        let backend = Self {
            username: username.to_string(),
            calls: Arc::clone(&controls.calls),
            events,
            position: Arc::clone(&controls.position),
            dropped: Arc::clone(&controls.dropped),
        };
        (backend, controls)
    }

    fn record(&self, call: Call) {
        self.calls.lock().expect("calls").push(call);
    }
}

impl Backend for FakeBackend {
    fn username(&self) -> String {
        self.username.clone()
    }

    fn apply(&self, effect: &PlaybackEffect) -> Result<(), PlayerError> {
        self.record(Call::Apply(effect.clone()));
        Ok(())
    }

    fn preload(&self, uri: &str) -> Result<(), PlayerError> {
        self.record(Call::Preload(uri.to_string()));
        Ok(())
    }

    fn position(&self) -> Duration {
        *self.position.lock().expect("position")
    }

    async fn next_event(&mut self) -> Option<PlaybackEvent> {
        self.events.recv().await
    }
}

impl Drop for FakeBackend {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

/// The test's side of the fake player.
pub struct FakeControls {
    calls: Arc<Mutex<Vec<Call>>>,
    events: mpsc::UnboundedSender<PlaybackEvent>,
    position: Arc<Mutex<Duration>>,
    dropped: Arc<AtomicBool>,
}

impl FakeControls {
    /// Everything the engine asked of the player so far.
    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().expect("calls").clone()
    }

    /// The calls that change playback: not the volume, which the engine sets at every new
    /// session, nor the effects that ask for nothing.
    pub fn playback_calls(&self) -> Vec<Call> {
        let calls = self.calls();
        calls
            .into_iter()
            .filter(|call| {
                !matches!(
                    call,
                    Call::Apply(PlaybackEffect::SetVolume(_) | PlaybackEffect::None)
                )
            })
            .collect()
    }

    /// Waits until the engine has made `call`.
    pub fn wait_for(&self, call: &Call) {
        let deadline = Instant::now() + PATIENCE;
        while !self.calls().contains(call) {
            assert!(Instant::now() < deadline, "{call:?} never came, calls: {:?}", self.calls());
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Plays an event of the player to the engine.
    pub fn emit(&self, event: PlaybackEvent) {
        self.events.send(event).expect("the engine must be listening");
    }

    pub fn set_position(&self, position: Duration) {
        *self.position.lock().expect("position") = position;
    }

    /// Whether the engine has let go of the player.
    pub fn dropped(&self) -> bool {
        self.dropped.load(Ordering::SeqCst)
    }
}

/// Feeds the engine's events to `view` until `done` holds; fails the test after a while.
pub fn pump_until(engine: &EngineHandle, view: &mut ViewModel, what: &str, done: impl Fn(&ViewModel) -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !done(view) {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = engine
            .wait_event(left)
            .unwrap_or_else(|| panic!("the engine never got to: {what}"));
        view.apply(event);
    }
}
