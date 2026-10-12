use std::io::{self, Write};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use clap::{Args, CommandFactory, Parser, Subcommand};
use spotilite::auth::{self, AuthError, PendingLogin, TokenStore};
use spotilite::config;

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
    /// Play a track, album or playlist
    Play,
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
        Some(Command::Play) => not_yet_implemented("play"),
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
    let client_id = args
        .client_id
        .unwrap_or_else(|| librespot::core::SessionConfig::default().client_id);
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

/// Says whether a token is saved and when it expires, never what it contains.
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
        writeln!(out, "scaduto il {expiry}")?;
    } else {
        writeln!(out, "scade il {expiry} (tra {} minuti)", (token.expires_at - now) / 60)?;
    }
    Ok(ExitCode::SUCCESS)
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
    use super::format_utc;

    #[test]
    fn format_utc_matches_known_dates() {
        assert_eq!(format_utc(0), "1970-01-01 00:00 UTC");
        assert_eq!(format_utc(951_782_400), "2000-02-29 00:00 UTC");
        assert_eq!(format_utc(1_800_000_000), "2027-01-15 08:00 UTC");
    }
}
