//! Stage 3: Component extraction from MDX AST and lint-time validation.

use std::collections::HashMap;
use std::path::Path;

use markdown::mdast;
use supersigil_core::{ComponentDefs, ExtractedComponent, ParseError, SourcePosition};

/// Returns `true` if the name starts with an uppercase ASCII letter (`PascalCase`).
fn is_pascal_case(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
}

/// Collect body text from the direct children of an MDX JSX flow element.
///
/// Concatenates all `Text` node values found among the children (recursing
/// into non-component wrapper nodes like paragraphs), ignoring child
/// components. Returns `None` if no text was found or the result is empty
/// after trimming.
fn collect_body_text(children: &[mdast::Node]) -> Option<String> {
    let mut buf = String::new();
    collect_text_recursive(&mut buf, children);
    let trimmed = buf.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// Recursively collect text values, skipping `MdxJsxFlowElement` children
/// (those become child components, not body text).
fn collect_text_recursive(buf: &mut String, nodes: &[mdast::Node]) {
    for node in nodes {
        match node {
            mdast::Node::Text(t) => buf.push_str(&t.value),
            mdast::Node::InlineCode(c) => {
                buf.push('`');
                buf.push_str(&c.value);
                buf.push('`');
            }
            // Skip flow-level JSX elements — they are child components.
            mdast::Node::MdxJsxFlowElement(_) => {}
            // Recurse into wrapper nodes (paragraphs, etc.) to find text.
            other => {
                if let Some(children) = other.children() {
                    collect_text_recursive(buf, children);
                }
            }
        }
    }
}

/// Extract attributes from an MDX JSX element, recording errors for
/// expression attributes.
fn extract_attributes(
    attrs: &[mdast::AttributeContent],
    component_name: &str,
    path: &Path,
    position: &SourcePosition,
    errors: &mut Vec<ParseError>,
) -> HashMap<String, String> {
    let mut map = HashMap::with_capacity(attrs.len());
    for attr in attrs {
        match attr {
            mdast::AttributeContent::Property(prop) => {
                if let Some(ref value) = prop.value {
                    match value {
                        mdast::AttributeValue::Literal(s) => {
                            map.insert(prop.name.clone(), s.clone());
                        }
                        mdast::AttributeValue::Expression(_) => {
                            errors.push(ParseError::ExpressionAttribute {
                                path: path.to_path_buf(),
                                component: component_name.to_owned(),
                                attribute: prop.name.clone(),
                                position: *position,
                            });
                        }
                    }
                }
                // Boolean attribute (no value) — skip, not relevant for supersigil.
            }
            mdast::AttributeContent::Expression(_) => {
                // Spread expression like {...obj} — record error with empty attribute name.
                errors.push(ParseError::ExpressionAttribute {
                    path: path.to_path_buf(),
                    component: component_name.to_owned(),
                    attribute: String::new(),
                    position: *position,
                });
            }
        }
    }
    map
}

/// Walk the AST and extract `PascalCase` `MdxJsxFlowElement` nodes as
/// [`ExtractedComponent`] values.
///
/// Only block-level flow elements with `PascalCase` names are extracted.
/// Lowercase HTML elements and inline `MdxJsxTextElement` nodes are ignored.
///
/// `body_offset` is the byte length of the front matter block (including
/// delimiters and trailing newline) so that positions from `markdown-rs`
/// (relative to the MDX body) are adjusted to be file-relative.
///
/// Errors (e.g., expression attributes) are appended to `errors`.
pub fn extract_components(
    node: &mdast::Node,
    body_offset: usize,
    path: &Path,
    errors: &mut Vec<ParseError>,
) -> Vec<ExtractedComponent> {
    let mut components = Vec::new();
    collect_components(node, body_offset, path, errors, &mut components);
    components
}

/// Recursively collect components from `children` into `out`, avoiding
/// intermediate `Vec` allocations.
fn collect_from_children(
    children: &[mdast::Node],
    body_offset: usize,
    path: &Path,
    errors: &mut Vec<ParseError>,
    out: &mut Vec<ExtractedComponent>,
) {
    for child in children {
        collect_components(child, body_offset, path, errors, out);
    }
}

fn collect_components(
    node: &mdast::Node,
    body_offset: usize,
    path: &Path,
    errors: &mut Vec<ParseError>,
    out: &mut Vec<ExtractedComponent>,
) {
    match node {
        mdast::Node::MdxJsxFlowElement(el) => {
            let Some(ref name) = el.name else {
                // Fragment (<> </>) — recurse into children.
                collect_from_children(&el.children, body_offset, path, errors, out);
                return;
            };

            if !is_pascal_case(name) {
                // Lowercase HTML element — silently ignore, but still recurse
                // in case there are supersigil components nested inside.
                collect_from_children(&el.children, body_offset, path, errors, out);
                return;
            }

            // Compute file-relative source position.
            let position = el.position.as_ref().map_or(
                SourcePosition {
                    byte_offset: body_offset,
                    line: 1,
                    column: 1,
                },
                |pos| SourcePosition {
                    byte_offset: pos.start.offset + body_offset,
                    line: pos.start.line,
                    column: pos.start.column,
                },
            );

            let attributes = extract_attributes(&el.attributes, name, path, &position, errors);

            // Recursively extract child components.
            let mut children = Vec::new();
            collect_from_children(&el.children, body_offset, path, errors, &mut children);

            let body_text = collect_body_text(&el.children);

            out.push(ExtractedComponent {
                name: name.clone(),
                attributes,
                children,
                body_text,
                position,
            });
        }
        // Ignore inline JSX (MdxJsxTextElement) entirely.
        mdast::Node::MdxJsxTextElement(_) => {}
        // For all other nodes, recurse into children.
        other => {
            if let Some(node_children) = other.children() {
                collect_from_children(node_children, body_offset, path, errors, out);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Lint-time validation (Req 21, 25)
// ---------------------------------------------------------------------------

/// Validate extracted components against the known component definitions.
///
/// Checks:
/// 1. Unknown `PascalCase` component names → `UnknownComponent` error.
/// 2. Missing required attributes → `MissingRequiredAttribute` error.
///
/// Lowercase element names are silently ignored (they are standard HTML).
/// Recurses into children.
pub fn validate_components(
    components: &[ExtractedComponent],
    component_defs: &ComponentDefs,
    path: &Path,
    errors: &mut Vec<ParseError>,
) {
    for comp in components {
        // Skip lowercase names (standard HTML elements). When called via
        // extract_components this is always true, but validate_components
        // is a public API so we guard defensively.
        if !is_pascal_case(&comp.name) {
            continue;
        }

        if let Some(def) = component_defs.get(&comp.name) {
            // Check required attributes
            for (attr_name, attr_def) in &def.attributes {
                if attr_def.required && !comp.attributes.contains_key(attr_name) {
                    errors.push(ParseError::MissingRequiredAttribute {
                        path: path.to_path_buf(),
                        component: comp.name.clone(),
                        attribute: attr_name.clone(),
                        position: comp.position,
                    });
                }
            }
        } else {
            errors.push(ParseError::UnknownComponent {
                path: path.to_path_buf(),
                component: comp.name.clone(),
                position: comp.position,
            });
        }

        // Recurse into children
        validate_components(&comp.children, component_defs, path, errors);
    }
}
