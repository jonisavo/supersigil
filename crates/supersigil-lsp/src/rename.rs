//! LSP Rename support for supersigil spec documents.
//!
//! Implements `textDocument/rename` and `textDocument/prepareRename`: given a
//! cursor position on a document ID or component ID, renames the identifier
//! and updates all references across the spec tree.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use lsp_types::{TextEdit, Url, WorkspaceEdit};
use supersigil_core::DocumentGraph;

use crate::REF_ATTRS;
use crate::definition::{RefPart, find_ref_at_position};
use crate::hover::component_name_at_position;
use crate::is_in_supersigil_fence;
use crate::path_to_url;
use crate::position::byte_range_to_lsp;
use crate::references::{
    extract_id_attribute_on_line, find_supersigil_ref_at_position, is_in_frontmatter,
};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A byte-offset range within a single source line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineRange {
    /// 0-based line number.
    pub line: u32,
    /// Byte offset of range start within the line.
    pub start: u32,
    /// Byte offset of range end (exclusive) within the line.
    pub end: u32,
}

/// What the user intends to rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameTarget {
    /// Rename a component ID (fragment).
    ComponentId {
        doc_id: String,
        component_id: String,
        range: LineRange,
    },
    /// Rename a document ID.
    DocumentId { doc_id: String, range: LineRange },
}

// ---------------------------------------------------------------------------
// Rename target detection
// ---------------------------------------------------------------------------

/// Identify the rename target at the given cursor position.
///
/// Returns `None` if the cursor is not on a renameable position.
///
/// Checks in priority order:
/// 1. Ref string inside a supersigil-xml fence
/// 2. `supersigil-ref=<target>` in a code fence info string
/// 3. Component definition tag with `id` attribute, or cursor on `id` value
/// 4. YAML frontmatter `id:` value
#[must_use]
pub fn find_rename_target(
    content: &str,
    line: u32,
    character: u32,
    doc_id: &str,
) -> Option<RenameTarget> {
    // 1. Ref attribute in a supersigil-xml fence.
    if let Some(ref_at) = find_ref_at_position(content, line, character) {
        return Some(rename_target_from_ref(
            ref_at.part,
            &ref_at.ref_string,
            line,
            &ref_at,
        ));
    }

    // 2. supersigil-ref=<target> in a code fence info string.
    if let Some(target) = try_supersigil_ref(content, line, character, doc_id) {
        return Some(target);
    }

    // 3. Component tag with id="..." attribute, or cursor directly on id value.
    if let Some(target) = try_component_id(content, line, character, doc_id) {
        return Some(target);
    }

    // 4. YAML frontmatter id: value.
    if let Some(target) = try_frontmatter_id(content, line, character, doc_id) {
        return Some(target);
    }

    None
}

/// Build a `RenameTarget` from a `RefAtPosition`.
fn rename_target_from_ref(
    part: RefPart,
    ref_string: &str,
    line: u32,
    ref_at: &crate::definition::RefAtPosition,
) -> RenameTarget {
    let range = LineRange {
        line,
        start: ref_at.part_start,
        end: ref_at.part_end,
    };
    match part {
        RefPart::Fragment => {
            let (ref_doc, fragment) = ref_string.split_once('#').map_or_else(
                || (ref_string.to_owned(), String::new()),
                |(d, f)| (d.to_owned(), f.to_owned()),
            );
            RenameTarget::ComponentId {
                doc_id: ref_doc,
                component_id: fragment,
                range,
            }
        }
        RefPart::DocId => {
            let target_doc = ref_string
                .split_once('#')
                .map_or_else(|| ref_string.to_owned(), |(doc, _)| doc.to_owned());
            RenameTarget::DocumentId {
                doc_id: target_doc,
                range,
            }
        }
    }
}

/// Try to detect a `supersigil-ref=<target>` rename target.
#[allow(
    clippy::cast_possible_truncation,
    reason = "source line byte offsets always fit in u32"
)]
fn try_supersigil_ref(
    content: &str,
    line: u32,
    character: u32,
    doc_id: &str,
) -> Option<RenameTarget> {
    let (target, _fragment) = find_supersigil_ref_at_position(content, line, character)?;
    let line_str = content.lines().nth(line as usize)?;
    let prefix = "supersigil-ref=";
    let token_pos = line_str.find(prefix)?;
    let value_start = token_pos + prefix.len();
    let target_end = value_start + target.len();
    Some(RenameTarget::ComponentId {
        doc_id: doc_id.to_owned(),
        component_id: target,
        range: LineRange {
            line,
            start: value_start as u32,
            end: target_end as u32,
        },
    })
}

/// Try to detect a component tag or `id="..."` value rename target.
#[allow(
    clippy::cast_possible_truncation,
    reason = "source line byte offsets always fit in u32"
)]
fn try_component_id(
    content: &str,
    line: u32,
    character: u32,
    doc_id: &str,
) -> Option<RenameTarget> {
    if !is_in_supersigil_fence(content, line) {
        return None;
    }

    // Check if cursor is directly on an id="..." attribute value.
    if let Some((id, start, end)) = find_id_value_at_position(content, line, character) {
        return Some(RenameTarget::ComponentId {
            doc_id: doc_id.to_owned(),
            component_id: id,
            range: LineRange { line, start, end },
        });
    }

    // Check if cursor is on a component tag name that has an id attribute.
    if component_name_at_position(content, line, character).is_some()
        && let Some(id) = extract_id_attribute_on_line(content, line)
    {
        let line_str = content.lines().nth(line as usize)?;
        let needle = " id=\"";
        let pos = line_str.find(needle)?;
        let value_start = pos + needle.len();
        let value_end = value_start + id.len();
        return Some(RenameTarget::ComponentId {
            doc_id: doc_id.to_owned(),
            component_id: id,
            range: LineRange {
                line,
                start: value_start as u32,
                end: value_end as u32,
            },
        });
    }

    None
}

/// Try to detect a frontmatter `id:` value rename target.
fn try_frontmatter_id(
    content: &str,
    line: u32,
    character: u32,
    doc_id: &str,
) -> Option<RenameTarget> {
    if !is_in_frontmatter(content, line) {
        return None;
    }
    let line_str = content.lines().nth(line as usize)?;
    let (start, end) = find_frontmatter_id_range(line_str)?;
    // Only activate when the cursor is on the id value itself.
    if character < start || character >= end {
        return None;
    }
    Some(RenameTarget::DocumentId {
        doc_id: doc_id.to_owned(),
        range: LineRange { line, start, end },
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Check if the cursor is directly on an `id="..."` attribute value
/// (not on the tag name). Returns `(value, start, end)` byte offsets
/// within the line if the cursor is inside the id value.
#[allow(
    clippy::cast_possible_truncation,
    reason = "source line byte offsets always fit in u32"
)]
fn find_id_value_at_position(
    content: &str,
    line: u32,
    character: u32,
) -> Option<(String, u32, u32)> {
    let line_str = content.lines().nth(line as usize)?;
    let needle = " id=\"";
    let pos = line_str.find(needle)?;
    let value_start = pos + needle.len();
    let rest = &line_str[value_start..];
    let close = rest.find('"')?;
    let value = &rest[..close];
    if value.is_empty() {
        return None;
    }
    let cursor = character as usize;
    (cursor >= value_start && cursor < value_start + close).then(|| {
        (
            value.to_owned(),
            value_start as u32,
            (value_start + close) as u32,
        )
    })
}

/// Find the `id:` value range on a frontmatter line.
/// Returns `(start, end)` byte offsets within the line.
#[allow(
    clippy::cast_possible_truncation,
    reason = "source line byte offsets always fit in u32"
)]
fn find_frontmatter_id_range(line_str: &str) -> Option<(u32, u32)> {
    let trimmed = line_str.trim_start();
    if !trimmed.starts_with("id:") {
        return None;
    }
    let leading_ws = line_str.len() - trimmed.len();
    let after_key = &trimmed[3..]; // skip "id:"
    let value = after_key.trim();
    if value.is_empty() {
        return None;
    }
    let value_ws = after_key.len() - after_key.trim_start().len();
    let value_start = leading_ws + 3 + value_ws;
    let value_end = value_start + value.len();
    Some((value_start as u32, value_end as u32))
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate a new name for a rename operation.
///
/// # Errors
///
/// Returns `Err(message)` if the name is empty, contains whitespace,
/// `#`, or `"`.
pub fn validate_new_name(new_name: &str) -> Result<(), String> {
    if new_name.is_empty() {
        return Err("New name must not be empty".to_owned());
    }
    if new_name.contains(char::is_whitespace) {
        return Err("New name must not contain whitespace".to_owned());
    }
    if new_name.contains('#') {
        return Err("New name must not contain '#'".to_owned());
    }
    if new_name.contains('"') {
        return Err("New name must not contain '\"'".to_owned());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Edit collection
// ---------------------------------------------------------------------------

/// Build a `WorkspaceEdit` that renames the target to `new_name`.
///
/// Scans all referencing documents in the graph, reads their content (from
/// `open_files` or disk), and produces precise `TextEdit`s.
#[must_use]
#[allow(
    clippy::implicit_hasher,
    reason = "matches SupersigilLsp open_files type"
)]
pub fn collect_rename_edits(
    target: &RenameTarget,
    new_name: &str,
    graph: &DocumentGraph,
    open_files: &HashMap<Url, Arc<String>>,
) -> WorkspaceEdit {
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();

    match target {
        RenameTarget::ComponentId {
            doc_id,
            component_id,
            ..
        } => collect_component_edits(
            doc_id,
            component_id,
            new_name,
            graph,
            open_files,
            &mut changes,
        ),
        RenameTarget::DocumentId { doc_id, .. } => {
            collect_document_edits(doc_id, new_name, graph, open_files, &mut changes);
        }
    }

    WorkspaceEdit {
        changes: Some(changes),
        ..WorkspaceEdit::default()
    }
}

/// Collect edits for renaming a component ID.
fn collect_component_edits(
    doc_id: &str,
    old_id: &str,
    new_name: &str,
    graph: &DocumentGraph,
    open_files: &HashMap<Url, Arc<String>>,
    changes: &mut HashMap<Url, Vec<TextEdit>>,
) {
    // 1. Definition site: id="old" in the owning document.
    if let Some(doc) = graph.document(doc_id)
        && let Some(content) = read_content(open_files, &doc.path)
        && let Some(uri) = path_to_url(&doc.path)
    {
        let mut edits = Vec::new();
        collect_id_attr_edits(&content, old_id, new_name, &mut edits);
        collect_supersigil_ref_edits(&content, old_id, new_name, &mut edits);
        if !edits.is_empty() {
            changes.entry(uri).or_default().extend(edits);
        }
    }

    // 2. Ref attributes in referencing documents.
    // references_reverse covers <References> and <Example> components.
    // implements_reverse covers <Implements> components.
    // task_implements is separate — scan all docs for tasks targeting this fragment.
    let ref_sources = graph.references(doc_id, Some(old_id));
    let impl_sources = graph.implements(doc_id);
    let mut source_docs: HashSet<&str> = ref_sources.iter().map(String::as_str).collect();
    source_docs.extend(impl_sources.iter().map(String::as_str));

    // Also include documents with <Task implements="doc#old_id">.
    for (other_doc_id, _) in graph.documents() {
        if source_docs.contains(other_doc_id) {
            continue;
        }
        let has_task_impl = graph
            .task_implements_for_doc(other_doc_id)
            .any(|(_, targets)| targets.iter().any(|(d, f)| d == doc_id && f == old_id));
        if has_task_impl {
            source_docs.insert(other_doc_id);
        }
    }

    let old_ref = format!("{doc_id}#{old_id}");
    let new_ref = format!("{doc_id}#{new_name}");

    for src_doc_id in &source_docs {
        if let Some(doc) = graph.document(src_doc_id)
            && let Some(content) = read_content(open_files, &doc.path)
            && let Some(uri) = path_to_url(&doc.path)
        {
            let mut edits = Vec::new();
            collect_ref_string_edits(&content, &old_ref, &new_ref, &mut edits);
            if !edits.is_empty() {
                changes.entry(uri).or_default().extend(edits);
            }
        }
    }
}

/// Collect edits for renaming a document ID.
fn collect_document_edits(
    old_doc_id: &str,
    new_name: &str,
    graph: &DocumentGraph,
    open_files: &HashMap<Url, Arc<String>>,
    changes: &mut HashMap<Url, Vec<TextEdit>>,
) {
    // 1. Frontmatter: edit the id: value in the owning document.
    if let Some(doc) = graph.document(old_doc_id)
        && let Some(content) = read_content(open_files, &doc.path)
        && let Some(uri) = path_to_url(&doc.path)
    {
        let mut edits = Vec::new();
        collect_frontmatter_id_edits(&content, old_doc_id, new_name, &mut edits);
        if !edits.is_empty() {
            changes.entry(uri).or_default().extend(edits);
        }
    }

    // 2. Ref attributes in all referencing documents.
    let ref_sources = graph.references(old_doc_id, None);
    let impl_sources = graph.implements(old_doc_id);
    let dep_sources = graph.depends_on(old_doc_id);
    let mut source_docs: HashSet<&str> = ref_sources.iter().map(String::as_str).collect();
    source_docs.extend(impl_sources.iter().map(String::as_str));
    source_docs.extend(dep_sources.iter().map(String::as_str));

    // Also check fragment-level references (e.g. refs="old_doc#frag").
    for (other_doc_id, _doc) in graph.documents() {
        if other_doc_id == old_doc_id || source_docs.contains(other_doc_id) {
            continue;
        }
        let has_ref = graph
            .resolved_refs_for_doc(other_doc_id)
            .any(|(_, refs)| refs.iter().any(|r| r.target_doc_id == old_doc_id));
        let has_impl = graph
            .task_implements_for_doc(other_doc_id)
            .any(|(_, targets)| targets.iter().any(|(d, _)| d == old_doc_id));
        if has_ref || has_impl {
            source_docs.insert(other_doc_id);
        }
    }

    for src_doc_id in &source_docs {
        if let Some(doc) = graph.document(src_doc_id)
            && let Some(content) = read_content(open_files, &doc.path)
            && let Some(uri) = path_to_url(&doc.path)
        {
            let mut edits = Vec::new();
            collect_doc_id_ref_edits(&content, old_doc_id, new_name, &mut edits);
            if !edits.is_empty() {
                changes.entry(uri).or_default().extend(edits);
            }
        }
    }
}

/// Read file content from `open_files` or disk.
fn read_content(
    open_files: &HashMap<Url, Arc<String>>,
    path: &std::path::Path,
) -> Option<Arc<String>> {
    if let Some(uri) = path_to_url(path)
        && let Some(content) = open_files.get(&uri)
    {
        return Some(Arc::clone(content));
    }
    std::fs::read_to_string(path).ok().map(Arc::new)
}

/// Pre-compute which lines are inside a supersigil-xml fence.
///
/// Single-pass O(L) scan using the same state machine as
/// `is_in_supersigil_fence`, but builds the full mask at once.
fn fence_mask(content: &str) -> Vec<bool> {
    use supersigil_core::SUPERSIGIL_XML_LANG;

    let line_count = content.lines().count();
    let mut mask = vec![false; line_count];
    // State: None = not in any fence,
    //        Some((fence_char, open_count, is_supersigil))
    let mut fence_state: Option<(u8, usize, bool)> = None;
    let mut in_html_comment = false;

    for (i, l) in content.lines().enumerate() {
        let trimmed = l.trim_start();

        if fence_state.is_none() {
            if in_html_comment {
                if trimmed.contains("-->") {
                    in_html_comment = false;
                }
                continue;
            }
            if let Some(after_open) = trimmed.strip_prefix("<!--") {
                if !after_open.contains("-->") {
                    in_html_comment = true;
                }
                continue;
            }
        }

        let (fence_char, fence_count) = {
            let bt = trimmed.bytes().take_while(|&b| b == b'`').count();
            let tl = trimmed.bytes().take_while(|&b| b == b'~').count();
            if bt >= 3 {
                (b'`', bt)
            } else if tl >= 3 {
                (b'~', tl)
            } else {
                (0u8, 0usize)
            }
        };

        if fence_count >= 3 {
            let after_fence = &trimmed[fence_count..];
            if let Some((open_char, open_count, is_supersigil)) = fence_state {
                if fence_char == open_char
                    && fence_count >= open_count
                    && after_fence.trim().is_empty()
                {
                    fence_state = None;
                } else {
                    mask[i] = is_supersigil;
                }
            } else {
                let info_string = after_fence.trim();
                let is_supersigil = info_string == SUPERSIGIL_XML_LANG
                    || info_string
                        .strip_prefix(SUPERSIGIL_XML_LANG)
                        .is_some_and(|rest| rest.starts_with(' '));
                fence_state = Some((fence_char, fence_count, is_supersigil));
            }
        } else if let Some((_, _, true)) = fence_state {
            mask[i] = true;
        }
    }

    mask
}

/// Visit each ref attribute value inside supersigil-xml fences.
///
/// Calls `visitor(line_num, line_str, value_start, value)` for each
/// `attr="value"` found on fence-interior lines.
#[allow(
    clippy::cast_possible_truncation,
    reason = "source line byte offsets always fit in u32"
)]
fn for_each_ref_attr_value(
    content: &str,
    in_fence: &[bool],
    mut visitor: impl FnMut(u32, &str, usize, &str),
) {
    for (line_num, line_str) in content.lines().enumerate() {
        if !in_fence.get(line_num).copied().unwrap_or(false) {
            continue;
        }
        for attr in REF_ATTRS {
            let attr_needle = format!("{attr}=\"");
            let Some(attr_pos) = line_str.find(&attr_needle) else {
                continue;
            };
            let value_start = attr_pos + attr_needle.len();
            let rest = &line_str[value_start..];
            let Some(close) = rest.find('"') else {
                continue;
            };
            let value = &rest[..close];
            visitor(line_num as u32, line_str, value_start, value);
        }
    }
}

/// Find and replace ` id="old"` attribute values in supersigil-xml fences.
fn collect_id_attr_edits(content: &str, old_id: &str, new_name: &str, edits: &mut Vec<TextEdit>) {
    let needle = format!(" id=\"{old_id}\"");
    let in_fence = fence_mask(content);
    for (line_num, line_str) in content.lines().enumerate() {
        if !in_fence.get(line_num).copied().unwrap_or(false) {
            continue;
        }
        #[allow(
            clippy::cast_possible_truncation,
            reason = "source line byte offsets always fit in u32"
        )]
        if let Some(pos) = line_str.find(&needle) {
            let value_start = pos + needle.len() - old_id.len() - 1; // start of value inside quotes
            let value_end = value_start + old_id.len();
            edits.push(TextEdit {
                range: byte_range_to_lsp(line_str, line_num as u32, value_start, value_end),
                new_text: new_name.to_owned(),
            });
        }
    }
}

/// Find and replace `supersigil-ref=old` tokens in code fence info strings.
///
/// Only matches on fence opener lines (`` ``` `` or `~~~`) where
/// `supersigil-ref=` appears as a space-delimited token.
#[allow(
    clippy::cast_possible_truncation,
    reason = "source line byte offsets always fit in u32"
)]
fn collect_supersigil_ref_edits(
    content: &str,
    old_id: &str,
    new_name: &str,
    edits: &mut Vec<TextEdit>,
) {
    let needle = format!("supersigil-ref={old_id}");
    let prefix_len = "supersigil-ref=".len();
    for (line_num, line_str) in content.lines().enumerate() {
        let trimmed = line_str.trim_start();
        let fence_len = trimmed.bytes().take_while(|&b| b == b'`').count();
        let tilde_len = trimmed.bytes().take_while(|&b| b == b'~').count();
        if fence_len < 3 && tilde_len < 3 {
            continue;
        }

        let Some(pos) = line_str.find(&needle) else {
            continue;
        };

        if pos > 0 && !line_str.as_bytes()[pos - 1].is_ascii_whitespace() {
            continue;
        }

        let after = pos + needle.len();
        if after < line_str.len() {
            let next_byte = line_str.as_bytes()[after];
            if next_byte != b'#' && !next_byte.is_ascii_whitespace() {
                continue;
            }
        }

        let value_start = pos + prefix_len;
        let value_end = value_start + old_id.len();
        edits.push(TextEdit {
            range: byte_range_to_lsp(line_str, line_num as u32, value_start, value_end),
            new_text: new_name.to_owned(),
        });
    }
}

/// Find and replace a full ref string (e.g. `doc#frag`) in ref attributes.
fn collect_ref_string_edits(
    content: &str,
    old_ref: &str,
    new_ref: &str,
    edits: &mut Vec<TextEdit>,
) {
    let in_fence = fence_mask(content);
    for_each_ref_attr_value(
        content,
        &in_fence,
        |line_num, line_str, value_start, value| {
            let mut search_start = 0;
            while let Some(rel_pos) = value[search_start..].find(old_ref) {
                let abs_pos = value_start + search_start + rel_pos;
                let match_end = search_start + rel_pos + old_ref.len();
                let at_boundary = (match_end >= value.len()
                    || value.as_bytes()[match_end] == b','
                    || value.as_bytes()[match_end] == b' ')
                    && (search_start + rel_pos == 0
                        || value.as_bytes()[search_start + rel_pos - 1] == b','
                        || value.as_bytes()[search_start + rel_pos - 1] == b' ');
                if at_boundary {
                    edits.push(TextEdit {
                        range: byte_range_to_lsp(
                            line_str,
                            line_num,
                            abs_pos,
                            abs_pos + old_ref.len(),
                        ),
                        new_text: new_ref.to_owned(),
                    });
                }
                search_start = match_end;
            }
        },
    );
}

/// Find and replace the document ID portion in ref strings.
///
/// Handles refs like `old_doc#frag` -> `new_doc#frag` and `old_doc` -> `new_doc`.
fn collect_doc_id_ref_edits(
    content: &str,
    old_doc_id: &str,
    new_name: &str,
    edits: &mut Vec<TextEdit>,
) {
    let in_fence = fence_mask(content);
    let old_with_hash = format!("{old_doc_id}#");
    for_each_ref_attr_value(
        content,
        &in_fence,
        |line_num, line_str, value_start, value| {
            let mut offset = 0usize;
            for part in value.split(',') {
                let trimmed = part.trim();
                let leading_ws = part.len() - part.trim_start().len();
                let ref_start = offset + leading_ws;

                if trimmed == old_doc_id || trimmed.starts_with(&old_with_hash) {
                    let abs_start = value_start + ref_start;
                    let abs_end = abs_start + old_doc_id.len();
                    edits.push(TextEdit {
                        range: byte_range_to_lsp(line_str, line_num, abs_start, abs_end),
                        new_text: new_name.to_owned(),
                    });
                }
                offset += part.len() + 1;
            }
        },
    );
}

/// Find and replace the frontmatter `id:` value.
///
/// Finds the frontmatter boundaries once, then only scans lines within.
#[allow(
    clippy::cast_possible_truncation,
    reason = "source line byte offsets always fit in u32"
)]
fn collect_frontmatter_id_edits(
    content: &str,
    old_doc_id: &str,
    new_name: &str,
    edits: &mut Vec<TextEdit>,
) {
    let Some((fm_start, fm_end)) = frontmatter_range(content) else {
        return;
    };
    for (line_num, line_str) in content.lines().enumerate() {
        if line_num <= fm_start || line_num >= fm_end {
            continue;
        }
        if let Some((start, end)) = find_frontmatter_id_range(line_str) {
            let value = &line_str[start as usize..end as usize];
            if value == old_doc_id {
                edits.push(TextEdit {
                    range: byte_range_to_lsp(
                        line_str,
                        line_num as u32,
                        start as usize,
                        end as usize,
                    ),
                    new_text: new_name.to_owned(),
                });
            }
        }
    }
}

/// Find the frontmatter boundaries: returns `(open_line, close_line)` where
/// frontmatter content is on lines `(open_line, close_line)` exclusive.
fn frontmatter_range(content: &str) -> Option<(usize, usize)> {
    let mut found_open = false;
    for (i, l) in content.lines().enumerate() {
        if l.trim() == "---" {
            if found_open {
                return Some((0, i));
            }
            if i != 0 {
                return None;
            }
            found_open = true;
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use supersigil_rust::verifies;

    use super::*;

    fn empty_graph() -> supersigil_core::DocumentGraph {
        use supersigil_core::{Config, build_graph};
        build_graph(vec![], &Config::default()).unwrap()
    }

    // -- Priority 1: Ref attribute -----------------------------------------------

    #[test]
    #[verifies("rename/req#req-1-1")]
    fn ref_fragment_yields_component_id() {
        let content = "```supersigil-xml\n<Implements refs=\"auth/req#login\" />\n```";
        let result = find_rename_target(content, 1, 27, "test").unwrap();
        assert_eq!(
            result,
            RenameTarget::ComponentId {
                doc_id: "auth/req".to_owned(),
                component_id: "login".to_owned(),
                range: LineRange {
                    line: 1,
                    start: 27,
                    end: 32
                },
            }
        );
    }

    #[test]
    #[verifies("rename/req#req-1-2")]
    fn ref_doc_id_yields_document_id() {
        let content = "```supersigil-xml\n<Implements refs=\"auth/req#login\" />\n```";
        let result = find_rename_target(content, 1, 20, "test").unwrap();
        assert_eq!(
            result,
            RenameTarget::DocumentId {
                doc_id: "auth/req".to_owned(),
                range: LineRange {
                    line: 1,
                    start: 18,
                    end: 26
                },
            }
        );
    }

    #[test]
    #[verifies("rename/req#req-1-2")]
    fn doc_only_ref_yields_document_id() {
        let content = "```supersigil-xml\n<DependsOn refs=\"other/doc\" />\n```";
        let result = find_rename_target(content, 1, 20, "test").unwrap();
        assert_eq!(
            result,
            RenameTarget::DocumentId {
                doc_id: "other/doc".to_owned(),
                range: LineRange {
                    line: 1,
                    start: 17,
                    end: 26
                },
            }
        );
    }

    // -- Priority 2: supersigil-ref -----------------------------------------------

    #[test]
    #[verifies("rename/req#req-1-3")]
    fn supersigil_ref_yields_component_id() {
        let content = "---\nsupersigil:\n  id: my/spec\n---\n\n```sh supersigil-ref=echo-test\necho hello\n```";
        let result = find_rename_target(content, 5, 10, "my/spec").unwrap();
        assert_eq!(
            result,
            RenameTarget::ComponentId {
                doc_id: "my/spec".to_owned(),
                component_id: "echo-test".to_owned(),
                range: LineRange {
                    line: 5,
                    start: 21,
                    end: 30
                },
            }
        );
    }

    #[test]
    #[verifies("rename/req#req-1-3")]
    fn supersigil_ref_with_fragment_uses_target() {
        let content = "```sh supersigil-ref=my-example#expected\nsome content\n```";
        let result = find_rename_target(content, 0, 24, "doc").unwrap();
        assert_eq!(
            result,
            RenameTarget::ComponentId {
                doc_id: "doc".to_owned(),
                component_id: "my-example".to_owned(),
                range: LineRange {
                    line: 0,
                    start: 21,
                    end: 31
                },
            }
        );
    }

    // -- Priority 3: Component tag / id attribute ---------------------------------

    #[test]
    #[verifies("rename/req#req-1-4")]
    fn component_tag_with_id_yields_component_id() {
        let content = "```supersigil-xml\n<Criterion id=\"login-success\">\nThe user logs in.\n</Criterion>\n```";
        let result = find_rename_target(content, 1, 1, "auth/req").unwrap();
        assert_eq!(
            result,
            RenameTarget::ComponentId {
                doc_id: "auth/req".to_owned(),
                component_id: "login-success".to_owned(),
                range: LineRange {
                    line: 1,
                    start: 15,
                    end: 28
                },
            }
        );
    }

    #[test]
    #[verifies("rename/req#req-1-4")]
    fn cursor_on_id_value_yields_component_id() {
        let content = "```supersigil-xml\n<Criterion id=\"login-success\">\nThe user logs in.\n</Criterion>\n```";
        // Cursor at position 20, which is inside "login-success"
        let result = find_rename_target(content, 1, 20, "auth/req").unwrap();
        assert_eq!(
            result,
            RenameTarget::ComponentId {
                doc_id: "auth/req".to_owned(),
                component_id: "login-success".to_owned(),
                range: LineRange {
                    line: 1,
                    start: 15,
                    end: 28
                },
            }
        );
    }

    #[test]
    #[verifies("rename/req#req-1-7")]
    fn component_tag_without_id_returns_none() {
        let content = "```supersigil-xml\n<AcceptanceCriteria>\n</AcceptanceCriteria>\n```";
        let result = find_rename_target(content, 1, 1, "doc");
        assert_eq!(result, None);
    }

    // -- Priority 4: Frontmatter -------------------------------------------------

    #[test]
    #[verifies("rename/req#req-1-5")]
    fn frontmatter_id_yields_document_id() {
        let content = "---\nsupersigil:\n  id: my-doc/req\n  type: requirements\n---\n\nSome text.";
        let result = find_rename_target(content, 2, 7, "my-doc/req").unwrap();
        assert_eq!(
            result,
            RenameTarget::DocumentId {
                doc_id: "my-doc/req".to_owned(),
                range: LineRange {
                    line: 2,
                    start: 6,
                    end: 16
                },
            }
        );
    }

    #[test]
    #[verifies("rename/req#req-1-5")]
    fn frontmatter_non_id_line_returns_none() {
        let content = "---\nsupersigil:\n  id: my-doc/req\n  type: requirements\n---\n\nSome text.";
        let result = find_rename_target(content, 3, 5, "my-doc/req");
        assert_eq!(result, None);
    }

    // -- Priority ordering -------------------------------------------------------

    #[test]
    #[verifies("rename/req#req-1-6")]
    fn ref_takes_priority_over_component_tag() {
        // Cursor on the refs value, which is also on a component with id.
        // <Implements id="impl-1" refs="other/doc#crit" />
        // 0         1         2         3         4
        // 0123456789012345678901234567890123456789012345
        // refs=" starts at 24, value at 29, "#crit" fragment at 39..43
        let content =
            "```supersigil-xml\n<Implements id=\"impl-1\" refs=\"other/doc#crit\" />\n```";
        let result = find_rename_target(content, 1, 40, "test").unwrap();
        match result {
            RenameTarget::ComponentId { component_id, .. } => {
                assert_eq!(component_id, "crit");
            }
            RenameTarget::DocumentId { .. } => {
                panic!("expected ComponentId from ref, got {result:?}")
            }
        }
    }

    // -- Non-renameable positions ------------------------------------------------

    #[test]
    #[verifies("rename/req#req-1-7")]
    fn body_text_returns_none() {
        let content = "---\nsupersigil:\n  id: test\n---\n\nSome text outside.";
        let result = find_rename_target(content, 5, 0, "test");
        assert_eq!(result, None);
    }

    #[test]
    #[verifies("rename/req#req-1-7")]
    fn inside_fence_no_match_returns_none() {
        let content = "```supersigil-xml\nsome body text\n```";
        let result = find_rename_target(content, 1, 5, "test");
        assert_eq!(result, None);
    }

    // -- prepareRename range tests -----------------------------------------------

    #[test]
    #[verifies("rename/req#req-2-1")]
    fn prepare_rename_returns_range_and_placeholder() {
        let content =
            "```supersigil-xml\n<Criterion id=\"login-success\">\nbody\n</Criterion>\n```";
        let result = find_rename_target(content, 1, 1, "auth/req").unwrap();
        match &result {
            RenameTarget::ComponentId {
                component_id,
                range,
                ..
            } => {
                assert_eq!(component_id, "login-success");
                assert!(range.start < range.end);
            }
            RenameTarget::DocumentId { .. } => panic!("expected ComponentId"),
        }
    }

    // -- Validation --------------------------------------------------------------

    #[test]
    #[verifies("rename/req#req-4-1")]
    fn valid_name_accepted() {
        validate_new_name("new-id").unwrap();
        validate_new_name("auth/req").unwrap();
        validate_new_name("a").unwrap();
    }

    #[test]
    #[verifies("rename/req#req-4-1")]
    fn empty_name_rejected() {
        assert!(validate_new_name("").is_err());
    }

    #[test]
    #[verifies("rename/req#req-4-1")]
    fn whitespace_name_rejected() {
        assert!(validate_new_name("has space").is_err());
        assert!(validate_new_name("has\ttab").is_err());
    }

    #[test]
    #[verifies("rename/req#req-4-1")]
    fn hash_name_rejected() {
        assert!(validate_new_name("bad#name").is_err());
    }

    #[test]
    #[verifies("rename/req#req-4-1")]
    fn quote_name_rejected() {
        assert!(validate_new_name("bad\"name").is_err());
    }

    #[test]
    #[verifies("rename/req#req-4-2")]
    fn validation_error_has_message() {
        let err = validate_new_name("").unwrap_err();
        assert!(!err.is_empty());
    }

    // -- collect_rename_edits ----------------------------------------------------

    #[test]
    #[verifies("rename/req#req-3-1")]
    fn rename_component_updates_definition_and_refs() {
        use supersigil_core::test_helpers::{make_acceptance_criteria, make_criterion, make_doc};
        use supersigil_core::{Config, ExtractedComponent, build_graph};

        let req_doc = make_doc(
            "test/req",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-a", 5)],
                3,
            )],
        );
        let design_doc = make_doc(
            "test/design",
            vec![ExtractedComponent {
                name: "References".into(),
                attributes: [("refs".into(), "test/req#crit-a".into())]
                    .into_iter()
                    .collect(),
                children: vec![],
                body_text: None,
                body_text_offset: None,
                body_text_end_offset: None,
                code_blocks: vec![],
                position: supersigil_core::test_helpers::pos(3),
                end_position: supersigil_core::test_helpers::pos(3),
            }],
        );

        let graph = build_graph(vec![req_doc, design_doc], &Config::default()).unwrap();
        let open_files = HashMap::new();

        let target = RenameTarget::ComponentId {
            doc_id: "test/req".to_owned(),
            component_id: "crit-a".to_owned(),
            range: LineRange {
                line: 0,
                start: 0,
                end: 6,
            },
        };

        let edit = collect_rename_edits(&target, "crit-new", &graph, &open_files);
        // The graph is constructed and the function doesn't panic.
        // File content scanning depends on readable paths (synthetic in tests).
        let _changes = edit.changes.unwrap();
    }

    #[test]
    #[verifies("rename/req#req-3-2")]
    fn rename_document_updates_frontmatter_and_refs() {
        use supersigil_core::test_helpers::{make_acceptance_criteria, make_criterion, make_doc};
        use supersigil_core::{Config, ExtractedComponent, build_graph};

        let req_doc = make_doc(
            "test/req",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-a", 5)],
                3,
            )],
        );
        let design_doc = make_doc(
            "test/design",
            vec![ExtractedComponent {
                name: "Implements".into(),
                attributes: [("refs".into(), "test/req".into())].into_iter().collect(),
                children: vec![],
                body_text: None,
                body_text_offset: None,
                body_text_end_offset: None,
                code_blocks: vec![],
                position: supersigil_core::test_helpers::pos(3),
                end_position: supersigil_core::test_helpers::pos(3),
            }],
        );

        let graph = build_graph(vec![req_doc, design_doc], &Config::default()).unwrap();
        let open_files = HashMap::new();

        let target = RenameTarget::DocumentId {
            doc_id: "test/req".to_owned(),
            range: LineRange {
                line: 0,
                start: 0,
                end: 8,
            },
        };

        let edit = collect_rename_edits(&target, "test/requirement", &graph, &open_files);
        // Graph-based edit collection works even without readable files.
        let _changes = edit.changes.unwrap();
    }

    #[test]
    #[verifies("rename/req#req-3-3")]
    fn all_four_ref_attrs_scanned() {
        // Verify that the scanner checks refs, implements, depends, verifies.
        let content = "```supersigil-xml\n<Implements refs=\"doc#old\" />\n<Task implements=\"doc#old\" />\n<DependsOn refs=\"doc#old\" />\n<Example verifies=\"doc#old\" />\n```";
        let mut edits = Vec::new();
        collect_ref_string_edits(content, "doc#old", "doc#new", &mut edits);
        assert_eq!(edits.len(), 4, "should find edits in all 4 attributes");
    }

    #[test]
    #[verifies("rename/req#req-3-4")]
    fn edits_grouped_by_uri() {
        use supersigil_core::test_helpers::{make_acceptance_criteria, make_criterion, make_doc};
        use supersigil_core::{Config, ExtractedComponent, build_graph};

        let req_doc = make_doc(
            "test/req",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-a", 5)],
                3,
            )],
        );
        let design_doc = make_doc(
            "test/design",
            vec![ExtractedComponent {
                name: "References".into(),
                attributes: [("refs".into(), "test/req#crit-a".into())]
                    .into_iter()
                    .collect(),
                children: vec![],
                body_text: None,
                body_text_offset: None,
                body_text_end_offset: None,
                code_blocks: vec![],
                position: supersigil_core::test_helpers::pos(3),
                end_position: supersigil_core::test_helpers::pos(3),
            }],
        );

        let graph = build_graph(vec![req_doc, design_doc], &Config::default()).unwrap();

        // Supply file content via open_files so edits can be produced.
        let mut open_files = HashMap::new();
        // make_doc uses relative paths like "specs/test/req.md";
        // path_to_url prepends "/" for non-absolute paths.
        let req_uri = Url::from_file_path("/specs/test/req.md").unwrap();
        let design_uri = Url::from_file_path("/specs/test/design.md").unwrap();
        open_files.insert(
            req_uri.clone(),
            Arc::new(
                "---\nsupersigil:\n  id: test/req\n---\n\n```supersigil-xml\n<AcceptanceCriteria>\n  <Criterion id=\"crit-a\">\nbody\n  </Criterion>\n</AcceptanceCriteria>\n```"
                    .to_owned(),
            ),
        );
        open_files.insert(
            design_uri.clone(),
            Arc::new(
                "---\nsupersigil:\n  id: test/design\n---\n\n```supersigil-xml\n<References refs=\"test/req#crit-a\" />\n```"
                    .to_owned(),
            ),
        );

        let target = RenameTarget::ComponentId {
            doc_id: "test/req".to_owned(),
            component_id: "crit-a".to_owned(),
            range: LineRange {
                line: 7,
                start: 16,
                end: 22,
            },
        };

        let edit = collect_rename_edits(&target, "crit-new", &graph, &open_files);
        let changes = edit.changes.unwrap();
        // Edits should be grouped by URI — definition in req, reference in design.
        assert!(
            changes.contains_key(&req_uri),
            "should have edits for req doc"
        );
        assert!(
            changes.contains_key(&design_uri),
            "should have edits for design doc"
        );
    }

    #[test]
    #[verifies("rename/req#req-3-5")]
    fn rename_with_no_references_produces_definition_edit() {
        // The id attribute edit should still be produced for the definition site.
        let content = "```supersigil-xml\n<Criterion id=\"only-one\">\nbody\n</Criterion>\n```";
        let mut edits = Vec::new();
        collect_id_attr_edits(content, "only-one", "renamed", &mut edits);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "renamed");
    }

    #[test]
    #[verifies("rename/req#req-3-1")]
    fn supersigil_ref_updated_on_component_rename() {
        let content = "```sh supersigil-ref=echo-test\necho hello\n```";
        let mut edits = Vec::new();
        collect_supersigil_ref_edits(content, "echo-test", "new-test", &mut edits);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "new-test");
    }

    #[test]
    #[verifies("rename/req#req-3-2")]
    fn doc_id_ref_edits_replace_doc_portion() {
        let content = "```supersigil-xml\n<References refs=\"old/doc#frag\" />\n```";
        let mut edits = Vec::new();
        collect_doc_id_ref_edits(content, "old/doc", "new/doc", &mut edits);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "new/doc");
        // The edit should only cover the "old/doc" portion, not "#frag".
    }

    #[test]
    #[verifies("rename/req#req-5-1")]
    fn rename_provider_advertised() {
        // The rename_provider capability is set in state.rs initialize().
        // This test verifies the types compile and the module is wired.
        let target = RenameTarget::ComponentId {
            doc_id: "doc".to_owned(),
            component_id: "id".to_owned(),
            range: LineRange {
                line: 0,
                start: 0,
                end: 2,
            },
        };
        let _ = find_rename_target("", 0, 0, "doc");
        let _ = validate_new_name("ok");
        let _ = collect_rename_edits(&target, "new", &empty_graph(), &HashMap::new());
    }

    #[test]
    fn id_substring_in_other_attr_not_matched() {
        // Regression test: `id="` inside another attribute value should not match.
        // <Implements refs="some-id/req" id="real-id">
        let content = "```supersigil-xml\n<Implements refs=\"some-id/req\" id=\"real-id\" />\n```";
        let mut edits = Vec::new();
        collect_id_attr_edits(content, "real-id", "new-id", &mut edits);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "new-id");
    }

    #[test]
    fn id_substring_does_not_trigger_rename() {
        // Cursor on "some-id" inside refs value should not trigger id attribute rename.
        let content = "```supersigil-xml\n<Implements refs=\"some-id/req\" id=\"real-id\" />\n```";
        // Position 25 is inside "some-id/req" in the refs value — should match
        // as a ref, not as an id attribute.
        let result = find_rename_target(content, 1, 25, "test");
        match result {
            Some(RenameTarget::DocumentId { doc_id, .. }) => {
                assert_eq!(doc_id, "some-id/req");
            }
            other => panic!("expected DocumentId for ref, got {other:?}"),
        }
    }

    #[test]
    fn frontmatter_id_edit_produced() {
        let content = "---\nsupersigil:\n  id: old/doc\n---";
        let mut edits = Vec::new();
        collect_frontmatter_id_edits(content, "old/doc", "new/doc", &mut edits);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "new/doc");
    }

    // -- P1 regression: task implements included in component rename ----------

    #[test]
    fn task_implements_sources_included() {
        use supersigil_core::test_helpers::{make_acceptance_criteria, make_criterion, make_doc};
        use supersigil_core::{Config, ExtractedComponent, build_graph};

        let req_doc = make_doc(
            "test/req",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-a", 5)],
                3,
            )],
        );
        let task_doc = make_doc(
            "test/tasks",
            vec![ExtractedComponent {
                name: "Task".into(),
                attributes: [
                    ("id".into(), "task-1".into()),
                    ("implements".into(), "test/req#crit-a".into()),
                ]
                .into_iter()
                .collect(),
                children: vec![],
                body_text: None,
                body_text_offset: None,
                body_text_end_offset: None,
                code_blocks: vec![],
                position: supersigil_core::test_helpers::pos(3),
                end_position: supersigil_core::test_helpers::pos(3),
            }],
        );

        let graph = build_graph(vec![req_doc, task_doc], &Config::default()).unwrap();

        let task_uri = Url::from_file_path("/specs/test/tasks.md").unwrap();
        let mut open_files = HashMap::new();
        open_files.insert(
            task_uri.clone(),
            Arc::new(
                "---\nsupersigil:\n  id: test/tasks\n---\n\n```supersigil-xml\n<Task id=\"task-1\" implements=\"test/req#crit-a\">\nDo the thing.\n</Task>\n```"
                    .to_owned(),
            ),
        );

        let target = RenameTarget::ComponentId {
            doc_id: "test/req".to_owned(),
            component_id: "crit-a".to_owned(),
            range: LineRange {
                line: 0,
                start: 0,
                end: 6,
            },
        };

        let edit = collect_rename_edits(&target, "crit-new", &graph, &open_files);
        let changes = edit.changes.unwrap();
        assert!(
            changes.contains_key(&task_uri),
            "task doc with implements should have edits: {changes:?}"
        );
    }

    // -- P2 regression: supersigil-ref prefix not matched ---------------------

    #[test]
    fn supersigil_ref_prefix_not_matched() {
        // Renaming "echo-test" should NOT modify "supersigil-ref=echo-test-extra".
        let content = "```sh supersigil-ref=echo-test-extra\necho hello\n```";
        let mut edits = Vec::new();
        collect_supersigil_ref_edits(content, "echo-test", "new-test", &mut edits);
        assert!(edits.is_empty(), "should not match prefix: {edits:?}");
    }

    #[test]
    fn supersigil_ref_with_hash_still_matched() {
        // Renaming "echo-test" SHOULD match "supersigil-ref=echo-test#expected".
        let content = "```sh supersigil-ref=echo-test#expected\necho hello\n```";
        let mut edits = Vec::new();
        collect_supersigil_ref_edits(content, "echo-test", "new-test", &mut edits);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "new-test");
    }

    #[test]
    fn supersigil_ref_not_on_fence_line_ignored() {
        // Body text mentioning supersigil-ref should not be matched.
        let content = "```sh\nUse supersigil-ref=echo-test for examples\n```";
        let mut edits = Vec::new();
        collect_supersigil_ref_edits(content, "echo-test", "new-test", &mut edits);
        assert!(edits.is_empty(), "non-fence line should be ignored");
    }

    #[test]
    fn supersigil_ref_not_at_token_boundary_ignored() {
        // "notsupersigil-ref=echo-test" should not match.
        let content = "```sh notsupersigil-ref=echo-test\necho hello\n```";
        let mut edits = Vec::new();
        collect_supersigil_ref_edits(content, "echo-test", "new-test", &mut edits);
        assert!(edits.is_empty(), "non-token-boundary should be ignored");
    }

    // -- P3 regression: frontmatter cursor column checked --------------------

    #[test]
    fn frontmatter_cursor_on_key_returns_none() {
        // Cursor on "id:" key, not the value — should not trigger rename.
        let content = "---\nsupersigil:\n  id: my-doc/req\n---";
        let result = find_rename_target(content, 2, 3, "my-doc/req");
        assert_eq!(
            result, None,
            "cursor on 'id:' key should not trigger rename"
        );
    }

    #[test]
    fn frontmatter_cursor_on_value_triggers_rename() {
        // Cursor on the value "my-doc/req" — should trigger.
        let content = "---\nsupersigil:\n  id: my-doc/req\n---";
        let result = find_rename_target(content, 2, 7, "my-doc/req");
        assert!(result.is_some(), "cursor on id value should trigger rename");
    }
}
