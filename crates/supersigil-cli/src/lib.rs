pub mod commands;
pub mod discover;
pub mod error;
pub mod format;
pub mod loader;

pub use commands::{
    AffectedArgs, Command, ContextArgs, GraphArgs, GraphFormat, ImportArgs, ImportSource, LsArgs,
    NewArgs, PlanArgs, SchemaArgs, SchemaFormat, StatusArgs, VerifyArgs, VerifyFormat,
};
pub use discover::discover_spec_files;
pub use format::{ColorChoice, ColorConfig, ExitStatus, OutputFormat};
pub use loader::{find_config, load_graph, parse_all};

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "supersigil", about = "Spec-driven development with AI agents")]
pub struct Cli {
    /// Color output: always, never, or auto (default)
    #[arg(long, default_value = "auto", global = true)]
    pub color: ColorChoice,

    #[command(subcommand)]
    pub command: Command,
}
