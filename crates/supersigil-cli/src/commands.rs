pub mod affected;
pub mod context;
pub mod examples;
pub mod graph;
pub mod import;
pub mod init;
pub mod lint;
pub mod ls;
pub mod new;
pub mod plan;
pub mod refs;
pub mod schema;
pub mod skills;
pub mod status;
pub mod verify;

use clap::Subcommand;
use std::path::PathBuf;

use crate::format::OutputFormat;

/// Built-in document types recognized without explicit configuration.
pub const BUILTIN_DOC_TYPES: &[&str] = &["requirements", "design", "tasks"];

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
    /// Cross-document verification
    Verify(VerifyArgs),
    /// Project or document status overview
    Status(StatusArgs),
    /// Documents affected by file changes since a git ref
    Affected(AffectedArgs),
    /// Visualize the document dependency graph
    Graph(GraphArgs),
    /// Create a new supersigil.toml config
    Init(InitArgs),
    /// Scaffold a new spec document
    New(NewArgs),
    /// List criterion refs in the project
    Refs(RefsArgs),
    /// List executable examples in the spec
    Examples(ExamplesArgs),
    /// Manage agent skills
    Skills(SkillsArgs),
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
    #[arg(short, long)]
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
    /// Show all criteria and full task details
    #[arg(long)]
    pub verbose: bool,
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

/// Verify output format (terminal includes color, markdown for CI).
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum VerifyFormat {
    Terminal,
    Json,
    Markdown,
}

#[derive(Debug, clap::Args)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "CLI flags map naturally to independent boolean options"
)]
pub struct VerifyArgs {
    /// Filter to a project (multi-project mode)
    #[arg(short, long)]
    pub project: Option<String>,
    /// Git ref for staleness checks
    #[arg(long)]
    pub since: Option<String>,
    /// Only consider committed changes
    #[arg(long)]
    pub committed_only: bool,
    /// Use merge-base for git diff
    #[arg(long)]
    pub merge_base: bool,
    /// Output format
    #[arg(long, default_value = "terminal")]
    pub format: VerifyFormat,
    /// Skip example execution
    #[arg(long)]
    pub skip_examples: bool,
    /// Update snapshot expectations with actual output
    #[arg(long)]
    pub update_snapshots: bool,
    /// Number of examples to run concurrently (overrides config)
    #[arg(short = 'j', long)]
    pub parallelism: Option<usize>,
}

#[derive(Debug, clap::Args)]
pub struct StatusArgs {
    /// Document ID (omit for project-wide overview)
    pub id: Option<String>,
    /// Output format
    #[arg(long, default_value = "terminal")]
    pub format: OutputFormat,
}

#[derive(Debug, clap::Args)]
pub struct AffectedArgs {
    /// Git ref to diff against
    #[arg(long)]
    pub since: String,
    /// Only consider committed changes
    #[arg(long)]
    pub committed_only: bool,
    /// Use merge-base for git diff
    #[arg(long)]
    pub merge_base: bool,
    /// Output format
    #[arg(long, default_value = "terminal")]
    pub format: OutputFormat,
}

/// Graph visualization format.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum GraphFormat {
    Mermaid,
    Dot,
}

#[derive(Debug, clap::Args)]
pub struct GraphArgs {
    /// Output format
    #[arg(long, default_value = "mermaid")]
    pub format: GraphFormat,
}

#[derive(Debug, clap::Args)]
pub struct NewArgs {
    /// Document type (e.g., requirements, design, tasks)
    pub doc_type: String,
    /// Feature name (e.g., auth, cli)
    pub id: String,
    /// Target project (multi-project mode)
    #[arg(short, long)]
    pub project: Option<String>,
}

#[derive(Debug, clap::Args)]
pub struct RefsArgs {
    /// Filter refs by document ID prefix
    pub prefix: Option<String>,
    /// Show all criterion refs (no context scoping)
    #[arg(long)]
    pub all: bool,
    /// Output format
    #[arg(long, default_value = "terminal")]
    pub format: OutputFormat,
}

#[derive(Debug, clap::Args)]
pub struct ExamplesArgs {
    /// Filter by document ID prefix
    pub prefix: Option<String>,
    /// Output format
    #[arg(long, default_value = "terminal")]
    pub format: OutputFormat,
}

#[derive(Debug, clap::Args)]
pub struct InitArgs {
    /// Accept all defaults without prompting
    #[arg(short = 'y')]
    pub yes: bool,
    /// Install agent skills (default in non-interactive mode)
    #[arg(long, conflicts_with = "no_skills")]
    pub skills: bool,
    /// Skip skills installation
    #[arg(long, conflicts_with = "skills")]
    pub no_skills: bool,
    /// Directory for skills installation (implies --skills)
    #[arg(long, conflicts_with = "no_skills")]
    pub skills_path: Option<PathBuf>,
}

#[derive(Debug, clap::Args)]
pub struct SkillsArgs {
    #[command(subcommand)]
    pub command: SkillsCommand,
}

#[derive(Debug, Subcommand)]
pub enum SkillsCommand {
    /// Install or update embedded agent skills
    Install(SkillsInstallArgs),
}

#[derive(Debug, clap::Args)]
pub struct SkillsInstallArgs {
    /// Target directory for skills
    #[arg(long)]
    pub path: Option<PathBuf>,
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
