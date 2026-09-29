//! Command definitions.

use std::path::PathBuf;

use crate::format::OutputFormat;

/// The `completions` command.
pub mod completions;
/// The `session` command group.
pub mod session;

/// Available subcommands.
#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Generate shell completions
    Completions(CompletionsArgs),
    /// Sync and inspect agent sessions recorded for this checkout
    Session(SessionArgs),
}

/// Arguments for the `completions` command.
#[derive(Debug, clap::Args)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    pub shell: clap_complete::Shell,
}

/// Arguments for the `session` command group.
#[derive(Debug, clap::Args)]
pub struct SessionArgs {
    /// Records directory (default: `$SUPERSIGIL_RECORD_DIR`, then `$XDG_DATA_HOME/supersigil/records`)
    #[arg(long, global = true)]
    pub record_dir: Option<PathBuf>,
    /// Checkout to operate on (default: current directory)
    #[arg(long, global = true)]
    pub checkout: Option<PathBuf>,
    /// Subcommand
    #[command(subcommand)]
    pub command: SessionCommand,
}

/// Subcommands of `session`.
#[derive(Debug, clap::Subcommand)]
pub enum SessionCommand {
    /// Append new observations from transcripts to this checkout's record
    Sync(SessionSyncArgs),
    /// List sessions in this checkout's record
    List(SessionListArgs),
    /// Print one session's observations and derivations as JSON
    Show(SessionShowArgs),
}

/// Arguments for `session sync`.
#[derive(Debug, clap::Args)]
pub struct SessionSyncArgs {
    /// Transcript file to read (repeatable). Without it, transcripts are discovered.
    #[arg(long = "transcript")]
    pub transcripts: Vec<PathBuf>,
    /// Claude home directory used for discovery (default: ~/.claude)
    #[arg(long)]
    pub claude_home: Option<PathBuf>,
    /// Output format
    #[arg(long, default_value = "terminal")]
    pub format: OutputFormat,
}

/// Arguments for `session list`.
#[derive(Debug, clap::Args)]
pub struct SessionListArgs {
    /// Output format
    #[arg(long, default_value = "terminal")]
    pub format: OutputFormat,
}

/// Arguments for `session show`.
#[derive(Debug, clap::Args)]
pub struct SessionShowArgs {
    /// Session id; a unique prefix is enough
    pub session: String,
}
