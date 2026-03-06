pub mod discover;
pub mod emit;
pub mod ids;
pub mod parse;
pub mod refs;
pub mod write;

use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportConfig {
    pub kiro_specs_dir: PathBuf,
    pub output_dir: PathBuf,
    pub id_prefix: Option<String>,
    pub force: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportResult {
    pub files_written: Vec<OutputFile>,
    pub ambiguity_count: usize,
    pub summary: ImportSummary,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportPlan {
    pub documents: Vec<PlannedDocument>,
    pub ambiguity_count: usize,
    pub summary: ImportSummary,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PlannedDocument {
    pub output_path: PathBuf,
    pub document_id: String,
    pub content: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OutputFile {
    pub path: PathBuf,
    pub document_id: String,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ImportSummary {
    pub criteria_converted: usize,
    pub validates_resolved: usize,
    pub tasks_converted: usize,
    pub features_processed: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum Diagnostic {
    SkippedDir { path: PathBuf, reason: String },
    Warning { message: String },
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

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("kiro specs directory not found: {path}")]
    SpecsDirNotFound { path: PathBuf },
    #[error("I/O error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },
    #[error("file already exists and --force not set: {path}")]
    FileExists { path: PathBuf },
}

/// Perform the full import: parse, convert, and write MDX files.
///
/// # Errors
///
/// Returns `ImportError` on discovery, I/O, or file conflict errors.
pub fn import_kiro(config: &ImportConfig) -> Result<ImportResult, ImportError> {
    let plan = plan_kiro_import(config)?;
    let files_written = write::write_files(&plan.documents, config.force)?;

    Ok(ImportResult {
        files_written,
        ambiguity_count: plan.ambiguity_count,
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
        total_ambiguity: 0,
        summary: ImportSummary::default(),
        diagnostics,
    };

    let prefix_str = config.id_prefix.as_deref();

    for spec_dir in &spec_dirs {
        process_feature(spec_dir, prefix_str, &config.output_dir, &mut acc)?;
    }

    Ok(ImportPlan {
        documents: acc.documents,
        ambiguity_count: acc.total_ambiguity,
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
    let req_doc_id = ids::make_document_id(prefix_str, "req", feature);
    let design_doc_id = ids::make_document_id(prefix_str, "design", feature);
    let tasks_doc_id = ids::make_document_id(prefix_str, "tasks", feature);

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

fn output_filename(feature: &str, type_hint: &str) -> String {
    format!("{feature}.{type_hint}.mdx")
}

/// Accumulator for building an `ImportPlan` across multiple features.
struct PlanAccumulator {
    documents: Vec<PlannedDocument>,
    total_ambiguity: usize,
    summary: ImportSummary,
    diagnostics: Vec<Diagnostic>,
}

fn emit_requirements(
    reqs: &parse::requirements::ParsedRequirements,
    ctx: &FeatureContext<'_>,
    acc: &mut PlanAccumulator,
) {
    // Requirement 2.5: emit diagnostic warning when no parseable sections found
    if reqs.requirements.is_empty() {
        acc.diagnostics.push(Diagnostic::Warning {
            message: format!(
                "requirements.md for feature '{}' contains no parseable requirement sections",
                ctx.feature
            ),
        });
    }

    let (content, amb) =
        emit::requirements::emit_requirements_mdx(reqs, ctx.req_doc_id, ctx.feature_title);
    acc.total_ambiguity += amb;
    acc.summary.criteria_converted += reqs
        .requirements
        .iter()
        .map(|r| r.criteria.len())
        .sum::<usize>();

    acc.documents.push(PlannedDocument {
        output_path: ctx
            .output_dir
            .join(ctx.feature)
            .join(output_filename(ctx.feature, "req")),
        document_id: ctx.req_doc_id.to_string(),
        content,
    });
}

fn emit_design(
    design: &parse::design::ParsedDesign,
    req_index: Option<&refs::RequirementIndex<'_>>,
    design_doc_id: &str,
    ctx: &FeatureContext<'_>,
    acc: &mut PlanAccumulator,
) {
    let mut resolved_validates: HashMap<emit::design::ValidatesKey, Vec<String>> = HashMap::new();
    let mut design_markers: Vec<String> = Vec::new();

    if let Some(index) = req_index {
        for (section_idx, section) in design.sections.iter().enumerate() {
            for (block_idx, block) in section.content.iter().enumerate() {
                if let parse::design::DesignBlock::ValidatesLine { refs, .. } = block
                    && !refs.is_empty()
                {
                    let (resolved, markers) = refs::resolve_refs(refs, index, ctx.req_doc_id);
                    if !resolved.is_empty() {
                        acc.summary.validates_resolved += resolved.len();
                        resolved_validates
                            .entry((section_idx, block_idx))
                            .or_default()
                            .extend(resolved);
                    }
                    design_markers.extend(markers);
                }
            }
        }
    }

    let req_id_ref = req_index.is_some().then_some(ctx.req_doc_id);

    let (content, amb) = emit::design::emit_design_mdx(
        design,
        design_doc_id,
        req_id_ref,
        &resolved_validates,
        ctx.feature_title,
        &design_markers,
    );
    acc.total_ambiguity += amb;

    acc.documents.push(PlannedDocument {
        output_path: ctx
            .output_dir
            .join(ctx.feature)
            .join(output_filename(ctx.feature, "design")),
        document_id: design_doc_id.to_string(),
        content,
    });
}

fn emit_tasks(
    tasks: &parse::tasks::ParsedTasks,
    req_index: Option<&refs::RequirementIndex<'_>>,
    tasks_doc_id: &str,
    ctx: &FeatureContext<'_>,
    acc: &mut PlanAccumulator,
) {
    let mut resolved_implements: HashMap<String, Vec<String>> = HashMap::new();
    let mut task_markers: Vec<String> = Vec::new();

    if let Some(index) = req_index {
        // Use a flat positional index as key to avoid collisions when
        // multiple tasks/sub-tasks share the same number.
        let mut pos: usize = 0;
        for task in &tasks.tasks {
            resolve_task_refs(
                &task.requirement_refs,
                &pos.to_string(),
                index,
                ctx.req_doc_id,
                &mut resolved_implements,
                &mut task_markers,
                &mut acc.summary,
            );
            pos += 1;

            for sub in &task.sub_tasks {
                resolve_task_refs(
                    &sub.requirement_refs,
                    &pos.to_string(),
                    index,
                    ctx.req_doc_id,
                    &mut resolved_implements,
                    &mut task_markers,
                    &mut acc.summary,
                );
                pos += 1;
            }
        }
    }

    for task in &tasks.tasks {
        acc.summary.tasks_converted += 1 + task.sub_tasks.len();
    }

    let (content, amb) = emit::tasks::emit_tasks_mdx(
        tasks,
        tasks_doc_id,
        &resolved_implements,
        ctx.feature_title,
        &task_markers,
    );
    acc.total_ambiguity += amb;

    acc.documents.push(PlannedDocument {
        output_path: ctx
            .output_dir
            .join(ctx.feature)
            .join(output_filename(ctx.feature, "tasks")),
        document_id: tasks_doc_id.to_string(),
        content,
    });
}

/// Resolve requirement refs for a single task/sub-task and accumulate results.
fn resolve_task_refs(
    task_refs: &parse::tasks::TaskRefs,
    task_id: &str,
    index: &refs::RequirementIndex<'_>,
    req_doc_id: &str,
    resolved_implements: &mut HashMap<String, Vec<String>>,
    markers: &mut Vec<String>,
    summary: &mut ImportSummary,
) {
    if let parse::tasks::TaskRefs::Refs(raw_refs) = task_refs
        && !raw_refs.is_empty()
    {
        let (resolved, ref_markers) = refs::resolve_refs(raw_refs, index, req_doc_id);
        if !resolved.is_empty() {
            summary.validates_resolved += resolved.len();
            resolved_implements.insert(task_id.to_string(), resolved);
        }
        markers.extend(ref_markers);
    }
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
}
