use std::process::Command;

#[test]
fn version_prints_name_and_semver() {
    let out = Command::new(env!("CARGO_BIN_EXE_spotilite"))
        .arg("--version")
        .output()
        .expect("the binary must run");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "exit status must be 0, got {:?}", out.status);
    assert!(text.contains("spotilite"), "version output must name the app, got: {text}");
    assert!(
        text.split_whitespace().any(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit())),
        "version output must contain a number, got: {text}"
    );
}

/// `--selftest` is for automatic checks: hidden from the help, and it needs at least one
/// frame. Neither case opens a window.
#[test]
fn the_gui_selftest_is_hidden_and_counts_at_least_one_frame() {
    let help = Command::new(env!("CARGO_BIN_EXE_spotilite"))
        .args(["gui", "--help"])
        .output()
        .expect("the binary must run");
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(help.status.success(), "gui --help must succeed, got {:?}", help.status);
    assert!(!text.contains("selftest"), "the flag must stay hidden, got: {text}");

    let zero = Command::new(env!("CARGO_BIN_EXE_spotilite"))
        .args(["gui", "--selftest", "0"])
        .output()
        .expect("the binary must run");
    assert_eq!(zero.status.code(), Some(2), "zero frames is a usage error");
}

#[test]
fn help_lists_the_planned_commands() {
    let out = Command::new(env!("CARGO_BIN_EXE_spotilite"))
        .arg("--help")
        .output()
        .expect("the binary must run");
    let text = String::from_utf8_lossy(&out.stdout);
    for expected in ["login", "play", "gui"] {
        assert!(text.contains(expected), "help must mention '{expected}', got: {text}");
    }
}
