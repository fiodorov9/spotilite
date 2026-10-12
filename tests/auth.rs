use std::fs;
use std::path::PathBuf;

use spotilite::auth::{self, Token, TokenStore};
use spotilite::config;

/// Environment variable that holds the base of the state folder on this platform.
const STATE_BASE_VAR: &str = if cfg!(windows) { "APPDATA" } else { "XDG_STATE_HOME" };

/// Empty scratch folder under the cargo target dir, so tests never write to the real profile.
fn scratch_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("auth-tests").join(name);
    let _ = fs::remove_dir_all(&dir);
    dir
}

fn sample_token() -> Token {
    Token {
        access_token: "access-123".to_string(),
        refresh_token: "refresh-456".to_string(),
        expires_at: 1_800_000_000,
        scopes: vec!["streaming".to_string(), "user-read-private".to_string()],
    }
}

#[test]
fn state_dir_ends_with_spotilite_and_is_created() {
    let base = scratch_dir("state-base");
    assert!(!base.exists(), "the scratch base must start missing");
    // SAFETY: this is the only test in this binary that touches the environment,
    // and none of the other tests read it.
    unsafe { std::env::set_var(STATE_BASE_VAR, &base) };

    let dir = config::state_dir().expect("state_dir must succeed");

    assert!(dir.ends_with("spotilite"), "state dir must end with 'spotilite', got {}", dir.display());
    assert!(dir.starts_with(&base), "state dir must live under {STATE_BASE_VAR}, got {}", dir.display());
    assert!(dir.is_dir(), "state dir must be created, missing: {}", dir.display());
}

#[test]
fn token_store_round_trip() {
    let dir = scratch_dir("round-trip");
    let store = TokenStore::new(&dir);
    let token = sample_token();

    store.save(&token).expect("save must succeed");
    let loaded = store.load().expect("load must succeed");

    assert_eq!(loaded, Some(token));
    assert!(dir.join("token.json").is_file(), "the token must be saved as token.json");
}

#[test]
fn token_store_save_replaces_the_previous_token() {
    let dir = scratch_dir("replace");
    let store = TokenStore::new(&dir);
    store.save(&sample_token()).expect("first save must succeed");

    let newer = Token { access_token: "access-789".to_string(), ..sample_token() };
    store.save(&newer).expect("second save must succeed");

    assert_eq!(store.load().expect("load must succeed"), Some(newer));
    let leftovers: Vec<_> = fs::read_dir(&dir)
        .expect("the store folder must exist")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .filter(|name| name != "token.json")
        .collect();
    assert!(leftovers.is_empty(), "no temporary file may be left behind, found {leftovers:?}");
}

#[test]
fn token_store_load_on_empty_dir_is_none() {
    let dir = scratch_dir("empty");
    fs::create_dir_all(&dir).expect("scratch dir");

    let loaded = TokenStore::new(&dir).load().expect("load on an empty dir must not fail");

    assert_eq!(loaded, None);
}

#[test]
fn token_store_load_reports_a_damaged_file() {
    let dir = scratch_dir("damaged");
    fs::create_dir_all(&dir).expect("scratch dir");
    fs::write(dir.join("token.json"), "{ not json").expect("write the damaged file");

    let err = TokenStore::new(&dir).load().expect_err("a damaged file must be an error");

    assert!(err.to_string().contains("token.json"), "the error must name the file, got: {err}");
}

#[test]
fn authorize_url_has_the_pkce_parameters_and_the_scopes() {
    let url = auth::authorize_url("client-abc", "http://127.0.0.1:5588/login", "challenge-xyz", "state-s1");

    assert!(url.starts_with("https://accounts.spotify.com/authorize?"), "wrong endpoint: {url}");
    for expected in [
        "client_id=client-abc",
        "response_type=code",
        "redirect_uri=http%3A%2F%2F127.0.0.1%3A5588%2Flogin",
        "code_challenge=challenge-xyz",
        "code_challenge_method=S256",
        "state=state-s1",
        "scope=",
    ] {
        assert!(url.contains(expected), "url must contain '{expected}', got: {url}");
    }
    for scope in ["streaming", "user-read-private"] {
        assert!(auth::SCOPES.contains(&scope), "scope '{scope}' must be requested");
    }
    for scope in auth::SCOPES {
        assert!(url.contains(scope), "url must request scope '{scope}', got: {url}");
    }
}

#[test]
fn parse_callback_returns_the_code_and_the_state() {
    let callback = auth::parse_callback("https://127.0.0.1/callback?code=abc&state=s1")
        .expect("a callback with a code must parse");

    assert_eq!(callback.code, "abc");
    assert_eq!(callback.state.as_deref(), Some("s1"));
}

#[test]
fn parse_callback_turns_access_denied_into_a_readable_error() {
    let err = auth::parse_callback("https://127.0.0.1/callback?error=access_denied&state=s1")
        .expect_err("a denied callback must be an error");
    let message = err.to_string();

    assert!(message.contains("access_denied"), "the error must keep the reason, got: {message}");
    assert!(message.contains("negato"), "the error must be readable, got: {message}");
}

#[test]
fn parse_callback_without_code_is_an_error() {
    assert!(auth::parse_callback("https://127.0.0.1/callback?state=s1").is_err());
    assert!(auth::parse_callback("not a url").is_err());
}

#[test]
fn pkce_challenge_matches_the_rfc_7636_example() {
    // Test vector from RFC 7636, Appendix B.
    let challenge = auth::pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");

    assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK52-QSjxE4fNBVqKAo");
}

#[test]
fn pkce_verifier_is_random_and_well_formed() {
    let first = auth::Pkce::generate();
    let second = auth::Pkce::generate();

    assert_ne!(first.verifier, second.verifier, "two verifiers must differ");
    assert!((43..=128).contains(&first.verifier.len()), "verifier length out of range: {}", first.verifier.len());
    assert!(
        first.verifier.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b)),
        "verifier must use only unreserved characters: {}",
        first.verifier
    );
    assert_eq!(first.challenge, auth::pkce_challenge(&first.verifier));
}
