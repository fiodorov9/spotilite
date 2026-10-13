//! The messages between the window and the engine thread, which owns the Spotify
//! session, the player and the state machine.

use std::time::Duration;

use crate::player::{PlayRequest, PlaybackEvent};
use crate::state::PlaybackState;

/// What the window asks the engine to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Pause if playing, play otherwise. The engine decides, on its own state.
    TogglePlay,
    Next,
    Prev,
    /// Volume from 0 to 100.
    SetVolume(u8),
    /// Queue a track, an episode or an album, already recognised by
    /// [`crate::player::parse_request`].
    Add(PlayRequest),
    /// Try again to open the session, after `spotilite login` for instance.
    Reconnect,
}

/// What the engine reports to the window.
#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// The Spotify session is being opened.
    Connecting,
    /// The session is open, signed in as this Spotify user.
    Connected { username: String },
    /// There is no session, for this reason.
    Offline(Problem),
    /// The state machine after a change: queue, current track, status, volume.
    State(PlaybackState),
    /// Requests that wait for the session, in the order they were made.
    Pending(Vec<PlayRequest>),
    /// What the player reported.
    Playback(PlaybackEvent),
    /// Position in the current track, sent from time to time while it plays.
    Position(Duration),
    /// An album has been read and its tracks queued.
    AlbumQueued { uri: String, tracks: usize },
    /// A command or a track went wrong; the session is still there.
    Failed(Problem),
}

/// Something that went wrong, sorted by what the user can do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// No token has been saved yet.
    NotSignedIn,
    /// The sign-in cannot be used any more: expired, revoked or refused.
    SignIn(String),
    /// The audio device cannot be used.
    Audio(String),
    /// Spotify cannot be reached.
    Network(String),
    /// Something that cannot be played.
    Uri(String),
    Other(String),
}
