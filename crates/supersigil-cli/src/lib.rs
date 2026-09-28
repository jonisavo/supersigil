//! CLI interface for supersigil.
//!
//! This crate provides the `supersigil` binary and its command
//! implementations, and exports the argument types for integration tests.

/// CLI command definitions and argument types.
pub mod commands;
/// CLI error types.
pub mod error;
/// Terminal formatting, color configuration, and output helpers.
pub mod format;

pub use commands::{Command, CompletionsArgs};
pub use format::{ColorChoice, ColorConfig, ExitStatus, OutputFormat};

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "supersigil",
    about = "Review agent-made changes with the reasoning that produced them",
    version = env!("CARGO_PKG_VERSION")
)]
/// Top-level CLI entry point parsed by clap.
pub struct Cli {
    /// Color output: always, never, or auto (default)
    #[arg(long, default_value = "auto", global = true)]
    pub color: ColorChoice,

    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: Command,
}
