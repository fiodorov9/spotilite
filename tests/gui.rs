//! Checks of `spotilite gui` that open no window: every case here stops before it.

use std::process::{Command, Output};

fn gui(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spotilite"))
        .arg("gui")
        .args(args)
        .output()
        .expect("the binary must run")
}

fn assert_refused(out: &Output, expected: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "gui must exit with 1, stderr: {stderr}");
    assert!(!stderr.contains("panicked"), "gui must not panic, stderr: {stderr}");
    assert!(stderr.contains(expected), "stderr must mention '{expected}', got: {stderr}");
}

#[test]
fn an_invalid_uri_is_refused_before_the_window_opens() {
    let out = gui(&["spotify:track:4uLU6hMCjMI75M1A2tKUQC", "spotify:track:xxx"]);
    assert_refused(&out, "indirizzo Spotify non valido: spotify:track:xxx");
}

#[test]
fn a_playlist_is_refused_before_the_window_opens() {
    let out = gui(&["spotify:playlist:37i9dQZF1DXcBWIGoYBM5M"]);
    assert_refused(&out, "le playlist arriveranno");
}

#[test]
fn the_help_explains_the_uris() {
    let out = gui(&["--help"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "gui --help must succeed");
    assert!(text.contains("[URI]") && text.contains("--client-id"), "got: {text}");
}
