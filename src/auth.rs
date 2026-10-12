//! Spotify sign-in: OAuth authorization code flow with PKCE, and the token cache.

use std::fmt::{self, Write as _};
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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
