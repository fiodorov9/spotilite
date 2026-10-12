use spotilite::state::{DEFAULT_VOLUME, PlaybackEffect, PlaybackState, StateError, Status};

const A: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQC";
const B: &str = "spotify:track:6rqhFgbbKwnb9MLmUQDhG6";
const C: &str = "spotify:track:0VjIjW4GlUZAMYd2vXMi3b";

fn start(uri: &str) -> PlaybackEffect {
    PlaybackEffect::StartTrack(uri.to_string())
}

/// A state that is playing `A` with `uris` queued after it.
fn playing(uris: &[&str]) -> PlaybackState {
    let mut state = PlaybackState::new();
    for uri in uris {
        state.enqueue(*uri).expect("enqueue must accept a URI");
    }
    assert_eq!(state.current(), Some(uris[0]));
    state
}

// 1. Startup

#[test]
fn starts_stopped_with_an_empty_queue() {
    let state = PlaybackState::new();
    assert_eq!(state.status(), Status::Stopped);
    assert!(state.queue().is_empty());
    assert_eq!(state.current(), None);
    assert_eq!(state.current_index(), None);
    assert_eq!(state.volume(), DEFAULT_VOLUME);
}

// 2. enqueue

#[test]
fn enqueue_when_stopped_plays_the_first_track() {
    let mut state = PlaybackState::new();
    assert_eq!(state.enqueue(A), Ok(start(A)));
    assert_eq!(state.status(), Status::Playing);
    assert_eq!(state.queue(), [A]);
    assert_eq!(state.current(), Some(A));
    assert_eq!(state.current_index(), Some(0));
}

#[test]
fn enqueue_while_playing_only_appends() {
    let mut state = playing(&[A]);
    assert_eq!(state.enqueue(B), Ok(PlaybackEffect::None));
    assert_eq!(state.queue(), [A, B]);
    assert_eq!(state.current(), Some(A));
    assert_eq!(state.status(), Status::Playing);
}

#[test]
fn enqueue_while_paused_stays_paused() {
    let mut state = playing(&[A]);
    state.pause().unwrap();
    assert_eq!(state.enqueue(B), Ok(PlaybackEffect::None));
    assert_eq!(state.status(), Status::Paused);
    assert_eq!(state.current(), Some(A));
}

#[test]
fn enqueue_after_the_queue_ended_plays_the_new_track() {
    let mut state = playing(&[A]);
    assert_eq!(state.next(), Ok(PlaybackEffect::Stop));
    assert_eq!(state.enqueue(B), Ok(start(B)));
    assert_eq!(state.status(), Status::Playing);
    assert_eq!(state.current_index(), Some(1));
}

#[test]
fn enqueue_rejects_an_empty_uri() {
    let mut state = PlaybackState::new();
    assert_eq!(state.enqueue(""), Err(StateError::EmptyUri));
    assert_eq!(state.enqueue("   "), Err(StateError::EmptyUri));
    assert!(state.queue().is_empty());
    assert_eq!(state.status(), Status::Stopped);
}

// 3. next

#[test]
fn next_in_the_middle_advances_by_one() {
    let mut state = playing(&[A, B, C]);
    assert_eq!(state.next(), Ok(start(B)));
    assert_eq!(state.current(), Some(B));
    assert_eq!(state.current_index(), Some(1));
    assert_eq!(state.status(), Status::Playing);
}

#[test]
fn next_at_the_end_stops() {
    let mut state = playing(&[A, B]);
    state.next().unwrap();
    assert_eq!(state.next(), Ok(PlaybackEffect::Stop));
    assert_eq!(state.status(), Status::Stopped);
    assert_eq!(state.current(), None);
    assert_eq!(state.queue(), [A, B], "the queue is kept for prev()");
    // Once stopped at the end, more next() calls change nothing.
    assert_eq!(state.next(), Ok(PlaybackEffect::None));
    assert_eq!(state.status(), Status::Stopped);
}

#[test]
fn next_while_paused_plays_the_next_track() {
    let mut state = playing(&[A, B]);
    state.pause().unwrap();
    assert_eq!(state.next(), Ok(start(B)));
    assert_eq!(state.status(), Status::Playing);
}

#[test]
fn next_on_an_empty_queue_is_an_error_not_a_panic() {
    let mut state = PlaybackState::new();
    assert_eq!(state.next(), Err(StateError::EmptyQueue));
    assert_eq!(state.status(), Status::Stopped);
}

// 4. prev

#[test]
fn prev_on_the_first_track_stays_there() {
    let mut state = playing(&[A, B]);
    for _ in 0..3 {
        // Restarts the first track, as music players do.
        assert_eq!(state.prev(), Ok(start(A)));
        assert_eq!(state.current(), Some(A));
        assert_eq!(state.current_index(), Some(0));
        assert_eq!(state.status(), Status::Playing);
    }
}

#[test]
fn prev_goes_back_one_track() {
    let mut state = playing(&[A, B, C]);
    state.next().unwrap();
    state.next().unwrap();
    assert_eq!(state.prev(), Ok(start(B)));
    assert_eq!(state.current_index(), Some(1));
}

#[test]
fn prev_after_the_queue_ended_plays_the_last_track() {
    let mut state = playing(&[A, B]);
    state.next().unwrap();
    state.next().unwrap();
    assert_eq!(state.prev(), Ok(start(B)));
    assert_eq!(state.status(), Status::Playing);
}

#[test]
fn prev_on_an_empty_queue_is_an_error_not_a_panic() {
    let mut state = PlaybackState::new();
    assert_eq!(state.prev(), Err(StateError::EmptyQueue));
    assert_eq!(state.current_index(), None);
}

// 5. Volume

#[test]
fn volume_is_cut_to_100() {
    let mut state = PlaybackState::new();
    assert_eq!(state.set_volume(150), Ok(PlaybackEffect::SetVolume(100)));
    assert_eq!(state.volume(), 100);
    assert_eq!(state.set_volume(u8::MAX), Ok(PlaybackEffect::SetVolume(100)));
    assert_eq!(state.volume(), 100);
}

#[test]
fn volume_accepts_zero_and_values_in_range() {
    let mut state = playing(&[A]);
    assert_eq!(state.set_volume(0), Ok(PlaybackEffect::SetVolume(0)));
    assert_eq!(state.volume(), 0);
    assert_eq!(state.set_volume(42), Ok(PlaybackEffect::SetVolume(42)));
    assert_eq!(state.volume(), 42);
    assert_eq!(state.status(), Status::Playing, "the volume does not touch playback");
}

// 6. play and pause are idempotent

#[test]
fn pause_when_stopped_is_harmless() {
    let mut state = PlaybackState::new();
    assert_eq!(state.pause(), Ok(PlaybackEffect::None));
    assert_eq!(state.pause(), Ok(PlaybackEffect::None));
    assert_eq!(state.status(), Status::Stopped);
}

#[test]
fn play_when_playing_is_harmless() {
    let mut state = playing(&[A]);
    assert_eq!(state.play(), Ok(PlaybackEffect::None));
    assert_eq!(state.play(), Ok(PlaybackEffect::None));
    assert_eq!(state.status(), Status::Playing);
    assert_eq!(state.current(), Some(A));
}

#[test]
fn pause_and_play_toggle_a_loaded_track() {
    let mut state = playing(&[A]);
    assert_eq!(state.pause(), Ok(PlaybackEffect::Pause));
    assert_eq!(state.status(), Status::Paused);
    assert_eq!(state.pause(), Ok(PlaybackEffect::None));
    assert_eq!(state.play(), Ok(PlaybackEffect::Resume));
    assert_eq!(state.status(), Status::Playing);
}

#[test]
fn play_on_an_empty_queue_is_an_error() {
    let mut state = PlaybackState::new();
    assert_eq!(state.play(), Err(StateError::EmptyQueue));
    assert_eq!(state.status(), Status::Stopped);
}

#[test]
fn play_after_the_queue_ended_starts_again_from_the_first_track() {
    let mut state = playing(&[A, B]);
    state.next().unwrap();
    state.next().unwrap();
    assert_eq!(state.play(), Ok(start(A)));
    assert_eq!(state.current_index(), Some(0));
}

// stop

#[test]
fn stop_keeps_the_track_and_play_restarts_it() {
    let mut state = playing(&[A, B]);
    state.next().unwrap();
    assert_eq!(state.stop(), Ok(PlaybackEffect::Stop));
    assert_eq!(state.status(), Status::Stopped);
    assert_eq!(state.current(), Some(B));
    assert_eq!(state.stop(), Ok(PlaybackEffect::None));
    assert_eq!(state.play(), Ok(start(B)));
    assert_eq!(state.status(), Status::Playing);
}
