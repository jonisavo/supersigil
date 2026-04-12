use std::collections::HashMap;

use crate::emit::format_marker;

/// Construct a document ID from an optional prefix, feature name, and type hint.
///
/// Without prefix: `{feature_name}/{type_hint}`.
/// With prefix: `{prefix}/{feature_name}/{type_hint}` (trailing slashes stripped from prefix).
#[must_use]
pub fn make_document_id(id_prefix: Option<&str>, feature_name: &str, type_hint: &str) -> String {
    let prefix = id_prefix
        .map(|p| p.trim_end_matches('/'))
        .filter(|s| !s.is_empty());
    match prefix {
        Some(p) => format!("{p}/{feature_name}/{type_hint}"),
        None => format!("{feature_name}/{type_hint}"),
    }
}

/// Generate a criterion ID: `req-{requirement_number}-{criterion_index}`.
///
/// Preserves alphanumeric indices (e.g., `8a` → `req-1-8a`).
#[must_use]
pub fn make_criterion_id(requirement_number: &str, criterion_index: &str) -> String {
    format!("req-{requirement_number}-{criterion_index}")
}

/// Generate a task ID: `task-{N}` for top-level, `task-{N}-{M}` for sub-tasks.
#[must_use]
pub fn make_task_id(task_number: &str, sub_task_number: Option<&str>) -> String {
    match sub_task_number {
        Some(sub) => format!("task-{task_number}-{sub}"),
        None => format!("task-{task_number}"),
    }
}

/// Check for ID collisions and append disambiguation suffixes.
///
/// Returns `(deduplicated_ids, ambiguity_markers)`. For each collision, the
/// second and subsequent occurrences get a `-2`, `-3`, etc. suffix and an
/// ambiguity marker is emitted. The suffix is incremented until a globally
/// unique name is found (avoiding collisions with both original and
/// previously-suffixed IDs).
#[must_use]
pub fn deduplicate_ids(ids: &[String]) -> (Vec<String>, Vec<String>) {
    // Pre-populate counts with 0 for all IDs so contains_key can detect
    // future IDs when checking suffix collisions.
    let mut counts: HashMap<&str, usize> = ids.iter().map(|id| (id.as_str(), 0)).collect();
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut deduped = Vec::with_capacity(ids.len());
    let mut markers = Vec::new();

    for id in ids {
        let count = counts.entry(id).or_insert(0);
        *count += 1;

        if *count == 1 && !used.contains(id.as_str()) {
            used.insert(id.clone());
            deduped.push(id.clone());
        } else {
            // Find a suffix that doesn't collide with any original or already-used ID.
            let mut suffix = *count;
            let mut suffixed = format!("{id}-{suffix}");
            while counts.contains_key(suffixed.as_str()) || used.contains(&suffixed) {
                suffix += 1;
                suffixed = format!("{id}-{suffix}");
            }
            markers.push(format_marker(&format!(
                "Duplicate ID '{id}', renamed to '{suffixed}'"
            )));
            used.insert(suffixed.clone());
            deduped.push(suffixed);
        }
    }

    (deduped, markers)
}
