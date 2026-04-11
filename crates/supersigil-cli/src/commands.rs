/// Documents affected by file changes since a git ref.
pub mod affected;
/// Shell completion script generation.
pub mod completions;
/// Structured context view of a single document.
pub mod context;
/// Interactive graph explorer served in the browser.
pub mod explore;
/// Document dependency graph visualization.
pub mod graph;
/// Import specs from external formats.
pub mod import;
/// Initialize a new supersigil project.
pub mod init;
/// List all documents in the project.
pub mod ls;
/// Scaffold a new spec document.
pub mod new;
/// Outstanding work plan for documents or the project.
pub mod plan;
/// List criterion refs in the project.
pub mod refs;
/// Render component trees with verification data.
pub mod render;
/// Output component and document type schema.
pub mod schema;
/// Manage embedded agent skills.
pub mod skills;
/// Project or document status overview.
pub mod status;
/// Cross-document verification.
pub mod verify;

use clap::Subcommand;
use std::path::PathBuf;

use crate::format::{Detail, OutputFormat};

/// Available CLI subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// List all documents
    #[command(alias = "list")]
    Ls(LsArgs),
    /// Output component and document type schema
    Schema(SchemaArgs),
    /// Structured view of a document: criteria, decisions, tasks, and relationships
    #[command(
        after_help = "Examples:\n  supersigil context auth/req\n  supersigil context auth/design --format json"
    )]
    Context(ContextArgs),
    /// Outstanding work for a document, prefix, or the whole project
    #[command(
        after_help = "Examples:\n  supersigil plan\n  supersigil plan auth/\n  supersigil plan auth/tasks --full"
    )]
    Plan(PlanArgs),
    /// Import specs from another format
    Import(ImportArgs),
    /// Cross-document verification
    #[command(
        after_help = "Examples:\n  supersigil verify\n  supersigil verify --project backend\n  supersigil verify --since main --format markdown"
    )]
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
    #[command(
        after_help = "Examples:\n  supersigil new requirements auth\n  supersigil new design auth\n  supersigil new tasks auth\n  supersigil new adr auth"
    )]
    New(NewArgs),
    /// List criterion refs in the project
    Refs(RefsArgs),
    /// Manage agent skills
    Skills(SkillsArgs),
    /// Open an interactive graph explorer in the browser
    Explore(ExploreArgs),
    /// Render component trees with verification data for all documents
    Render(RenderArgs),
    /// Generate shell completion scripts
    Completions(CompletionsArgs),
}

/// Arguments for the `ls` command.
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

/// Output format for the `schema` command.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum SchemaFormat {
    /// JSON output.
    Json,
    /// YAML output.
    Yaml,
}

/// Arguments for the `schema` command.
#[derive(Debug, clap::Args)]
pub struct SchemaArgs {
    /// Output format
    #[arg(long, default_value = "yaml")]
    pub format: SchemaFormat,
}

/// Arguments for the `context` command.
#[derive(Debug, clap::Args)]
pub struct ContextArgs {
    /// Document ID
    pub id: String,
    /// Output format
    #[arg(long, default_value = "terminal")]
    pub format: OutputFormat,
    /// JSON detail level (compact omits raw components; full includes everything)
    #[arg(long, default_value = "compact")]
    pub detail: Detail,
}

/// Arguments for the `plan` command.
#[derive(Debug, clap::Args)]
pub struct PlanArgs {
    /// Document ID, prefix, or omit for all
    pub id_or_prefix: Option<String>,
    /// Output format
    #[arg(long, default_value = "terminal")]
    pub format: OutputFormat,
    /// Show all criteria and full task details including completed items
    #[arg(long)]
    pub full: bool,
}

/// Supported import source formats.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum ImportSource {
    /// Import from Amazon Kiro spec format.
    Kiro,
}

/// Arguments for the `import` command.
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
    /// Colored terminal output.
    Terminal,
    /// JSON output.
    Json,
    /// Markdown output for CI.
    Markdown,
}

/// Arguments for the `verify` command.
#[derive(Debug, clap::Args)]
pub struct VerifyArgs {
    /// Filter to a project (multi-project mode)
    #[arg(short, long)]
    pub project: Option<String>,
    /// Git ref for staleness checks (e.g., main, HEAD~3, a commit SHA)
    #[arg(long)]
    pub since: Option<String>,
    /// Only consider committed changes, ignoring staged and unstaged work
    #[arg(long)]
    pub committed_only: bool,
    /// Diff against the merge-base of --since instead of the ref itself
    #[arg(long)]
    pub merge_base: bool,
    /// Output format
    #[arg(long, default_value = "terminal")]
    pub format: VerifyFormat,
    /// JSON detail level (compact omits evidence records on clean runs; full includes everything)
    #[arg(long, default_value = "compact")]
    pub detail: Detail,
}

/// Arguments for the `status` command.
#[derive(Debug, clap::Args)]
pub struct StatusArgs {
    /// Document ID (omit for project-wide overview)
    pub id: Option<String>,
    /// Output format
    #[arg(long, default_value = "terminal")]
    pub format: OutputFormat,
}

/// Arguments for the `affected` command.
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
    /// Mermaid diagram syntax.
    Mermaid,
    /// Graphviz DOT syntax.
    Dot,
    /// JSON output.
    Json,
}

/// Arguments for the `graph` command.
#[derive(Debug, clap::Args)]
pub struct GraphArgs {
    /// Output format
    #[arg(long, default_value = "mermaid")]
    pub format: GraphFormat,
}

/// Arguments for the `new` command.
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

/// Arguments for the `refs` command.
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

/// Arguments for the `init` command.
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

/// Arguments for the `skills` command.
#[derive(Debug, clap::Args)]
pub struct SkillsArgs {
    /// Skills subcommand.
    #[command(subcommand)]
    pub command: SkillsCommand,
}

/// Available skills subcommands.
#[derive(Debug, Subcommand)]
pub enum SkillsCommand {
    /// Install or update embedded agent skills
    Install(SkillsInstallArgs),
}

/// Arguments for the `skills install` command.
#[derive(Debug, clap::Args)]
pub struct SkillsInstallArgs {
    /// Target directory for skills
    #[arg(long)]
    pub path: Option<PathBuf>,
}

/// Arguments for the `explore` command.
#[derive(Debug, clap::Args)]
pub struct ExploreArgs {
    /// Write to this path instead of opening in browser
    #[arg(long)]
    pub output: Option<PathBuf>,
}

/// Render output format.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum RenderFormat {
    /// JSON output.
    Json,
}

/// Arguments for the `render` command.
#[derive(Debug, clap::Args)]
pub struct RenderArgs {
    /// Output format
    #[arg(long, default_value = "json")]
    pub format: RenderFormat,
}

/// Arguments for the `completions` command.
#[derive(Debug, clap::Args)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    pub shell: clap_complete::Shell,
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
