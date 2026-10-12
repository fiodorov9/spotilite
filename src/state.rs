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
pub struct PlaybackState {
    queue: Vec<String>,
    /// The current track. When stopped it is the track `play()` starts, and it equals
    /// `queue.len()` once the queue has been played to the end.
    cursor: usize,
    status: Status,
    volume: u8,
}

impl PlaybackState {
    /// Stopped, with an empty queue and the volume at [`DEFAULT_VOLUME`].
    pub fn new() -> Self {
        Self {
            queue: Vec::new(),
            cursor: 0,
            status: Status::Stopped,
            volume: DEFAULT_VOLUME,
        }
    }

    pub fn status(&self) -> Status {
        self.status
    }

    pub fn queue(&self) -> &[String] {
        &self.queue
    }

    /// Position in the queue of the current track.
    pub fn current_index(&self) -> Option<usize> {
        (self.cursor < self.queue.len()).then_some(self.cursor)
    }

    /// The track playing, paused, or that `play()` would start after `stop()`.
    pub fn current(&self) -> Option<&str> {
        self.queue.get(self.cursor).map(String::as_str)
    }

    pub fn volume(&self) -> u8 {
        self.volume
    }

    /// Appends `uri` to the queue and, when stopped, starts playing.
    pub fn enqueue(&mut self, uri: impl Into<String>) -> CommandResult {
        let uri = uri.into();
        let uri = uri.trim();
        if uri.is_empty() {
            return Err(StateError::EmptyUri);
        }
        self.queue.push(uri.to_string());
        if self.status == Status::Stopped {
            // The cursor is on the first track of a new queue, on the added track
            // once the queue has ended, or on the track left by `stop()`.
            self.start(self.cursor)
        } else {
            Ok(PlaybackEffect::None)
        }
    }

    pub fn play(&mut self) -> CommandResult {
        match self.status {
            Status::Playing => Ok(PlaybackEffect::None),
            Status::Paused => {
                self.status = Status::Playing;
                Ok(PlaybackEffect::Resume)
            }
            // Once the queue has ended, it starts again from the top.
            Status::Stopped => self.start(self.current_index().unwrap_or(0)),
        }
    }

    pub fn pause(&mut self) -> CommandResult {
        if self.status != Status::Playing {
            return Ok(PlaybackEffect::None);
        }
        self.status = Status::Paused;
        Ok(PlaybackEffect::Pause)
    }

    pub fn stop(&mut self) -> CommandResult {
        if self.status == Status::Stopped {
            return Ok(PlaybackEffect::None);
        }
        self.status = Status::Stopped;
        Ok(PlaybackEffect::Stop)
    }

    /// Skips to the next track; after the last one, stops.
    #[expect(clippy::should_implement_trait, reason = "a player command, not an iterator")]
    pub fn next(&mut self) -> CommandResult {
        if self.queue.is_empty() {
            return Err(StateError::EmptyQueue);
        }
        let next = self.cursor + 1;
        if next < self.queue.len() {
            return self.start(next);
        }
        self.cursor = self.queue.len();
        self.stop()
    }

    /// Goes back one track; on the first one, restarts it.
    pub fn prev(&mut self) -> CommandResult {
        // From the end of the queue this is the last track.
        self.start(self.cursor.saturating_sub(1))
    }

    /// Sets the volume, cut to [`MAX_VOLUME`].
    pub fn set_volume(&mut self, volume: u8) -> CommandResult {
        self.volume = volume.min(MAX_VOLUME);
        Ok(PlaybackEffect::SetVolume(self.volume))
    }

    /// Moves the cursor to `index` and plays that track from the beginning.
    fn start(&mut self, index: usize) -> CommandResult {
        let uri = self.queue.get(index).ok_or(StateError::EmptyQueue)?.clone();
        self.cursor = index;
        self.status = Status::Playing;
        Ok(PlaybackEffect::StartTrack(uri))
    }
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self::new()
    }
}
