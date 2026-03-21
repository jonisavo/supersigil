//! Autocomplete support for supersigil MDX documents.
//!
//! Implements:
//! - req-3-1: Document ID completions inside ref-accepting attributes before `#`.
//! - req-3-2: Fragment ID completions inside ref-accepting attributes after `#`.
//! - req-3-3: Component name completions with snippet after `<`.
//! - req-3-4: Attribute value completions for `strategy` and `status`.

use lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};
use supersigil_core::{ComponentDefs, DocumentGraph};

use crate::REF_ATTRS;

// ---------------------------------------------------------------------------
// CompletionContext
// ---------------------------------------------------------------------------

/// Describes what the cursor is positioned inside, determining which
/// completion provider to invoke.
#[derive(Debug, PartialEq)]
pub enum CompletionContext {
    /// Inside a ref-accepting attribute value before `#` or with no `#`.
    RefDocId { prefix: String },
    /// Inside a ref-accepting attribute value after `doc-id#`.
    RefFragment { doc_id: String, prefix: String },
    /// After `<` — completing a component name.
    ComponentName { prefix: String },
    /// Inside a `strategy="..."` attribute value.
    AttributeStrategy { prefix: String },
    /// Inside a `status="..."` attribute value.
    AttributeStatus { prefix: String },
    /// No recognized completion context.
    None,
}

// ---------------------------------------------------------------------------
// Context detection
// ---------------------------------------------------------------------------

/// Detect the completion context at the given cursor position.
///
/// `line` and `character` are 0-based LSP coordinates.
#[must_use]
pub fn detect_context(content: &str, line: u32, character: u32) -> CompletionContext {
    let Some(line_str) = content.lines().nth(line as usize) else {
        return CompletionContext::None;
    };

    let char_pos = character as usize;
    // Clamp to line length in case character is past end.
    let char_pos = char_pos.min(line_str.len());

    let before_cursor = &line_str[..char_pos];

    // --- Check for ref-accepting attributes ---
    for attr in REF_ATTRS {
        let needle = format!("{attr}=\"");
        if let Some(attr_start) = before_cursor.rfind(needle.as_str()) {
            let value_start = attr_start + needle.len();
            let value_so_far = &before_cursor[value_start..];

            // If there's a closing quote, the cursor is past this attribute.
            if value_so_far.contains('"') {
                continue;
            }

            // Find the last comma to get the current token being typed.
            let token = if let Some(comma_pos) = value_so_far.rfind(',') {
                value_so_far[comma_pos + 1..].trim_start()
            } else {
                value_so_far
            };

            // Split on `#` to determine if we're after a doc-id.
            if let Some(hash_pos) = token.rfind('#') {
                let doc_id = token[..hash_pos].to_owned();
                let frag_prefix = token[hash_pos + 1..].to_owned();
                return CompletionContext::RefFragment {
                    doc_id,
                    prefix: frag_prefix,
                };
            }

            return CompletionContext::RefDocId {
                prefix: token.to_owned(),
            };
        }
    }

    // --- Check for strategy="..." ---
    let strategy_needle = "strategy=\"";
    if let Some(attr_start) = before_cursor.rfind(strategy_needle) {
        let value_start = attr_start + strategy_needle.len();
        let value_so_far = &before_cursor[value_start..];
        if !value_so_far.contains('"') {
            return CompletionContext::AttributeStrategy {
                prefix: value_so_far.to_owned(),
            };
        }
    }

    // --- Check for status="..." ---
    let status_needle = "status=\"";
    if let Some(attr_start) = before_cursor.rfind(status_needle) {
        let value_start = attr_start + status_needle.len();
        let value_so_far = &before_cursor[value_start..];
        if !value_so_far.contains('"') {
            return CompletionContext::AttributeStatus {
                prefix: value_so_far.to_owned(),
            };
        }
    }

    // --- Check for component name after `<` ---
    // Find the last `<` in the text before cursor, ignoring `</` closing tags.
    if let Some(lt_pos) = before_cursor.rfind('<') {
        let after_lt = &before_cursor[lt_pos + 1..];
        // Skip if it's a closing tag or an HTML comment.
        if !after_lt.starts_with('/') && !after_lt.starts_with('!') {
            // The prefix is everything after `<` until the cursor,
            // provided it doesn't contain whitespace (which would mean we're
            // past the component name into attributes).
            if !after_lt.contains(char::is_whitespace) {
                return CompletionContext::ComponentName {
                    prefix: after_lt.to_owned(),
                };
            }
        }
    }

    CompletionContext::None
}

// ---------------------------------------------------------------------------
// Completion providers
// ---------------------------------------------------------------------------

/// Return all document IDs that start with `prefix`.
///
/// Each item has kind `REFERENCE` and detail set to the document type or ID.
#[must_use]
pub fn complete_document_ids(prefix: &str, graph: &DocumentGraph) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = graph
        .documents()
        .filter(|(id, _)| id.starts_with(prefix))
        .map(|(id, doc)| {
            let detail = doc
                .frontmatter
                .doc_type
                .as_deref()
                .or(Some(id))
                .map(str::to_owned);
            CompletionItem {
                label: id.to_owned(),
                kind: Some(CompletionItemKind::REFERENCE),
                detail,
                ..CompletionItem::default()
            }
        })
        .collect();

    // Sort for deterministic ordering.
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

/// Return all referenceable component IDs within `doc_id` that start with `prefix`.
///
/// Each item has kind `REFERENCE` and detail set to a preview of the body text.
#[must_use]
pub fn complete_fragment_ids(
    doc_id: &str,
    prefix: &str,
    graph: &DocumentGraph,
) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = graph
        .criteria()
        .filter(|(d, frag, _)| *d == doc_id && frag.starts_with(prefix))
        .map(|(_, frag, comp)| {
            let detail = comp.body_text.as_deref().map(|t| {
                let preview: String = t.chars().take(60).collect();
                if t.len() > 60 {
                    format!("{preview}…")
                } else {
                    preview
                }
            });
            CompletionItem {
                label: frag.to_owned(),
                kind: Some(CompletionItemKind::REFERENCE),
                detail,
                ..CompletionItem::default()
            }
        })
        .collect();

    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

/// Return all component names starting with `prefix`.
///
/// Each item has kind `CLASS`, detail `"Supersigil"`, and an insert text snippet
/// with the required attributes pre-filled.
#[must_use]
pub fn complete_component_names(prefix: &str, defs: &ComponentDefs) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = defs
        .iter()
        .filter(|(name, _)| name.starts_with(prefix))
        .map(|(name, def)| {
            let snippet = build_component_snippet(name, def);
            CompletionItem {
                label: name.to_owned(),
                kind: Some(CompletionItemKind::CLASS),
                detail: Some("Supersigil".to_owned()),
                insert_text: Some(snippet),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..CompletionItem::default()
            }
        })
        .collect();

    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

/// Build a snippet string for a component based on its definition.
///
/// Required attributes are included in order; optional ones are omitted.
/// If the component has a body (non-self-closing), the snippet includes
/// a body placeholder and a closing tag.
fn build_component_snippet(name: &str, def: &supersigil_core::ComponentDef) -> String {
    // Collect required attributes sorted for determinism.
    let mut required: Vec<&str> = def
        .attributes
        .iter()
        .filter(|(_, attr)| attr.required)
        .map(|(k, _)| k.as_str())
        .collect();
    required.sort_unstable();

    // Build attribute snippet fragments: `attr="$N"`.
    let mut tab_stop = 1u32;
    let attr_snippets: Vec<String> = required
        .iter()
        .map(|attr| {
            let s = format!("{attr}=\"${tab_stop}\"");
            tab_stop += 1;
            s
        })
        .collect();

    // Determine whether the component has a body (any children-accepting use).
    // Heuristic: components that are referenceable or have no `refs` attribute
    // and have description mentioning body content tend to have bodies.
    // Simpler: use `referenceable` as proxy — referenceable components typically
    // have meaningful body content.
    let has_body = def.referenceable;

    if has_body {
        let body_stop = tab_stop;
        if attr_snippets.is_empty() {
            format!("{name}>\n${body_stop}\n</{name}>")
        } else {
            let attrs = attr_snippets.join(" ");
            format!("{name} {attrs}>\n${body_stop}\n</{name}>")
        }
    } else if attr_snippets.is_empty() {
        format!("{name} />")
    } else {
        let attrs = attr_snippets.join(" ");
        format!("{name} {attrs} />")
    }
}

/// Return completion items for known attribute values.
///
/// - `"strategy"`: offers `"tag"` and `"file-glob"`.
/// - `"status"`: offers all known task/document status values.
#[must_use]
pub fn complete_attribute_values(attr: &str, prefix: &str) -> Vec<CompletionItem> {
    let candidates: &[&str] = match attr {
        "strategy" => &["tag", "file-glob"],
        "status" => &[
            "draft",
            "approved",
            "implemented",
            "done",
            "ready",
            "in-progress",
            "rejected",
            "considered",
            "blocked",
            "cancelled",
        ],
        _ => &[],
    };

    let mut items: Vec<CompletionItem> = candidates
        .iter()
        .filter(|v| v.starts_with(prefix))
        .map(|v| CompletionItem {
            label: (*v).to_owned(),
            kind: Some(CompletionItemKind::ENUM_MEMBER),
            detail: Some("Supersigil".to_owned()),
            ..CompletionItem::default()
        })
        .collect();

    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

// ---------------------------------------------------------------------------
// Top-level dispatch
// ---------------------------------------------------------------------------

/// Compute completion items for the given cursor position.
///
/// Detects context and dispatches to the appropriate provider.
/// Returns an empty vec if no completions are available.
#[must_use]
pub fn complete(
    content: &str,
    line: u32,
    character: u32,
    graph: &DocumentGraph,
    defs: &ComponentDefs,
) -> Vec<CompletionItem> {
    match detect_context(content, line, character) {
        CompletionContext::RefDocId { prefix } => complete_document_ids(&prefix, graph),
        CompletionContext::RefFragment { doc_id, prefix } => {
            complete_fragment_ids(&doc_id, &prefix, graph)
        }
        CompletionContext::ComponentName { prefix } => complete_component_names(&prefix, defs),
        CompletionContext::AttributeStrategy { prefix } => {
            complete_attribute_values("strategy", &prefix)
        }
        CompletionContext::AttributeStatus { prefix } => {
            complete_attribute_values("status", &prefix)
        }
        CompletionContext::None => vec![],
    }
}
