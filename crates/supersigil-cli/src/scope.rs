//! Context-aware scoping via `TrackedFiles` globs and the current working directory,
//! and shared graph-walking utilities used by multiple commands.

use std::collections::HashSet;
use std::path::Path;

use supersigil_core::{DocumentGraph, ExtractedComponent, glob_prefix};

/// Check whether `cwd` falls within the non-wildcard prefix of `glob_str`.
///
/// The prefix is the longest directory path that contains no glob meta
/// characters (`*`, `?`, `[`). If `cwd` (relative to `project_root`)
/// starts with that prefix, the glob is considered relevant.
#[must_use]
pub fn cwd_matches_glob(cwd: &Path, project_root: &Path, glob_str: &str) -> bool {
    let Ok(relative_cwd) = cwd.strip_prefix(project_root) else {
        return false;
    };

    let prefix = glob_prefix(glob_str);
    if prefix.is_empty() {
        // Glob like `**/*.rs`: the prefix is the project root itself.
        // Any cwd inside the project matches.
        return true;
    }

    let prefix_path = Path::new(&prefix);
    // cwd is inside (or equal to) the prefix directory
    relative_cwd.starts_with(prefix_path)
}

/// Determine which document IDs are relevant based on cwd and `TrackedFiles` globs.
///
/// Returns `Some(set)` when at least one document's `TrackedFiles` globs match
/// the current working directory, or `None` when no documents match (caller
/// should fall back to showing everything).
///
/// After matching `TrackedFiles`, the scope is expanded by following `<Implements>`
/// relationships from matched documents. This ensures that when a design doc
/// has `TrackedFiles` but the criteria live on its requirement doc, the criteria
/// are still included in the scoped output.
pub fn resolve_context_scope(
    graph: &DocumentGraph,
    project_root: &Path,
    cwd: &Path,
) -> Option<HashSet<String>> {
    let mut matched_doc_ids = HashSet::new();

    for (doc_id, globs) in graph.all_tracked_files() {
        for glob_pattern in globs {
            if cwd_matches_glob(cwd, project_root, glob_pattern) {
                matched_doc_ids.insert(doc_id.to_owned());
                break;
            }
        }
    }

    if matched_doc_ids.is_empty() {
        return None;
    }

    // Expand scope: for each matched doc, also include documents it implements.
    let expansion: Vec<String> = matched_doc_ids
        .iter()
        .flat_map(|doc_id| graph.implements_targets(doc_id))
        .map(str::to_owned)
        .collect();
    matched_doc_ids.extend(expansion);

    Some(matched_doc_ids)
}

/// Collect all criterion refs declared in `<Example verifies="...">` attributes
/// across all documents, without executing the examples.
#[must_use]
pub fn collect_example_verifies_refs(graph: &DocumentGraph) -> HashSet<String> {
    let mut refs = HashSet::new();
    for (_, doc) in graph.documents() {
        collect_example_refs_recursive(&doc.components, &mut refs);
    }
    refs
}

fn collect_example_refs_recursive(components: &[ExtractedComponent], refs: &mut HashSet<String>) {
    for comp in components {
        if comp.name == "Example"
            && let Some(verifies) = comp.attributes.get("verifies")
        {
            for r in verifies.split(',') {
                let trimmed = r.trim();
                if !trimmed.is_empty() {
                    refs.insert(trimmed.to_string());
                }
            }
        }
        collect_example_refs_recursive(&comp.children, refs);
    }
}
