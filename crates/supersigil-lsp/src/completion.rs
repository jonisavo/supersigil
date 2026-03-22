//! Autocomplete support for supersigil MDX documents.
//!
//! Implements:
//! - req-3-1: Document ID completions inside ref-accepting attributes before `#`.
//! - req-3-2: Fragment ID completions inside ref-accepting attributes after `#`.
//! - req-3-3: Component name completions with snippet after `<`.
//! - req-3-4: Context-sensitive attribute value completions for `strategy` and
//!   `status`, scoped to the enclosing component or frontmatter.

use lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};
use supersigil_core::{ComponentDefs, Config, DocumentGraph};

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
    /// Inside a `strategy="..."` attribute value with enclosing component.
    AttributeStrategy {
        prefix: String,
        component: Option<String>,
    },
    /// Inside a `status="..."` attribute value with enclosing context.
    AttributeStatus {
        prefix: String,
        context: StatusContext,
    },
    /// No recognized completion context.
    None,
}

/// The enclosing context for a `status` attribute, determining which values
/// are valid.
#[derive(Debug, PartialEq)]
pub enum StatusContext {
    /// Inside YAML frontmatter — valid statuses come from the document type def.
    Frontmatter,
    /// Inside a `<Task>` component.
    Task,
    /// Inside an `<Alternative>` component.
    Alternative,
    /// Inside an `<Expected>` component (free-form exit code).
    Expected,
    /// Inside an unknown or other component.
    Other(String),
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

    // Determine if we're in frontmatter (before closing `---`)
    let in_frontmatter = is_in_frontmatter(content, line as usize);

    // --- Check for strategy="..." ---
    let strategy_needle = "strategy=\"";
    if let Some(attr_start) = before_cursor.rfind(strategy_needle) {
        let value_start = attr_start + strategy_needle.len();
        let value_so_far = &before_cursor[value_start..];
        if !value_so_far.contains('"') {
            let component = if in_frontmatter {
                None
            } else {
                find_enclosing_component(content, line as usize)
            };
            return CompletionContext::AttributeStrategy {
                prefix: value_so_far.to_owned(),
                component,
            };
        }
    }

    // --- Check for status="..." ---
    let status_needle = "status=\"";
    if let Some(attr_start) = before_cursor.rfind(status_needle) {
        let value_start = attr_start + status_needle.len();
        let value_so_far = &before_cursor[value_start..];
        if !value_so_far.contains('"') {
            let status_ctx = if in_frontmatter {
                StatusContext::Frontmatter
            } else {
                match find_enclosing_component(content, line as usize) {
                    Some(ref name) if name == "Task" => StatusContext::Task,
                    Some(ref name) if name == "Alternative" => StatusContext::Alternative,
                    Some(ref name) if name == "Expected" => StatusContext::Expected,
                    Some(name) => StatusContext::Other(name),
                    None => StatusContext::Other(String::new()),
                }
            };
            return CompletionContext::AttributeStatus {
                prefix: value_so_far.to_owned(),
                context: status_ctx,
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

/// Check whether the given line is inside the YAML frontmatter block.
///
/// Frontmatter is the region between the opening `---` (line 0) and the
/// closing `---`.
fn is_in_frontmatter(content: &str, line: usize) -> bool {
    let mut in_fm = false;
    for (i, l) in content.lines().enumerate() {
        let trimmed = l.trim();
        if i == 0 {
            if trimmed == "---" {
                in_fm = true;
            } else {
                return false;
            }
        } else if in_fm && trimmed == "---" {
            // Found closing delimiter — anything at or after this line is body.
            return line < i;
        }
        if i >= line && !in_fm {
            return false;
        }
    }
    false
}

/// Scan backward from `line` to find the nearest unmatched `<ComponentName`.
///
/// Returns the component name if found, or `None` if the cursor is not
/// inside any component tag.
fn find_enclosing_component(content: &str, line: usize) -> Option<String> {
    // Scan backward from the cursor line through previous lines.
    let lines: Vec<&str> = content.lines().take(line + 1).collect();
    for l in lines.into_iter().rev() {
        let trimmed = l.trim();
        // Look for opening component tags: `<Name` followed by space or `>`
        if let Some(rest) = trimmed.strip_prefix('<') {
            // Skip closing tags, comments, and lowercase HTML elements.
            if rest.starts_with('/')
                || rest.starts_with('!')
                || rest.starts_with(|c: char| c.is_ascii_lowercase())
            {
                continue;
            }
            // Extract the component name (up to first space, '>', or '/')
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
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

/// Task lifecycle statuses.
const TASK_STATUSES: &[&str] = &["draft", "ready", "in-progress", "done"];

/// Recognized Alternative statuses.
const ALTERNATIVE_STATUSES: &[&str] = &["rejected", "deferred", "superseded"];

/// Return context-sensitive completion items for `status` attribute values.
#[must_use]
pub fn complete_status(
    prefix: &str,
    context: &StatusContext,
    config: Option<&Config>,
    doc_type: Option<&str>,
) -> Vec<CompletionItem> {
    let candidates: Vec<&str> = match context {
        StatusContext::Frontmatter => {
            // Look up valid statuses from the document type definition.
            if let Some(config) = config
                && let Some(dt) = doc_type
                && let Some(type_def) = config.documents.types.get(dt)
                && !type_def.status.is_empty()
            {
                type_def.status.iter().map(String::as_str).collect()
            } else {
                // Fallback: no config or unknown doc type — offer nothing.
                vec![]
            }
        }
        StatusContext::Task => TASK_STATUSES.to_vec(),
        StatusContext::Alternative => ALTERNATIVE_STATUSES.to_vec(),
        StatusContext::Expected | StatusContext::Other(_) => vec![],
    };

    filter_to_completion_items(&candidates, prefix)
}

/// Return context-sensitive completion items for `strategy` attribute values.
#[must_use]
pub fn complete_strategy(prefix: &str, component: Option<&str>) -> Vec<CompletionItem> {
    let candidates: &[&str] = match component {
        Some("VerifiedBy") => &["tag", "file-glob"],
        _ => &[],
    };

    filter_to_completion_items(candidates, prefix)
}

/// Filter candidate strings by prefix and convert to `CompletionItem`s.
fn filter_to_completion_items(candidates: &[&str], prefix: &str) -> Vec<CompletionItem> {
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
    config: Option<&Config>,
    doc_type: Option<&str>,
) -> Vec<CompletionItem> {
    match detect_context(content, line, character) {
        CompletionContext::RefDocId { prefix } => complete_document_ids(&prefix, graph),
        CompletionContext::RefFragment { doc_id, prefix } => {
            complete_fragment_ids(&doc_id, &prefix, graph)
        }
        CompletionContext::ComponentName { prefix } => complete_component_names(&prefix, defs),
        CompletionContext::AttributeStrategy { prefix, component } => {
            complete_strategy(&prefix, component.as_deref())
        }
        CompletionContext::AttributeStatus { prefix, context } => {
            complete_status(&prefix, &context, config, doc_type)
        }
        CompletionContext::None => vec![],
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- is_in_frontmatter --

    #[test]
    fn frontmatter_line_inside() {
        let content = "---\ntitle: Hello\nstatus: draft\n---\nbody";
        assert!(is_in_frontmatter(content, 1));
        assert!(is_in_frontmatter(content, 2));
    }

    #[test]
    fn frontmatter_line_outside() {
        let content = "---\ntitle: Hello\n---\nbody";
        assert!(!is_in_frontmatter(content, 3));
    }

    #[test]
    fn frontmatter_on_closing_delimiter() {
        let content = "---\ntitle: Hello\n---\nbody";
        // Line 2 is the closing `---` — cursor on it is NOT inside.
        assert!(!is_in_frontmatter(content, 2));
    }

    #[test]
    fn no_frontmatter() {
        let content = "just body text";
        assert!(!is_in_frontmatter(content, 0));
    }

    // -- find_enclosing_component --

    #[test]
    fn finds_task_component() {
        let content = "<Task\n  id=\"task-1\"\n  status=\"";
        assert_eq!(
            find_enclosing_component(content, 2),
            Some("Task".to_owned())
        );
    }

    #[test]
    fn finds_alternative_component() {
        let content = "<Decision id=\"d1\">\n<Alternative id=\"a1\" status=\"";
        assert_eq!(
            find_enclosing_component(content, 1),
            Some("Alternative".to_owned())
        );
    }

    #[test]
    fn skips_closing_tags() {
        let content = "</Task>\n<Alternative id=\"a1\" status=\"";
        assert_eq!(
            find_enclosing_component(content, 1),
            Some("Alternative".to_owned())
        );
    }

    #[test]
    fn skips_lowercase_html() {
        let content = "<div>\n<Task status=\"";
        assert_eq!(
            find_enclosing_component(content, 1),
            Some("Task".to_owned())
        );
    }

    // -- detect_context with status --

    #[test]
    fn status_in_frontmatter() {
        let content = "---\nsupersigil:\n  id: test\n  status: \n---";
        // This doesn't match because frontmatter uses `: ` not `="`.
        // Status in frontmatter is YAML, not JSX attributes.
        // The `status="` pattern only matches JSX component attributes.
        let ctx = detect_context(content, 3, 10);
        assert_eq!(ctx, CompletionContext::None);
    }

    #[test]
    fn status_on_task_component() {
        let content = "<Task\n  id=\"task-1\"\n  status=\"dr";
        let ctx = detect_context(content, 2, 13);
        assert_eq!(
            ctx,
            CompletionContext::AttributeStatus {
                prefix: "dr".to_owned(),
                context: StatusContext::Task,
            }
        );
    }

    #[test]
    fn status_on_alternative() {
        let content = "<Decision id=\"d1\">\n  <Alternative id=\"a1\" status=\"rej";
        let ctx = detect_context(content, 1, 51);
        assert_eq!(
            ctx,
            CompletionContext::AttributeStatus {
                prefix: "rej".to_owned(),
                context: StatusContext::Alternative,
            }
        );
    }

    #[test]
    fn strategy_on_verified_by() {
        let content = "<VerifiedBy strategy=\"ta";
        #[expect(clippy::cast_possible_truncation, reason = "test string is short")]
        let ctx = detect_context(content, 0, content.len() as u32);
        assert_eq!(
            ctx,
            CompletionContext::AttributeStrategy {
                prefix: "ta".to_owned(),
                component: Some("VerifiedBy".to_owned()),
            }
        );
    }

    // -- complete_status --

    #[test]
    fn task_status_completions() {
        let items = complete_status("", &StatusContext::Task, None, None);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, vec!["done", "draft", "in-progress", "ready"]);
    }

    #[test]
    fn alternative_status_completions() {
        let items = complete_status("", &StatusContext::Alternative, None, None);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, vec!["deferred", "rejected", "superseded"]);
    }

    #[test]
    fn expected_status_no_completions() {
        let items = complete_status("", &StatusContext::Expected, None, None);
        assert!(items.is_empty());
    }

    #[test]
    fn frontmatter_status_from_config() {
        let mut config = Config::default();
        config.documents.types.insert(
            "requirements".into(),
            supersigil_core::DocumentTypeDef {
                status: vec![
                    "draft".into(),
                    "review".into(),
                    "approved".into(),
                    "implemented".into(),
                ],
                required_components: vec![],
                description: None,
            },
        );
        let items = complete_status(
            "",
            &StatusContext::Frontmatter,
            Some(&config),
            Some("requirements"),
        );
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, vec!["approved", "draft", "implemented", "review"]);
    }

    #[test]
    fn frontmatter_status_unknown_doc_type_no_completions() {
        let items = complete_status(
            "",
            &StatusContext::Frontmatter,
            Some(&Config::default()),
            Some("unknown"),
        );
        assert!(items.is_empty());
    }

    // -- complete_strategy --

    #[test]
    fn strategy_on_verified_by_completions() {
        let items = complete_strategy("", Some("VerifiedBy"));
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, vec!["file-glob", "tag"]);
    }

    #[test]
    fn strategy_on_other_component_no_completions() {
        let items = complete_strategy("", Some("Task"));
        assert!(items.is_empty());
    }

    #[test]
    fn strategy_prefix_filter() {
        let items = complete_strategy("t", Some("VerifiedBy"));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "tag");
    }
}
