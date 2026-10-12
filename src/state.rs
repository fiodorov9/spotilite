//! Playback state machine: the queue, the current track, play/pause and volume.
//!
//! Pure logic, with no network and no audio device, so the tests run anywhere. Every
//! command changes the state and returns a [`PlaybackEffect`] that tells the audio
//! side what to do; the user interface and the player consume that outcome.

use thiserror::Error;

/// Highest volume accepted by [`PlaybackState::set_volume`].
pub const MAX_VOLUME: u8 = 100;

/// Volume at startup: no attenuation, the system volume stays in charge.
pub const DEFAULT_VOLUME: u8 = MAX_VOLUME;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Stopped,
    Playing,
    Paused,
}

/// What the audio side has to do after a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybackEffect {
    /// Nothing to change: the command was already satisfied.
    None,
    /// Load this track and play it from the beginning.
    StartTrack(String),
    /// Resume the paused track.
    Resume,
    /// Pause the current track.
    Pause,
    /// Stop playback and unload the track.
    Stop,
    /// Apply this volume, from 0 to [`MAX_VOLUME`].
    SetVolume(u8),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StateError {
    #[error("la coda è vuota")]
    EmptyQueue,
    #[error("indirizzo del brano vuoto")]
    EmptyUri,
}

pub type CommandResult = Result<PlaybackEffect, StateError>;

/// The queue with a cursor on the current track, the playback status and the volume.
#[derive(Debug, Clone)]
pub struct PlaybackState {}

impl PlaybackState {
    /// Stopped, with an empty queue and the volume at [`DEFAULT_VOLUME`].
    pub fn new() -> Self {
        todo!("F2-2")
    }

    pub fn status(&self) -> Status {
        todo!("F2-2")
    }

    pub fn queue(&self) -> &[String] {
        todo!("F2-2")
    }

    /// Position in the queue of the current track.
    pub fn current_index(&self) -> Option<usize> {
        todo!("F2-2")
    }

    /// The track playing, paused, or that `play()` would start after `stop()`.
    pub fn current(&self) -> Option<&str> {
        todo!("F2-2")
    }

    pub fn volume(&self) -> u8 {
        todo!("F2-2")
    }

    /// Appends `uri` to the queue and, when stopped, starts playing.
    pub fn enqueue(&mut self, _uri: impl Into<String>) -> CommandResult {
        todo!("F2-2")
    }

    pub fn play(&mut self) -> CommandResult {
        todo!("F2-2")
    }

    pub fn pause(&mut self) -> CommandResult {
        todo!("F2-2")
    }

    pub fn stop(&mut self) -> CommandResult {
        todo!("F2-2")
    }

    /// Skips to the next track; after the last one, stops.
    pub fn next(&mut self) -> CommandResult {
        todo!("F2-2")
    }

    /// Goes back one track; on the first one, restarts it.
    pub fn prev(&mut self) -> CommandResult {
        todo!("F2-2")
    }

    /// Sets the volume, cut to [`MAX_VOLUME`].
    pub fn set_volume(&mut self, _volume: u8) -> CommandResult {
        todo!("F2-2")
    }
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self::new()
    }
}
