//! Checks of the automatic renewal of the access token when `spotilite play` starts.
//!
//! The Spotify token endpoint is a local fake (`--token-endpoint`), and the Spotify
//! session goes through a local fake proxy (`--proxy`) that closes every connection: once
//! the token is ready, `play` fails to connect and exits, without leaving the machine.

use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use spotilite::auth::{self, Token, TokenStore};

/// Environment variable that holds the base of the state folder on this platform.
const STATE_BASE_VAR: &str = if cfg!(windows) { "APPDATA" } else { "XDG_STATE_HOME" };

const TRACK: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQC";

const RENEWED: &str =
    r#"{"access_token":"at-new","token_type":"Bearer","expires_in":3600,"scope":"streaming user-read-private"}"#;
const RENEWED_WITH_A_NEW_REFRESH_TOKEN: &str = r#"{"access_token":"at-new","token_type":"Bearer","expires_in":3600,"refresh_token":"rt-new","scope":"streaming user-read-private"}"#;
const INVALID_GRANT: &str = r#"{"error":"invalid_grant","error_description":"Refresh token revoked"}"#;

/// Empty scratch folder under the cargo target dir, used as the state base of one test.
fn scratch_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("refresh-tests").join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn store(state_base: &Path) -> TokenStore {
    TokenStore::new(state_base.join("spotilite"))
}

/// Saves a token with the access token `at-old` and returns the bytes of the file.
fn save_token(state_base: &Path, refresh_token: &str, expires_at: u64) -> Vec<u8> {
    let token = Token {
        access_token: "at-old".to_string(),
        refresh_token: refresh_token.to_string(),
        expires_at,
        scopes: vec!["streaming".to_string()],
    };
    let store = store(state_base);
    store.save(&token).expect("save the token");
    fs::read(store.path()).expect("read the saved token")
}

fn saved_token(state_base: &Path) -> Token {
    store(state_base).load().expect("load").expect("a saved token")
}

/// Files in the token folder other than `token.json`.
fn leftovers(state_base: &Path) -> Vec<String> {
    fs::read_dir(state_base.join("spotilite"))
        .expect("the token folder must exist")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name != "token.json")
        .collect()
}

/// A local server that hands every connection to a handler until it is stopped, and
/// returns what the handler gave back for each one.
struct FakeServer<T> {
    url: String,
    stop: Arc<AtomicBool>,
    thread: JoinHandle<Vec<T>>,
}

impl<T: Send + 'static> FakeServer<T> {
    fn start(path: &str, mut handle: impl FnMut(TcpStream) -> T + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind the fake server");
        let url = format!("http://{}{path}", listener.local_addr().expect("local address"));
        listener.set_nonblocking(true).expect("non-blocking listener");
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            let mut results = Vec::new();
            loop {
                // Reads the stop flag before accepting, so the connections that were
                // already waiting are still served after the stop.
                let stopping = stopped.load(Ordering::Acquire);
                match listener.accept() {
                    Ok((stream, _)) => {
                        // On Windows the accepted socket inherits the listener's non-blocking mode.
                        stream.set_nonblocking(false).expect("blocking stream");
                        results.push(handle(stream));
                    }
                    Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                        if stopping {
                            return results;
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("the fake server cannot accept: {err}"),
                }
            }
        });
        Self { url, stop, thread }
    }

    /// Stops the server and returns the results, one per connection.
    fn finish(self) -> Vec<T> {
        self.stop.store(true, Ordering::Release);
        self.thread.join().expect("fake server thread")
    }
}

/// Fake token endpoint: answers every request with `status` and `body` and records the
/// form the client posted.
fn fake_token_endpoint(status: &'static str, body: &'static str) -> FakeServer<String> {
    FakeServer::start("/api/token", move |stream| {
        let mut reader = BufReader::new(stream);
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("read a header");
            if line.trim_end().is_empty() {
                break;
            }
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse().expect("numeric content-length");
            }
        }
        let mut form = vec![0; length];
        reader.read_exact(&mut form).expect("read the form");
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        reader.get_mut().write_all(response.as_bytes()).expect("write the response");
        String::from_utf8(form).expect("utf-8 form")
    })
}

/// Fake HTTP proxy for the Spotify session: closes every connection at once.
fn fake_proxy() -> FakeServer<()> {
    FakeServer::start("", drop)
}

/// Address on which nothing listens: a port that was free a moment ago.
fn closed_endpoint() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("local address");
    drop(listener);
    format!("http://{address}/api/token")
}

struct Run {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

fn read_all(mut pipe: impl Read + Send + 'static) -> JoinHandle<String> {
    thread::spawn(move || {
        let mut text = String::new();
        pipe.read_to_string(&mut text).expect("read the output");
        text
    })
}

/// Runs `spotilite play TRACK` against the fake endpoint and the fake proxy, killing
/// it after 60 s.
fn play(state_base: &Path, token_endpoint: &str, proxy: &str) -> Run {
    let mut child = Command::new(env!("CARGO_BIN_EXE_spotilite"))
        .env(STATE_BASE_VAR, state_base)
        .args(["play", "--token-endpoint", token_endpoint, "--proxy", proxy, TRACK])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary must start");
    let stdout = read_all(child.stdout.take().expect("piped stdout"));
    let stderr = read_all(child.stderr.take().expect("piped stderr"));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            break status;
        }
        if started.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            panic!("play did not exit within 60 s");
        }
        thread::sleep(Duration::from_millis(50));
    };
    Run {
        status,
        stdout: stdout.join().expect("stdout reader"),
        stderr: stderr.join().expect("stderr reader"),
    }
}

fn assert_no_secrets(run: &Run) {
    for secret in ["at-old", "at-new", "rt-old", "rt-new"] {
        assert!(
            !run.stdout.contains(secret) && !run.stderr.contains(secret),
            "the output must not show '{secret}', stdout: {}, stderr: {}",
            run.stdout,
            run.stderr
        );
    }
}

/// `play` got a valid token and went on to open the Spotify session: through the fake
/// proxy, or as far as the audio device on a computer without one.
fn assert_went_on_to_connect(run: &Run, proxy_connections: usize) {
    assert!(!run.stderr.contains("panicked"), "play must not panic, stderr: {}", run.stderr);
    assert!(
        run.stdout.contains("Collegamento a Spotify"),
        "play must go on to connect, stdout: {}, stderr: {}",
        run.stdout,
        run.stderr
    );
    assert!(!run.stderr.contains("scadut"), "the session must get a valid token, stderr: {}", run.stderr);
    assert!(
        proxy_connections > 0 || run.stderr.contains("audio"),
        "the session must go through the fake proxy, stderr: {}",
        run.stderr
    );
    assert_eq!(run.status.code(), Some(1), "the fake proxy stops the session, stderr: {}", run.stderr);
    assert_no_secrets(run);
}

fn assert_failed_cleanly(run: &Run, expected: &str) {
    assert_eq!(run.status.code(), Some(1), "play must exit with 1, stderr: {}", run.stderr);
    assert!(!run.stderr.contains("panicked"), "play must not panic, stderr: {}", run.stderr);
    assert!(run.stderr.contains(expected), "stderr must mention '{expected}', got: {}", run.stderr);
    assert!(!run.stdout.contains("Collegamento"), "nothing must be contacted, stdout: {}", run.stdout);
    assert_no_secrets(run);
}

#[test]
fn an_expired_token_is_renewed_saved_and_used() {
    let base = scratch_dir("expired");
    save_token(&base, "rt-old", 1_000_000);
    let endpoint = fake_token_endpoint("200 OK", RENEWED);
    let proxy = fake_proxy();

    let run = play(&base, &endpoint.url, &proxy.url);
    let forms = endpoint.finish();
    let proxy_connections = proxy.finish().len();

    assert_eq!(forms.len(), 1, "exactly one renewal is expected, got: {forms:?}");
    for expected in ["grant_type=refresh_token", "refresh_token=rt-old", "client_id="] {
        assert!(forms[0].contains(expected), "the renewal must contain '{expected}', got: {}", forms[0]);
    }
    let saved = saved_token(&base);
    assert_eq!(saved.access_token, "at-new", "the new access token must be saved");
    assert!(saved.expires_at > auth::unix_now() + 3000, "the new expiry must be saved: {}", saved.expires_at);
    assert_eq!(saved.scopes, ["streaming", "user-read-private"]);
    assert!(leftovers(&base).is_empty(), "no temporary file may be left: {:?}", leftovers(&base));
    assert_went_on_to_connect(&run, proxy_connections);
}

#[test]
fn the_refresh_token_is_kept_when_spotify_sends_none() {
    let base = scratch_dir("keep-refresh-token");
    save_token(&base, "rt-old", 1_000_000);
    let endpoint = fake_token_endpoint("200 OK", RENEWED);
    let proxy = fake_proxy();

    let run = play(&base, &endpoint.url, &proxy.url);
    let forms = endpoint.finish();
    let proxy_connections = proxy.finish().len();

    assert_eq!(forms.len(), 1, "exactly one renewal is expected, got: {forms:?}");
    let saved = saved_token(&base);
    assert_eq!(saved.access_token, "at-new");
    assert_eq!(saved.refresh_token, "rt-old", "the old refresh token must stay");
    assert_went_on_to_connect(&run, proxy_connections);
}

#[test]
fn the_refresh_token_is_replaced_when_spotify_sends_a_new_one() {
    let base = scratch_dir("new-refresh-token");
    save_token(&base, "rt-old", 1_000_000);
    let endpoint = fake_token_endpoint("200 OK", RENEWED_WITH_A_NEW_REFRESH_TOKEN);
    let proxy = fake_proxy();

    let run = play(&base, &endpoint.url, &proxy.url);
    let forms = endpoint.finish();
    let proxy_connections = proxy.finish().len();

    assert_eq!(forms.len(), 1, "exactly one renewal is expected, got: {forms:?}");
    let saved = saved_token(&base);
    assert_eq!(saved.access_token, "at-new");
    assert_eq!(saved.refresh_token, "rt-new", "the new refresh token must replace the old one");
    assert_went_on_to_connect(&run, proxy_connections);
}

#[test]
fn a_refused_renewal_asks_to_sign_in_again_and_keeps_the_file() {
    let base = scratch_dir("refused");
    let before = save_token(&base, "rt-old", 1_000_000);
    let endpoint = fake_token_endpoint("400 Bad Request", INVALID_GRANT);
    let proxy = fake_proxy();

    let run = play(&base, &endpoint.url, &proxy.url);
    let forms = endpoint.finish();
    let proxy_connections = proxy.finish().len();

    assert_eq!(forms.len(), 1, "the renewal must be tried once, got: {forms:?}");
    assert_failed_cleanly(&run, "sessione scaduta");
    assert!(run.stderr.contains("spotilite login"), "stderr must say what to do, got: {}", run.stderr);
    assert_eq!(fs::read(store(&base).path()).expect("read the token"), before, "the token file must stay as it was");
    assert!(leftovers(&base).is_empty(), "no temporary file may be left: {:?}", leftovers(&base));
    assert_eq!(proxy_connections, 0, "no session may be opened");
}

#[test]
fn a_network_failure_during_the_renewal_is_reported_and_keeps_the_file() {
    let base = scratch_dir("offline");
    let before = save_token(&base, "rt-old", 1_000_000);
    let proxy = fake_proxy();

    let run = play(&base, &closed_endpoint(), &proxy.url);
    let proxy_connections = proxy.finish().len();

    assert_failed_cleanly(&run, "errore di rete");
    assert_eq!(fs::read(store(&base).path()).expect("read the token"), before, "the token file must stay as it was");
    assert!(leftovers(&base).is_empty(), "no temporary file may be left: {:?}", leftovers(&base));
    assert_eq!(proxy_connections, 0, "no session may be opened");
}

#[test]
fn a_valid_token_is_used_without_calling_the_endpoint() {
    let base = scratch_dir("valid");
    let before = save_token(&base, "rt-old", auth::unix_now() + 3600);
    let endpoint = fake_token_endpoint("200 OK", RENEWED);
    let proxy = fake_proxy();

    let run = play(&base, &endpoint.url, &proxy.url);
    let forms = endpoint.finish();
    let proxy_connections = proxy.finish().len();

    assert!(forms.is_empty(), "a valid token must not be renewed, got: {forms:?}");
    assert_eq!(fs::read(store(&base).path()).expect("read the token"), before, "the token file must stay as it was");
    assert_went_on_to_connect(&run, proxy_connections);
}

#[test]
fn a_token_about_to_expire_is_renewed() {
    let base = scratch_dir("expiring");
    save_token(&base, "rt-old", auth::unix_now() + 60);
    let endpoint = fake_token_endpoint("200 OK", RENEWED);
    let proxy = fake_proxy();

    let run = play(&base, &endpoint.url, &proxy.url);
    let forms = endpoint.finish();
    let proxy_connections = proxy.finish().len();

    assert_eq!(forms.len(), 1, "a token that expires within a minute must be renewed, got: {forms:?}");
    assert_eq!(saved_token(&base).access_token, "at-new");
    assert_went_on_to_connect(&run, proxy_connections);
}

#[test]
fn without_a_token_play_still_asks_to_sign_in_first() {
    let base = scratch_dir("no-token");
    let endpoint = fake_token_endpoint("200 OK", RENEWED);
    let proxy = fake_proxy();

    let run = play(&base, &endpoint.url, &proxy.url);
    let forms = endpoint.finish();
    let proxy_connections = proxy.finish().len();

    assert_failed_cleanly(&run, "nessun token salvato: esegui prima `spotilite login`");
    assert!(forms.is_empty(), "nothing may be renewed, got: {forms:?}");
    assert_eq!(proxy_connections, 0, "no session may be opened");
}

#[test]
fn auth_status_says_the_token_has_expired_without_renewing_it() {
    let base = scratch_dir("status-expired");
    let before = save_token(&base, "rt-old", 1_000_000);

    let out = Command::new(env!("CARGO_BIN_EXE_spotilite"))
        .env(STATE_BASE_VAR, &base)
        .args(["auth", "status"])
        .output()
        .expect("the binary must run");
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(out.status.success(), "auth status only informs, stdout: {stdout}");
    assert!(stdout.contains("scaduto il 1970-01-12"), "stdout must say when the token expired, got: {stdout}");
    assert!(!stdout.contains("at-old") && !stdout.contains("rt-old"), "stdout must not show the token: {stdout}");
    assert_eq!(fs::read(store(&base).path()).expect("read the token"), before, "auth status must not renew the token");
}
