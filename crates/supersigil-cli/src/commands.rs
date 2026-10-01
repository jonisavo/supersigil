//! Command definitions.

use std::path::PathBuf;

use crate::format::{AutoFormat, OutputFormat};

/// The `completions` command.
pub mod completions;
/// The `review` command.
pub mod review;
/// The `session` command group.
pub mod session;
/// The `why` command.
pub mod why;

/// Available subcommands.
#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Generate shell completions
    Completions(CompletionsArgs),
    /// Review a change with the recorded edits behind each line
    Review(ReviewArgs),
    /// Sync and inspect agent sessions recorded for this checkout
    Session(SessionArgs),
    /// Explain where one line of a file came from
    Why(WhyArgs),
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

/// Arguments for the `review` command.
#[derive(Debug, clap::Args)]
pub struct ReviewArgs {
    /// Base revision (default: HEAD for the working tree, the target's first parent for a commit)
    #[arg(long)]
    pub base: Option<String>,
    /// Target revision (default: the working tree)
    #[arg(long, conflicts_with = "working_tree")]
    pub target: Option<String>,
    /// Review the working tree (the default)
    #[arg(long)]
    pub working_tree: bool,
    /// Untracked file to include, relative to the current directory (repeatable; working tree only)
    #[arg(
        long = "include-untracked",
        value_name = "PATH",
        conflicts_with = "target"
    )]
    pub include_untracked: Vec<String>,
    /// Output format: `auto` prints text on a terminal and JSON otherwise
    #[arg(long, default_value = "auto")]
    pub format: AutoFormat,
    /// Checkout to review (default: current directory)
    #[arg(long)]
    pub checkout: Option<PathBuf>,
    /// Records directory (default: `$SUPERSIGIL_RECORD_DIR`, then `$XDG_DATA_HOME/supersigil/records`)
    #[arg(long)]
    pub record_dir: Option<PathBuf>,
    /// Claude home directory used for discovery (default: ~/.claude)
    #[arg(long)]
    pub claude_home: Option<PathBuf>,
    /// Limit the review to these paths, relative to the current directory
    #[arg(last = true, value_name = "PATH")]
    pub paths: Vec<String>,
}

/// Arguments for the `why` command.
#[derive(Debug, clap::Args)]
pub struct WhyArgs {
    /// `<file>:<line>`: a file relative to the current directory and a 1-based line
    pub location: String,
    /// Output format: `auto` prints text on a terminal and JSON otherwise
    #[arg(long, default_value = "auto")]
    pub format: AutoFormat,
    /// Checkout the file belongs to (default: current directory)
    #[arg(long)]
    pub checkout: Option<PathBuf>,
    /// Records directory (default: `$SUPERSIGIL_RECORD_DIR`, then `$XDG_DATA_HOME/supersigil/records`)
    #[arg(long)]
    pub record_dir: Option<PathBuf>,
    /// Claude home directory used for discovery (default: ~/.claude)
    #[arg(long)]
    pub claude_home: Option<PathBuf>,
}
