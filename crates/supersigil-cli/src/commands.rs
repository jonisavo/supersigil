//! Command definitions.

use std::path::PathBuf;

use crate::format::{AutoFormat, OutputFormat};

/// The `annotate` command group.
pub mod annotate;
/// The `completions` command.
pub mod completions;
/// The `judge` command group.
pub mod judge;
/// The `review` command.
pub mod review;
/// The `session` command group.
pub mod session;
/// The `why` command.
pub mod why;

/// Available subcommands.
#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Prepare, import and list annotations of recorded sessions
    ///
    /// An annotation names the decisions a session made, each citing the
    /// edits that carried it out, with every rationale labelled by who gave
    /// it. Prepare writes what an annotator reads. Import checks the
    /// citations and quotes against the record the document names, this
    /// checkout's or an ancestor checkout's; it does not judge whether the
    /// explanation is right.
    ///
    /// Agents may run these commands.
    Annotate(AnnotateArgs),
    /// Generate shell completions
    Completions(CompletionsArgs),
    /// Record judgments on a saved review, undo them, and read them back
    ///
    /// A completion carries over to any review of the same content, not
    /// only to the same review id.
    ///
    /// Judgments stay on this machine: refs/supersigil/ is not pushed by
    /// default.
    ///
    /// The first judgment on a review needs a git identity and keeps the
    /// reviewed snapshot as a ref.
    ///
    /// judge records against the saved review and never looks at the
    /// working tree.
    Judge(JudgeArgs),
    /// Review a change with the recorded edits behind each line
    Review(ReviewArgs),
    /// Sync and inspect agent sessions recorded for this checkout
    Session(SessionArgs),
    /// Explain where one line of a file came from
    Why(WhyArgs),
}

/// Where a command finds the checkout's record and its repository's review
/// store: the options `review` takes, with the same defaults.
///
/// Every `annotate` command and `why` take all four, so one set of options
/// serves each. `prepare` and `why` reconcile the checkout's records first,
/// discovering transcripts under `--claude-home`; `import` and `list` read
/// the record without syncing it and ignore that option. `why` reads the
/// corrections of the decisions behind its line from the review store.
#[derive(Debug, clap::Args)]
pub struct StoreArgs {
    /// Checkout whose record to use (default: current directory)
    #[arg(long, global = true)]
    pub checkout: Option<PathBuf>,
    /// Records directory (default: `$SUPERSIGIL_RECORD_DIR`, then `$XDG_DATA_HOME/supersigil/records`)
    #[arg(long, global = true)]
    pub record_dir: Option<PathBuf>,
    /// Reviews directory (default: `$SUPERSIGIL_REVIEW_DIR`, then `$XDG_DATA_HOME/supersigil/reviews`, then `~/.local/share/supersigil/reviews`)
    #[arg(long, global = true)]
    pub review_dir: Option<PathBuf>,
    /// Claude home directory used for discovery (default: ~/.claude)
    #[arg(long, global = true)]
    pub claude_home: Option<PathBuf>,
}

/// Arguments for the `annotate` command group.
#[derive(Debug, clap::Args)]
pub struct AnnotateArgs {
    /// Where to find the record and the review store
    #[command(flatten)]
    pub store: StoreArgs,
    /// Subcommand
    #[command(subcommand)]
    pub command: AnnotateCommand,
}

/// Subcommands of `annotate`.
#[derive(Debug, clap::Subcommand)]
pub enum AnnotateCommand {
    /// Write the brief an annotator reads for a session of this checkout's record or an ancestor's, as JSON
    Prepare(AnnotatePrepareArgs),
    /// Check an annotation document against the record it names, this checkout's or an ancestor's, and store it as the session's next revision
    Import(AnnotateImportArgs),
    /// List this checkout's sessions with their current annotations
    List(AnnotateListArgs),
}

/// Arguments for `annotate prepare`.
#[derive(Debug, clap::Args)]
pub struct AnnotatePrepareArgs {
    /// Session id; a unique prefix is enough
    pub session: String,
    /// Write the brief to this file, created or replaced, instead of standard output
    #[arg(long, value_name = "FILE")]
    pub out: Option<PathBuf>,
}

/// Arguments for `annotate import`.
#[derive(Debug, clap::Args)]
pub struct AnnotateImportArgs {
    /// Annotation document to import, or - to read standard input
    #[arg(value_name = "FILE")]
    pub file: PathBuf,
    /// Output format: `auto` prints text on a terminal and JSON otherwise
    #[arg(long, default_value = "auto")]
    pub format: AutoFormat,
}

/// Arguments for `annotate list`.
#[derive(Debug, clap::Args)]
pub struct AnnotateListArgs {
    /// Output format: `auto` prints text on a terminal and JSON otherwise
    #[arg(long, default_value = "auto")]
    pub format: AutoFormat,
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
    /// Reviews directory (default: `$SUPERSIGIL_REVIEW_DIR`, then `$XDG_DATA_HOME/supersigil/reviews`, then `~/.local/share/supersigil/reviews`)
    #[arg(long)]
    pub review_dir: Option<PathBuf>,
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
    /// Where to find the records and the review store
    #[command(flatten)]
    pub store: StoreArgs,
}

/// Arguments for the `judge` command group.
#[derive(Debug, clap::Args)]
pub struct JudgeArgs {
    /// Review to judge, by at least 8 hex digits of its id (default: the review last run in this worktree)
    #[arg(long, global = true, value_name = "ID")]
    pub review: Option<String>,
    /// Checkout whose worktree and repository to use (default: current directory)
    #[arg(long, global = true)]
    pub checkout: Option<PathBuf>,
    /// Reviews directory (default: `$SUPERSIGIL_REVIEW_DIR`, then `$XDG_DATA_HOME/supersigil/reviews`, then `~/.local/share/supersigil/reviews`)
    #[arg(long, global = true)]
    pub review_dir: Option<PathBuf>,
    /// Output format: `auto` prints text on a terminal and JSON otherwise
    #[arg(long, global = true, default_value = "auto")]
    pub format: AutoFormat,
    /// Subcommand
    #[command(subcommand)]
    pub command: JudgeCommand,
}

/// Subcommands of `judge`.
#[derive(Debug, clap::Subcommand)]
pub enum JudgeCommand {
    /// Acknowledge items: "I have considered this item"
    #[command(visible_alias = "ack")]
    Acknowledge(CloseArgs),
    /// Dismiss items: "it needs no follow-up, for the recorded reason"
    Dismiss(CloseArgs),
    /// Add a note to the review or an item; it leaves the item's state alone
    Comment(CommentArgs),
    /// Correct an annotated decision's explanation, grouping, or constraint
    ///
    /// Record that its explanation is wrong (--wrong), that its edits do not
    /// belong together (--not-together), or what the actual constraint was
    /// (--constraint). Name the decision as `supersigil review` prints it.
    ///
    /// A correction stays with the decision: a later review shows it before
    /// the rationale it addresses, the next annotation receives it, and
    /// re-annotating never clears it; only `judge undo --judgment` withdraws
    /// it. It closes no item and never keeps a review from completing.
    Correct(CorrectArgs),
    /// Record completion of this saved review; it does not commit or merge
    Complete(CompleteArgs),
    /// Withdraw the judgments closing items, or one judgment by its id
    Undo(UndoArgs),
    /// List the saved reviews with judgments and each worktree's selection
    List,
    /// Show a saved review's gate, comments, corrections, and withdrawn judgments
    Show(ShowArgs),
}

/// Arguments for `judge acknowledge` and `judge dismiss`, which close
/// items.
#[derive(Debug, clap::Args)]
pub struct CloseArgs {
    /// Gate items, named as the review prints them; after `--` when one starts with `-`
    #[arg(required = true, value_name = "ITEM")]
    pub items: Vec<String>,
    /// Why, recorded with the judgment; a dismissal needs one
    #[arg(long)]
    pub reason: Option<String>,
}

/// Arguments for `judge comment`.
#[derive(Debug, clap::Args)]
pub struct CommentArgs {
    /// Gate item the note is about (default: the review as a whole)
    #[arg(value_name = "ITEM")]
    pub item: Option<String>,
    /// The note
    #[arg(long)]
    pub text: String,
}

/// Arguments for `judge correct`: the decision and exactly one kind of
/// correction, a dispute with its reason or the actual constraint.
#[derive(Debug, clap::Args)]
#[command(group(
    clap::ArgGroup::new("kind")
        .required(true)
        .args(["wrong", "not_together", "constraint"])
))]
pub struct CorrectArgs {
    /// The decision's name as `supersigil review` prints it
    #[arg(value_name = "DECISION")]
    pub decision: String,
    /// Its explanation is wrong: the rationale given is not why it was done
    #[arg(long, requires = "reason")]
    pub wrong: bool,
    /// The edits it cites do not belong together
    #[arg(long, requires = "reason")]
    pub not_together: bool,
    /// The actual constraint behind it, shown before its rationale
    #[arg(long, value_name = "TEXT")]
    pub constraint: Option<String>,
    /// Why, recorded with --wrong or --not-together
    #[arg(long, conflicts_with = "constraint")]
    pub reason: Option<String>,
}

/// Arguments for `judge complete`.
#[derive(Debug, clap::Args)]
pub struct CompleteArgs {
    /// A note recorded with the completion
    #[arg(long)]
    pub text: Option<String>,
}

/// Arguments for `judge undo`, which withdraws the judgments closing the
/// named items, or one judgment named by its id.
#[derive(Debug, clap::Args)]
pub struct UndoArgs {
    /// Gate items whose closing judgments to withdraw, named as the review prints them
    #[arg(
        required_unless_present = "judgment",
        conflicts_with = "judgment",
        value_name = "ITEM"
    )]
    pub items: Vec<String>,
    /// Withdraw this judgment whole instead, named by at least 8 characters of its id.
    /// It must be recorded on this review or close one of its items
    #[arg(long, value_name = "ID")]
    pub judgment: Option<String>,
    /// Why, recorded with the undo
    #[arg(long)]
    pub reason: Option<String>,
}

/// Arguments for `judge show`.
#[derive(Debug, clap::Args)]
pub struct ShowArgs {
    /// Review to show, by at least 8 hex digits of its id (default: --review, then the review last run in this worktree)
    #[arg(value_name = "REVIEW")]
    pub review: Option<String>,
}
