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

// ---------------------------------------------------------------------------
// Enriched ref-at-position
// ---------------------------------------------------------------------------

/// Which part of a ref string the cursor is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefPart {
    /// Cursor is on the document ID portion (before `#`), or no `#` present.
    DocId,
    /// Cursor is on the fragment portion (after `#`).
    Fragment,
}

/// A ref string found at a cursor position, with span information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefAtPosition {
    /// The full ref string (e.g. "auth/req#crit-a").
    pub ref_string: String,
    /// Which part of the ref the cursor is on.
    pub part: RefPart,
    /// Byte offset within the line where the relevant part starts.
    pub part_start: u32,
    /// Byte offset within the line where the relevant part ends (exclusive).
    pub part_end: u32,
}

/// Extract the ref string at the given cursor position within `content`.
///
/// `line` and `character` are 0-based LSP coordinates.
///
/// Returns a [`RefAtPosition`] with the ref string, which part the cursor
/// is on, and the byte span of that part within the line. Returns `None` if
/// the cursor is not inside a ref-accepting attribute value.
#[must_use]
pub fn find_ref_at_position(content: &str, line: u32, character: u32) -> Option<RefAtPosition> {
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
        let mut offset = 0usize;
        for part in value.split(',') {
            let end = offset + part.len();
            // The cursor falls in [offset, end).
            if cursor_in_value >= offset && cursor_in_value < end {
                let trimmed = part.trim();
                if trimmed.is_empty() {
                    return None;
                }

                // Compute the absolute byte position of the trimmed ref
                // within the line.
                let leading_ws = part.len() - part.trim_start().len();
                let ref_abs_start = value_start + offset + leading_ws;

                // Determine which part the cursor is on and compute the
                // part span within the line.
                return Some(ref_part_at(trimmed, ref_abs_start, character));
            }
            // +1 to skip the comma.
            offset = end + 1;
        }
    }

    None
}

/// Given a trimmed ref string and its absolute byte start within the line,
/// determine which part the cursor is on and return a `RefAtPosition`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "source line byte offsets always fit in u32"
)]
fn ref_part_at(trimmed: &str, ref_abs_start: usize, character: u32) -> RefAtPosition {
    let cursor = character as usize;
    if let Some(hash_pos) = trimmed.find('#') {
        let doc_abs_start = ref_abs_start;
        let doc_abs_end = ref_abs_start + hash_pos;
        let frag_abs_start = ref_abs_start + hash_pos + 1;
        let frag_abs_end = ref_abs_start + trimmed.len();

        if cursor < frag_abs_start {
            // Cursor is on the doc ID portion (or on the `#` itself).
            RefAtPosition {
                ref_string: trimmed.to_owned(),
                part: RefPart::DocId,
                part_start: doc_abs_start as u32,
                part_end: doc_abs_end as u32,
            }
        } else {
            // Cursor is on the fragment portion.
            RefAtPosition {
                ref_string: trimmed.to_owned(),
                part: RefPart::Fragment,
                part_start: frag_abs_start as u32,
                part_end: frag_abs_end as u32,
            }
        }
    } else {
        // No `#` — entire ref is a doc ID.
        RefAtPosition {
            ref_string: trimmed.to_owned(),
            part: RefPart::DocId,
            part_start: ref_abs_start as u32,
            part_end: (ref_abs_start + trimmed.len()) as u32,
        }
    }
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use supersigil_rust::verifies;

    use super::*;

    #[test]
    #[verifies("rename/req#req-1-1")]
    fn cursor_on_fragment_returns_fragment_part() {
        // <Implements refs="auth/req#login" />
        //                           ^^^^^  cursor on "login"
        let content = "```supersigil-xml\n<Implements refs=\"auth/req#login\" />\n```";
        let result = find_ref_at_position(content, 1, 27).unwrap();
        assert_eq!(result.ref_string, "auth/req#login");
        assert_eq!(result.part, RefPart::Fragment);
        // "login" starts at byte 27 in the line, ends at 32
        assert_eq!(result.part_start, 27);
        assert_eq!(result.part_end, 32);
    }

    #[test]
    #[verifies("rename/req#req-1-2")]
    fn cursor_on_doc_id_returns_doc_id_part() {
        // <Implements refs="auth/req#login" />
        //                   ^^^^^^^^  cursor on "auth/req"
        let content = "```supersigil-xml\n<Implements refs=\"auth/req#login\" />\n```";
        let result = find_ref_at_position(content, 1, 18).unwrap();
        assert_eq!(result.ref_string, "auth/req#login");
        assert_eq!(result.part, RefPart::DocId);
        // "auth/req" starts at byte 18 in the line, ends at 26
        assert_eq!(result.part_start, 18);
        assert_eq!(result.part_end, 26);
    }

    #[test]
    #[verifies("rename/req#req-1-2")]
    fn cursor_on_hash_returns_doc_id_part() {
        // Cursor on the `#` itself should be treated as doc ID part.
        let content = "```supersigil-xml\n<Implements refs=\"auth/req#login\" />\n```";
        let result = find_ref_at_position(content, 1, 26).unwrap();
        assert_eq!(result.part, RefPart::DocId);
    }

    #[test]
    #[verifies("rename/req#req-1-2")]
    fn doc_only_ref_returns_doc_id_part() {
        // <DependsOn refs="other/doc" />
        //                  ^^^^^^^^^  no fragment
        let content = "```supersigil-xml\n<DependsOn refs=\"other/doc\" />\n```";
        let result = find_ref_at_position(content, 1, 17).unwrap();
        assert_eq!(result.ref_string, "other/doc");
        assert_eq!(result.part, RefPart::DocId);
        assert_eq!(result.part_start, 17);
        assert_eq!(result.part_end, 26);
    }

    #[test]
    #[verifies("rename/req#req-2-2")]
    fn fragment_span_covers_only_fragment() {
        // refs="doc#frag" — fragment span should be just "frag"
        let content = "```supersigil-xml\n<References refs=\"doc#frag\" />\n```";
        let result = find_ref_at_position(content, 1, 22).unwrap();
        assert_eq!(result.part, RefPart::Fragment);
        // "frag" starts after '#'
        assert_eq!(result.part_start, 22);
        assert_eq!(result.part_end, 26);
    }

    #[test]
    #[verifies("rename/req#req-2-3")]
    fn doc_id_span_covers_only_doc_id() {
        // refs="doc#frag" — doc span should be just "doc"
        let content = "```supersigil-xml\n<References refs=\"doc#frag\" />\n```";
        let result = find_ref_at_position(content, 1, 18).unwrap();
        assert_eq!(result.part, RefPart::DocId);
        assert_eq!(result.part_start, 18);
        assert_eq!(result.part_end, 21);
    }

    #[test]
    fn comma_separated_refs_second_item() {
        // refs="a/b#c, d/e#f" — cursor on "d/e" in second ref
        let content = "```supersigil-xml\n<Implements refs=\"a/b#c, d/e#f\" />\n```";
        // value starts at byte 18; "a/b#c" is 5 bytes + comma = 6; " d/e#f" has 1 space
        // so "d/e#f" starts at byte 25 (18 + 6 + 1)
        let result = find_ref_at_position(content, 1, 26).unwrap();
        assert_eq!(result.ref_string, "d/e#f");
        assert_eq!(result.part, RefPart::DocId);
        // "d/e" spans bytes 25..28, "f" is at 29..30
        assert_eq!(result.part_start, 25);
        assert_eq!(result.part_end, 28);
    }

    #[test]
    fn outside_fence_returns_none() {
        let content = "Not in a fence\n<Implements refs=\"auth/req#login\" />";
        assert!(find_ref_at_position(content, 1, 20).is_none());
    }
}
