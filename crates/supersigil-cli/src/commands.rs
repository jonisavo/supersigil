pub mod context;
pub mod import;
pub mod lint;
pub mod ls;
pub mod plan;
pub mod schema;

use clap::Subcommand;
use std::path::PathBuf;

use crate::format::OutputFormat;

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Structural checks on spec files (per-file, no graph)
    Lint,
    /// List all documents
    #[command(alias = "list")]
    Ls(LsArgs),
    /// Output component and document type schema
    Schema(SchemaArgs),
    /// Agent-friendly view of a document and its relationships
    Context(ContextArgs),
    /// Outstanding work for a document, prefix, or the whole project
    Plan(PlanArgs),
    /// Import specs from another format
    Import(ImportArgs),
}

#[derive(Debug, clap::Args)]
pub struct LsArgs {
    /// Filter by document type
    #[arg(long = "type")]
    pub doc_type: Option<String>,
    /// Filter by status
    #[arg(long)]
    pub status: Option<String>,
    /// Filter by project (multi-project mode)
    #[arg(long)]
    pub project: Option<String>,
    /// Output format
    #[arg(long, default_value = "terminal")]
    pub format: OutputFormat,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum SchemaFormat {
    Json,
    Yaml,
}

#[derive(Debug, clap::Args)]
pub struct SchemaArgs {
    /// Output format
    #[arg(long, default_value = "yaml")]
    pub format: SchemaFormat,
}

#[derive(Debug, clap::Args)]
pub struct ContextArgs {
    /// Document ID
    pub id: String,
    /// Output format
    #[arg(long, default_value = "terminal")]
    pub format: OutputFormat,
}

#[derive(Debug, clap::Args)]
pub struct PlanArgs {
    /// Document ID, prefix, or omit for all
    pub id_or_prefix: Option<String>,
    /// Output format
    #[arg(long, default_value = "terminal")]
    pub format: OutputFormat,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum ImportSource {
    Kiro,
}

#[derive(Debug, clap::Args)]
pub struct ImportArgs {
    /// Source format to import from
    #[arg(long)]
    pub from: ImportSource,
    /// Preview import without writing files
    #[arg(long)]
    pub dry_run: bool,
    /// Source directory for Kiro specs
    #[arg(long)]
    pub source_dir: Option<PathBuf>,
    /// Output directory for generated files
    #[arg(long)]
    pub output_dir: Option<PathBuf>,
    /// Prefix for generated document IDs
    #[arg(long, value_parser = parse_import_prefix)]
    pub prefix: Option<String>,
    /// Overwrite existing files
    #[arg(long)]
    pub force: bool,
}

fn parse_import_prefix(raw: &str) -> Result<String, String> {
    if raw.is_empty() {
        return Err("prefix must not be empty".to_string());
    }

    if raw.ends_with('/') {
        return Err("prefix must not end with '/'".to_string());
    }

    Ok(raw.to_string())
}
