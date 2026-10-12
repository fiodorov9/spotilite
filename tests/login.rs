//! End-to-end checks of `spotilite login` and `spotilite auth status`.
//!
//! The browser is played by the test (it calls the local redirect port itself) and the
//! Spotify token endpoint is a local fake, so these tests need neither a browser nor the
//! network. They always pass `--print-url`, so no browser window is ever opened.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use spotilite::auth::{Token, TokenStore};

/// Environment variable that holds the base of the state folder on this platform.
const STATE_BASE_VAR: &str = if cfg!(windows) { "APPDATA" } else { "XDG_STATE_HOME" };

const TOKEN_JSON: &str = r#"{"access_token":"at-secret","token_type":"Bearer","expires_in":3600,"refresh_token":"rt-secret","scope":"streaming user-read-private"}"#;

/// Empty scratch folder under the cargo target dir, used as the state base of one test.
fn scratch_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("login-tests").join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn token_path(state_base: &Path) -> PathBuf {
    state_base.join("spotilite").join("token.json")
}

fn spotilite(state_base: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_spotilite"));
    command.env(STATE_BASE_VAR, state_base);
    command
}

/// Fake token endpoint: answers one request with `status` and `body`, and returns the
/// form the client posted.
fn fake_token_endpoint(status: &'static str, body: &'static str) -> (String, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind the fake endpoint");
    let url = format!("http://{}/api/token", listener.local_addr().expect("local address"));
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("the client must call the token endpoint");
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
    });
    (url, handle)
}

/// Address on which nothing listens: a port that was free a moment ago.
fn closed_endpoint() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("local address");
    drop(listener);
    format!("http://{address}/api/token")
}

struct Login {
    child: Child,
    sign_in_url: String,
    stdout: JoinHandle<String>,
}

/// Starts `spotilite login --print-url` on a free port and waits for the sign-in address.
fn start_login(state_base: &Path, token_endpoint: &str, timeout_secs: u64) -> Login {
    let mut child = spotilite(state_base)
        .args(["login", "--print-url", "--port", "0", "--timeout"])
        .arg(timeout_secs.to_string())
        .args(["--token-endpoint", token_endpoint])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary must start");
    let mut lines = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
    let sign_in_url = lines
        .by_ref()
        .map_while(Result::ok)
        .find(|line| line.starts_with("https://accounts.spotify.com/authorize?"))
        .expect("login must print the sign-in address");
    // Keep reading so the child never blocks on a full pipe.
    let stdout = thread::spawn(move || lines.map_while(Result::ok).collect::<Vec<_>>().join("\n"));
    Login { child, sign_in_url, stdout }
}

/// Value of a query parameter, with the few escapes these tests need undone.
fn query_param(url: &str, name: &str) -> String {
    let query = url.split_once('?').expect("url with a query").1;
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
        .unwrap_or_else(|| panic!("parameter {name} missing in {url}"))
        .replace("%3A", ":")
        .replace("%2F", "/")
}

/// Plays the browser coming back from Spotify: calls the redirect URI with `query`.
fn send_redirect(sign_in_url: &str, query: &str) -> String {
    let redirect_uri = query_param(sign_in_url, "redirect_uri");
    let address = redirect_uri
        .strip_prefix("http://")
        .and_then(|rest| rest.split('/').next())
        .expect("http redirect uri");
    let mut stream = TcpStream::connect(address).expect("the login command must listen on the redirect port");
    write!(stream, "GET /login?{query} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n").expect("send");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read the page");
    response
}

/// Waits for the login command to exit (killing it after 60 s) and returns its stderr.
fn finish(mut login: Login) -> (ExitStatus, String, String) {
    let started = Instant::now();
    let status = loop {
        if let Some(status) = login.child.try_wait().expect("try_wait") {
            break status;
        }
        if started.elapsed() > Duration::from_secs(60) {
            let _ = login.child.kill();
            panic!("login did not exit within 60 s");
        }
        thread::sleep(Duration::from_millis(50));
    };
    let mut stderr = String::new();
    login.child.stderr.take().expect("piped stderr").read_to_string(&mut stderr).expect("read stderr");
    let stdout = login.stdout.join().expect("stdout reader");
    (status, stdout, stderr)
}

fn assert_failed_cleanly(status: ExitStatus, stderr: &str, expected: &str) {
    assert!(!status.success(), "login must fail, stderr: {stderr}");
    assert_eq!(status.code(), Some(1), "login must exit with 1, stderr: {stderr}");
    assert!(!stderr.contains("panicked"), "login must not panic, stderr: {stderr}");
    assert!(stderr.contains(expected), "stderr must mention '{expected}', got: {stderr}");
}

#[test]
fn auth_status_without_token_says_so_and_exits_with_1() {
    let base = scratch_dir("status-empty");

    let out = spotilite(&base).args(["auth", "status"]).output().expect("the binary must run");
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert_eq!(out.status.code(), Some(1), "exit code must be 1, stdout: {stdout}");
    assert!(stdout.contains("nessun token"), "stdout must say 'nessun token', got: {stdout}");
}

#[test]
fn auth_status_shows_the_expiry_but_not_the_token() {
    let base = scratch_dir("status-saved");
    let token = Token {
        access_token: "at-secret".to_string(),
        refresh_token: "rt-secret".to_string(),
        expires_at: 1_800_000_000,
        scopes: vec!["streaming".to_string()],
    };
    TokenStore::new(base.join("spotilite")).save(&token).expect("save the token");

    let out = spotilite(&base).args(["auth", "status"]).output().expect("the binary must run");
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(out.status.success(), "exit code must be 0, stdout: {stdout}");
    assert!(stdout.contains("2027-01-15 08:00 UTC"), "stdout must show the expiry, got: {stdout}");
    assert!(!stdout.contains("at-secret") && !stdout.contains("rt-secret"), "stdout must not show the token: {stdout}");
}

#[test]
fn login_saves_the_token_from_the_redirect() {
    let base = scratch_dir("login-ok");
    let (endpoint, form) = fake_token_endpoint("200 OK", TOKEN_JSON);
    let login = start_login(&base, &endpoint, 30);
    let state = query_param(&login.sign_in_url, "state");
    let redirect_uri = query_param(&login.sign_in_url, "redirect_uri");

    let page = send_redirect(&login.sign_in_url, &format!("code=code-123&state={state}"));
    let (status, stdout, stderr) = finish(login);

    assert!(page.starts_with("HTTP/1.1 200"), "the browser must get a page, got: {page}");
    assert!(status.success(), "login must succeed, stderr: {stderr}");
    assert!(stdout.contains("Accesso riuscito"), "stdout must confirm the sign-in, got: {stdout}");
    assert!(!stdout.contains("at-secret"), "stdout must not show the token: {stdout}");

    let form = form.join().expect("fake endpoint");
    for expected in ["grant_type=authorization_code", "code=code-123", "code_verifier=", "client_id="] {
        assert!(form.contains(expected), "the token request must contain '{expected}', got: {form}");
    }
    assert!(form.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A"), "the token request must repeat the redirect uri, got: {form}");
    assert!(redirect_uri.ends_with("/login"), "unexpected redirect uri: {redirect_uri}");

    let saved = TokenStore::new(base.join("spotilite")).load().expect("load").expect("a saved token");
    assert_eq!(saved.access_token, "at-secret");
    assert_eq!(saved.refresh_token, "rt-secret");
    assert_eq!(saved.scopes, ["streaming", "user-read-private"]);
    assert!(saved.expires_at > spotilite::auth::unix_now(), "the token must expire in the future");
}

#[test]
fn login_times_out_without_the_browser() {
    let base = scratch_dir("login-timeout");
    let login = start_login(&base, &closed_endpoint(), 1);

    let (status, _, stderr) = finish(login);

    assert_failed_cleanly(status, &stderr, "nessuna risposta dal browser");
    assert!(!token_path(&base).exists(), "no token may be saved");
}

#[test]
fn login_reports_access_denied() {
    let base = scratch_dir("login-denied");
    let login = start_login(&base, &closed_endpoint(), 30);
    let state = query_param(&login.sign_in_url, "state");

    send_redirect(&login.sign_in_url, &format!("error=access_denied&state={state}"));
    let (status, _, stderr) = finish(login);

    assert_failed_cleanly(status, &stderr, "negato l'accesso (access_denied)");
    assert!(!token_path(&base).exists(), "no token may be saved");
}

#[test]
fn login_rejects_a_forged_state() {
    let base = scratch_dir("login-state");
    let login = start_login(&base, &closed_endpoint(), 30);

    send_redirect(&login.sign_in_url, "code=code-123&state=forged");
    let (status, _, stderr) = finish(login);

    assert_failed_cleanly(status, &stderr, "state");
    assert!(!token_path(&base).exists(), "no token may be saved");
}

#[test]
fn login_reports_a_network_failure() {
    let base = scratch_dir("login-offline");
    let login = start_login(&base, &closed_endpoint(), 30);
    let state = query_param(&login.sign_in_url, "state");

    send_redirect(&login.sign_in_url, &format!("code=code-123&state={state}"));
    let (status, _, stderr) = finish(login);

    assert_failed_cleanly(status, &stderr, "errore di rete");
    assert!(!token_path(&base).exists(), "no token may be saved");
}

#[test]
fn login_reports_a_refused_exchange() {
    let base = scratch_dir("login-refused");
    let (endpoint, _form) = fake_token_endpoint(
        "400 Bad Request",
        r#"{"error":"invalid_grant","error_description":"Invalid authorization code"}"#,
    );
    let login = start_login(&base, &endpoint, 30);
    let state = query_param(&login.sign_in_url, "state");

    send_redirect(&login.sign_in_url, &format!("code=bad-code&state={state}"));
    let (status, _, stderr) = finish(login);

    assert_failed_cleanly(status, &stderr, "invalid_grant: Invalid authorization code");
    assert!(!token_path(&base).exists(), "no token may be saved");
}

#[test]
fn login_ignores_requests_that_are_not_the_redirect() {
    let base = scratch_dir("login-favicon");
    let (endpoint, _form) = fake_token_endpoint("200 OK", TOKEN_JSON);
    let login = start_login(&base, &endpoint, 30);
    let state = query_param(&login.sign_in_url, "state");
    let redirect_uri = query_param(&login.sign_in_url, "redirect_uri");
    let address = redirect_uri.trim_start_matches("http://").split('/').next().expect("address").to_string();

    let mut favicon = TcpStream::connect(&address).expect("connect");
    write!(favicon, "GET /favicon.ico HTTP/1.1\r\nHost: {address}\r\n\r\n").expect("send");
    let mut page = String::new();
    favicon.read_to_string(&mut page).expect("read");
    send_redirect(&login.sign_in_url, &format!("code=code-123&state={state}"));
    let (status, _, stderr) = finish(login);

    assert!(page.starts_with("HTTP/1.1 404"), "other paths must get a 404, got: {page}");
    assert!(status.success(), "login must still succeed, stderr: {stderr}");
}
