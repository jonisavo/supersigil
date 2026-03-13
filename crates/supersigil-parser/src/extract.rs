//! Stage 3: Component extraction from MDX AST and lint-time validation.

use std::collections::HashMap;
use std::path::Path;

use markdown::{mdast, unist};
use supersigil_core::{
    CodeBlock, ComponentDefs, EXAMPLE, EXPECTED, ExtractedComponent, ParseError, SourcePosition,
};

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

/// Recursively collect text values, skipping child component nodes.
///
/// Flow-level JSX elements are always child components. Inline JSX elements
/// (`MdxJsxTextElement`) are child components only when `PascalCase`; lowercase
/// inline elements (`<span>`, `<strong>`, etc.) are HTML formatting wrappers
/// whose text content belongs to the parent's body.
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
            // Inline JSX: skip PascalCase (child components), recurse into
            // lowercase (HTML formatting wrappers).
            mdast::Node::MdxJsxTextElement(el) => {
                let is_component = el.name.as_ref().is_some_and(|n| is_pascal_case(n));
                if !is_component {
                    collect_text_recursive(buf, &el.children);
                }
            }
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

/// Extract fenced code blocks from the direct children of an MDX component.
///
/// Only `Example` and `Expected` components have code blocks extracted; all
/// other components return an empty vector (no allocation).
///
/// Each `Code` node in the AST becomes a [`CodeBlock`] with its language tag,
/// content, and a byte offset pointing to the start of the opening fence line
/// in the normalized source file. The snapshot rewrite code handles precise
/// content slicing from this offset.
fn extract_code_blocks(
    children: &[mdast::Node],
    body_offset: usize,
    component_name: &str,
) -> Vec<CodeBlock> {
    if component_name != EXAMPLE && component_name != EXPECTED {
        return Vec::new();
    }

    let mut blocks = Vec::new();
    for child in children {
        if let mdast::Node::Code(code) = child {
            // content_offset points to the start of the opening fence line.
            // The snapshot rewrite code (Task 10) handles slicing from here
            // to locate the actual content within the fence.
            let content_offset = code
                .position
                .as_ref()
                .map_or(0, |pos| body_offset + pos.start.offset);

            blocks.push(CodeBlock {
                lang: code.lang.clone(),
                content: code.value.clone(),
                content_offset,
            });
        }
    }
    blocks
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
            process_jsx_element(
                el.name.as_deref(),
                &el.children,
                &el.attributes,
                el.position.as_ref(),
                body_offset,
                path,
                errors,
                out,
            );
        }
        // Inline JSX (MdxJsxTextElement) — extract PascalCase components.
        // This handles cases like <Criterion id="ac-1">text</Criterion>
        // appearing on a single line inside a parent component, which MDX
        // classifies as text-level rather than flow-level.
        mdast::Node::MdxJsxTextElement(el) => {
            process_jsx_element(
                el.name.as_deref(),
                &el.children,
                &el.attributes,
                el.position.as_ref(),
                body_offset,
                path,
                errors,
                out,
            );
        }
        // For all other nodes, recurse into children.
        other => {
            if let Some(node_children) = other.children() {
                collect_from_children(node_children, body_offset, path, errors, out);
            }
        }
    }
}

/// Shared logic for processing both `MdxJsxFlowElement` and `MdxJsxTextElement`.
#[expect(
    clippy::too_many_arguments,
    reason = "factored out of two match arms to eliminate duplication"
)]
fn process_jsx_element(
    name: Option<&str>,
    children: &[mdast::Node],
    attributes: &[mdast::AttributeContent],
    position: Option<&unist::Position>,
    body_offset: usize,
    path: &Path,
    errors: &mut Vec<ParseError>,
    out: &mut Vec<ExtractedComponent>,
) {
    let Some(name) = name else {
        // Fragment (<> </>) — recurse into children.
        collect_from_children(children, body_offset, path, errors, out);
        return;
    };

    if !is_pascal_case(name) {
        // Lowercase HTML element — recurse in case there are
        // supersigil components nested inside.
        collect_from_children(children, body_offset, path, errors, out);
        return;
    }

    let pos = position.map_or(
        SourcePosition {
            byte_offset: body_offset,
            line: 1,
            column: 1,
        },
        |p| SourcePosition {
            byte_offset: p.start.offset + body_offset,
            line: p.start.line,
            column: p.start.column,
        },
    );

    let attrs = extract_attributes(attributes, name, path, &pos, errors);

    let mut child_components = Vec::new();
    collect_from_children(children, body_offset, path, errors, &mut child_components);

    let body_text = collect_body_text(children);
    let code_blocks = extract_code_blocks(children, body_offset, name);

    out.push(ExtractedComponent {
        name: name.to_owned(),
        attributes: attrs,
        children: child_components,
        body_text,
        code_blocks,
        position: pos,
    });
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
