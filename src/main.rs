use std::io::{self, Write};

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};

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
    Login,
    /// Play a track, album or playlist
    Play,
    /// Open the graphical interface
    Gui,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Login) => not_yet_implemented("login"),
        Some(Command::Play) => not_yet_implemented("play"),
        Some(Command::Gui) => not_yet_implemented("gui"),
        None => {
            Cli::command().print_help()?;
            Ok(())
        }
    }
}

fn not_yet_implemented(command: &str) -> Result<()> {
    writeln!(io::stdout(), "{command}: non ancora implementato")?;
    Ok(())
}
