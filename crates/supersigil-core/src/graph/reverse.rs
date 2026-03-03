//! Reverse mapping computation for Validates/Implements/Illustrates (pipeline stage 8).

use std::collections::{BTreeSet, HashMap};

use crate::{ExtractedComponent, SpecDocument};

use super::{ILLUSTRATES, IMPLEMENTS, ResolvedRef, VALIDATES};

// ---------------------------------------------------------------------------
// Stage 8: Reverse mappings
// ---------------------------------------------------------------------------

/// Build reverse mappings from resolved refs.
///
/// Iterates all resolved refs, identifies the source component type
/// (Validates, Implements, or Illustrates), and populates the three
/// reverse indexes.
///
/// - `Validates`: keyed by `(target_doc_id, Option<fragment>)` → `BTreeSet<source_doc_id>`
/// - `Implements`: keyed by `target_doc_id` → `BTreeSet<source_doc_id>` (fragments discarded)
/// - `Illustrates`: keyed by `(target_doc_id, Option<fragment>)` → `BTreeSet<source_doc_id>`
#[expect(clippy::type_complexity, reason = "return type is clear in context")]
pub(super) fn build_reverse_mappings(
    resolved_refs: &HashMap<(String, Vec<usize>), Vec<ResolvedRef>>,
    doc_index: &HashMap<String, SpecDocument>,
) -> (
    HashMap<(String, Option<String>), BTreeSet<String>>,
    HashMap<String, BTreeSet<String>>,
    HashMap<(String, Option<String>), BTreeSet<String>>,
) {
    let mut validates_reverse: HashMap<(String, Option<String>), BTreeSet<String>> = HashMap::new();
    let mut implements_reverse: HashMap<String, BTreeSet<String>> = HashMap::new();
    let mut illustrates_reverse: HashMap<(String, Option<String>), BTreeSet<String>> =
        HashMap::new();

    for ((source_doc_id, component_path), refs) in resolved_refs {
        // Look up the source document to find the component at this path.
        let Some(doc) = doc_index.get(source_doc_id) else {
            continue;
        };

        let Some(component) = resolve_component_path(&doc.components, component_path) else {
            continue;
        };

        let component_name = component.name.as_str();

        for resolved in refs {
            match component_name {
                VALIDATES => {
                    let key = (resolved.target_doc_id.clone(), resolved.fragment.clone());
                    validates_reverse
                        .entry(key)
                        .or_default()
                        .insert(source_doc_id.clone());
                }
                IMPLEMENTS => {
                    // Fragments discarded — mapping is document-level only.
                    implements_reverse
                        .entry(resolved.target_doc_id.clone())
                        .or_default()
                        .insert(source_doc_id.clone());
                }
                ILLUSTRATES => {
                    let key = (resolved.target_doc_id.clone(), resolved.fragment.clone());
                    illustrates_reverse
                        .entry(key)
                        .or_default()
                        .insert(source_doc_id.clone());
                }
                _ => {
                    // Other component types (e.g., DependsOn) don't contribute
                    // to reverse mappings.
                }
            }
        }
    }

    (validates_reverse, implements_reverse, illustrates_reverse)
}

/// Walk a component tree following the index path to find the component.
///
/// For a path like `[2, 1]`, this returns `components[2].children[1]`.
fn resolve_component_path<'a>(
    components: &'a [ExtractedComponent],
    path: &[usize],
) -> Option<&'a ExtractedComponent> {
    let mut current_slice = components;
    let mut result = None;

    for &idx in path {
        let component = current_slice.get(idx)?;
        result = Some(component);
        current_slice = &component.children;
    }

    result
}
