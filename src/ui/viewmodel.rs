//! What the window shows, worked out from the engine's events and the state machine.
//!
//! Pure logic, with no window, no network and no audio, so the tests run anywhere.

use std::collections::HashMap;
use std::time::Duration;

use crate::player::{self, PlayRequest, PlaybackEvent};
use crate::state::{DEFAULT_VOLUME, MAX_VOLUME, PlaybackState, Status};

use super::engine::{Command, EngineEvent, Problem};

/// Shown in place of a length that is not known yet.
const UNKNOWN_TIME: &str = "-:--";

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

/// What the player said about a track.
#[derive(Debug, Clone)]
struct TrackDetails {
    title: String,
    artists: Vec<String>,
    duration: Duration,
}

#[derive(Debug, Clone, Default)]
enum Connection {
    /// No news from the engine yet.
    #[default]
    Idle,
    Connecting,
    Connected(String),
    Offline(Problem),
}

/// The window's view of the engine: one per window, fed with every [`EngineEvent`].
#[derive(Debug)]
pub struct ViewModel {
    connection: Connection,
    /// The latest copy of the engine's state machine.
    state: PlaybackState,
    pending: Vec<PlayRequest>,
    /// Every track the player has described so far, by URI.
    tracks: HashMap<String, TrackDetails>,
    /// Place in the queue and URI of the current track, to notice when it changes.
    current: Option<(usize, String)>,
    position: Duration,
    /// The current track is loading: between `Loading` and `Playing` or `Paused`.
    loading: bool,
    volume: u8,
    /// A volume sent with [`ViewModel::set_volume`] that the engine has not confirmed
    /// yet: older states must not move the slider back.
    volume_sent: Option<u8>,
    notice: Option<Notice>,
}

impl ViewModel {
    pub fn new() -> Self {
        Self {
            connection: Connection::Idle,
            state: PlaybackState::new(),
            pending: Vec::new(),
            tracks: HashMap::new(),
            current: None,
            position: Duration::ZERO,
            loading: false,
            volume: DEFAULT_VOLUME,
            volume_sent: None,
            notice: None,
        }
    }

    /// Takes in an event of the engine.
    pub fn apply(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::Connecting => self.connection = Connection::Connecting,
            EngineEvent::Connected { username } => self.connection = Connection::Connected(username),
            EngineEvent::Offline(problem) => self.connection = Connection::Offline(problem),
            EngineEvent::State(state) => self.set_state(state),
            EngineEvent::Pending(requests) => self.pending = requests,
            EngineEvent::Playback(event) => self.playback(event),
            EngineEvent::Position(position) => {
                if self.current.is_some() {
                    self.position = position;
                }
            }
            EngineEvent::AlbumQueued { tracks, .. } => {
                self.notify(Level::Info, format!("Album accodato: {}", count_tracks(tracks)));
            }
            EngineEvent::Failed(problem) => self.notify(Level::Error, problem_text(&problem)),
        }
    }

    fn set_state(&mut self, state: PlaybackState) {
        let current = state.current_index().zip(state.current()).map(|(index, uri)| (index, uri.to_string()));
        if current != self.current || state.status() == Status::Stopped {
            self.position = Duration::ZERO;
            self.loading = false;
        }
        self.current = current;
        match self.volume_sent {
            Some(sent) if sent != state.volume() => {}
            _ => {
                self.volume = state.volume();
                self.volume_sent = None;
            }
        }
        self.state = state;
    }

    fn playback(&mut self, event: PlaybackEvent) {
        match event {
            PlaybackEvent::TrackInfo {
                uri,
                title,
                artists,
                duration,
            } => {
                self.tracks.insert(
                    uri,
                    TrackDetails {
                        title,
                        artists,
                        duration,
                    },
                );
            }
            PlaybackEvent::Loading { uri } if self.is_current(&uri) => {
                self.loading = true;
                self.position = Duration::ZERO;
            }
            PlaybackEvent::Playing { uri, position } | PlaybackEvent::Paused { uri, position }
                if self.is_current(&uri) =>
            {
                self.loading = false;
                self.position = position;
            }
            PlaybackEvent::Unavailable { uri } => {
                let name = self.tracks.get(&uri).map_or(uri.as_str(), |track| track.title.as_str());
                let text = format!("«{name}» non è disponibile e viene saltato");
                self.notify(Level::Warning, text);
            }
            _ => {}
        }
    }

    fn is_current(&self, uri: &str) -> bool {
        self.current.as_ref().is_some_and(|(_, current)| current == uri)
    }

    fn notify(&mut self, level: Level, text: String) {
        self.notice = Some(Notice { level, text });
    }

    /// One line about the session: starting, connecting, signed in, or why not.
    pub fn connection_label(&self) -> String {
        match &self.connection {
            Connection::Idle => "Non collegato a Spotify".to_string(),
            Connection::Connecting => "Collegamento a Spotify...".to_string(),
            Connection::Connected(username) => format!("Collegato a Spotify come {username}"),
            Connection::Offline(problem) => problem_text(problem),
        }
    }

    /// Whether the session could not be opened, so the window offers to retry.
    pub fn is_offline(&self) -> bool {
        matches!(self.connection, Connection::Offline(_))
    }

    /// Whether the window should invite the user to run `spotilite login`.
    pub fn needs_login(&self) -> bool {
        matches!(
            self.connection,
            Connection::Offline(Problem::NotSignedIn | Problem::SignIn(_))
        )
    }

    /// The current track; `None` when there is none.
    pub fn now_playing(&self) -> Option<NowPlaying> {
        let (_, uri) = self.current.as_ref()?;
        Some(match self.tracks.get(uri) {
            Some(track) => NowPlaying {
                title: track.title.clone(),
                artists: track.artists.join(", "),
                duration: format_time(track.duration),
            },
            None => NowPlaying {
                title: uri.clone(),
                artists: String::new(),
                duration: UNKNOWN_TIME.to_string(),
            },
        })
    }

    pub fn status(&self) -> Status {
        self.state.status()
    }

    /// `Fermo`, `In riproduzione`, `In pausa`, or `Caricamento...` while the track loads.
    pub fn status_label(&self) -> &'static str {
        match self.status() {
            Status::Stopped => "Fermo",
            Status::Playing if self.loading => "Caricamento...",
            Status::Playing => "In riproduzione",
            Status::Paused => "In pausa",
        }
    }

    /// Label of the play/pause button: what pressing it does.
    pub fn play_label(&self) -> &'static str {
        if self.status() == Status::Playing { "Pausa" } else { "Play" }
    }

    /// `position / duration` as `m:ss / m:ss`.
    pub fn position_label(&self) -> String {
        let duration = self.current_duration();
        let position = duration.map_or(self.position, |duration| self.position.min(duration));
        let duration = duration.map_or_else(|| UNKNOWN_TIME.to_string(), format_time);
        format!("{} / {duration}", format_time(position))
    }

    /// How far the current track has played, from 0 to 1.
    pub fn progress(&self) -> f32 {
        match self.current_duration() {
            Some(duration) if !duration.is_zero() => {
                (self.position.as_secs_f64() / duration.as_secs_f64()).clamp(0.0, 1.0) as f32
            }
            _ => 0.0,
        }
    }

    fn current_duration(&self) -> Option<Duration> {
        let (_, uri) = self.current.as_ref()?;
        self.tracks.get(uri).map(|track| track.duration)
    }

    pub fn volume(&self) -> u8 {
        self.volume
    }

    /// The queue, then the requests that wait for the session.
    pub fn queue_rows(&self) -> Vec<QueueRow> {
        (0..self.queue_len()).filter_map(|index| self.queue_row(index)).collect()
    }

    /// Row of the current track in [`ViewModel::queue_rows`], the highlighted one.
    pub fn current_row(&self) -> Option<usize> {
        self.state.current_index()
    }

    /// Rows of [`ViewModel::queue_rows`], pending requests included.
    pub fn queue_len(&self) -> usize {
        self.state.queue().len() + self.pending.len()
    }

    /// The row at `index` of [`ViewModel::queue_rows`], worked out alone, so the window
    /// only spends time on the rows it shows.
    pub fn queue_row(&self, index: usize) -> Option<QueueRow> {
        let queue = self.state.queue();
        if let Some(uri) = queue.get(index) {
            return Some(QueueRow {
                number: index + 1,
                label: self.track_label(uri),
                current: self.state.current_index() == Some(index),
                pending: false,
            });
        }
        let label = match self.pending.get(index - queue.len())? {
            PlayRequest::Item(uri) => self.track_label(uri),
            PlayRequest::Album(uri) => format!("Album {uri}"),
        };
        Some(QueueRow {
            number: index + 1,
            label,
            current: false,
            pending: true,
        })
    }

    /// `Title - Artist, Artist` once the player has described the track, its URI before.
    fn track_label(&self, uri: &str) -> String {
        match self.tracks.get(uri) {
            Some(track) if track.artists.is_empty() => track.title.clone(),
            Some(track) => format!("{} - {}", track.title, track.artists.join(", ")),
            None => uri.to_string(),
        }
    }

    /// The latest message for the user, until it is dismissed.
    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    pub fn dismiss_notice(&mut self) {
        self.notice = None;
    }

    /// The command for the volume slider, cut to 100; the slider shows it at once.
    pub fn set_volume(&mut self, volume: u8) -> Command {
        let volume = volume.min(MAX_VOLUME);
        self.volume = volume;
        self.volume_sent = Some(volume);
        Command::SetVolume(volume)
    }

    /// The command that queues what the user typed, or `None` with a warning when it
    /// cannot be played. Empty text is ignored.
    pub fn submit_uri(&mut self, text: &str) -> Option<Command> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let uri = uri_from_link(text).unwrap_or_else(|| text.to_string());
        match player::parse_request(&uri) {
            Ok(request) => {
                // The warning about an earlier attempt no longer applies.
                self.notice = None;
                Some(Command::Add(request))
            }
            Err(err) => {
                self.notify(Level::Warning, problem_text(&Problem::Uri(err.to_string())));
                None
            }
        }
    }
}

impl Default for ViewModel {
    fn default() -> Self {
        Self::new()
    }
}

/// `spotify:track:<id>` from a link such as `https://open.spotify.com/intl-it/track/<id>?si=...`,
/// which is what Spotify copies with "Copy link". `None` for anything else.
fn uri_from_link(text: &str) -> Option<String> {
    let rest = text.strip_prefix("https://").or_else(|| text.strip_prefix("http://"))?;
    let path = rest.strip_prefix("open.spotify.com/")?;
    let path = path.split(['?', '#']).next()?;
    let mut segments = path
        .split('/')
        .filter(|segment| !segment.is_empty() && !segment.starts_with("intl-"));
    let kind = segments.next()?;
    let id = segments.next()?;
    Some(format!("spotify:{kind}:{id}"))
}

/// `m:ss`, with the minutes running past 59.
pub fn format_time(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// `1 brano`, `12 brani`.
fn count_tracks(count: usize) -> String {
    if count == 1 {
        "1 brano".to_string()
    } else {
        format!("{count} brani")
    }
}

/// A sentence for the user about `problem`, saying what to do when there is something
/// to do.
pub fn problem_text(problem: &Problem) -> String {
    match problem {
        Problem::NotSignedIn => "Nessun accesso a Spotify salvato: esegui `spotilite login`".to_string(),
        Problem::SignIn(reason) if reason.contains("spotilite login") => capitalize(reason),
        Problem::SignIn(reason) => format!("{}: esegui `spotilite login`", capitalize(reason)),
        Problem::Audio(reason) => format!("Problema con l'audio: {reason}"),
        Problem::Network(reason) => format!("Problema di rete: {reason}"),
        Problem::Uri(reason) => format!("Impossibile accodare: {reason}"),
        Problem::Other(reason) => capitalize(reason),
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_name_the_same_item_as_uris() {
        let id = "4uLU6hMCjMI75M1A2tKUQC";
        assert_eq!(
            uri_from_link(&format!("https://open.spotify.com/episode/{id}#t=10")),
            Some(format!("spotify:episode:{id}"))
        );
        assert_eq!(
            uri_from_link(&format!("http://open.spotify.com//track/{id}/")),
            Some(format!("spotify:track:{id}"))
        );
        assert_eq!(uri_from_link("https://open.spotify.com/"), None);
        assert_eq!(uri_from_link(&format!("spotify:track:{id}")), None);
        assert_eq!(uri_from_link(&format!("https://spotify.example/track/{id}")), None);
    }

    #[test]
    fn capitalize_keeps_the_rest() {
        assert_eq!(capitalize("è tardi: `spotilite`"), "È tardi: `spotilite`");
        assert_eq!(capitalize(""), "");
    }
}
