//! The view model of the window: what it shows for the engine's events and the state
//! machine. Pure tests: no window, no network, no audio device.

use std::time::Duration;

use spotilite::player::{PlayRequest, PlaybackEvent};
use spotilite::state::{DEFAULT_VOLUME, PlaybackState, Status};
use spotilite::ui::engine::{Command, EngineEvent, Problem};
use spotilite::ui::viewmodel::{Level, NowPlaying, ViewModel, format_time, problem_text};

const A: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQC";
const B: &str = "spotify:track:6rqhFgbbKwnb9MLmUQDhG6";
const C: &str = "spotify:track:0VjIjW4GlUZAMYd2vXMi3b";
const ALBUM: &str = "spotify:album:6akEvsycLGftJxYudPjmqK";

/// The state machine playing the first of `uris`, with the others queued after it.
fn queue(uris: &[&str]) -> PlaybackState {
    let mut state = PlaybackState::new();
    for uri in uris {
        state.enqueue(*uri).expect("enqueue must accept a URI");
    }
    state
}

/// A view that has received `state` from the engine.
fn view_of(state: &PlaybackState) -> ViewModel {
    let mut view = ViewModel::new();
    view.apply(EngineEvent::State(state.clone()));
    view
}

fn track_info(uri: &str, title: &str, artists: &[&str], millis: u64) -> EngineEvent {
    EngineEvent::Playback(PlaybackEvent::TrackInfo {
        uri: uri.to_string(),
        title: title.to_string(),
        artists: artists.iter().map(|artist| artist.to_string()).collect(),
        duration: Duration::from_millis(millis),
    })
}

fn playing_at(uri: &str, seconds: u64) -> EngineEvent {
    EngineEvent::Playback(PlaybackEvent::Playing {
        uri: uri.to_string(),
        position: Duration::from_secs(seconds),
    })
}

fn paused_at(uri: &str, seconds: u64) -> EngineEvent {
    EngineEvent::Playback(PlaybackEvent::Paused {
        uri: uri.to_string(),
        position: Duration::from_secs(seconds),
    })
}

fn position(seconds: u64) -> EngineEvent {
    EngineEvent::Position(Duration::from_secs(seconds))
}

// 1. Times

#[test]
fn times_are_minutes_and_seconds() {
    assert_eq!(format_time(Duration::ZERO), "0:00");
    assert_eq!(format_time(Duration::from_millis(5_999)), "0:05");
    assert_eq!(format_time(Duration::from_secs(65)), "1:05");
    assert_eq!(format_time(Duration::from_millis(213_573)), "3:33");
    assert_eq!(format_time(Duration::from_secs(3_600)), "60:00", "minutes run past 59");
    assert_eq!(format_time(Duration::from_secs(4_000)), "66:40");
}

// 2. Startup

#[test]
fn a_new_view_shows_nothing_playing() {
    let view = ViewModel::new();
    assert_eq!(view.status(), Status::Stopped);
    assert_eq!(view.status_label(), "Fermo");
    assert_eq!(view.play_label(), "Play");
    assert_eq!(view.now_playing(), None);
    assert!(view.queue_rows().is_empty());
    assert_eq!(view.notice(), None);
    assert_eq!(view.progress(), 0.0);
    assert!(view.position_label().starts_with("0:00"), "{}", view.position_label());
    assert_eq!(view.volume(), DEFAULT_VOLUME);
    assert!(!view.is_offline());
    assert!(!view.needs_login());
}

// 3. The current track

#[test]
fn the_current_track_shows_title_artists_and_length() {
    let mut view = view_of(&queue(&[A, B]));
    view.apply(track_info(A, "Never Gonna Give You Up", &["Rick Astley", "Altro"], 213_573));
    assert_eq!(
        view.now_playing(),
        Some(NowPlaying {
            title: "Never Gonna Give You Up".to_string(),
            artists: "Rick Astley, Altro".to_string(),
            duration: "3:33".to_string(),
        })
    );
}

#[test]
fn the_uri_stands_in_for_the_title_until_the_track_info_arrives() {
    let mut view = view_of(&queue(&[A]));
    view.apply(EngineEvent::Playback(PlaybackEvent::Loading { uri: A.to_string() }));
    let now = view.now_playing().expect("A is the current track");
    assert_eq!(now.title, A);
    assert_eq!(now.artists, "");
    assert_eq!(now.duration, "-:--");
    assert_eq!(view.status_label(), "Caricamento...");

    view.apply(playing_at(A, 0));
    assert_eq!(view.status_label(), "In riproduzione");
}

#[test]
fn the_info_of_another_track_is_kept_for_later() {
    let mut state = queue(&[A, B]);
    let mut view = view_of(&state);
    view.apply(track_info(B, "Seconda", &["Qualcuno"], 60_000));
    assert_eq!(view.now_playing().expect("A is current").title, A, "B is not playing yet");

    state.next().expect("B follows A");
    view.apply(EngineEvent::State(state));
    let now = view.now_playing().expect("B is current");
    assert_eq!(now.title, "Seconda");
    assert_eq!(now.duration, "1:00");
}

#[test]
fn nothing_plays_once_the_queue_has_ended() {
    let mut state = queue(&[A]);
    let mut view = view_of(&state);
    view.apply(track_info(A, "Prima", &["Uno"], 60_000));
    view.apply(position(59));

    state.next().expect("the queue ends");
    view.apply(EngineEvent::State(state));
    assert_eq!(view.now_playing(), None);
    assert_eq!(view.status_label(), "Fermo");
    assert_eq!(view.progress(), 0.0);
}

// 4. Status

#[test]
fn the_status_follows_the_state_machine() {
    let mut state = queue(&[A]);
    let mut view = view_of(&state);
    view.apply(playing_at(A, 0));
    assert_eq!(view.status(), Status::Playing);
    assert_eq!(view.status_label(), "In riproduzione");
    assert_eq!(view.play_label(), "Pausa");

    state.pause().expect("pause");
    view.apply(EngineEvent::State(state.clone()));
    view.apply(paused_at(A, 3));
    assert_eq!(view.status(), Status::Paused);
    assert_eq!(view.status_label(), "In pausa");
    assert_eq!(view.play_label(), "Play");

    state.stop().expect("stop");
    view.apply(EngineEvent::State(state));
    assert_eq!(view.status(), Status::Stopped);
    assert_eq!(view.status_label(), "Fermo");
    assert_eq!(view.play_label(), "Play");
}

// 5. Position

#[test]
fn the_position_follows_the_player() {
    let mut view = view_of(&queue(&[A]));
    view.apply(track_info(A, "Prima", &["Uno"], 200_000));
    view.apply(playing_at(A, 1));
    assert_eq!(view.position_label(), "0:01 / 3:20");

    view.apply(position(65));
    assert_eq!(view.position_label(), "1:05 / 3:20");
    assert!((view.progress() - 65.0 / 200.0).abs() < 1e-3, "{}", view.progress());

    view.apply(paused_at(A, 70));
    assert_eq!(view.position_label(), "1:10 / 3:20");

    view.apply(position(500));
    assert_eq!(view.progress(), 1.0, "the bar never goes past the end");
}

#[test]
fn the_position_of_another_track_is_ignored() {
    let mut view = view_of(&queue(&[A, B]));
    view.apply(track_info(A, "Prima", &["Uno"], 200_000));
    view.apply(position(30));
    view.apply(playing_at(B, 90));
    view.apply(paused_at(B, 95));
    assert_eq!(view.position_label(), "0:30 / 3:20");
}

#[test]
fn a_new_track_starts_from_zero() {
    let mut state = queue(&[A, B]);
    let mut view = view_of(&state);
    view.apply(track_info(A, "Prima", &["Uno"], 200_000));
    view.apply(position(65));

    state.next().expect("B follows A");
    view.apply(EngineEvent::State(state));
    assert_eq!(view.position_label(), "0:00 / -:--");
    assert_eq!(view.progress(), 0.0);
}

#[test]
fn without_a_length_the_bar_stays_empty() {
    let mut view = view_of(&queue(&[A]));
    view.apply(position(30));
    assert_eq!(view.position_label(), "0:30 / -:--");
    assert_eq!(view.progress(), 0.0);
}

// 6. Queue

#[test]
fn the_queue_highlights_the_current_track() {
    let mut state = queue(&[A, B, C]);
    state.next().expect("B follows A");
    let mut view = view_of(&state);
    view.apply(track_info(A, "Prima", &["Uno", "Due"], 1_000));
    view.apply(track_info(B, "Seconda", &[], 1_000));

    let rows = view.queue_rows();
    let numbers: Vec<_> = rows.iter().map(|row| row.number).collect();
    let current: Vec<_> = rows.iter().map(|row| row.current).collect();
    let labels: Vec<_> = rows.iter().map(|row| row.label.as_str()).collect();
    assert_eq!(numbers, [1, 2, 3]);
    assert_eq!(current, [false, true, false]);
    assert_eq!(labels, ["Prima - Uno, Due", "Seconda", C], "unknown tracks show their URI");
    assert!(rows.iter().all(|row| !row.pending));
}

#[test]
fn no_track_is_highlighted_once_the_queue_has_ended() {
    let mut state = queue(&[A, B]);
    state.next().expect("B follows A");
    state.next().expect("the queue ends");
    let rows = view_of(&state).queue_rows();
    assert_eq!(rows.len(), 2, "the queue is kept");
    assert!(rows.iter().all(|row| !row.current));
}

#[test]
fn requests_waiting_for_the_session_follow_the_queue() {
    let mut view = view_of(&queue(&[A]));
    view.apply(EngineEvent::Pending(vec![
        PlayRequest::Item(B.to_string()),
        PlayRequest::Album(ALBUM.to_string()),
    ]));
    let rows = view.queue_rows();
    assert_eq!(rows.len(), 3);
    assert!(!rows[0].pending);
    assert!(rows[1].pending && rows[2].pending);
    assert_eq!((rows[1].number, rows[1].label.as_str()), (2, B));
    assert!(
        rows[2].label.contains(ALBUM) && rows[2].label.to_lowercase().contains("album"),
        "{}",
        rows[2].label
    );
    assert!(rows.iter().skip(1).all(|row| !row.current));

    view.apply(EngineEvent::Pending(Vec::new()));
    assert_eq!(view.queue_rows().len(), 1);
}

// 7. Volume

#[test]
fn the_volume_comes_from_the_state_machine() {
    let mut state = queue(&[A]);
    state.set_volume(40).expect("volume");
    assert_eq!(view_of(&state).volume(), 40);
}

#[test]
fn the_slider_moves_at_once_and_sends_the_command() {
    let mut view = ViewModel::new();
    assert_eq!(view.set_volume(70), Command::SetVolume(70));
    assert_eq!(view.volume(), 70);
    assert_eq!(view.set_volume(150), Command::SetVolume(100));
    assert_eq!(view.volume(), 100);
}

// 8. Typed URIs

#[test]
fn a_valid_uri_becomes_a_command_to_queue_it() {
    let mut view = ViewModel::new();
    let item = |uri: &str| Some(Command::Add(PlayRequest::Item(uri.to_string())));
    assert_eq!(view.submit_uri(&format!("  {A} ")), item(A));
    assert_eq!(view.submit_uri("spotify:user:someone:track:4uLU6hMCjMI75M1A2tKUQC"), item(A));
    assert_eq!(
        view.submit_uri(ALBUM),
        Some(Command::Add(PlayRequest::Album(ALBUM.to_string())))
    );
    assert_eq!(view.notice(), None);
}

/// "Copy link" in Spotify gives an https address: it names the same item.
#[test]
fn a_spotify_link_is_accepted_too() {
    let mut view = ViewModel::new();
    assert_eq!(
        view.submit_uri("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC?si=abc123"),
        Some(Command::Add(PlayRequest::Item(A.to_string())))
    );
    assert_eq!(
        view.submit_uri("https://open.spotify.com/intl-it/album/6akEvsycLGftJxYudPjmqK"),
        Some(Command::Add(PlayRequest::Album(ALBUM.to_string())))
    );
    assert_eq!(view.notice(), None);
}

#[test]
fn an_invalid_uri_is_refused_with_a_warning() {
    for (typed, expected) in [
        ("spotify:track:xxx", "non valido"),
        ("ciao", "non valido"),
        ("https://example.com/track/4uLU6hMCjMI75M1A2tKUQC", "non valido"),
        ("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M", "playlist"),
        ("spotify:artist:0gxyHStUsqpMadRV0Di1Qt", "artista"),
    ] {
        let mut view = ViewModel::new();
        assert_eq!(view.submit_uri(typed), None, "{typed}");
        let notice = view.notice().unwrap_or_else(|| panic!("a warning for {typed}"));
        assert_eq!(notice.level, Level::Warning, "{typed}");
        assert!(notice.text.contains(expected), "{typed}: {}", notice.text);
    }
}

#[test]
fn empty_text_is_ignored() {
    let mut view = ViewModel::new();
    assert_eq!(view.submit_uri("   "), None);
    assert_eq!(view.notice(), None);
}

// 9. Session

#[test]
fn the_session_is_described() {
    let mut view = ViewModel::new();
    view.apply(EngineEvent::Connecting);
    assert!(view.connection_label().contains("Collegamento a Spotify"), "{}", view.connection_label());
    assert!(!view.is_offline());

    view.apply(EngineEvent::Connected {
        username: "mario".to_string(),
    });
    assert!(view.connection_label().contains("mario"), "{}", view.connection_label());
    assert!(!view.is_offline());
    assert!(!view.needs_login());
}

#[test]
fn without_a_token_the_window_invites_to_sign_in() {
    let mut view = ViewModel::new();
    view.apply(EngineEvent::Offline(Problem::NotSignedIn));
    assert!(view.is_offline());
    assert!(view.needs_login());
    assert!(view.connection_label().contains("spotilite login"), "{}", view.connection_label());
}

#[test]
fn an_expired_sign_in_invites_to_sign_in_again() {
    let mut view = ViewModel::new();
    view.apply(EngineEvent::Offline(Problem::SignIn(
        "sessione scaduta: esegui `spotilite login`".to_string(),
    )));
    assert!(view.needs_login());
    let label = view.connection_label();
    assert!(label.contains("essione scaduta"), "{label}");
    assert_eq!(label.matches("spotilite login").count(), 1, "said once: {label}");

    view.apply(EngineEvent::Offline(Problem::SignIn(
        "il file C:\\dati\\token.json è danneggiato".to_string(),
    )));
    let label = view.connection_label();
    assert!(label.contains("danneggiato") && label.contains("spotilite login"), "{label}");
}

#[test]
fn audio_and_network_problems_do_not_ask_to_sign_in() {
    let mut view = ViewModel::new();
    view.apply(EngineEvent::Offline(Problem::Audio(
        "nessun dispositivo audio di uscita: collega cuffie o altoparlanti".to_string(),
    )));
    assert!(view.is_offline());
    assert!(!view.needs_login());
    let label = view.connection_label();
    assert!(label.contains("audio") && label.contains("collega cuffie"), "{label}");

    view.apply(EngineEvent::Offline(Problem::Network(
        "impossibile collegarsi a Spotify: timeout".to_string(),
    )));
    assert!(!view.needs_login());
    let label = view.connection_label();
    assert!(label.to_lowercase().contains("rete") && label.contains("timeout"), "{label}");

    view.apply(EngineEvent::Connected {
        username: "mario".to_string(),
    });
    assert!(!view.is_offline());
}

// 10. Messages

#[test]
fn problems_read_as_sentences() {
    for problem in [
        Problem::NotSignedIn,
        Problem::SignIn("sessione scaduta: esegui `spotilite login`".to_string()),
        Problem::Audio("impossibile aprire il dispositivo audio: occupato".to_string()),
        Problem::Network("impossibile collegarsi a Spotify: timeout".to_string()),
        Problem::Uri("indirizzo Spotify non valido: x".to_string()),
        Problem::Other("errore interno del player: x".to_string()),
    ] {
        let text = problem_text(&problem);
        assert!(text.chars().next().is_some_and(char::is_uppercase), "{problem:?}: {text}");
        for noise in ["NotSignedIn", "SignIn(", "Audio(", "Network(", "Uri(", "Other(", "\""] {
            assert!(!text.contains(noise), "{problem:?}: {text}");
        }
    }
}

#[test]
fn engine_failures_become_notices() {
    let mut view = ViewModel::new();
    view.apply(EngineEvent::Connected {
        username: "mario".to_string(),
    });
    view.apply(EngineEvent::Failed(Problem::Audio("il dispositivo è sparito".to_string())));
    let notice = view.notice().expect("the failure must be shown");
    assert_eq!(notice.level, Level::Error);
    assert!(
        notice.text.contains("audio") && notice.text.contains("il dispositivo è sparito"),
        "{}",
        notice.text
    );
    assert!(!view.is_offline(), "the session is still there");

    view.dismiss_notice();
    assert_eq!(view.notice(), None);
}

#[test]
fn an_unavailable_track_is_reported() {
    let mut view = view_of(&queue(&[A, B]));
    view.apply(track_info(A, "Prima", &["Uno"], 1_000));
    view.apply(EngineEvent::Playback(PlaybackEvent::Unavailable { uri: A.to_string() }));
    let notice = view.notice().expect("the skip must be shown");
    assert_eq!(notice.level, Level::Warning);
    assert!(
        notice.text.contains("Prima") && notice.text.contains("non è disponibile"),
        "{}",
        notice.text
    );
}

#[test]
fn a_queued_album_is_reported() {
    let mut view = ViewModel::new();
    view.apply(EngineEvent::AlbumQueued {
        uri: ALBUM.to_string(),
        tracks: 12,
    });
    let notice = view.notice().expect("the album must be reported");
    assert_eq!(notice.level, Level::Info);
    assert!(notice.text.contains("12 brani"), "{}", notice.text);

    view.apply(EngineEvent::AlbumQueued {
        uri: ALBUM.to_string(),
        tracks: 1,
    });
    let text = &view.notice().expect("the album must be reported").text;
    assert!(text.contains("1 brano") && !text.contains("brani"), "{text}");
}
