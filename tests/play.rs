//! Checks of `spotilite play` that need neither the network nor an audio device: every
//! case here stops before the connection to Spotify.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use spotilite::auth::{self, Token, TokenStore};

/// Environment variable that holds the base of the state folder on this platform.
const STATE_BASE_VAR: &str = if cfg!(windows) { "APPDATA" } else { "XDG_STATE_HOME" };

const TRACK: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQC";

/// Empty scratch folder under the cargo target dir, used as the state base of one test.
fn scratch_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("play-tests").join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn save_token(state_base: &Path, expires_at: u64) {
    let token = Token {
        access_token: "at-secret".to_string(),
        refresh_token: "rt-secret".to_string(),
        expires_at,
        scopes: vec!["streaming".to_string()],
    };
    TokenStore::new(state_base.join("spotilite")).save(&token).expect("save the token");
}

fn play(state_base: &Path, args: &[&str]) -> (ExitStatus, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_spotilite"))
        .env(STATE_BASE_VAR, state_base)
        .arg("play")
        .args(args)
        .output()
        .expect("the binary must run");
    (
        out.status,
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn assert_failed_cleanly(status: ExitStatus, stderr: &str, expected: &str) {
    assert_eq!(status.code(), Some(1), "play must exit with 1, stderr: {stderr}");
    assert!(!stderr.contains("panicked"), "play must not panic, stderr: {stderr}");
    assert!(stderr.contains(expected), "stderr must mention '{expected}', got: {stderr}");
}

#[test]
fn play_without_a_token_asks_to_sign_in_first() {
    let base = scratch_dir("no-token");
    let (status, _, stderr) = play(&base, &["spotify:track:xxx"]);
    assert_failed_cleanly(status, &stderr, "esegui prima `spotilite login`");
}

/// An expired token is renewed with its refresh token (see `tests/refresh.rs`); without
/// one there is nothing to renew it with.
#[test]
fn play_with_an_expired_token_that_cannot_be_renewed_asks_to_sign_in_again() {
    let base = scratch_dir("expired");
    let token = Token {
        access_token: "at-secret".to_string(),
        refresh_token: String::new(),
        expires_at: 1_000_000,
        scopes: vec!["streaming".to_string()],
    };
    TokenStore::new(base.join("spotilite")).save(&token).expect("save the token");
    let (status, _, stderr) = play(&base, &[TRACK]);
    assert_failed_cleanly(status, &stderr, "scaduto");
    assert!(stderr.contains("spotilite login"), "stderr must say what to do, got: {stderr}");
    assert!(!stderr.contains("at-secret"), "stderr must not show the token: {stderr}");
}

#[test]
fn play_rejects_an_invalid_uri_before_connecting() {
    let base = scratch_dir("invalid-uri");
    save_token(&base, auth::unix_now() + 3600);
    let (status, stdout, stderr) = play(&base, &[TRACK, "spotify:track:xxx"]);
    assert_failed_cleanly(status, &stderr, "indirizzo Spotify non valido: spotify:track:xxx");
    assert!(!stdout.contains("Collegamento"), "nothing must be contacted, stdout: {stdout}");
}

#[test]
fn play_explains_that_albums_are_not_supported_yet() {
    let base = scratch_dir("album");
    save_token(&base, auth::unix_now() + 3600);
    let (status, _, stderr) = play(&base, &["spotify:album:6akEvsycLGftJxYudPjmqK"]);
    assert_failed_cleanly(status, &stderr, "è un album");
}

#[test]
fn play_needs_at_least_one_uri() {
    let base = scratch_dir("no-uri");
    let (status, _, stderr) = play(&base, &[]);
    assert_eq!(status.code(), Some(2), "a usage error exits with 2, stderr: {stderr}");
    assert!(stderr.contains("<URI>"), "stderr must name the missing argument, got: {stderr}");
}

#[test]
fn play_rejects_a_volume_above_100() {
    let base = scratch_dir("volume");
    let (status, _, stderr) = play(&base, &["--volume", "150", TRACK]);
    assert_eq!(status.code(), Some(2), "a usage error exits with 2, stderr: {stderr}");
    assert!(stderr.contains("150"), "stderr must quote the bad value, got: {stderr}");
}
