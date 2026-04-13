//! Import and convert external spec formats into supersigil documents.
//!
//! This crate handles discovering, parsing, and converting specification
//! documents from other formats (e.g. Kiro specs) into supersigil's native
//! Markdown-based format. It provides a plan-then-execute workflow: first
//! preview what would be imported, then write the output files.

/// Post-import scanning for unresolved TODO markers.
pub mod check;
/// Discovery of Kiro spec directories on the filesystem.
pub mod discover;
/// Emission of supersigil Markdown from parsed intermediate representations.
pub mod emit;
/// Document and criterion ID generation and deduplication.
pub mod ids;
/// Parsing of Kiro spec files into intermediate representations.
pub mod parse;
/// Requirement reference parsing and resolution.
pub mod refs;
/// Writing generated spec documents to disk.
pub mod write;

use std::path::PathBuf;

/// Categories of import ambiguity markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum AmbiguityKind {
    /// A duplicate ID was detected and renamed.
    DuplicateId,
    /// A requirement reference could not be resolved.
    UnresolvedRef,
    /// A reference token could not be parsed.
    UnparseableRef,
    /// Required context (e.g. requirements document) was missing.
    MissingContext,
    /// A Kiro feature has no supersigil equivalent.
    UnsupportedFeature,
}

/// Per-category counts of ambiguity markers emitted during import.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct AmbiguityBreakdown {
    /// Duplicate IDs that were renamed with a suffix.
    pub duplicate_id: usize,
    /// Requirement references that could not be resolved.
    pub unresolved_ref: usize,
    /// Reference tokens that could not be parsed.
    pub unparseable_ref: usize,
    /// Missing structural context (e.g. no requirements document).
    pub missing_context: usize,
    /// Kiro features with no supersigil equivalent.
    pub unsupported_feature: usize,
}

impl AmbiguityBreakdown {
    /// Total number of ambiguity markers across all categories.
    #[must_use]
    pub fn total(&self) -> usize {
        self.duplicate_id
            + self.unresolved_ref
            + self.unparseable_ref
            + self.missing_context
            + self.unsupported_feature
    }

    /// Iterate over `(name, count)` pairs for each category.
    #[must_use]
    pub fn iter_named(&self) -> [(&'static str, usize); 5] {
        [
            ("duplicate_id", self.duplicate_id),
            ("unresolved_ref", self.unresolved_ref),
            ("unparseable_ref", self.unparseable_ref),
            ("missing_context", self.missing_context),
            ("unsupported_feature", self.unsupported_feature),
        ]
    }

    /// Increment the count for the given ambiguity kind.
    pub fn record(&mut self, kind: AmbiguityKind) {
        match kind {
            AmbiguityKind::DuplicateId => self.duplicate_id += 1,
            AmbiguityKind::UnresolvedRef => self.unresolved_ref += 1,
            AmbiguityKind::UnparseableRef => self.unparseable_ref += 1,
            AmbiguityKind::MissingContext => self.missing_context += 1,
            AmbiguityKind::UnsupportedFeature => self.unsupported_feature += 1,
        }
    }
}

impl std::ops::AddAssign for AmbiguityBreakdown {
    fn add_assign(&mut self, rhs: Self) {
        self.duplicate_id += rhs.duplicate_id;
        self.unresolved_ref += rhs.unresolved_ref;
        self.unparseable_ref += rhs.unparseable_ref;
        self.missing_context += rhs.missing_context;
        self.unsupported_feature += rhs.unsupported_feature;
    }
}

/// Configuration for a Kiro-to-supersigil import operation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportConfig {
    /// Path to the Kiro specs directory (e.g., `.kiro/specs`).
    pub kiro_specs_dir: PathBuf,
    /// Directory where converted spec documents will be written.
    pub output_dir: PathBuf,
    /// Optional prefix for generated document IDs.
    pub id_prefix: Option<String>,
    /// Whether to overwrite existing files.
    pub force: bool,
}

/// Result of a completed import operation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportResult {
    /// Files successfully written to disk.
    pub files_written: Vec<OutputFile>,
    /// Per-category breakdown of ambiguity markers emitted during conversion.
    pub ambiguity_breakdown: AmbiguityBreakdown,
    /// Aggregate statistics for the import.
    pub summary: ImportSummary,
    /// Non-fatal warnings and skipped-directory notices.
    pub diagnostics: Vec<Diagnostic>,
}

/// Dry-run preview of an import operation (no files written).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportPlan {
    /// Documents that would be written.
    pub documents: Vec<PlannedDocument>,
    /// Per-category breakdown of ambiguity markers that would be emitted.
    pub ambiguity_breakdown: AmbiguityBreakdown,
    /// Aggregate statistics for the planned import.
    pub summary: ImportSummary,
    /// Non-fatal warnings and skipped-directory notices.
    pub diagnostics: Vec<Diagnostic>,
}

/// A single document planned for output.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlannedDocument {
    /// Filesystem path where the document will be written.
    pub output_path: PathBuf,
    /// The supersigil document ID.
    pub document_id: String,
    /// Rendered Markdown content.
    pub content: String,
}

/// A file that was written to disk during import.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OutputFile {
    /// Filesystem path of the written file.
    pub path: PathBuf,
    /// The supersigil document ID of the written file.
    pub document_id: String,
}

/// Aggregate statistics for an import operation.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ImportSummary {
    /// Total acceptance criteria converted.
    pub criteria_converted: usize,
    /// Total validates/implements references successfully resolved.
    pub validates_resolved: usize,
    /// Total tasks (including sub-tasks) converted.
    pub tasks_converted: usize,
    /// Number of feature directories processed.
    pub features_processed: usize,
}

/// A non-fatal diagnostic produced during import.
#[derive(Debug, Clone, serde::Serialize)]
pub enum Diagnostic {
    /// A directory was skipped during discovery.
    SkippedDir {
        /// Path of the skipped directory.
        path: PathBuf,
        /// Reason the directory was skipped.
        reason: String,
    },
    /// A general warning.
    Warning {
        /// Warning message text.
        message: String,
    },
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SkippedDir { path, reason } => {
                write!(f, "skipped directory '{}': {reason}", path.display())
            }
            Self::Warning { message } => write!(f, "warning: {message}"),
        }
    }
}

/// Errors that can occur during a Kiro import.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// The specified Kiro specs directory does not exist.
    #[error("kiro specs directory not found: {path}")]
    SpecsDirNotFound {
        /// Path that was not found.
        path: PathBuf,
    },
    /// An I/O error occurred during discovery, reading, or writing.
    #[error("I/O error: {source}")]
    Io {
        /// The underlying I/O error.
        #[from]
        source: std::io::Error,
    },
    /// A target file already exists and `--force` was not set.
    #[error("file already exists and --force not set: {path}")]
    FileExists {
        /// Path of the existing file.
        path: PathBuf,
    },
}

/// Perform the full import: parse, convert, and write spec documents.
///
/// # Errors
///
/// Returns `ImportError` on discovery, I/O, or file conflict errors.
pub fn import_kiro(config: &ImportConfig) -> Result<ImportResult, ImportError> {
    let plan = plan_kiro_import(config)?;
    let files_written = write::write_files(&plan.documents, config.force)?;

    Ok(ImportResult {
        files_written,
        ambiguity_breakdown: plan.ambiguity_breakdown,
        summary: plan.summary,
        diagnostics: plan.diagnostics,
    })
}

/// Return a dry-run preview without writing files.
///
/// Orchestrates the full pipeline: discover → parse → resolve → emit.
/// Builds an `ImportPlan` with `PlannedDocument` entries, ambiguity count,
/// and summary. No files are written to disk.
///
/// # Errors
///
/// Returns `ImportError` on discovery or I/O errors.
pub fn plan_kiro_import(config: &ImportConfig) -> Result<ImportPlan, ImportError> {
    let (spec_dirs, diagnostics) = discover::discover_kiro_specs(&config.kiro_specs_dir)?;

    let mut acc = PlanAccumulator {
        documents: Vec::new(),
        breakdown: AmbiguityBreakdown::default(),
        summary: ImportSummary::default(),
        diagnostics,
    };

    let prefix_str = config.id_prefix.as_deref();

    for spec_dir in &spec_dirs {
        process_feature(spec_dir, prefix_str, &config.output_dir, &mut acc)?;
    }

    Ok(ImportPlan {
        documents: acc.documents,
        ambiguity_breakdown: acc.breakdown,
        summary: acc.summary,
        diagnostics: acc.diagnostics,
    })
}

/// Process a single Kiro spec directory: parse → resolve → emit.
fn process_feature(
    spec_dir: &discover::KiroSpecDir,
    prefix_str: Option<&str>,
    output_dir: &std::path::Path,
    acc: &mut PlanAccumulator,
) -> Result<(), ImportError> {
    let feature = &spec_dir.feature_name;

    // Parse each present file
    let parsed_reqs = spec_dir
        .has_requirements
        .then(|| std::fs::read_to_string(spec_dir.path.join("requirements.md")))
        .transpose()?
        .map(|c| parse::requirements::parse_requirements(&c));

    let parsed_design = spec_dir
        .has_design
        .then(|| std::fs::read_to_string(spec_dir.path.join("design.md")))
        .transpose()?
        .map(|c| parse::design::parse_design(&c));

    let parsed_tasks = spec_dir
        .has_tasks
        .then(|| std::fs::read_to_string(spec_dir.path.join("tasks.md")))
        .transpose()?
        .map(|c| parse::tasks::parse_tasks(&c));

    // Build document IDs
    let req_doc_id = ids::make_document_id(prefix_str, feature, "req");
    let design_doc_id = ids::make_document_id(prefix_str, feature, "design");
    let tasks_doc_id = ids::make_document_id(prefix_str, feature, "tasks");

    // Determine feature title (prefer requirements → design → tasks → feature name)
    let feature_title = parsed_reqs
        .as_ref()
        .and_then(|r| r.title.as_deref())
        .or_else(|| parsed_design.as_ref().and_then(|d| d.title.as_deref()))
        .or_else(|| parsed_tasks.as_ref().and_then(|t| t.title.as_deref()))
        .unwrap_or(feature)
        .to_string();

    let ctx = FeatureContext {
        feature,
        feature_title: &feature_title,
        req_doc_id: &req_doc_id,
        output_dir,
    };

    if let Some(ref reqs) = parsed_reqs {
        emit_requirements(reqs, &ctx, acc);
    }

    let req_index = parsed_reqs.as_ref().map(refs::RequirementIndex::new);

    if let Some(ref design) = parsed_design {
        emit_design(design, req_index.as_ref(), &design_doc_id, &ctx, acc);
    }

    if let Some(ref tasks) = parsed_tasks {
        emit_tasks(tasks, req_index.as_ref(), &tasks_doc_id, &ctx, acc);
    }

    acc.summary.features_processed += 1;
    Ok(())
}

/// Shared context for processing a single feature.
struct FeatureContext<'a> {
    feature: &'a str,
    feature_title: &'a str,
    req_doc_id: &'a str,
    output_dir: &'a std::path::Path,
}

/// Accumulator for building an `ImportPlan` across multiple features.
struct PlanAccumulator {
    documents: Vec<PlannedDocument>,
    breakdown: AmbiguityBreakdown,
    summary: ImportSummary,
    diagnostics: Vec<Diagnostic>,
}

impl PlanAccumulator {
    fn push_document(
        &mut self,
        ctx: &FeatureContext<'_>,
        type_hint: &str,
        doc_id: &str,
        content: String,
        ambiguity: AmbiguityBreakdown,
    ) {
        self.breakdown += ambiguity;
        self.documents.push(PlannedDocument {
            output_path: ctx
                .output_dir
                .join(ctx.feature)
                .join(format!("{}.{type_hint}.md", ctx.feature)),
            document_id: doc_id.to_string(),
            content,
        });
    }
}

fn emit_requirements(
    reqs: &parse::requirements::ParsedRequirements,
    ctx: &FeatureContext<'_>,
    acc: &mut PlanAccumulator,
) {
    if reqs.requirements.is_empty() {
        acc.diagnostics.push(Diagnostic::Warning {
            message: format!(
                "requirements.md for feature '{}' contains no parseable requirement sections",
                ctx.feature
            ),
        });
    }

    let (content, breakdown) =
        emit::requirements::emit_requirements_md(reqs, ctx.req_doc_id, ctx.feature_title);
    acc.summary.criteria_converted += reqs
        .requirements
        .iter()
        .map(|r| r.criteria.len())
        .sum::<usize>();
    acc.push_document(ctx, "req", ctx.req_doc_id, content, breakdown);
}

fn emit_design(
    design: &parse::design::ParsedDesign,
    req_index: Option<&refs::RequirementIndex<'_>>,
    design_doc_id: &str,
    ctx: &FeatureContext<'_>,
    acc: &mut PlanAccumulator,
) {
    let (content, breakdown, validates_resolved) = emit::design::emit_design_md(
        design,
        design_doc_id,
        req_index,
        ctx.req_doc_id,
        ctx.feature_title,
    );
    acc.summary.validates_resolved += validates_resolved;
    acc.push_document(ctx, "design", design_doc_id, content, breakdown);
}

fn emit_tasks(
    tasks: &parse::tasks::ParsedTasks,
    req_index: Option<&refs::RequirementIndex<'_>>,
    tasks_doc_id: &str,
    ctx: &FeatureContext<'_>,
    acc: &mut PlanAccumulator,
) {
    for task in &tasks.tasks {
        acc.summary.tasks_converted += 1 + task.sub_tasks.len();
    }
    let (content, breakdown, validates_resolved) = emit::tasks::emit_tasks_md(
        tasks,
        tasks_doc_id,
        req_index,
        ctx.req_doc_id,
        ctx.feature_title,
    );
    acc.summary.validates_resolved += validates_resolved;
    acc.push_document(ctx, "tasks", tasks_doc_id, content, breakdown);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_types_constructible() {
        let _config = ImportConfig {
            kiro_specs_dir: PathBuf::from(".kiro/specs"),
            output_dir: PathBuf::from("specs"),
            id_prefix: None,
            force: false,
        };
        let summary = ImportSummary::default();
        assert_eq!(summary.criteria_converted, 0);
    }

    #[test]
    fn ambiguity_breakdown_total() {
        let mut b = AmbiguityBreakdown::default();
        assert_eq!(b.total(), 0);
        b.record(AmbiguityKind::DuplicateId);
        b.record(AmbiguityKind::UnresolvedRef);
        b.record(AmbiguityKind::UnresolvedRef);
        b.record(AmbiguityKind::UnparseableRef);
        b.record(AmbiguityKind::MissingContext);
        b.record(AmbiguityKind::UnsupportedFeature);
        assert_eq!(b.total(), 6);
        assert_eq!(b.duplicate_id, 1);
        assert_eq!(b.unresolved_ref, 2);
        assert_eq!(b.unparseable_ref, 1);
        assert_eq!(b.missing_context, 1);
        assert_eq!(b.unsupported_feature, 1);
    }

    #[test]
    fn ambiguity_breakdown_add_assign() {
        let mut a = AmbiguityBreakdown::default();
        a.record(AmbiguityKind::DuplicateId);
        a.record(AmbiguityKind::UnresolvedRef);

        let mut b = AmbiguityBreakdown::default();
        b.record(AmbiguityKind::MissingContext);
        b.record(AmbiguityKind::UnresolvedRef);

        a += b;
        assert_eq!(a.duplicate_id, 1);
        assert_eq!(a.unresolved_ref, 2);
        assert_eq!(a.missing_context, 1);
        assert_eq!(a.total(), 4);
    }
}
