use std::io::{self, Write};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Result, bail};
use clap::{Args, CommandFactory, Parser, Subcommand};
use spotilite::auth::{self, AuthError, PendingLogin, TokenStore};
use spotilite::config;
use spotilite::player::{self, PlaybackEvent, Player};
use spotilite::state::{self, PlaybackState, Status};

/// Local port that receives the redirect from Spotify after sign-in.
const DEFAULT_LOGIN_PORT: u16 = 5588;

/// Lightweight desktop client for Spotify Premium.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Sign in with a Spotify Premium account
    Login(LoginArgs),
    /// Play Spotify tracks or episodes, in order
    Play(PlayArgs),
    /// Open the graphical interface
    Gui,
    /// Inspect the saved sign-in
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
}

#[derive(Args)]
struct LoginArgs {
    /// Print the sign-in address without opening the browser
    #[arg(long)]
    print_url: bool,
    /// Local port that receives the redirect from Spotify (0 picks a free one)
    #[arg(long, default_value_t = DEFAULT_LOGIN_PORT)]
    port: u16,
    /// Seconds to wait for the browser to come back
    #[arg(long, default_value_t = 300)]
    timeout: u64,
    /// OAuth client ID [default: the one librespot uses]
    #[arg(long)]
    client_id: Option<String>,
    /// Token endpoint, replaced by the tests with a local fake
    #[arg(long, hide = true, default_value = auth::TOKEN_ENDPOINT)]
    token_endpoint: String,
}

#[derive(Args)]
struct PlayArgs {
    /// Spotify URIs of tracks or episodes, such as spotify:track:4uLU6hMCjMI75M1A2tKUQC
    #[arg(required = true, value_name = "URI")]
    uris: Vec<String>,
    /// Volume from 0 to 100
    #[arg(long, default_value_t = state::DEFAULT_VOLUME,
          value_parser = clap::value_parser!(u8).range(0..=i64::from(state::MAX_VOLUME)))]
    volume: u8,
    /// OAuth client ID used at sign-in, needed to renew the token [default: the one librespot uses]
    #[arg(long)]
    client_id: Option<String>,
    /// Token endpoint, replaced by the tests with a local fake
    #[arg(long, hide = true, default_value = auth::TOKEN_ENDPOINT)]
    token_endpoint: String,
    /// HTTP proxy for the Spotify session, pointed by the tests at a local fake
    #[arg(long, hide = true)]
    proxy: Option<String>,
}

#[derive(Subcommand)]
enum AuthCommand {
    /// Show whether a token is saved and when it expires
    Status,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Some(Command::Login(args)) => login(args).await,
        Some(Command::Auth {
            command: AuthCommand::Status,
        }) => auth_status(),
        Some(Command::Play(args)) => play(args).await,
        Some(Command::Gui) => not_yet_implemented("gui"),
        None => Cli::command()
            .print_help()
            .map(|()| ExitCode::SUCCESS)
            .map_err(Into::into),
    };
    result.unwrap_or_else(|err| {
        let _ = writeln!(io::stderr(), "errore: {err:#}");
        ExitCode::FAILURE
    })
}

async fn login(args: LoginArgs) -> Result<ExitCode> {
    let store = TokenStore::new(config::state_dir()?);
    let client_id = args.client_id.unwrap_or_else(default_client_id);
    let pending = PendingLogin::start(&client_id, args.port, &args.token_endpoint)?;

    let mut out = io::stdout();
    writeln!(out, "Per accedere apri questo indirizzo nel browser:")?;
    writeln!(out, "{}", pending.url())?;
    if !args.print_url
        && let Err(err) = auth::open_browser(pending.url())
    {
        writeln!(
            io::stderr(),
            "attenzione: impossibile aprire il browser ({err}); apri l'indirizzo a mano"
        )?;
    }
    writeln!(
        out,
        "In attesa della risposta di Spotify (al massimo {} s, Ctrl+C per annullare)...",
        args.timeout
    )?;
    out.flush()?;

    let cancel = Arc::new(AtomicBool::new(false));
    let token = tokio::select! {
        result = pending.finish(Duration::from_secs(args.timeout), Arc::clone(&cancel)) => result?,
        Ok(()) = tokio::signal::ctrl_c() => {
            // Stops the thread that waits on the local port before the runtime shuts down.
            cancel.store(true, Ordering::Relaxed);
            return Err(AuthError::Cancelled.into());
        }
    };
    store.save(&token)?;
    writeln!(
        out,
        "Accesso riuscito. Token salvato in {}, scade il {}.",
        store.path().display(),
        format_utc(token.expires_at)
    )?;
    Ok(ExitCode::SUCCESS)
}

/// Says whether a token is saved and when it expires, never what it contains. It never
/// renews the token: `play` does that when it needs one.
fn auth_status() -> Result<ExitCode> {
    let store = TokenStore::new(config::state_dir()?);
    let mut out = io::stdout();
    let Some(token) = store.load()? else {
        writeln!(out, "nessun token salvato: esegui `spotilite login`")?;
        return Ok(ExitCode::FAILURE);
    };
    let expiry = format_utc(token.expires_at);
    let now = auth::unix_now();
    writeln!(out, "token salvato in {}", store.path().display())?;
    if token.is_expired_at(now) {
        let next_step = if token.refresh_token.is_empty() {
            "esegui `spotilite login`"
        } else {
            "`spotilite play` lo rinnova da sé"
        };
        writeln!(out, "scaduto il {expiry}: {next_step}")?;
    } else {
        writeln!(out, "scade il {expiry} (tra {} minuti)", (token.expires_at - now) / 60)?;
    }
    Ok(ExitCode::SUCCESS)
}

/// Plays the URIs in order through the state machine, until the queue ends or Ctrl+C.
async fn play(args: PlayArgs) -> Result<ExitCode> {
    let store = TokenStore::new(config::state_dir()?);
    let Some(token) = store.load()? else {
        bail!("nessun token salvato: esegui prima `spotilite login`");
    };
    // A typo is reported before touching the audio device or the network.
    for uri in &args.uris {
        player::check_uri(uri)?;
    }
    let client_id = args.client_id.unwrap_or_else(default_client_id);
    let token = auth::fresh_token(&store, token, &args.token_endpoint, &client_id).await?;

    let mut out = io::stdout();
    writeln!(out, "Collegamento a Spotify...")?;
    out.flush()?;
    let mut player = Player::connect_via(&token, args.proxy.as_deref()).await?;
    writeln!(out, "Collegato come {}.", player.username())?;

    let mut state = PlaybackState::new();
    player.apply(&state.set_volume(args.volume)?)?;
    for uri in &args.uris {
        player.apply(&state.enqueue(uri.as_str())?)?;
    }

    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);
    let mut ctrl_c_done = false;
    let mut unavailable = 0;
    while state.status() != Status::Stopped {
        let event = tokio::select! {
            event = player.next_event() => event,
            pressed = &mut ctrl_c, if !ctrl_c_done => {
                ctrl_c_done = true;
                if pressed.is_err() {
                    // No Ctrl+C handler: playback simply runs to the end of the queue.
                    continue;
                }
                player.apply(&state.stop()?)?;
                writeln!(out, "Riproduzione interrotta.")?;
                return Ok(ExitCode::SUCCESS);
            }
        };
        match event {
            None => bail!("il motore di riproduzione si è fermato"),
            Some(PlaybackEvent::Loading { uri }) => writeln!(out, "Caricamento di {uri}...")?,
            Some(PlaybackEvent::TrackInfo {
                title,
                artists,
                duration,
                ..
            }) => writeln!(out, "In riproduzione: {}", describe_track(&title, &artists, duration))?,
            Some(PlaybackEvent::EndOfTrack { .. }) => player.apply(&state.next()?)?,
            Some(PlaybackEvent::Unavailable { uri }) => {
                unavailable += 1;
                writeln!(io::stderr(), "attenzione: {uri} non è disponibile, passo al successivo")?;
                player.apply(&state.next()?)?;
            }
            Some(_) => {}
        }
    }
    if unavailable == args.uris.len() {
        bail!("nessuno dei brani richiesti è disponibile");
    }
    writeln!(out, "Fine della coda.")?;
    Ok(ExitCode::SUCCESS)
}

/// `Title (Artist, Other artist, 3:07)`.
fn describe_track(title: &str, artists: &[String], duration: Duration) -> String {
    let seconds = duration.as_secs();
    let length = format!("{}:{:02}", seconds / 60, seconds % 60);
    if artists.is_empty() {
        format!("{title} ({length})")
    } else {
        format!("{title} ({}, {length})", artists.join(", "))
    }
}

/// The client ID librespot signs in with, used when `--client-id` is not given.
fn default_client_id() -> String {
    librespot::core::SessionConfig::default().client_id
}

fn not_yet_implemented(command: &str) -> Result<ExitCode> {
    writeln!(io::stdout(), "{command}: non ancora implementato")?;
    Ok(ExitCode::SUCCESS)
}

/// Formats seconds since the Unix epoch as `YYYY-MM-DD HH:MM UTC`.
fn format_utc(unix_seconds: u64) -> String {
    let (year, month, day) = civil_from_days(unix_seconds / 86_400);
    let seconds_of_day = unix_seconds % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        seconds_of_day / 3600,
        seconds_of_day % 3600 / 60
    )
}

/// Days since 1970-01-01 to a Gregorian date, after Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let day_of_era = z % 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{describe_track, format_utc};

    #[test]
    fn describe_track_lists_artists_and_length() {
        let artists = ["Rick Astley".to_string(), "Altro".to_string()];
        assert_eq!(
            describe_track("Never Gonna Give You Up", &artists, Duration::from_millis(213_573)),
            "Never Gonna Give You Up (Rick Astley, Altro, 3:33)"
        );
        assert_eq!(describe_track("Senza autore", &[], Duration::from_secs(65)), "Senza autore (1:05)");
    }

    #[test]
    fn format_utc_matches_known_dates() {
        assert_eq!(format_utc(0), "1970-01-01 00:00 UTC");
        assert_eq!(format_utc(951_782_400), "2000-02-29 00:00 UTC");
        assert_eq!(format_utc(1_800_000_000), "2027-01-15 08:00 UTC");
    }
}
