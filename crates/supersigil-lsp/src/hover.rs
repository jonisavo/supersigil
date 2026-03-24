//! Hover support for supersigil component names and ref strings.
//!
//! Implements:
//! - req-4-1: Hover over a component name shows definition (attributes, flags).
//! - req-4-2: Hover over a ref string shows target context (title, body).

use std::fmt::Write as _;

use lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Url};
use supersigil_core::{ComponentDefs, DocumentGraph, SourcePosition, SpecDocument};

use crate::definition::find_ref_at_position;
use crate::is_in_supersigil_fence;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Return hover information for the component definition named `name`.
///
/// Returns `None` if the component is not found in `defs`.
#[must_use]
pub fn hover_component(name: &str, defs: &ComponentDefs) -> Option<Hover> {
    let def = defs.get(name)?;

    let mut md = format!("### {name}\n");

    if let Some(desc) = &def.description {
        md.push('\n');
        md.push_str(desc);
        md.push('\n');
    }

    // Build attribute table.
    if def.attributes.is_empty() {
        md.push_str("\n_No attributes._\n");
    } else {
        md.push_str("\n| Attribute | Required | List |\n");
        md.push_str("|-----------|----------|------|\n");

        // Sort attributes for stable output.
        let mut attrs: Vec<(&str, bool, bool)> = def
            .attributes
            .iter()
            .map(|(k, v)| (k.as_str(), v.required, v.list))
            .collect();
        attrs.sort_by_key(|(k, _, _)| *k);

        for (attr, required, list) in attrs {
            let req_str = if required { "yes" } else { "no" };
            let list_str = if list { "yes" } else { "no" };
            let _ = writeln!(md, "| {attr} | {req_str} | {list_str} |");
        }
    }

    let ref_str = if def.referenceable { "yes" } else { "no" };
    let ver_str = if def.verifiable { "yes" } else { "no" };
    let _ = writeln!(md, "\nReferenceable: {ref_str} | Verifiable: {ver_str}");

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: md,
        }),
        range: None,
    })
}

/// Return hover information for a ref string (fragment or document-level).
///
/// - Fragment refs (`doc-id#fragment-id`): shows document title, type/status,
///   and the criterion's body text.
/// - Document-level refs (`doc-id`): shows document title, type, and status.
///
/// Returns `None` if the target is not found in the graph.
#[must_use]
pub fn hover_ref(ref_str: &str, graph: &DocumentGraph) -> Option<Hover> {
    let md = if let Some((doc_id, fragment_id)) = ref_str.split_once('#') {
        let doc = graph.document(doc_id)?;
        let component = graph.component(doc_id, fragment_id)?;

        let title = doc_title(doc);
        let kind = &component.name;
        let link = file_link(&doc.path, Some(&component.position));

        let mut md = format!("### [{title} — {kind} `{fragment_id}`]({link})\n\n");

        if let Some(body) = &component.body_text {
            let _ = writeln!(md, "> {body}");
        }

        md
    } else {
        let doc = graph.document(ref_str)?;

        let title = doc_title(doc);
        let doc_type = doc.frontmatter.doc_type.as_deref().unwrap_or("unknown");
        let status = doc.frontmatter.status.as_deref().unwrap_or("unknown");
        let link = file_link(&doc.path, None);

        format!("### [{title}]({link})\n\n**Type:** {doc_type} | **Status:** {status}\n")
    };

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: md,
        }),
        range: None,
    })
}

/// Detect what is at the cursor position and return the appropriate hover.
///
/// - If the cursor is on a word immediately after `<`, calls [`hover_component`].
/// - If the cursor is inside a ref attribute value, calls [`hover_ref`].
/// - Otherwise returns `None`.
#[must_use]
pub fn hover_at_position(
    content: &str,
    line: u32,
    character: u32,
    defs: &ComponentDefs,
    graph: &DocumentGraph,
) -> Option<Hover> {
    // Only provide hover inside supersigil-xml fences.
    if !is_in_supersigil_fence(content, line) {
        return None;
    }

    // Check ref first (more specific).
    if let Some(ref_str) = find_ref_at_position(content, line, character) {
        return hover_ref(&ref_str, graph);
    }

    // Check for component name: cursor on a word after `<`.
    if let Some(name) = component_name_at_position(content, line, character) {
        return hover_component(&name, defs);
    }

    None
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Extract the component name at the cursor position, if any.
///
/// Returns `Some(name)` if the cursor is on an identifier that immediately
/// follows a `<` character on the same line (allowing for the closing tag
/// prefix `</` as well).
pub(crate) fn component_name_at_position(
    content: &str,
    line: u32,
    character: u32,
) -> Option<String> {
    let line_str = content.lines().nth(line as usize)?;
    let char_idx = character as usize;

    // The character under the cursor must be alphanumeric (component names are
    // PascalCase identifiers).
    let ch = line_str.chars().nth(char_idx)?;
    if !ch.is_alphanumeric() {
        return None;
    }

    // Find the start of the word containing `char_idx`.
    let word_start = line_str[..char_idx]
        .rfind(|c: char| !c.is_alphanumeric())
        .map_or(0, |p| p + 1);

    // Find the end of the word.
    let word_end = line_str[char_idx..]
        .find(|c: char| !c.is_alphanumeric())
        .map_or(line_str.len(), |p| char_idx + p);

    let word = &line_str[word_start..word_end];

    // Check that the character just before the word is `<` (open tag) or
    // that the sequence before is `</` (close tag).
    if word_start == 0 {
        return None;
    }

    let before = &line_str[..word_start];
    if !before.ends_with('<') && !before.ends_with("</") {
        return None;
    }

    // Only return if the word starts with an uppercase letter (PascalCase).
    if !word.starts_with(|c: char| c.is_uppercase()) {
        return None;
    }

    Some(word.to_owned())
}

/// Build a `file://` URI with an optional line fragment for clickable links
/// in hover tooltips.
fn file_link(path: &std::path::Path, position: Option<&SourcePosition>) -> String {
    let uri = Url::from_file_path(path)
        .map_or_else(|()| format!("file://{}", path.display()), |u| u.to_string());
    match position {
        Some(pos) if pos.line > 0 => format!("{uri}#{}", pos.line),
        _ => uri,
    }
}

/// Get the document title from the `extra` [`HashMap`], falling back to the ID.
fn doc_title(doc: &SpecDocument) -> String {
    doc.extra
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or(&doc.frontmatter.id)
        .to_owned()
}
