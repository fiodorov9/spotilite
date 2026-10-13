//! The engine thread: the Spotify session, the player and the state machine, on a tokio
//! runtime of their own, so the window never waits for the network or the audio device.
//!
//! The window sends [`Command`]s and receives [`EngineEvent`]s on two channels; after
//! every event the engine wakes the window up, so that it draws the news.

use std::future::Future;
use std::io;
use std::sync::mpsc as std_mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::auth::{self, AuthError, TokenStore};
use crate::config;
use crate::player::{PlayRequest, PlaybackEvent, Player, PlayerError};
use crate::state::PlaybackState;

/// How long the engine thread waits for leftover blocking work (an audio device that
/// does not answer, for instance) when it stops.
const RUNTIME_SHUTDOWN: Duration = Duration::from_secs(1);

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

/// Opens the Spotify session: [`SpotifyConnector`] in the application, a fake in the
/// tests.
pub trait Connector {
    type Backend: Backend;

    /// The player of a new session, or why there is none.
    fn connect(&self) -> impl Future<Output = Result<Self::Backend, Problem>>;
}

/// The player as the engine drives it: [`Player`] in the application, a fake in the
/// tests.
pub trait Backend {
    /// Name of the Spotify account of the session.
    fn username(&self) -> String;

    /// Next event of the player; `None` once it has stopped for good.
    fn next_event(&mut self) -> impl Future<Output = Option<PlaybackEvent>>;
}

impl Backend for Player {
    fn username(&self) -> String {
        Player::username(self)
    }

    async fn next_event(&mut self) -> Option<PlaybackEvent> {
        Player::next_event(self).await
    }
}

/// Opens the session with the token saved by `spotilite login`, renewed when it is
/// about to expire, like `spotilite play` does.
#[derive(Debug, Clone)]
pub struct SpotifyConnector {
    /// The OAuth client ID used at sign-in, needed to renew the token.
    pub client_id: String,
    pub token_endpoint: String,
}

impl Connector for SpotifyConnector {
    type Backend = Player;

    async fn connect(&self) -> Result<Player, Problem> {
        let dir = config::state_dir().map_err(|err| Problem::Other(error_chain(&err)))?;
        let store = TokenStore::new(dir);
        let token = store.load().map_err(auth_problem)?.ok_or(Problem::NotSignedIn)?;
        let token = auth::fresh_token(&store, token, &self.token_endpoint, &self.client_id)
            .await
            .map_err(auth_problem)?;
        Player::connect(&token).await.map_err(player_problem)
    }
}

/// Never opens a session: `spotilite gui --selftest` must not touch the network or the
/// audio device.
pub struct NoConnection;

impl Connector for NoConnection {
    type Backend = Player;

    async fn connect(&self) -> Result<Player, Problem> {
        Err(Problem::Other(
            "prova automatica (--selftest): nessun collegamento a Spotify".to_string(),
        ))
    }
}

/// Sorts the errors of the token by what the user can do about them.
fn auth_problem(err: AuthError) -> Problem {
    let text = error_chain(&err);
    match err {
        AuthError::Network(_) | AuthError::RefreshNetwork(_) => Problem::Network(text),
        AuthError::SessionExpired
        | AuthError::NotRenewable
        | AuthError::RefreshRefused(_)
        | AuthError::Corrupt { .. } => Problem::SignIn(text),
        _ => Problem::Other(text),
    }
}

/// Sorts the errors of the player by what the user can do about them.
fn player_problem(err: PlayerError) -> Problem {
    let text = err.to_string();
    match err {
        PlayerError::TokenExpired | PlayerError::Rejected(_) => Problem::SignIn(text),
        PlayerError::Connect(_) => Problem::Network(text),
        PlayerError::Audio(_) => Problem::Audio(text),
        PlayerError::InvalidUri(_)
        | PlayerError::NotPlayable { .. }
        | PlayerError::PlaylistNotYet(_)
        | PlayerError::AlbumNotLoadable(_) => Problem::Uri(text),
        PlayerError::Album { .. } | PlayerError::Internal(_) => Problem::Other(text),
    }
}

/// An error with its causes, which often hold the useful detail.
fn error_chain(err: &dyn std::error::Error) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// The window's end of the two channels, and the engine thread.
pub struct EngineHandle {
    commands: mpsc::UnboundedSender<Command>,
    events: std_mpsc::Receiver<EngineEvent>,
    thread: JoinHandle<()>,
}

impl EngineHandle {
    /// Starts the engine thread, with its own tokio runtime. `wake` is called after
    /// every event, so the window draws it.
    pub fn spawn<C>(connector: C, wake: impl Fn() + Send + 'static) -> io::Result<Self>
    where
        C: Connector + Send + 'static,
    {
        let (commands, command_receiver) = mpsc::unbounded_channel();
        let (event_sender, events) = std_mpsc::channel();
        let notifier = Notifier {
            events: event_sender,
            wake: Box::new(wake),
        };
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("spotilite-engine-worker")
            .enable_all()
            .build()?;
        let thread = thread::Builder::new().name("spotilite-engine".to_string()).spawn(move || {
            runtime.block_on(run(connector, command_receiver, &notifier));
            // Blocking work still waiting on a device or on the network is left behind.
            runtime.shutdown_timeout(RUNTIME_SHUTDOWN);
        })?;
        Ok(Self {
            commands,
            events,
            thread,
        })
    }

    /// Sends a command; does nothing once the engine has stopped.
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// The next event that has already arrived, without waiting.
    pub fn try_event(&self) -> Option<EngineEvent> {
        self.events.try_recv().ok()
    }

    /// The next event, waiting at most `timeout` for it.
    pub fn wait_event(&self, timeout: Duration) -> Option<EngineEvent> {
        self.events.recv_timeout(timeout).ok()
    }

    /// Stops the engine, closing the command channel, and waits at most `timeout` for
    /// its thread to end. Returns whether it ended.
    pub fn shutdown(self, timeout: Duration) -> bool {
        let Self { commands, thread, .. } = self;
        drop(commands);
        let deadline = Instant::now() + timeout;
        while !thread.is_finished() {
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
        thread.join().is_ok()
    }
}

/// The engine's end of the event channel.
struct Notifier {
    events: std_mpsc::Sender<EngineEvent>,
    wake: Box<dyn Fn() + Send>,
}

impl Notifier {
    fn send(&self, event: EngineEvent) {
        if self.events.send(event).is_ok() {
            (self.wake)();
        }
    }
}

/// How a session ended.
enum SessionEnd {
    /// The window has closed the command channel.
    Closed,
    /// The player has stopped by itself.
    Lost(Problem),
}

/// The engine: opens the session and keeps it, until the window closes the command
/// channel. Without a session it waits for [`Command::Reconnect`].
async fn run<C: Connector>(connector: C, mut commands: mpsc::UnboundedReceiver<Command>, notifier: &Notifier) {
    loop {
        notifier.send(EngineEvent::Connecting);
        let connecting = connector.connect();
        tokio::pin!(connecting);
        let connected = loop {
            tokio::select! {
                connected = &mut connecting => break connected,
                command = commands.recv() => if command.is_none() {
                    return;
                },
            }
        };
        match connected {
            Ok(backend) => {
                notifier.send(EngineEvent::Connected {
                    username: backend.username(),
                });
                match session(backend, &mut commands, notifier).await {
                    SessionEnd::Closed => return,
                    SessionEnd::Lost(problem) => notifier.send(EngineEvent::Offline(problem)),
                }
            }
            Err(problem) => notifier.send(EngineEvent::Offline(problem)),
        }
        loop {
            match commands.recv().await {
                Some(Command::Reconnect) => break,
                Some(_) => {}
                None => return,
            }
        }
    }
}

/// Forwards the player's events to the window until the session ends.
async fn session<B: Backend>(
    mut backend: B,
    commands: &mut mpsc::UnboundedReceiver<Command>,
    notifier: &Notifier,
) -> SessionEnd {
    loop {
        tokio::select! {
            command = commands.recv() => if command.is_none() {
                return SessionEnd::Closed;
            },
            event = backend.next_event() => match event {
                Some(event) => notifier.send(EngineEvent::Playback(event)),
                None => {
                    return SessionEnd::Lost(Problem::Other(
                        "il motore di riproduzione si è fermato".to_string(),
                    ));
                }
            },
        }
    }
}
