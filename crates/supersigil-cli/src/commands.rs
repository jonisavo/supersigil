//! Command definitions.

/// The `completions` command.
pub mod completions;

/// Available subcommands.
#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Generate shell completions
    Completions(CompletionsArgs),
}

/// Arguments for the `completions` command.
#[derive(Debug, clap::Args)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    pub shell: clap_complete::Shell,
}
