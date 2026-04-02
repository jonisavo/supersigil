//! Component extraction from parsed XML nodes.
//!
//! Transforms `XmlNode` values (produced by [`crate::xml_parser`]) into
//! [`ExtractedComponent`] values using the same `ComponentDefs`-based
//! filtering as the previous extraction pipeline.
//!
//! **Key behaviors:**
//! - Only known `PascalCase` elements (those in `ComponentDefs`) become components.
//! - Unknown `PascalCase` elements are transparent wrappers — their children are
//!   still traversed.
//! - Lowercase elements are ignored (but their children are traversed).
//! - Attributes are stored as `HashMap<String, String>` (raw strings).
//! - `body_text` is computed from direct `Text` children, trimmed, `None` if empty.
//! - `code_blocks` is always empty (code blocks come from `supersigil-ref` fences).
//! - Nested child components are collected recursively.

use std::collections::HashMap;

use supersigil_core::{ComponentDefs, ExtractedComponent, SourcePosition};

use crate::util::{is_pascal_case, line_col};
use crate::xml_parser::XmlNode;

/// Collect body text from the direct children of an XML element.
///
/// Concatenates `Text` node values, recursing into non-component wrapper
/// elements (unknown `PascalCase` and lowercase). Known component children
/// are excluded from the body text.
/// Returns `(text, start_offset, end_offset)` where `text` is `None` if no text
/// was found or the result is empty after trimming, `start_offset` is the byte
/// offset of the first contributing text node, and `end_offset` is the raw source
/// byte offset of the end of the last contributing text node.
fn collect_body_text(
    children: &[XmlNode],
    defs: &ComponentDefs,
) -> (Option<String>, Option<usize>, Option<usize>) {
    let mut buf = String::new();
    let mut first_offset: Option<usize> = None;
    let mut last_end_offset: Option<usize> = None;
    collect_text_recursive(
        &mut buf,
        &mut first_offset,
        &mut last_end_offset,
        children,
        defs,
    );
    let trimmed = buf.trim();
    if trimmed.is_empty() {
        (None, None, None)
    } else {
        // Adjust offset to account for leading whitespace trimmed from the text.
        let leading_ws = buf.len() - buf.trim_start().len();
        let offset = first_offset.map(|o| o + leading_ws);
        // Adjust end offset to account for trailing whitespace trimmed from the text.
        let trailing_ws = buf.len() - buf.trim_end().len();
        let end_offset = last_end_offset.map(|o| o - trailing_ws);
        (Some(trimmed.to_owned()), offset, end_offset)
    }
}

/// Recursively collect text values, skipping known component nodes.
fn collect_text_recursive(
    buf: &mut String,
    first_offset: &mut Option<usize>,
    last_end_offset: &mut Option<usize>,
    nodes: &[XmlNode],
    defs: &ComponentDefs,
) {
    for node in nodes {
        match node {
            XmlNode::Text {
                content,
                offset,
                end_offset,
            } => {
                if first_offset.is_none() {
                    *first_offset = Some(*offset);
                }
                *last_end_offset = Some(*end_offset);
                buf.push_str(content);
            }
            XmlNode::Element { name, children, .. } => {
                // Known components are child components — their text is excluded.
                if defs.is_known(name) {
                    continue;
                }
                // Unknown PascalCase or lowercase elements are transparent
                // wrappers — recurse into their children.
                collect_text_recursive(buf, first_offset, last_end_offset, children, defs);
            }
        }
    }
}

/// Convert a `Vec<(String, String)>` attribute list into a `HashMap`.
fn attributes_to_map(attrs: &[(String, String)]) -> HashMap<String, String> {
    attrs.iter().cloned().collect()
}

// ---------------------------------------------------------------------------
// Extraction context
// ---------------------------------------------------------------------------

/// Shared context threaded through the recursive extraction pipeline.
struct ExtractionCtx<'a> {
    /// The full normalized file content, for line/column computation.
    content: &'a str,
    defs: &'a ComponentDefs,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Walk parsed XML nodes and extract known components as [`ExtractedComponent`]
/// values.
///
/// `nodes` are the top-level `XmlNode` values from [`crate::parse_supersigil_xml`].
/// `content` is the full normalized file content (for line/column computation).
/// `component_defs` defines which `PascalCase` element names are known components.
#[must_use]
pub fn extract_components_from_xml(
    nodes: &[XmlNode],
    content: &str,
    component_defs: &ComponentDefs,
) -> Vec<ExtractedComponent> {
    let ctx = ExtractionCtx {
        content,
        defs: component_defs,
    };
    let mut components = Vec::new();
    collect_from_nodes(nodes, &ctx, &mut components);
    components
}

// ---------------------------------------------------------------------------
// Recursive helpers
// ---------------------------------------------------------------------------

/// Process a list of XML nodes, collecting known components into `out`.
fn collect_from_nodes(
    nodes: &[XmlNode],
    ctx: &ExtractionCtx<'_>,
    out: &mut Vec<ExtractedComponent>,
) {
    for node in nodes {
        collect_component(node, ctx, out);
    }
}

/// Process a single XML node. If it's a known component element, extract it;
/// otherwise recurse into children looking for nested components.
fn collect_component(node: &XmlNode, ctx: &ExtractionCtx<'_>, out: &mut Vec<ExtractedComponent>) {
    match node {
        XmlNode::Text { .. } => {}

        XmlNode::Element {
            name,
            attributes,
            children,
            offset,
            end_offset,
        } => {
            if !is_pascal_case(name) {
                collect_from_nodes(children, ctx, out);
                return;
            }

            if !ctx.defs.is_known(name) {
                collect_from_nodes(children, ctx, out);
                return;
            }

            // Known component — extract it.
            let (line, column) = line_col(ctx.content, *offset);
            let position = SourcePosition {
                byte_offset: *offset,
                line,
                column,
            };

            let (end_line, end_column) = line_col(ctx.content, *end_offset);
            let end_position = SourcePosition {
                byte_offset: *end_offset,
                line: end_line,
                column: end_column,
            };

            let attrs = attributes_to_map(attributes);

            let mut child_components = Vec::new();
            collect_from_nodes(children, ctx, &mut child_components);

            let (body_text, body_text_offset, body_text_end_offset) =
                collect_body_text(children, ctx.defs);

            out.push(ExtractedComponent {
                name: name.clone(),
                attributes: attrs,
                children: child_components,
                body_text,
                body_text_offset,
                body_text_end_offset,
                code_blocks: Vec::new(),
                position,
                end_position,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
