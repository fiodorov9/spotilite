//! User folders where spotilite keeps its state and its configuration.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Name of the spotilite folder inside the user's base folders.
const APP_DIR: &str = "spotilite";

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("impossibile trovare la cartella {0} dell'utente")]
    NoBaseDir(&'static str),
    #[error("impossibile creare la cartella {}", .path.display())]
    CreateDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Folder for state such as the token cache, created if missing.
///
/// `%APPDATA%\spotilite` on Windows, `$XDG_STATE_HOME/spotilite` on Linux
/// (by default `~/.local/state/spotilite`).
pub fn state_dir() -> Result<PathBuf, ConfigError> {
    let dir = state_base().ok_or(ConfigError::NoBaseDir("di stato"))?.join(APP_DIR);
    create_if_missing(&dir)?;
    Ok(dir)
}

/// Folder for the user's settings, created if missing.
///
/// `%APPDATA%\spotilite` on Windows, `$XDG_CONFIG_HOME/spotilite` on Linux
/// (by default `~/.config/spotilite`).
pub fn config_dir() -> Result<PathBuf, ConfigError> {
    let dir = config_base().ok_or(ConfigError::NoBaseDir("di configurazione"))?.join(APP_DIR);
    create_if_missing(&dir)?;
    Ok(dir)
}

/// Creates `dir` and its parents; does nothing if it already exists.
pub fn create_if_missing(dir: &Path) -> Result<(), ConfigError> {
    fs::create_dir_all(dir).map_err(|source| ConfigError::CreateDir {
        path: dir.to_path_buf(),
        source,
    })
}

#[cfg(windows)]
fn state_base() -> Option<PathBuf> {
    roaming_app_data()
}

#[cfg(windows)]
fn config_base() -> Option<PathBuf> {
    roaming_app_data()
}

/// `%APPDATA%`, read from the environment first so that it can be redirected
/// (the tests do), then from the known-folder API.
#[cfg(windows)]
fn roaming_app_data() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(dirs::config_dir)
}

#[cfg(not(windows))]
fn state_base() -> Option<PathBuf> {
    // dirs honours $XDG_STATE_HOME and falls back to ~/.local/state on Linux.
    dirs::state_dir().or_else(dirs::data_local_dir)
}

#[cfg(not(windows))]
fn config_base() -> Option<PathBuf> {
    dirs::config_dir()
}
