//! Spotify sign-in: OAuth authorization code flow with PKCE, and the token cache.

use std::fmt::{self, Write as _};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const AUTHORIZE_ENDPOINT: &str = "https://accounts.spotify.com/authorize";
pub const TOKEN_ENDPOINT: &str = "https://accounts.spotify.com/api/token";

/// Permissions requested at sign-in: playback through librespot, account type
/// (Premium check), playback state, and the user's library and playlists.
pub const SCOPES: &[&str] = &[
    "streaming",
    "user-read-private",
    "user-read-playback-state",
    "user-modify-playback-state",
    "user-read-currently-playing",
    "user-library-read",
    "playlist-read-private",
    "playlist-read-collaborative",
];

const TOKEN_FILE: &str = "token.json";

/// A token that expires within this many seconds is renewed before it opens a session.
pub const REFRESH_MARGIN_SECS: u64 = 300;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("impossibile accedere a {}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("il file {} è danneggiato", .path.display())]
    Corrupt {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("indirizzo di ritorno non valido: {0}")]
    BadCallback(String),
    #[error("Spotify ha negato l'accesso ({0})")]
    Denied(String),
    #[error("l'indirizzo di ritorno non contiene il codice di autorizzazione")]
    MissingCode,
    #[error("il parametro state della risposta non corrisponde: risposta scartata")]
    StateMismatch,
    #[error("impossibile ricevere la risposta di Spotify sulla porta locale {port}")]
    Listen {
        port: u16,
        #[source]
        source: io::Error,
    },
    #[error("nessuna risposta dal browser entro {0} secondi")]
    Timeout(u64),
    #[error("accesso annullato")]
    Cancelled,
    #[error("errore di rete durante lo scambio del codice con Spotify: {0}")]
    Network(String),
    #[error("Spotify ha rifiutato lo scambio del codice ({0})")]
    Exchange(String),
    #[error("errore interno durante l'accesso: {0}")]
    Internal(String),
    #[error("sessione scaduta: esegui `spotilite login`")]
    SessionExpired,
    #[error("il token di accesso è scaduto e non si può rinnovare: esegui `spotilite login`")]
    NotRenewable,
    #[error("errore di rete durante il rinnovo del token: {0}")]
    RefreshNetwork(String),
    #[error("Spotify ha rifiutato il rinnovo del token ({0}): esegui `spotilite login`")]
    RefreshRefused(String),
}

/// Credentials returned by Spotify, as saved in `token.json`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    pub access_token: String,
    /// Empty if Spotify did not send one.
    #[serde(default)]
    pub refresh_token: String,
    /// Expiry time, in seconds since the Unix epoch.
    pub expires_at: u64,
    pub scopes: Vec<String>,
}

impl Token {
    pub fn is_expired_at(&self, unix_seconds: u64) -> bool {
        unix_seconds >= self.expires_at
    }

    /// Whether the token has expired at `unix_seconds` or expires within `margin` seconds.
    pub fn expires_within(&self, unix_seconds: u64, margin: u64) -> bool {
        self.is_expired_at(unix_seconds.saturating_add(margin))
    }
}

// Keeps the secrets out of logs and panic messages.
impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Token")
            .field("access_token", &"<hidden>")
            .field("refresh_token", &"<hidden>")
            .field("expires_at", &self.expires_at)
            .field("scopes", &self.scopes)
            .finish()
    }
}

/// Current time in seconds since the Unix epoch (0 if the clock is before 1970).
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Saves and loads the token as `token.json` inside one folder.
pub struct TokenStore {
    dir: PathBuf,
    path: PathBuf,
}

impl TokenStore {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref().to_path_buf();
        let path = dir.join(TOKEN_FILE);
        Self { dir, path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns `Ok(None)` when no token has been saved yet.
    pub fn load(&self) -> Result<Option<Token>, AuthError> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(self.io_error(source)),
        };
        serde_json::from_str(&text).map(Some).map_err(|source| AuthError::Corrupt {
            path: self.path.clone(),
            source,
        })
    }

    /// Writes a temporary file next to `token.json` and renames it over the old one,
    /// so an interrupted save never leaves a half-written token behind.
    pub fn save(&self, token: &Token) -> Result<(), AuthError> {
        fs::create_dir_all(&self.dir).map_err(|source| self.io_error(source))?;
        let json = serde_json::to_vec_pretty(token).map_err(|err| self.io_error(io::Error::other(err)))?;
        let temp = self.dir.join(format!("{TOKEN_FILE}.{}.tmp", std::process::id()));
        let written = write_private(&temp, &json).and_then(|()| fs::rename(&temp, &self.path));
        if let Err(source) = written {
            let _ = fs::remove_file(&temp);
            return Err(self.io_error(source));
        }
        Ok(())
    }

    fn io_error(&self, source: io::Error) -> AuthError {
        AuthError::Io {
            path: self.path.clone(),
            source,
        }
    }
}

/// Creates `path` readable only by its owner on Unix. On Windows the file inherits
/// the ACL of `%APPDATA%`, which is already private to the user.
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// PKCE pair (RFC 7636): the verifier stays here, the challenge goes in the sign-in URL.
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    /// 32 random bytes give a 43-character verifier, the shortest RFC 7636 allows.
    pub fn generate() -> Self {
        let verifier = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
        let challenge = pkce_challenge(&verifier);
        Self { verifier, challenge }
    }
}

/// S256 challenge: base64url without padding of the SHA-256 of the verifier.
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Random value for the `state` parameter, checked again on the way back.
pub fn random_state() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 16]>())
}

/// Address of Spotify's sign-in page for the authorization code flow with PKCE.
pub fn authorize_url(client_id: &str, redirect_uri: &str, challenge: &str, state: &str) -> String {
    let scope = SCOPES.join(" ");
    let params = [
        ("client_id", client_id),
        ("response_type", "code"),
        ("redirect_uri", redirect_uri),
        ("code_challenge_method", "S256"),
        ("code_challenge", challenge),
        ("state", state),
        ("scope", scope.as_str()),
    ];
    let query: Vec<String> = params
        .iter()
        .map(|(key, value)| format!("{key}={}", percent_encode(value)))
        .collect();
    format!("{AUTHORIZE_ENDPOINT}?{}", query.join("&"))
}

/// Percent-encodes every byte except the RFC 3986 unreserved characters.
fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

/// What Spotify sends back to the redirect URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Callback {
    pub code: String,
    pub state: Option<String>,
}

/// Reads the authorization code from the address Spotify redirected the browser to.
/// An `error` parameter (for instance `access_denied`) becomes [`AuthError::Denied`].
pub fn parse_callback(redirected: &str) -> Result<Callback, AuthError> {
    let url = Url::parse(redirected).map_err(|err| AuthError::BadCallback(err.to_string()))?;
    let (mut code, mut state, mut error) = (None, None, None);
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            _ => {}
        }
    }
    if let Some(reason) = error {
        return Err(AuthError::Denied(reason));
    }
    let code = code.filter(|code| !code.is_empty()).ok_or(AuthError::MissingCode)?;
    Ok(Callback { code, state })
}

/// A sign-in in progress: the local port is open and the sign-in address is ready.
pub struct PendingLogin {
    listener: TcpListener,
    port: u16,
    client_id: String,
    redirect_uri: String,
    token_endpoint: String,
    pkce: Pkce,
    state: String,
    url: String,
}

impl PendingLogin {
    /// Opens the local port that receives the redirect from Spotify (0 picks a free one)
    /// and prepares the sign-in address with a fresh PKCE pair and state.
    pub fn start(client_id: &str, port: u16, token_endpoint: &str) -> Result<Self, AuthError> {
        let listen_error = |source| AuthError::Listen { port, source };
        let listener = TcpListener::bind(("127.0.0.1", port)).map_err(listen_error)?;
        let port = listener.local_addr().map_err(listen_error)?.port();
        let redirect_uri = format!("http://127.0.0.1:{port}/login");
        let pkce = Pkce::generate();
        let state = random_state();
        let url = authorize_url(client_id, &redirect_uri, &pkce.challenge, &state);
        Ok(Self {
            listener,
            port,
            client_id: client_id.to_string(),
            redirect_uri,
            token_endpoint: token_endpoint.to_string(),
            pkce,
            state,
            url,
        })
    }

    /// Address of Spotify's sign-in page for this attempt.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Waits for the browser to come back with the code, then trades it for a token.
    /// Setting `cancel` ends the wait within a fraction of a second.
    pub async fn finish(self, timeout: Duration, cancel: Arc<AtomicBool>) -> Result<Token, AuthError> {
        let Self {
            listener,
            port,
            client_id,
            redirect_uri,
            token_endpoint,
            pkce,
            state,
            ..
        } = self;
        let callback = tokio::task::spawn_blocking(move || {
            wait_for_callback(&listener, port, &state, timeout, &cancel)
        })
        .await
        .map_err(|err| AuthError::Internal(err.to_string()))??;
        exchange_code(&token_endpoint, &client_id, &callback.code, &redirect_uri, &pkce.verifier).await
    }
}

/// Accepts connections on the local port until the redirect arrives, the time runs out
/// or `cancel` is set.
fn wait_for_callback(
    listener: &TcpListener,
    port: u16,
    state: &str,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<Callback, AuthError> {
    let listen_error = |source| AuthError::Listen { port, source };
    listener.set_nonblocking(true).map_err(listen_error)?;
    // No deadline if the timeout is too large to add to the clock.
    let deadline = Instant::now().checked_add(timeout);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AuthError::Cancelled);
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(AuthError::Timeout(timeout.as_secs()));
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(result) = answer_redirect(stream, port, state) {
                    return result;
                }
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(100)),
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::ConnectionAborted | io::ErrorKind::ConnectionReset | io::ErrorKind::Interrupted
                ) => {}
            Err(source) => return Err(listen_error(source)),
        }
    }
}

/// Reads one HTTP request from the browser and answers it. Returns `None` when the
/// request is not the redirect (a favicon, for instance), so the caller keeps waiting.
fn answer_redirect(mut stream: TcpStream, port: u16, expected_state: &str) -> Option<Result<Callback, AuthError>> {
    // On Windows the accepted socket inherits the listener's non-blocking mode.
    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let target = read_request_target(&stream)?;
    if target.split('?').next() != Some("/login") {
        respond(&mut stream, "404 Not Found", "Pagina non trovata.");
        return None;
    }
    let result = parse_callback(&format!("http://127.0.0.1:{port}{target}")).and_then(|callback| {
        if callback.state.as_deref() == Some(expected_state) {
            Ok(callback)
        } else {
            Err(AuthError::StateMismatch)
        }
    });
    let message = if result.is_ok() {
        "spotilite ha ricevuto la risposta di Spotify. Puoi chiudere questa scheda e tornare al terminale."
    } else {
        "Accesso non riuscito. Torna al terminale per i dettagli."
    };
    respond(&mut stream, "200 OK", message);
    Some(result)
}

/// Returns the target (path and query) of an HTTP request, after reading its headers.
fn read_request_target(stream: &TcpStream) -> Option<String> {
    // A browser request is a few hundred bytes; the limit guards against anything else.
    let mut reader = BufReader::new(stream.take(16 * 1024));
    let mut request_line = String::new();
    reader.read_line(&mut request_line).ok()?;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).ok()? == 0 || header.trim_end().is_empty() {
            break;
        }
    }
    request_line.split_whitespace().nth(1).map(str::to_string)
}

fn respond(stream: &mut TcpStream, status: &str, message: &str) {
    let body = format!(
        "<!doctype html><html lang=\"it\"><meta charset=\"utf-8\"><title>spotilite</title><p>{message}</p></html>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    // The page is a courtesy: the terminal reports the outcome either way.
    let _ = stream.write_all(response.as_bytes()).and_then(|()| stream.flush());
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    expires_in: u64,
    #[serde(default)]
    scope: String,
}

impl TokenResponse {
    fn into_token(self) -> Token {
        Token {
            access_token: self.access_token,
            refresh_token: self.refresh_token.unwrap_or_default(),
            expires_at: unix_now().saturating_add(self.expires_in),
            scopes: self.scope.split_whitespace().map(str::to_string).collect(),
        }
    }
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// How a request to the token endpoint went wrong.
enum EndpointError {
    Network(String),
    /// Spotify answered with an error, or with something that is not a token.
    Refused {
        /// OAuth error code (RFC 6749 section 5.2), such as `invalid_grant`.
        code: Option<String>,
        /// Readable reason, with Spotify's description when there is one.
        reason: String,
    },
}

/// Posts `form` to the token endpoint and reads the token it answers with.
async fn request_token(endpoint: &str, form: &[(&str, &str)]) -> Result<TokenResponse, EndpointError> {
    let network_error = |err: reqwest::Error| EndpointError::Network(error_chain(&err));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(network_error)?;
    let response = client.post(endpoint).form(form).send().await.map_err(network_error)?;
    let status = response.status();
    let body = response.bytes().await.map_err(network_error)?;
    if !status.is_success() {
        return Err(match serde_json::from_slice::<ErrorResponse>(&body) {
            Ok(ErrorResponse {
                error,
                error_description,
            }) => EndpointError::Refused {
                reason: match error_description {
                    Some(description) => format!("{error}: {description}"),
                    None => error.clone(),
                },
                code: Some(error),
            },
            Err(_) => EndpointError::Refused {
                code: None,
                reason: format!("HTTP {status}"),
            },
        });
    }
    serde_json::from_slice(&body).map_err(|err| EndpointError::Refused {
        code: None,
        reason: format!("risposta non valida: {err}"),
    })
}

/// Trades the authorization code for a token at the token endpoint (RFC 6749 section 4.1.3,
/// with the PKCE verifier of RFC 7636 section 4.5).
async fn exchange_code(
    endpoint: &str,
    client_id: &str,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<Token, AuthError> {
    let form = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
        ("code_verifier", verifier),
    ];
    match request_token(endpoint, &form).await {
        Ok(response) => Ok(response.into_token()),
        Err(EndpointError::Network(detail)) => Err(AuthError::Network(detail)),
        Err(EndpointError::Refused { reason, .. }) => Err(AuthError::Exchange(reason)),
    }
}

/// Returns `token` ready to open a new session: as it is while it stays valid for more
/// than [`REFRESH_MARGIN_SECS`], otherwise renewed with [`refresh`] and saved in `store`
/// before it is returned. A token without a refresh token is used until it expires.
pub async fn fresh_token(
    store: &TokenStore,
    token: Token,
    token_endpoint: &str,
    client_id: &str,
) -> Result<Token, AuthError> {
    let now = unix_now();
    let renewable = !token.refresh_token.is_empty();
    if !token.expires_within(now, REFRESH_MARGIN_SECS) || (!renewable && !token.is_expired_at(now)) {
        return Ok(token);
    }
    let renewed = refresh(token_endpoint, client_id, &token).await?;
    store.save(&renewed)?;
    Ok(renewed)
}

/// Trades the refresh token for a new access token at the token endpoint (RFC 6749
/// section 6). Spotify may send a new refresh token; if it does not, the old one stays,
/// and so do the scopes.
pub async fn refresh(token_endpoint: &str, client_id: &str, token: &Token) -> Result<Token, AuthError> {
    if token.refresh_token.is_empty() {
        return Err(AuthError::NotRenewable);
    }
    let form = [
        ("grant_type", "refresh_token"),
        ("refresh_token", token.refresh_token.as_str()),
        ("client_id", client_id),
    ];
    let mut renewed = match request_token(token_endpoint, &form).await {
        Ok(response) => response.into_token(),
        Err(EndpointError::Network(detail)) => return Err(AuthError::RefreshNetwork(detail)),
        // The refresh token has been revoked or has lapsed: only a new sign-in helps.
        Err(EndpointError::Refused { code: Some(code), .. }) if code == "invalid_grant" => {
            return Err(AuthError::SessionExpired);
        }
        Err(EndpointError::Refused { reason, .. }) => return Err(AuthError::RefreshRefused(reason)),
    };
    if renewed.refresh_token.is_empty() {
        renewed.refresh_token.clone_from(&token.refresh_token);
    }
    if renewed.scopes.is_empty() {
        renewed.scopes.clone_from(&token.scopes);
    }
    Ok(renewed)
}

/// Joins an error with its causes: reqwest keeps the useful detail in the sources.
fn error_chain(err: &dyn std::error::Error) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        let _ = write!(text, ": {cause}");
        source = cause.source();
    }
    text
}

/// Opens `url` in the default browser: `start` on Windows, `xdg-open` on Linux.
pub fn open_browser(url: &str) -> io::Result<()> {
    if url.contains('"') || url.chars().any(char::is_control) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "indirizzo non valido"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // `start` is a cmd builtin. The quotes keep cmd from splitting the URL at `&`,
        // and the empty first argument is the window title, so the URL is not taken for it.
        let status = Command::new("cmd")
            .raw_arg(format!("/C start \"\" \"{url}\""))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("start ha restituito {status}")))
        }
    }
    #[cfg(not(windows))]
    {
        // Not waited for: with some browsers xdg-open returns only when the browser closes.
        Command::new("xdg-open")
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(drop)
    }
}
