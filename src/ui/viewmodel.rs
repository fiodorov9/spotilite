//! What the window shows, worked out from the engine's events and the state machine.
//!
//! Pure logic, with no window, no network and no audio, so the tests run anywhere.

use std::time::Duration;

use crate::state::Status;

use super::engine::{Command, EngineEvent, Problem};

/// The track the window shows as playing, paused or about to play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowPlaying {
    pub title: String,
    /// The artists joined by commas, or the podcast name; empty when unknown.
    pub artists: String,
    /// Length as `m:ss`, `-:--` when unknown.
    pub duration: String,
}

/// One line of the queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueRow {
    /// Position in the queue, from 1.
    pub number: usize,
    pub label: String,
    /// The current track, highlighted.
    pub current: bool,
    /// A request that waits for the session.
    pub pending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

/// A message for the user, about a command or a track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub level: Level,
    pub text: String,
}

/// The window's view of the engine: one per window, fed with every [`EngineEvent`].
#[derive(Debug, Default)]
pub struct ViewModel {}

impl ViewModel {
    pub fn new() -> Self {
        todo!("F4-2")
    }

    /// Takes in an event of the engine.
    pub fn apply(&mut self, _event: EngineEvent) {
        todo!("F4-2")
    }

    /// One line about the session: starting, connecting, signed in, or why not.
    pub fn connection_label(&self) -> String {
        todo!("F4-2")
    }

    /// Whether the session could not be opened, so the window offers to retry.
    pub fn is_offline(&self) -> bool {
        todo!("F4-2")
    }

    /// Whether the window should invite the user to run `spotilite login`.
    pub fn needs_login(&self) -> bool {
        todo!("F4-2")
    }

    /// The current track; `None` when there is none.
    pub fn now_playing(&self) -> Option<NowPlaying> {
        todo!("F4-2")
    }

    pub fn status(&self) -> Status {
        todo!("F4-2")
    }

    /// `Fermo`, `In riproduzione`, `In pausa`, or `Caricamento...` while the track loads.
    pub fn status_label(&self) -> &'static str {
        todo!("F4-2")
    }

    /// Label of the play/pause button: what pressing it does.
    pub fn play_label(&self) -> &'static str {
        todo!("F4-2")
    }

    /// `position / duration` as `m:ss / m:ss`.
    pub fn position_label(&self) -> String {
        todo!("F4-2")
    }

    /// How far the current track has played, from 0 to 1.
    pub fn progress(&self) -> f32 {
        todo!("F4-2")
    }

    pub fn volume(&self) -> u8 {
        todo!("F4-2")
    }

    /// The queue, then the requests that wait for the session.
    pub fn queue_rows(&self) -> Vec<QueueRow> {
        todo!("F4-2")
    }

    /// The latest message for the user, until it is dismissed.
    pub fn notice(&self) -> Option<&Notice> {
        todo!("F4-2")
    }

    pub fn dismiss_notice(&mut self) {
        todo!("F4-2")
    }

    /// The command for the volume slider, cut to 100; the slider shows it at once.
    pub fn set_volume(&mut self, _volume: u8) -> Command {
        todo!("F4-2")
    }

    /// The command that queues what the user typed, or `None` with a warning when it
    /// cannot be played. Empty text is ignored.
    pub fn submit_uri(&mut self, _text: &str) -> Option<Command> {
        todo!("F4-2")
    }
}

/// `m:ss`, with the minutes running past 59.
pub fn format_time(_duration: Duration) -> String {
    todo!("F4-2")
}

/// A sentence for the user about `problem`, saying what to do when there is something
/// to do.
pub fn problem_text(_problem: &Problem) -> String {
    todo!("F4-2")
}
