pub mod commands;
pub mod discover;
pub mod error;
pub mod format;
pub mod loader;

pub use commands::{Command, ContextArgs, ImportArgs, ImportSource, LsArgs, PlanArgs};
pub use discover::discover_spec_files;
pub use format::OutputFormat;
pub use loader::{find_config, load_graph, parse_all};

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "supersigil", about = "Spec-driven development with AI agents")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}
