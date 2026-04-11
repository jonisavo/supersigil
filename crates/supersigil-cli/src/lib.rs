//! CLI interface for supersigil: spec-driven development with AI agents.
//!
//! This crate provides the `supersigil` binary and its command implementations.
//! It also exports types for programmatic use (e.g., editor extensions and
//! integration tests).

/// CLI command definitions and argument types.
pub mod commands;
/// Spec file discovery via glob expansion.
pub mod discover;
/// CLI error types.
pub mod error;
/// Terminal formatting, color configuration, and output helpers.
pub mod format;
/// Config loading, parallel parsing, and graph construction.
pub mod loader;
/// CLI-specific plugin helpers for repository resolution and evidence collection.
pub mod plugins;
/// Interactive prompt helpers for yes/no and text input.
pub mod prompt;
/// Context-aware scoping via TrackedFiles and working directory.
pub mod scope;
/// Embedded agent skills and installation logic.
pub mod skills;

pub use commands::{
    AffectedArgs, Command, CompletionsArgs, ContextArgs, ExploreArgs, GraphArgs, GraphFormat,
    ImportArgs, ImportSource, InitArgs, LsArgs, NewArgs, PlanArgs, RefsArgs, RenderArgs,
    RenderFormat, SchemaArgs, SchemaFormat, SkillsArgs, SkillsCommand, SkillsInstallArgs,
    StatusArgs, VerifyArgs, VerifyFormat,
};
pub use discover::discover_spec_files;
pub use format::{ColorChoice, ColorConfig, ExitStatus, OutputFormat};
pub use loader::{find_config, load_graph, parse_all};

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "supersigil", about = "Spec-driven development with AI agents", version = env!("CARGO_PKG_VERSION"))]
/// Top-level CLI entry point parsed by clap.
pub struct Cli {
    /// Color output: always, never, or auto (default)
    #[arg(long, default_value = "auto", global = true)]
    pub color: ColorChoice,

    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: Command,
}
