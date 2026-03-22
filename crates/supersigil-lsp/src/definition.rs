//! Go-to-definition support for supersigil ref attributes.
//!
//! Implements:
//! - req-2-1: Fragment refs (`doc-id#frag-id`) resolve to source position.
//! - req-2-2: Document-level refs (no `#`) resolve to file start.
//! - req-2-3: Missing targets return `None` (no error).

use lsp_types::Location;
use supersigil_core::DocumentGraph;

use crate::REF_ATTRS;
use crate::is_in_supersigil_fence;
use crate::path_to_url;
use crate::position;

/// Extract the ref string at the given cursor position within `content`.
///
/// `line` and `character` are 0-based LSP coordinates.
///
/// Returns the trimmed ref string the cursor is on, or `None` if the cursor
/// is not inside a ref-accepting attribute value.
#[must_use]
pub fn find_ref_at_position(content: &str, line: u32, character: u32) -> Option<String> {
    // Only resolve refs inside supersigil-xml fences.
    if !is_in_supersigil_fence(content, line) {
        return None;
    }

    let line_str = content.lines().nth(line as usize)?;

    for attr in REF_ATTRS {
        // Look for `attr="..."` on this line.
        let needle = format!("{attr}=\"");
        let Some(attr_pos) = line_str.find(needle.as_str()) else {
            continue;
        };

        let value_start = attr_pos + needle.len();
        let rest = &line_str[value_start..];
        let Some(close_pos) = rest.find('"') else {
            continue;
        };

        // Byte span of the quoted value within the line.
        // Line lengths in source files are always well within u32 range.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "source line byte offsets always fit in u32"
        )]
        let span_start = value_start as u32;
        #[allow(
            clippy::cast_possible_truncation,
            reason = "source line byte offsets always fit in u32"
        )]
        let span_end = (value_start + close_pos) as u32;

        if character < span_start || character >= span_end {
            continue;
        }

        // The cursor is inside the value; find which comma-separated ref it's on.
        let value = &rest[..close_pos];
        let cursor_in_value = (character - span_start) as usize;

        // Walk through comma-separated refs tracking byte positions.
        let mut start = 0usize;
        for part in value.split(',') {
            let end = start + part.len();
            // The cursor falls in [start, end).
            if cursor_in_value >= start && cursor_in_value < end {
                let trimmed = part.trim().to_owned();
                return if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                };
            }
            // +1 to skip the comma.
            start = end + 1;
        }
    }

    None
}

/// Resolve a ref string to an LSP `Location` using the document graph.
///
/// - If `ref_str` contains `#`, looks up the fragment's source position.
/// - Otherwise resolves to the top of the document file.
/// - Returns `None` if the target is not found in the graph.
#[must_use]
pub fn resolve_ref(ref_str: &str, graph: &DocumentGraph) -> Option<Location> {
    if let Some((doc_id, fragment_id)) = ref_str.split_once('#') {
        let doc = graph.document(doc_id)?;
        let component = graph.component(doc_id, fragment_id)?;
        let lsp_pos = std::fs::read_to_string(&doc.path).map_or_else(
            |_| position::source_to_lsp(&component.position),
            |content| position::source_to_lsp_utf16(&component.position, &content),
        );
        let uri = path_to_url(&doc.path)?;
        Some(Location {
            uri,
            range: position::zero_range(lsp_pos),
        })
    } else {
        let doc = graph.document(ref_str)?;
        let uri = path_to_url(&doc.path)?;
        Some(Location {
            uri,
            range: position::zero_range(position::raw_to_lsp(0, 0)),
        })
    }
}
