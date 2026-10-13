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
use tokio::time::MissedTickBehavior;

use crate::auth::{self, AuthError, TokenStore};
use crate::config;
use crate::player::{self, PlayRequest, PlaybackEvent, Player, PlayerError};
use crate::state::{CommandResult, PlaybackEffect, PlaybackState, Status};

/// How long the engine thread waits for leftover blocking work (an audio device that
/// does not answer, for instance) when it stops.
const RUNTIME_SHUTDOWN: Duration = Duration::from_secs(1);

/// How often the position reaches the window while a track plays: often enough for a
/// smooth bar and a clock in seconds, rarely enough to keep the window idle.
const POSITION_INTERVAL: Duration = Duration::from_millis(500);

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

    /// Carries out what the state machine decided.
    fn apply(&self, effect: &PlaybackEffect) -> Result<(), PlayerError>;

    /// Downloads a track ahead of time, so that it starts at once.
    fn preload(&self, uri: &str) -> Result<(), PlayerError>;

    /// Position in the current track.
    fn position(&self) -> Duration;

    /// Next event of the player; `None` once it has stopped for good.
    fn next_event(&mut self) -> impl Future<Output = Option<PlaybackEvent>>;
}

impl Backend for Player {
    fn username(&self) -> String {
        Player::username(self)
    }

    fn apply(&self, effect: &PlaybackEffect) -> Result<(), PlayerError> {
        Player::apply(self, effect)
    }

    fn preload(&self, uri: &str) -> Result<(), PlayerError> {
        Player::preload(self, uri)
    }

    fn position(&self) -> Duration {
        Player::position(self)
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
    let mut engine = Engine {
        state: PlaybackState::new(),
        notifier,
    };
    loop {
        notifier.send(EngineEvent::Connecting);
        let connecting = connector.connect();
        tokio::pin!(connecting);
        let connected = loop {
            tokio::select! {
                connected = &mut connecting => break connected,
                command = commands.recv() => match command {
                    Some(command) => engine.offline(command),
                    None => return,
                },
            }
        };
        match connected {
            Ok(backend) => {
                notifier.send(EngineEvent::Connected {
                    username: backend.username(),
                });
                match engine.session(backend, &mut commands).await {
                    SessionEnd::Closed => return,
                    SessionEnd::Lost(problem) => notifier.send(EngineEvent::Offline(problem)),
                }
            }
            Err(problem) => notifier.send(EngineEvent::Offline(problem)),
        }
        loop {
            match commands.recv().await {
                Some(Command::Reconnect) => break,
                Some(command) => engine.offline(command),
                None => return,
            }
        }
    }
}

/// The state machine, which outlives the sessions, and the way to the window.
struct Engine<'a> {
    state: PlaybackState,
    notifier: &'a Notifier,
}

impl Engine<'_> {
    /// Runs a session: the window's commands go through the state machine to the player,
    /// the player's events go to the window, and the queue goes on by itself.
    async fn session<B: Backend>(
        &mut self,
        mut backend: B,
        commands: &mut mpsc::UnboundedReceiver<Command>,
    ) -> SessionEnd {
        // The volume may have been chosen before the session was there.
        self.carry_out(Ok(PlaybackEffect::SetVolume(self.state.volume())), &backend);
        let mut ticks = tokio::time::interval(POSITION_INTERVAL);
        ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let end = loop {
            let playing = self.state.status() == Status::Playing;
            tokio::select! {
                command = commands.recv() => match command {
                    Some(command) => self.command(command, &backend),
                    None => break SessionEnd::Closed,
                },
                event = backend.next_event() => match event {
                    Some(event) => self.playback(event, &backend),
                    None => break SessionEnd::Lost(Problem::Other(
                        "il motore di riproduzione si è fermato".to_string(),
                    )),
                },
                _ = ticks.tick(), if playing => {
                    self.notifier.send(EngineEvent::Position(backend.position()));
                }
            }
        };
        if let SessionEnd::Lost(_) = end {
            // Nothing plays without the session; play() starts the current track again.
            let _ = self.state.stop();
            self.notifier.send(EngineEvent::State(self.state.clone()));
        }
        end
    }

    /// A command of the window, with the session open.
    fn command(&mut self, command: Command, backend: &impl Backend) {
        let result = match command {
            Command::TogglePlay if self.state.status() == Status::Playing => self.state.pause(),
            Command::TogglePlay => self.state.play(),
            Command::Next => self.state.next(),
            Command::Prev => self.state.prev(),
            Command::SetVolume(volume) => self.state.set_volume(volume),
            Command::Add(PlayRequest::Item(uri)) => self.state.enqueue(uri),
            Command::Add(PlayRequest::Album(uri)) => {
                let text = format!("{uri} è un album: gli album arriveranno con la coda della finestra");
                self.notifier.send(EngineEvent::Failed(Problem::Uri(text)));
                return;
            }
            // The session is already open.
            Command::Reconnect => return,
        };
        self.carry_out(result, backend);
    }

    /// A command of the window without a session: only the volume can wait for it.
    fn offline(&mut self, command: Command) {
        match command {
            Command::SetVolume(volume) => {
                if self.state.set_volume(volume).is_ok() {
                    self.notifier.send(EngineEvent::State(self.state.clone()));
                }
            }
            // Asking again while connecting changes nothing.
            Command::Reconnect => {}
            _ => self.notifier.send(EngineEvent::Failed(Problem::Other(
                "Spotify non è collegato: comando non eseguito".to_string(),
            ))),
        }
    }

    /// An event of the player: the window hears of it, and the queue goes on when a
    /// track ends or cannot be played, as in `spotilite play`.
    fn playback(&mut self, event: PlaybackEvent, backend: &impl Backend) {
        let preload = player::track_to_preload(&event, &self.state).map(str::to_string);
        let ended = matches!(
            event,
            PlaybackEvent::EndOfTrack { .. } | PlaybackEvent::Unavailable { .. }
        );
        self.notifier.send(EngineEvent::Playback(event));
        if let Some(next) = preload
            && let Err(err) = backend.preload(&next)
        {
            self.notifier.send(EngineEvent::Failed(player_problem(err)));
        }
        if ended {
            let next = self.state.next();
            self.carry_out(next, backend);
        }
    }

    /// Hands the outcome of the state machine to the player, then shows the window the
    /// new state.
    fn carry_out(&mut self, result: CommandResult, backend: &impl Backend) {
        match result {
            Ok(effect) => {
                if let Err(err) = backend.apply(&effect) {
                    self.notifier.send(EngineEvent::Failed(player_problem(err)));
                }
                self.notifier.send(EngineEvent::State(self.state.clone()));
            }
            Err(err) => self.notifier.send(EngineEvent::Failed(Problem::Other(err.to_string()))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::fake::{Call, FakeConnector, FakeControls, pump_until};
    use crate::ui::viewmodel::ViewModel;

    const A: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQC";
    const B: &str = "spotify:track:6rqhFgbbKwnb9MLmUQDhG6";
    const C: &str = "spotify:track:0VjIjW4GlUZAMYd2vXMi3b";

    /// An engine with an open session, and the view of a window that has seen it open.
    fn connected() -> (EngineHandle, FakeControls, ViewModel) {
        let (connector, controls) = FakeConnector::connected("mario");
        let engine = EngineHandle::spawn(connector, || {}).expect("the engine must start");
        let mut view = ViewModel::new();
        pump_until(&engine, &mut view, "the session", |view| {
            view.connection_label().contains("mario")
        });
        (engine, controls, view)
    }

    /// Queues `uris` the way the window does, and waits until the first one plays.
    fn queue(engine: &EngineHandle, view: &mut ViewModel, uris: &[&str]) {
        for uri in uris {
            engine.send(view.submit_uri(uri).expect("a valid URI"));
        }
        pump_until(engine, view, "the queue", |view| {
            view.queue_len() == uris.len() && title(view).as_deref() == Some(uris[0])
        });
    }

    fn title(view: &ViewModel) -> Option<String> {
        view.now_playing().map(|now| now.title)
    }

    fn start(uri: &str) -> Call {
        Call::Apply(PlaybackEffect::StartTrack(uri.to_string()))
    }

    fn ended(uri: &str) -> PlaybackEvent {
        PlaybackEvent::EndOfTrack { uri: uri.to_string() }
    }

    #[test]
    fn play_and_pause_go_through_the_engine_and_back_to_the_view() {
        let (engine, controls, mut view) = connected();
        queue(&engine, &mut view, &[A]);
        assert_eq!(view.status(), Status::Playing);
        assert_eq!(controls.playback_calls(), [start(A)]);

        engine.send(Command::TogglePlay);
        pump_until(&engine, &mut view, "the pause", |view| view.status() == Status::Paused);
        assert_eq!(view.status_label(), "In pausa");
        assert_eq!(controls.playback_calls().last(), Some(&Call::Apply(PlaybackEffect::Pause)));

        engine.send(Command::TogglePlay);
        pump_until(&engine, &mut view, "playing again", |view| view.status() == Status::Playing);
        assert_eq!(controls.playback_calls().last(), Some(&Call::Apply(PlaybackEffect::Resume)));
    }

    #[test]
    fn next_and_prev_move_through_the_queue() {
        let (engine, controls, mut view) = connected();
        queue(&engine, &mut view, &[A, B, C]);

        engine.send(Command::Next);
        pump_until(&engine, &mut view, "B", |view| title(view).as_deref() == Some(B));
        engine.send(Command::Next);
        pump_until(&engine, &mut view, "C", |view| title(view).as_deref() == Some(C));
        engine.send(Command::Prev);
        pump_until(&engine, &mut view, "B again", |view| title(view).as_deref() == Some(B));
        assert_eq!(controls.playback_calls(), [start(A), start(B), start(C), start(B)]);
        let rows = view.queue_rows();
        assert!(rows[1].current && !rows[0].current && !rows[2].current);
    }

    #[test]
    fn the_volume_reaches_the_player() {
        let (engine, controls, mut view) = connected();
        assert_eq!(
            controls.calls().first(),
            Some(&Call::Apply(PlaybackEffect::SetVolume(100))),
            "a new session gets the volume of the state machine"
        );
        engine.send(view.set_volume(30));
        controls.wait_for(&Call::Apply(PlaybackEffect::SetVolume(30)));
        queue(&engine, &mut view, &[A]);
        assert_eq!(view.volume(), 30, "the engine's state agrees with the slider");
    }

    #[test]
    fn the_end_of_a_track_starts_the_next_one_until_the_queue_ends() {
        let (engine, controls, mut view) = connected();
        queue(&engine, &mut view, &[A, B]);

        controls.emit(ended(A));
        pump_until(&engine, &mut view, "B", |view| title(view).as_deref() == Some(B));
        assert_eq!(controls.playback_calls(), [start(A), start(B)]);

        controls.emit(ended(B));
        pump_until(&engine, &mut view, "the end of the queue", |view| view.status() == Status::Stopped);
        assert_eq!(view.now_playing(), None);
        assert_eq!(controls.playback_calls().last(), Some(&Call::Apply(PlaybackEffect::Stop)));
    }

    #[test]
    fn the_next_track_is_preloaded_before_the_current_one_ends() {
        let (engine, controls, mut view) = connected();
        queue(&engine, &mut view, &[A, B]);
        controls.emit(PlaybackEvent::TimeToPreload { uri: A.to_string() });
        controls.wait_for(&Call::Preload(B.to_string()));
        assert_eq!(controls.playback_calls(), [start(A), Call::Preload(B.to_string())]);
    }

    #[test]
    fn an_unavailable_track_is_skipped_with_a_notice() {
        let (engine, controls, mut view) = connected();
        queue(&engine, &mut view, &[A, B]);
        controls.emit(PlaybackEvent::Unavailable { uri: A.to_string() });
        pump_until(&engine, &mut view, "B", |view| title(view).as_deref() == Some(B));
        let notice = view.notice().expect("the skip must be shown");
        assert!(notice.text.contains("non è disponibile"), "{}", notice.text);
    }

    #[test]
    fn the_position_reaches_the_view_while_playing() {
        let (engine, controls, mut view) = connected();
        controls.set_position(Duration::from_secs(42));
        queue(&engine, &mut view, &[A]);
        pump_until(&engine, &mut view, "the position", |view| {
            view.position_label().starts_with("0:42")
        });
    }

    #[test]
    fn playing_an_empty_queue_says_why_nothing_happens() {
        let (engine, controls, mut view) = connected();
        engine.send(Command::TogglePlay);
        pump_until(&engine, &mut view, "the notice", |view| view.notice().is_some());
        let notice = view.notice().expect("a notice");
        assert!(notice.text.contains("La coda è vuota"), "{}", notice.text);
        assert!(controls.playback_calls().is_empty());
    }

    #[test]
    fn closing_the_window_lets_go_of_the_player() {
        let (engine, controls, _view) = connected();
        assert!(engine.shutdown(Duration::from_secs(5)), "the engine thread must end");
        assert!(controls.dropped(), "the player must be dropped, which stops it");
    }
}
