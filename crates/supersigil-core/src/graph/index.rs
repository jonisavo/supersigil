//! Document and referenceable component indexing (pipeline stages 1–2).

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use crate::{
    ComponentDefs, Config, ExtractedComponent, SourcePosition, SpecDocument, split_list_attribute,
};

use super::{GraphError, TRACKED_FILES};

// ---------------------------------------------------------------------------
// Stage 1: Document indexing
// ---------------------------------------------------------------------------

/// Build the document index from a flat collection of parsed documents.
///
/// Returns the index (`HashMap<id, SpecDocument>`) and any `DuplicateId`
/// errors found during insertion.
pub(super) fn build_doc_index(
    documents: Vec<SpecDocument>,
) -> (HashMap<String, SpecDocument>, Vec<GraphError>) {
    let mut errors = Vec::new();
    let mut path_tracker: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut index: HashMap<String, SpecDocument> = HashMap::new();

    for doc in documents {
        let id = doc.frontmatter.id.clone();
        path_tracker
            .entry(id.clone())
            .or_default()
            .push(doc.path.clone());
        // Keep the first occurrence; duplicates are reported as errors.
        index.entry(id).or_insert(doc);
    }

    for (id, paths) in &path_tracker {
        if paths.len() > 1 {
            errors.push(GraphError::DuplicateId {
                id: id.clone(),
                paths: paths.clone(),
            });
        }
    }

    (index, errors)
}

/// Build the project membership map from config.
///
/// For multi-project configs, matches each document path against project
/// path globs to determine membership. For single-project configs, all
/// documents map to `None`.
pub(super) fn build_doc_project(
    doc_index: &HashMap<String, SpecDocument>,
    config: &Config,
) -> HashMap<String, Option<String>> {
    let mut doc_project = HashMap::new();

    match &config.projects {
        Some(projects) => {
            // Extract directory prefixes from glob patterns.
            // e.g. "project-a/specs/**/*.md" → ["project-a", "specs"]
            let prefixes: Vec<(&str, Vec<Vec<OsString>>)> = projects
                .iter()
                .map(|(name, pc)| {
                    let dirs: Vec<Vec<OsString>> = pc
                        .paths
                        .iter()
                        .map(|p| prefix_components(&glob_prefix(p)))
                        .collect();
                    (name.as_str(), dirs)
                })
                .collect();

            for (id, doc) in doc_index {
                let project = prefixes
                    .iter()
                    .filter_map(|(name, project_prefixes)| {
                        project_prefixes
                            .iter()
                            .filter(|prefix| path_matches_prefix(&doc.path, prefix))
                            .map(Vec::len)
                            .max()
                            .map(|matched_len| (*name, matched_len))
                    })
                    .max_by_key(|(_, matched_len)| *matched_len)
                    .map(|(name, _)| name.to_owned());
                doc_project.insert(id.clone(), project);
            }
        }
        None => {
            for id in doc_index.keys() {
                doc_project.insert(id.clone(), None);
            }
        }
    }

    doc_project
}

/// Extract the static directory prefix from a glob pattern.
///
/// Strips everything from the first glob metacharacter (`*`, `?`, `[`)
/// onward, then trims back to the last `/` to get a clean directory prefix
/// (with trailing slash).
///
/// Examples:
/// - `"project-a/specs/**/*.md"` → `"project-a/specs/"`
/// - `"specs/*.md"` → `"specs/"`
/// - `"**/*.md"` → `""`
#[must_use]
pub fn glob_prefix(pattern: &str) -> String {
    let meta_pos = pattern.find(['*', '?', '[']).unwrap_or(pattern.len());
    let prefix = &pattern[..meta_pos];
    // Trim back to last '/' for a clean directory prefix.
    match prefix.rfind('/') {
        Some(pos) => prefix[..=pos].to_owned(),
        None => String::new(),
    }
}

fn prefix_components(prefix: &str) -> Vec<OsString> {
    std::path::Path::new(prefix)
        .components()
        .filter_map(component_name)
        .map(OsStr::to_os_string)
        .collect()
}

fn path_matches_prefix(path: &std::path::Path, prefix: &[OsString]) -> bool {
    if prefix.is_empty() {
        return true;
    }

    let components: Vec<&OsStr> = path.components().filter_map(component_name).collect();
    components.windows(prefix.len()).any(|window| {
        window
            .iter()
            .zip(prefix)
            .all(|(component, prefix_component)| *component == prefix_component.as_os_str())
    })
}

fn component_name(component: std::path::Component<'_>) -> Option<&OsStr> {
    match component {
        std::path::Component::Normal(name) => Some(name),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Stage 2: Referenceable component indexing
// ---------------------------------------------------------------------------

/// Build the component index from the document index.
///
/// Iterates all documents and their components recursively. For each
/// component whose `ComponentDef` has `referenceable = true`, extracts the
/// `id` attribute and indexes as `(doc_id, component_id) → (doc_id, component)`.
///
/// Detects duplicate component IDs within the same document.
pub(super) fn build_component_index(
    doc_index: &HashMap<String, SpecDocument>,
    component_defs: &ComponentDefs,
) -> (
    HashMap<(String, String), ExtractedComponent>,
    Vec<GraphError>,
) {
    let mut errors = Vec::new();
    let mut index: HashMap<(String, String), ExtractedComponent> = HashMap::new();
    // Track positions per (doc_id, component_id) for duplicate detection.
    let mut position_tracker: HashMap<(String, String), Vec<SourcePosition>> = HashMap::new();

    for (doc_id, doc) in doc_index {
        index_components_recursive(
            doc_id,
            &doc.components,
            component_defs,
            &mut index,
            &mut position_tracker,
        );
    }

    // Emit errors for duplicate component IDs within the same document.
    for ((doc_id, component_id), positions) in &position_tracker {
        if positions.len() > 1 {
            errors.push(GraphError::DuplicateComponentId {
                doc_id: doc_id.clone(),
                component_id: component_id.clone(),
                positions: positions.clone(),
            });
        }
    }

    (index, errors)
}

/// Recursively walk components and index referenceable ones.
fn index_components_recursive(
    doc_id: &str,
    components: &[ExtractedComponent],
    component_defs: &ComponentDefs,
    index: &mut HashMap<(String, String), ExtractedComponent>,
    position_tracker: &mut HashMap<(String, String), Vec<SourcePosition>>,
) {
    for component in components {
        // Check if this component type is referenceable.
        if let Some(def) = component_defs.get(&component.name)
            && def.referenceable
            && let Some(id) = component.attributes.get("id")
        {
            let key = (doc_id.to_owned(), id.clone());
            position_tracker
                .entry(key.clone())
                .or_default()
                .push(component.position);
            // Keep the first occurrence.
            index.entry(key).or_insert_with(|| component.clone());
        }

        // Recurse into children (e.g., Criterion inside AcceptanceCriteria).
        if !component.children.is_empty() {
            index_components_recursive(
                doc_id,
                &component.children,
                component_defs,
                index,
                position_tracker,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Stage 9: TrackedFiles indexing
// ---------------------------------------------------------------------------

/// Build the `TrackedFiles` index from the document index.
///
/// Iterates all documents, finds `TrackedFiles` components, splits their
/// `paths` attribute using `split_list_attribute`, and aggregates all path
/// globs under the owning document ID.
pub(super) fn build_tracked_files_index(
    doc_index: &HashMap<String, SpecDocument>,
) -> HashMap<String, Vec<String>> {
    let mut index: HashMap<String, Vec<String>> = HashMap::new();

    for (doc_id, doc) in doc_index {
        collect_tracked_files_recursive(doc_id, &doc.components, &mut index);
    }

    index
}

/// Recursively walk components looking for `TrackedFiles` and aggregate paths.
fn collect_tracked_files_recursive(
    doc_id: &str,
    components: &[ExtractedComponent],
    index: &mut HashMap<String, Vec<String>>,
) {
    for component in components {
        if component.name == TRACKED_FILES
            && let Some(paths_raw) = component.attributes.get("paths")
            && let Ok(items) = split_list_attribute(paths_raw)
        {
            index
                .entry(doc_id.to_owned())
                .or_default()
                .extend(items.into_iter().map(str::to_owned));
        }

        if !component.children.is_empty() {
            collect_tracked_files_recursive(doc_id, &component.children, index);
        }
    }
}
