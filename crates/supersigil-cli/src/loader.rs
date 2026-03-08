use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use supersigil_core::{
    ComponentDefs, Config, DocumentGraph, ParseError, ParseResult, SpecDocument, build_graph,
    load_config,
};
use supersigil_parser::parse_file;

use crate::discover::discover_spec_files;
use crate::error::CliError;

const MAX_PARSE_WORKERS: usize = 8;

/// Derive the project root directory from a config file path.
#[must_use]
pub fn project_root(config_path: &Path) -> &Path {
    config_path.parent().unwrap_or_else(|| Path::new("."))
}

#[derive(Debug)]
pub(crate) struct ParseAllStats {
    pub config: Config,
    pub documents: Vec<SpecDocument>,
    pub errors: Vec<ParseError>,
    pub files_checked: usize,
}

/// Search upward from `start_dir` for `supersigil.toml`.
///
/// # Errors
///
/// Returns `CliError::ConfigNotFound` if no `supersigil.toml` is found
/// in `start_dir` or any ancestor directory.
pub fn find_config(start_dir: &Path) -> Result<PathBuf, CliError> {
    let mut current = start_dir.to_path_buf();
    loop {
        let candidate = current.join("supersigil.toml");
        match std::fs::metadata(&candidate) {
            Ok(metadata) if metadata.is_file() => return Ok(candidate),
            Ok(_) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }

        if !current.pop() {
            return Err(CliError::ConfigNotFound {
                start_dir: start_dir.to_path_buf(),
            });
        }
    }
}

/// Discover and parse all spec files. Returns config, successfully parsed
/// documents, and any parse errors encountered.
///
/// # Errors
///
/// Returns `CliError::Config` if the config file cannot be loaded, or
/// `CliError::Io` if glob resolution fails.
pub fn parse_all(
    config_path: &Path,
) -> Result<(Config, Vec<SpecDocument>, Vec<ParseError>), CliError> {
    let ParseAllStats {
        config,
        documents,
        errors,
        ..
    } = parse_all_with_stats(config_path)?;

    Ok((config, documents, errors))
}

pub(crate) fn parse_all_with_stats(config_path: &Path) -> Result<ParseAllStats, CliError> {
    let config = load_config(config_path).map_err(CliError::Config)?;
    let base_dir = project_root(config_path);

    let globs = collect_globs(&config);
    let file_paths = discover_spec_files(globs, base_dir)?;
    let files_checked = file_paths.len();

    let component_defs = ComponentDefs::merge(ComponentDefs::defaults(), config.components.clone())
        .map_err(CliError::ComponentDef)?;
    let (mut documents, errors) = parse_files(&file_paths, &component_defs);
    relativize_document_paths(&mut documents, base_dir);

    Ok(ParseAllStats {
        config,
        documents,
        errors,
        files_checked,
    })
}

/// Discover, parse, and build the document graph.
/// Parse errors and graph errors are fatal.
///
/// # Errors
///
/// Returns `CliError::Parse` if any spec files fail to parse, or
/// `CliError::Graph` if graph construction detects errors (duplicate IDs,
/// broken refs, cycles). Also propagates errors from [`parse_all`].
pub fn load_graph(config_path: &Path) -> Result<(Config, DocumentGraph), CliError> {
    let (config, documents, errors) = parse_all(config_path)?;

    if !errors.is_empty() {
        return Err(CliError::Parse(errors));
    }

    let graph = build_graph(documents, &config).map_err(CliError::Graph)?;
    Ok((config, graph))
}

/// Collect glob patterns from config (single-project or multi-project).
fn collect_globs(config: &Config) -> Vec<&str> {
    if let Some(paths) = &config.paths {
        return paths.iter().map(String::as_str).collect();
    }

    if let Some(projects) = &config.projects {
        return projects
            .values()
            .flat_map(|p| p.paths.iter().map(String::as_str))
            .collect();
    }

    Vec::new()
}

fn parse_files(
    file_paths: &[PathBuf],
    component_defs: &ComponentDefs,
) -> (Vec<SpecDocument>, Vec<ParseError>) {
    let worker_count = parse_worker_count(file_paths.len());
    if worker_count <= 1 {
        return parse_files_serial(file_paths, component_defs);
    }

    let next_index = AtomicUsize::new(0);
    let (tx, rx) = mpsc::channel::<(usize, ParseOutcome)>();

    thread::scope(|scope| {
        for _ in 0..worker_count {
            let tx = tx.clone();
            let next_index = &next_index;

            scope.spawn(move || {
                loop {
                    let idx = next_index.fetch_add(1, Ordering::Relaxed);
                    if idx >= file_paths.len() {
                        break;
                    }

                    let outcome = parse_one(&file_paths[idx], component_defs);
                    if tx.send((idx, outcome)).is_err() {
                        break;
                    }
                }
            });
        }
    });

    drop(tx);

    let mut outcomes: Vec<Option<ParseOutcome>> = std::iter::repeat_with(|| None)
        .take(file_paths.len())
        .collect();

    for (idx, outcome) in rx {
        debug_assert!(outcomes[idx].is_none());
        outcomes[idx] = Some(outcome);
    }

    collect_outcomes(outcomes)
}

fn parse_files_serial(
    file_paths: &[PathBuf],
    component_defs: &ComponentDefs,
) -> (Vec<SpecDocument>, Vec<ParseError>) {
    collect_outcomes(
        file_paths
            .iter()
            .map(|path| Some(parse_one(path, component_defs))),
    )
}

fn collect_outcomes<I>(outcomes: I) -> (Vec<SpecDocument>, Vec<ParseError>)
where
    I: IntoIterator<Item = Option<ParseOutcome>>,
{
    let mut documents = Vec::new();
    let mut errors = Vec::new();

    for outcome in outcomes {
        match outcome {
            Some(ParseOutcome::Document(doc)) => documents.push(doc),
            Some(ParseOutcome::Errors(mut errs)) => errors.append(&mut errs),
            Some(ParseOutcome::NotSupersigil) | None => {}
        }
    }

    (documents, errors)
}

fn relativize_document_paths(documents: &mut [SpecDocument], base_dir: &Path) {
    for doc in documents {
        if let Ok(relative) = doc.path.strip_prefix(base_dir) {
            doc.path = relative.to_path_buf();
        }
    }
}

fn parse_one(path: &Path, component_defs: &ComponentDefs) -> ParseOutcome {
    match parse_file(path, component_defs) {
        Ok(ParseResult::Document(doc)) => ParseOutcome::Document(doc),
        Ok(ParseResult::NotSupersigil(_)) => ParseOutcome::NotSupersigil,
        Err(errs) => ParseOutcome::Errors(errs),
    }
}

fn parse_worker_count(file_count: usize) -> usize {
    if file_count <= 1 {
        return file_count;
    }

    thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1)
        .min(MAX_PARSE_WORKERS)
        .min(file_count)
}

enum ParseOutcome {
    Document(SpecDocument),
    Errors(Vec<ParseError>),
    NotSupersigil,
}
