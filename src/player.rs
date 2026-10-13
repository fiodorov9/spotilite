//! Playback through librespot: one Spotify session and one librespot player, driven
//! by the effects of the state machine in [`crate::state`].
//!
//! librespot stays behind this module: callers see [`Player`], [`PlaybackEvent`] and
//! [`PlayerError`], never librespot's own types.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use librespot::core::authentication::Credentials;
use librespot::core::error::ErrorKind;
use librespot::core::{Session, SessionConfig, SpotifyUri};
use librespot::metadata::audio::{AudioItem, UniqueFields};
use librespot::playback::config::{Bitrate, PlayerConfig};
use librespot::playback::mixer::softmixer::SoftMixer;
use librespot::playback::mixer::{Mixer, MixerConfig};
use librespot::playback::player::{Player as Engine, PlayerEvent as EngineEvent, PlayerEventChannel};
use reqwest::Url;
use thiserror::Error;
use tokio::sync::mpsc;

use crate::audio::{AudioError, AudioOutput};
use crate::auth::{self, Token};
use crate::state::{DEFAULT_VOLUME, MAX_VOLUME, PlaybackEffect};

/// How long librespot's player thread may take to open the audio device.
const DEVICE_OPEN_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Error)]
pub enum PlayerError {
    #[error("il token di accesso è scaduto: esegui di nuovo `spotilite login`")]
    TokenExpired,
    #[error("Spotify ha rifiutato l'accesso ({0}): esegui di nuovo `spotilite login` con un account Premium")]
    Rejected(String),
    #[error("impossibile collegarsi a Spotify: {0}")]
    Connect(String),
    #[error(transparent)]
    Audio(#[from] AudioError),
    #[error("indirizzo Spotify non valido: {0} (atteso spotify:track:<id di 22 caratteri>)")]
    InvalidUri(String),
    #[error("{uri} è {kind}: per ora si possono riprodurre solo brani (spotify:track:...) ed episodi (spotify:episode:...)")]
    NotPlayable { uri: String, kind: &'static str },
    #[error("errore interno del player: {0}")]
    Internal(String),
}

/// What librespot reports about playback, in spotilite's terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybackEvent {
    /// The track is being fetched.
    Loading { uri: String },
    /// Title, artists (or podcast name) and length of the track about to play.
    TrackInfo {
        uri: String,
        title: String,
        artists: Vec<String>,
        duration: Duration,
    },
    Playing { uri: String, position: Duration },
    Paused { uri: String, position: Duration },
    /// The track has ended: time for `PlaybackState::next()`.
    EndOfTrack { uri: String },
    /// Spotify cannot play this track (not available in the country, removed, ...).
    Unavailable { uri: String },
    Stopped { uri: String },
}

/// A Spotify session with a librespot player on the system's default audio device.
pub struct Player {
    session: Session,
    engine: Arc<Engine>,
    mixer: SoftMixer,
    shared: Arc<Shared>,
    events: mpsc::UnboundedReceiver<PlaybackEvent>,
}

impl Player {
    /// Signs in to Spotify with the token saved by `spotilite login` and opens the
    /// default audio device. The device is checked before going to the network, so a
    /// computer without one fails fast, with an error and not a panic.
    pub async fn connect(token: &Token) -> Result<Self, PlayerError> {
        Self::connect_via(token, None).await
    }

    /// Like [`Player::connect`], with the Spotify session going through `proxy`, the
    /// address of an HTTP proxy, when there is one.
    pub async fn connect_via(token: &Token, proxy: Option<&str>) -> Result<Self, PlayerError> {
        if token.is_expired_at(auth::unix_now()) {
            return Err(PlayerError::TokenExpired);
        }
        let proxy = proxy
            .map(|proxy| {
                Url::parse(proxy)
                    .map_err(|err| PlayerError::Connect(format!("indirizzo del proxy non valido ({proxy}): {err}")))
            })
            .transpose()?;
        // Listing the devices may block, so it runs off the async threads.
        let output = tokio::task::spawn_blocking(AudioOutput::system_default)
            .await
            .map_err(internal)??;

        let session = Session::new(
            SessionConfig {
                proxy,
                ..SessionConfig::default()
            },
            None,
        );
        session
            .connect(Credentials::with_access_token(token.access_token.as_str()), false)
            .await
            .map_err(connect_error)?;

        let mixer = SoftMixer::open(MixerConfig::default()).map_err(internal)?;
        mixer.set_volume(mixer_volume(DEFAULT_VOLUME));
        let config = PlayerConfig {
            // The best quality librespot offers; Premium is required anyway.
            bitrate: Bitrate::Bitrate320,
            ..PlayerConfig::default()
        };
        let (sink_builder, opened) = output.into_sink_builder();
        let engine = Engine::new(config, session.clone(), mixer.get_soft_volume(), sink_builder);
        let opened = tokio::task::spawn_blocking(move || opened.recv_timeout(DEVICE_OPEN_TIMEOUT)).await;
        let failure = match opened {
            Ok(Ok(Ok(()))) => None,
            Ok(Ok(Err(err))) => Some(err.into()),
            Ok(Err(RecvTimeoutError::Timeout)) => Some(
                AudioError::Open(format!(
                    "nessuna risposta entro {} secondi",
                    DEVICE_OPEN_TIMEOUT.as_secs()
                ))
                .into(),
            ),
            Ok(Err(RecvTimeoutError::Disconnected)) => {
                Some(AudioError::Open("il thread audio di librespot si è chiuso".to_string()).into())
            }
            Err(err) => Some(internal(err)),
        };
        if let Some(err) = failure {
            session.shutdown();
            // Dropping the engine joins its thread, which may still be stuck on the device.
            thread::spawn(move || drop(engine));
            return Err(err);
        }

        let shared = Arc::new(Shared::default());
        let (sender, events) = mpsc::unbounded_channel();
        tokio::spawn(forward_events(
            engine.get_player_event_channel(),
            Arc::clone(&shared),
            sender,
        ));
        Ok(Self {
            session,
            engine,
            mixer,
            shared,
            events,
        })
    }

    /// Name of the Spotify account of the session.
    pub fn username(&self) -> String {
        self.session.username()
    }

    /// Loads a track or an episode and plays it from the beginning.
    pub fn load(&self, uri: &str) -> Result<(), PlayerError> {
        let uri = parse_playable(uri)?;
        self.shared.loads.fetch_add(1, Ordering::AcqRel);
        *self.shared.clock() = Clock::default();
        self.engine.load(uri, true, 0);
        Ok(())
    }

    pub fn play(&self) {
        self.engine.play();
    }

    pub fn pause(&self) {
        self.engine.pause();
    }

    pub fn stop(&self) {
        self.engine.stop();
    }

    /// Sets the volume from 0 to 100; higher values count as 100.
    pub fn set_volume(&self, volume: u8) {
        self.mixer.set_volume(mixer_volume(volume));
    }

    /// Position in the current track; zero when nothing is playing.
    pub fn position(&self) -> Duration {
        self.shared.clock().at(Instant::now())
    }

    /// Carries out what the state machine decided.
    pub fn apply(&self, effect: &PlaybackEffect) -> Result<(), PlayerError> {
        match effect {
            PlaybackEffect::None => {}
            PlaybackEffect::StartTrack(uri) => self.load(uri)?,
            PlaybackEffect::Resume => self.play(),
            PlaybackEffect::Pause => self.pause(),
            PlaybackEffect::Stop => self.stop(),
            PlaybackEffect::SetVolume(volume) => self.set_volume(*volume),
        }
        Ok(())
    }

    /// Next event from librespot; `None` once its player thread has stopped.
    pub async fn next_event(&mut self) -> Option<PlaybackEvent> {
        self.events.recv().await
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.engine.stop();
        self.session.shutdown();
    }
}

/// Checks that `uri` names something [`Player::load`] can play, without the network.
pub fn check_uri(uri: &str) -> Result<(), PlayerError> {
    parse_playable(uri).map(drop)
}

fn parse_playable(uri: &str) -> Result<SpotifyUri, PlayerError> {
    let parsed = SpotifyUri::from_uri(uri.trim()).map_err(|_| PlayerError::InvalidUri(uri.to_string()))?;
    let kind = match parsed {
        SpotifyUri::Track { .. } | SpotifyUri::Episode { .. } => return Ok(parsed),
        SpotifyUri::Album { .. } => "un album",
        SpotifyUri::Artist { .. } => "un artista",
        SpotifyUri::Playlist { .. } => "una playlist",
        SpotifyUri::Show { .. } => "un podcast",
        SpotifyUri::Local { .. } => "un file locale",
        SpotifyUri::Unknown { .. } => "un elemento di tipo sconosciuto",
    };
    Err(PlayerError::NotPlayable {
        uri: uri.to_string(),
        kind,
    })
}

/// librespot's mixers take a volume from 0 to 65535.
fn mixer_volume(volume: u8) -> u16 {
    let volume = u32::from(volume.min(MAX_VOLUME));
    u16::try_from(volume * u32::from(u16::MAX) / u32::from(MAX_VOLUME)).unwrap_or(u16::MAX)
}

fn connect_error(err: librespot::core::Error) -> PlayerError {
    match err.kind {
        ErrorKind::PermissionDenied | ErrorKind::Unauthenticated => PlayerError::Rejected(err.error.to_string()),
        _ => PlayerError::Connect(err.to_string()),
    }
}

fn internal(err: impl std::fmt::Display) -> PlayerError {
    PlayerError::Internal(err.to_string())
}

/// State shared by the player and the task that reads librespot's events.
#[derive(Default)]
struct Shared {
    clock: Mutex<Clock>,
    /// Loads sent to librespot, to tell the events of the latest one from stale ones.
    loads: AtomicU64,
}

impl Shared {
    fn clock(&self) -> MutexGuard<'_, Clock> {
        self.clock.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Reads librespot's events until its player stops or the [`Player`] is dropped.
async fn forward_events(
    mut engine_events: PlayerEventChannel,
    shared: Arc<Shared>,
    events: mpsc::UnboundedSender<PlaybackEvent>,
) {
    let mut filter = RequestFilter::default();
    while let Some(event) = engine_events.recv().await {
        if !filter.is_current(&event, shared.loads.load(Ordering::Acquire)) {
            continue;
        }
        shared.clock().observe(&event, Instant::now());
        if let Some(event) = translate(event)
            && events.send(event).is_err()
        {
            break;
        }
    }
}

/// Drops the events of a load older than the latest one, so that a late `EndOfTrack`
/// cannot skip the track that replaced it.
#[derive(Debug, Default)]
struct RequestFilter {
    /// Loads librespot has acknowledged with a new play request id.
    loads_seen: u64,
    request_id: Option<u64>,
}

impl RequestFilter {
    fn is_current(&mut self, event: &EngineEvent, loads_sent: u64) -> bool {
        if let EngineEvent::PlayRequestIdChanged { play_request_id } = *event {
            // A seek during loading announces the same id again.
            if self.request_id != Some(play_request_id) {
                self.request_id = Some(play_request_id);
                self.loads_seen += 1;
            }
            return false;
        }
        self.loads_seen == loads_sent && event.get_play_request_id().is_none_or(|id| Some(id) == self.request_id)
    }
}

/// Playback position worked out from librespot's events, which only report it when
/// playback starts, pauses or jumps.
#[derive(Debug, Clone, Copy, Default)]
struct Clock {
    position: Duration,
    playing_since: Option<Instant>,
}

impl Clock {
    fn at(&self, now: Instant) -> Duration {
        self.playing_since.map_or(self.position, |since| {
            self.position.saturating_add(now.saturating_duration_since(since))
        })
    }

    fn observe(&mut self, event: &EngineEvent, now: Instant) {
        match event {
            EngineEvent::Playing { position_ms, .. } => {
                self.position = millis(*position_ms);
                self.playing_since = Some(now);
            }
            EngineEvent::Paused { position_ms, .. } | EngineEvent::Loading { position_ms, .. } => {
                self.position = millis(*position_ms);
                self.playing_since = None;
            }
            EngineEvent::Seeked { position_ms, .. }
            | EngineEvent::PositionCorrection { position_ms, .. }
            | EngineEvent::PositionChanged { position_ms, .. } => {
                self.position = millis(*position_ms);
                if self.playing_since.is_some() {
                    self.playing_since = Some(now);
                }
            }
            EngineEvent::Stopped { .. } | EngineEvent::EndOfTrack { .. } | EngineEvent::Unavailable { .. } => {
                *self = Self::default();
            }
            _ => {}
        }
    }
}

fn translate(event: EngineEvent) -> Option<PlaybackEvent> {
    let event = match event {
        EngineEvent::Loading { track_id, .. } => PlaybackEvent::Loading {
            uri: track_id.to_string(),
        },
        EngineEvent::TrackChanged { audio_item } => track_info(*audio_item),
        EngineEvent::Playing {
            track_id, position_ms, ..
        } => PlaybackEvent::Playing {
            uri: track_id.to_string(),
            position: millis(position_ms),
        },
        EngineEvent::Paused {
            track_id, position_ms, ..
        } => PlaybackEvent::Paused {
            uri: track_id.to_string(),
            position: millis(position_ms),
        },
        EngineEvent::EndOfTrack { track_id, .. } => PlaybackEvent::EndOfTrack {
            uri: track_id.to_string(),
        },
        EngineEvent::Unavailable { track_id, .. } => PlaybackEvent::Unavailable {
            uri: track_id.to_string(),
        },
        EngineEvent::Stopped { track_id, .. } => PlaybackEvent::Stopped {
            uri: track_id.to_string(),
        },
        _ => return None,
    };
    Some(event)
}

fn track_info(item: AudioItem) -> PlaybackEvent {
    let artists = match item.unique_fields {
        UniqueFields::Track { artists, .. } => artists.0.into_iter().map(|artist| artist.name).collect(),
        UniqueFields::Local { artists, .. } => artists.into_iter().collect(),
        UniqueFields::Episode { show_name, .. } => vec![show_name],
    };
    PlaybackEvent::TrackInfo {
        uri: item.track_id.to_string(),
        title: item.name,
        artists,
        duration: millis(item.duration_ms),
    }
}

fn millis(ms: u32) -> Duration {
    Duration::from_millis(u64::from(ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRACK: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQC";

    fn track() -> SpotifyUri {
        SpotifyUri::from_uri(TRACK).unwrap()
    }

    fn playing(play_request_id: u64, position_ms: u32) -> EngineEvent {
        EngineEvent::Playing {
            play_request_id,
            track_id: track(),
            position_ms,
        }
    }

    #[test]
    fn the_player_can_move_to_another_thread() {
        fn assert_send<T: Send>() {}
        assert_send::<Player>();
    }

    #[test]
    fn tracks_and_episodes_are_playable() {
        assert!(check_uri(TRACK).is_ok());
        assert!(check_uri(" spotify:episode:512ojhOuo1ktJprKbVcKyQ ").is_ok());
    }

    #[test]
    fn other_uris_are_refused_with_a_reason() {
        let album = check_uri("spotify:album:6akEvsycLGftJxYudPjmqK").unwrap_err();
        assert!(matches!(album, PlayerError::NotPlayable { kind: "un album", .. }), "{album:?}");
        for invalid in ["spotify:track:xxx", "https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC", ""] {
            assert!(matches!(check_uri(invalid), Err(PlayerError::InvalidUri(_))), "{invalid}");
        }
    }

    #[test]
    fn volume_maps_onto_the_mixer_range() {
        assert_eq!(mixer_volume(0), 0);
        assert_eq!(mixer_volume(50), 32_767);
        assert_eq!(mixer_volume(100), u16::MAX);
        assert_eq!(mixer_volume(150), u16::MAX);
    }

    #[test]
    fn the_clock_runs_only_while_playing() {
        let start = Instant::now();
        let mut clock = Clock::default();
        assert_eq!(clock.at(start), Duration::ZERO);

        clock.observe(&playing(1, 1_000), start);
        assert_eq!(clock.at(start + Duration::from_secs(2)), Duration::from_secs(3));

        let paused = EngineEvent::Paused {
            play_request_id: 1,
            track_id: track(),
            position_ms: 5_000,
        };
        clock.observe(&paused, start + Duration::from_secs(4));
        assert_eq!(clock.at(start + Duration::from_secs(60)), Duration::from_secs(5));

        let seeked = EngineEvent::Seeked {
            play_request_id: 1,
            track_id: track(),
            position_ms: 9_000,
        };
        clock.observe(&seeked, start + Duration::from_secs(61));
        assert_eq!(clock.at(start + Duration::from_secs(70)), Duration::from_secs(9), "still paused");

        let ended = EngineEvent::EndOfTrack {
            play_request_id: 1,
            track_id: track(),
        };
        clock.observe(&ended, start + Duration::from_secs(80));
        assert_eq!(clock.at(start + Duration::from_secs(90)), Duration::ZERO);
    }

    #[test]
    fn events_of_an_older_load_are_dropped() {
        let mut filter = RequestFilter::default();
        let changed = |play_request_id| EngineEvent::PlayRequestIdChanged { play_request_id };

        // First load: librespot gives it id 0.
        assert!(!filter.is_current(&changed(0), 1));
        assert!(filter.is_current(&playing(0, 0), 1));

        // A second load has been sent, librespot has not seen it yet.
        assert!(!filter.is_current(&playing(0, 0), 2), "stale event before the new id");
        assert!(!filter.is_current(&changed(1), 2));
        assert!(!filter.is_current(&playing(0, 0), 2), "stale event after the new id");
        assert!(filter.is_current(&playing(1, 0), 2));

        // A seek during loading repeats the id without a new load.
        assert!(!filter.is_current(&changed(1), 2));
        assert!(filter.is_current(&playing(1, 0), 2));

        // Events without a request id pass when no load is pending.
        assert!(filter.is_current(&EngineEvent::VolumeChanged { volume: 1 }, 2));
        assert!(!filter.is_current(&EngineEvent::VolumeChanged { volume: 1 }, 3));
    }

    #[test]
    fn events_are_translated_with_the_uri() {
        assert_eq!(
            translate(playing(0, 1_500)),
            Some(PlaybackEvent::Playing {
                uri: TRACK.to_string(),
                position: Duration::from_millis(1_500),
            })
        );
        assert_eq!(translate(EngineEvent::VolumeChanged { volume: 1 }), None);
    }
}
