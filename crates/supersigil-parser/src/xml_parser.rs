//! XML subset parser for `supersigil-xml` fence content.
//!
//! Parses a strict XML subset supporting:
//! - `PascalCase` elements with double-quoted string attributes
//! - Self-closing elements (`<Foo />`)
//! - Nested elements
//! - Text content between elements
//! - Entity references: `&amp;`, `&lt;`, `&gt;`, `&quot;`
//!
//! Rejects processing instructions, CDATA, DTD, comments, namespaces, and
//! unsupported entity references.
//!
//! Implemented on top of `quick-xml` for correctness and robustness.

use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;
use supersigil_core::ParseError;

use crate::util::line_col;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A parsed XML node from a `supersigil-xml` fence.
#[derive(Debug, Clone, PartialEq)]
pub enum XmlNode {
    /// An XML element with name, attributes, children, and source offset.
    Element {
        /// Element tag name (e.g. `Criterion`).
        name: String,
        /// Ordered list of `(key, value)` attribute pairs.
        attributes: Vec<(String, String)>,
        /// Child nodes (elements and/or text).
        children: Vec<XmlNode>,
        /// Byte offset of the opening `<` relative to the source file.
        offset: usize,
    },
    /// Raw text content (entity references already resolved).
    Text {
        content: String,
        offset: usize,
        /// Byte offset of the end of the raw source text (past the last
        /// byte of the original text/entity-ref run). This can differ
        /// from `offset + content.len()` when entity references are present
        /// because decoded text is shorter than the raw source.
        end_offset: usize,
    },
}

// ---------------------------------------------------------------------------
// Synthetic root tag
// ---------------------------------------------------------------------------

/// Tag used to wrap multiple top-level elements so that quick-xml sees
/// well-formed XML.  This name intentionally cannot be a valid `PascalCase`
/// supersigil element.
const SYNTHETIC_ROOT: &str = "__root";
const SYNTHETIC_OPEN: &str = "<__root>";
const SYNTHETIC_CLOSE: &str = "</__root>";

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse the content of a `supersigil-xml` fence into structured XML nodes.
///
/// `content` is the raw text between the fence delimiters.
/// `fence_offset` is the byte offset of the fence content start within the
/// source file, used to produce file-absolute offsets.
/// `path` is the file path for error messages.
///
/// # Errors
///
/// Returns [`ParseError::XmlSyntaxError`] for any syntax or validation error.
/// Line and column numbers in errors are **content-relative** (i.e. relative
/// to the start of the fence content, not the file). The caller must adjust
/// for the fence's position within the file if file-absolute positions are
/// needed.
pub fn parse_supersigil_xml(
    content: &str,
    fence_offset: usize,
    path: &Path,
) -> Result<Vec<XmlNode>, ParseError> {
    // Reject unsupported constructs that quick-xml would silently handle.
    reject_unsupported(content, path)?;

    // Wrap in a synthetic root so quick-xml can handle multiple top-level
    // elements (which is not valid XML on its own).
    let wrapped = format!("{SYNTHETIC_OPEN}{content}{SYNTHETIC_CLOSE}");

    let mut reader = Reader::from_str(&wrapped);
    reader.config_mut().trim_text(false);

    // The synthetic root open tag shifts all byte positions by its length.
    // quick-xml uses u64 for buffer positions.
    #[allow(clippy::cast_possible_truncation, reason = "SYNTHETIC_OPEN is 8 bytes")]
    let root_tag_len: u64 = SYNTHETIC_OPEN.len() as u64;

    // Skip the synthetic root's opening event.
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) if e.name().as_ref() == SYNTHETIC_ROOT.as_bytes() => break,
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(quick_xml_error_to_parse_error(&e, path));
            }
            _ => {}
        }
    }

    // Parse children of the synthetic root (= top-level nodes).
    let nodes = parse_children(
        &mut reader,
        SYNTHETIC_ROOT,
        content,
        fence_offset,
        root_tag_len,
        path,
    )?;

    Ok(nodes)
}

// ---------------------------------------------------------------------------
// Pre-scan: reject unsupported constructs before quick-xml parsing
// ---------------------------------------------------------------------------

/// Scan the raw content for constructs that our XML subset forbids.
///
/// We do this before handing the input to quick-xml because quick-xml would
/// either silently handle these (comments, CDATA) or produce cryptic errors.
fn reject_unsupported(content: &str, path: &Path) -> Result<(), ParseError> {
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            if bytes[i..].starts_with(b"<?") {
                return Err(make_error(
                    content,
                    i,
                    path,
                    "processing instructions (`<?...?>`) are not supported",
                ));
            }
            if bytes[i..].starts_with(b"<![CDATA[") {
                return Err(make_error(
                    content,
                    i,
                    path,
                    "CDATA sections (`<![CDATA[...]]>`) are not supported",
                ));
            }
            if bytes[i..].starts_with(b"<!DOCTYPE") || bytes[i..].starts_with(b"<!doctype") {
                return Err(make_error(
                    content,
                    i,
                    path,
                    "DTD declarations (`<!DOCTYPE ...>`) are not supported",
                ));
            }
            if bytes[i..].starts_with(b"<!--") {
                return Err(make_error(
                    content,
                    i,
                    path,
                    "XML comments (`<!-- ... -->`) are not supported",
                ));
            }
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Recursive event-driven parser
// ---------------------------------------------------------------------------

/// Parse child nodes until the closing tag for `parent_name` is encountered.
///
/// Text and entity-reference events are accumulated into a single `XmlNode::Text`
/// because quick-xml emits `Text` / `GeneralRef` / `Text` sequences for content
/// like `a &lt; b`.
#[allow(
    clippy::too_many_lines,
    reason = "event-loop structure is clearest as a single function"
)]
fn parse_children(
    reader: &mut Reader<&[u8]>,
    parent_name: &str,
    content: &str,
    fence_offset: usize,
    root_tag_len: u64,
    path: &Path,
) -> Result<Vec<XmlNode>, ParseError> {
    /// Flush accumulated text into the node list.
    fn flush_text(
        text_buf: &mut String,
        text_start: &mut Option<usize>,
        text_end: &mut Option<usize>,
        nodes: &mut Vec<XmlNode>,
        is_top_level: bool,
    ) {
        if text_buf.is_empty() {
            return;
        }
        // At top level, skip whitespace-only text nodes.
        if is_top_level && text_buf.trim().is_empty() {
            text_buf.clear();
            *text_start = None;
            *text_end = None;
            return;
        }
        let start = text_start.take().unwrap_or(0);
        let end = text_end.take().unwrap_or(start);
        nodes.push(XmlNode::Text {
            content: std::mem::take(text_buf),
            offset: start,
            end_offset: end,
        });
    }

    let mut nodes: Vec<XmlNode> = Vec::new();
    // Accumulator for runs of Text + GeneralRef events.
    let mut text_buf = String::new();
    // Byte offset (file-absolute) of the first event in the current text run.
    let mut text_start_offset: Option<usize> = None;
    // Byte offset (file-absolute) past the last byte of the current text run.
    let mut text_end_offset: Option<usize> = None;
    let is_top_level = parent_name == SYNTHETIC_ROOT;

    loop {
        let event_pos = reader.buffer_position();

        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                flush_text(
                    &mut text_buf,
                    &mut text_start_offset,
                    &mut text_end_offset,
                    &mut nodes,
                    is_top_level,
                );

                let offset_in_content = content_offset(event_pos, root_tag_len);
                let file_offset = fence_offset + offset_in_content;

                let tag_name = decode_name(e.name().as_ref(), content, offset_in_content, path)?;
                validate_element_name(&tag_name, content, offset_in_content, path)?;
                let attributes = parse_attributes(e, content, offset_in_content, path)?;

                let children =
                    parse_children(reader, &tag_name, content, fence_offset, root_tag_len, path)?;

                nodes.push(XmlNode::Element {
                    name: tag_name,
                    attributes,
                    children,
                    offset: file_offset,
                });
            }

            Ok(Event::Empty(ref e)) => {
                flush_text(
                    &mut text_buf,
                    &mut text_start_offset,
                    &mut text_end_offset,
                    &mut nodes,
                    is_top_level,
                );

                let offset_in_content = content_offset(event_pos, root_tag_len);
                let file_offset = fence_offset + offset_in_content;

                let tag_name = decode_name(e.name().as_ref(), content, offset_in_content, path)?;
                validate_element_name(&tag_name, content, offset_in_content, path)?;
                let attributes = parse_attributes(e, content, offset_in_content, path)?;

                nodes.push(XmlNode::Element {
                    name: tag_name,
                    attributes,
                    children: vec![],
                    offset: file_offset,
                });
            }

            Ok(Event::Text(ref e)) => {
                // Accumulate raw text (no entity refs in here — those come as GeneralRef).
                let raw = std::str::from_utf8(e.as_ref()).map_err(|_err| {
                    let off = content_offset(event_pos, root_tag_len);
                    make_error(content, off, path, "invalid UTF-8 in text content")
                })?;
                let off = content_offset(event_pos, root_tag_len);
                if text_start_offset.is_none() {
                    text_start_offset = Some(fence_offset + off);
                }
                // Text events have no entity expansion, so raw byte length
                // equals the source byte length.
                text_end_offset = Some(fence_offset + off + raw.len());
                text_buf.push_str(raw);
            }

            Ok(Event::GeneralRef(ref e)) => {
                // Entity reference: e.g. `&amp;` arrives as GeneralRef with content `amp`.
                let entity_name = std::str::from_utf8(e.as_ref()).map_err(|_err| {
                    let off = content_offset(event_pos, root_tag_len);
                    make_error(content, off, path, "invalid UTF-8 in entity reference")
                })?;
                let off = content_offset(event_pos, root_tag_len);
                if text_start_offset.is_none() {
                    text_start_offset = Some(fence_offset + off);
                }
                // Raw source: `&` + entity_name + `;` — so raw length is
                // entity_name.len() + 2.
                text_end_offset = Some(fence_offset + off + entity_name.len() + 2);
                let resolved = resolve_entity(entity_name, content, off, path)?;
                text_buf.push_str(resolved);
            }

            Ok(Event::End(ref e)) => {
                let name_bytes = e.name();
                let end_name = std::str::from_utf8(name_bytes.as_ref()).unwrap_or("<invalid>");
                if end_name == parent_name {
                    flush_text(
                        &mut text_buf,
                        &mut text_start_offset,
                        &mut text_end_offset,
                        &mut nodes,
                        is_top_level,
                    );
                    return Ok(nodes);
                }
                // Mismatch — provide our own message.
                let offset_in_content = content_offset(event_pos, root_tag_len);
                return Err(make_error(
                    content,
                    offset_in_content,
                    path,
                    &format!(
                        "mismatched closing tag: expected `</{parent_name}>`, found `</{end_name}>`"
                    ),
                ));
            }

            Ok(Event::Eof) => {
                flush_text(
                    &mut text_buf,
                    &mut text_start_offset,
                    &mut text_end_offset,
                    &mut nodes,
                    is_top_level,
                );
                if is_top_level {
                    return Ok(nodes);
                }
                return Err(make_error(
                    content,
                    content.len(),
                    path,
                    &format!("expected closing tag `</{parent_name}>`, found end of input"),
                ));
            }

            // Constructs rejected by pre-scan but caught here as a safety net.
            Ok(Event::Comment(_)) => {
                let off = content_offset(event_pos, root_tag_len);
                return Err(make_error(
                    content,
                    off,
                    path,
                    "XML comments (`<!-- ... -->`) are not supported",
                ));
            }
            Ok(Event::CData(_)) => {
                let off = content_offset(event_pos, root_tag_len);
                return Err(make_error(
                    content,
                    off,
                    path,
                    "CDATA sections (`<![CDATA[...]]>`) are not supported",
                ));
            }
            Ok(Event::PI(_) | Event::Decl(_)) => {
                let off = content_offset(event_pos, root_tag_len);
                return Err(make_error(
                    content,
                    off,
                    path,
                    "processing instructions (`<?...?>`) are not supported",
                ));
            }
            Ok(Event::DocType(_)) => {
                let off = content_offset(event_pos, root_tag_len);
                return Err(make_error(
                    content,
                    off,
                    path,
                    "DTD declarations (`<!DOCTYPE ...>`) are not supported",
                ));
            }

            Err(e) => {
                return Err(quick_xml_error_to_parse_error(&e, path));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Attribute parsing
// ---------------------------------------------------------------------------

/// Extract attributes from a quick-xml `BytesStart` event.
fn parse_attributes(
    event: &quick_xml::events::BytesStart<'_>,
    content: &str,
    offset_in_content: usize,
    path: &Path,
) -> Result<Vec<(String, String)>, ParseError> {
    // First, validate that all attribute values are double-quoted by scanning
    // the raw event bytes.
    validate_attribute_quotes(event, content, offset_in_content, path)?;

    let mut attrs = Vec::new();
    for attr_result in event.attributes() {
        let attr = attr_result.map_err(|e| {
            let msg = format!("{e}");
            make_error(content, offset_in_content, path, &msg)
        })?;

        let key = decode_name(attr.key.as_ref(), content, offset_in_content, path)?;

        // Reject namespaced attributes.
        if key.contains(':') {
            return Err(make_error(
                content,
                offset_in_content,
                path,
                &format!("namespaced attribute `{key}` is not supported"),
            ));
        }

        // Decode the value, resolving our supported entity subset.
        let raw_value = std::str::from_utf8(attr.value.as_ref()).map_err(|_err| {
            make_error(
                content,
                offset_in_content,
                path,
                "invalid UTF-8 in attribute value",
            )
        })?;
        let value = resolve_entities_in_str(raw_value, content, offset_in_content, path)?;

        attrs.push((key, value));
    }
    Ok(attrs)
}

/// Check that all attribute values use double quotes (not single quotes,
/// not unquoted).
///
/// quick-xml accepts both single- and double-quoted attribute values, so we
/// must inspect the raw tag bytes to enforce our subset requirement.
fn validate_attribute_quotes(
    event: &quick_xml::events::BytesStart<'_>,
    content: &str,
    offset_in_content: usize,
    path: &Path,
) -> Result<(), ParseError> {
    let raw: &[u8] = event.as_ref(); // bytes after the tag name
    let mut i = 0;
    let mut in_double_quote = false;
    while i < raw.len() {
        if in_double_quote {
            if raw[i] == b'"' {
                in_double_quote = false;
            }
            i += 1;
            continue;
        }
        if raw[i] == b'"' {
            in_double_quote = true;
            i += 1;
            continue;
        }
        if raw[i] == b'=' {
            // Skip whitespace after `=`.
            i += 1;
            while i < raw.len() && raw[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < raw.len() && raw[i] == b'\'' {
                return Err(make_error(
                    content,
                    offset_in_content,
                    path,
                    "attribute values must be double-quoted",
                ));
            }
            // Don't advance `i` here — let the main loop handle the
            // opening `"` so that double-quote tracking activates.
            continue;
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Entity resolution
// ---------------------------------------------------------------------------

/// Resolve a single entity name to its replacement character.
fn resolve_entity(
    name: &str,
    content: &str,
    offset_in_content: usize,
    path: &Path,
) -> Result<&'static str, ParseError> {
    match name {
        "amp" => Ok("&"),
        "lt" => Ok("<"),
        "gt" => Ok(">"),
        "quot" => Ok("\""),
        _ => Err(make_error(
            content,
            offset_in_content,
            path,
            &format!("unsupported entity reference `&{name};`"),
        )),
    }
}

/// Resolve entity references in a string (used for attribute values where
/// quick-xml delivers the raw encoded text).
fn resolve_entities_in_str(
    text: &str,
    content: &str,
    offset_in_content: usize,
    path: &Path,
) -> Result<String, ParseError> {
    if !text.contains('&') {
        return Ok(text.to_owned());
    }

    let mut result = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(amp_pos) = rest.find('&') {
        result.push_str(&rest[..amp_pos]);
        rest = &rest[amp_pos + 1..];
        if let Some(semi_pos) = rest.find(';') {
            let entity_name = &rest[..semi_pos];
            let resolved = resolve_entity(entity_name, content, offset_in_content, path)?;
            result.push_str(resolved);
            rest = &rest[semi_pos + 1..];
        } else {
            return Err(make_error(
                content,
                offset_in_content,
                path,
                "unterminated entity reference (missing `;`)",
            ));
        }
    }
    result.push_str(rest);
    Ok(result)
}

// ---------------------------------------------------------------------------
// Name decoding
// ---------------------------------------------------------------------------

/// Decode a raw byte slice to a `String`, producing a parse error for invalid
/// UTF-8.
fn decode_name(
    raw: &[u8],
    content: &str,
    offset_in_content: usize,
    path: &Path,
) -> Result<String, ParseError> {
    std::str::from_utf8(raw)
        .map(str::to_owned)
        .map_err(|_err| make_error(content, offset_in_content, path, "invalid UTF-8 in name"))
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// Validate that an element name is `PascalCase` (starts with uppercase ASCII)
/// and does not contain namespace prefixes.
fn validate_element_name(
    name: &str,
    content: &str,
    offset_in_content: usize,
    path: &Path,
) -> Result<(), ParseError> {
    if name.contains(':') {
        return Err(make_error(
            content,
            offset_in_content,
            path,
            &format!("namespaced element `{name}` is not supported"),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

/// Convert a quick-xml `u64` buffer position to a `usize` content offset by
/// subtracting the synthetic root tag length.
///
/// XML fence content is always small (well under 4 GiB), so truncation on
/// 32-bit targets is not a concern in practice.
#[allow(
    clippy::cast_possible_truncation,
    reason = "XML fence content is always small"
)]
fn content_offset(event_pos: u64, root_tag_len: u64) -> usize {
    (event_pos - root_tag_len) as usize
}

/// Build a `ParseError::XmlSyntaxError` from a byte position in the content.
fn make_error(content: &str, offset_in_content: usize, path: &Path, message: &str) -> ParseError {
    let (line, column) = line_col(content, offset_in_content);
    ParseError::XmlSyntaxError {
        path: path.to_path_buf(),
        line,
        column,
        message: message.to_owned(),
    }
}

/// Convert a `quick_xml::Error` into a `ParseError::XmlSyntaxError`.
///
/// The line 1, column 1 fallback is deliberate: quick-xml does not expose
/// byte positions for structural errors, so we cannot compute a more precise
/// location.
fn quick_xml_error_to_parse_error(err: &quick_xml::Error, path: &Path) -> ParseError {
    let message = match err {
        quick_xml::Error::IllFormed(ill) => match ill {
            quick_xml::errors::IllFormedError::MismatchedEndTag { expected, found } => {
                if expected == SYNTHETIC_ROOT {
                    // Closing tag at the top level with no matching open tag.
                    format!("unexpected closing tag `</{found}>` at top level")
                } else {
                    format!("mismatched closing tag: expected `</{expected}>`, found `</{found}>`")
                }
            }
            quick_xml::errors::IllFormedError::UnmatchedEndTag(name) => {
                format!("unexpected closing tag `</{name}>` at top level")
            }
            quick_xml::errors::IllFormedError::MissingEndTag(name) => {
                format!("expected closing tag `</{name}>`, found end of input")
            }
            quick_xml::errors::IllFormedError::UnclosedReference => {
                "unterminated entity reference (missing `;`)".to_owned()
            }
            other => format!("{other}"),
        },
        other => format!("{other}"),
    };

    ParseError::XmlSyntaxError {
        path: path.to_path_buf(),
        line: 1,
        column: 1,
        message,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::match_wildcard_for_single_variants,
    clippy::single_char_pattern,
    reason = "test assertions are clearer with wildcards and string patterns"
)]
mod tests {
    use super::*;

    fn parse(content: &str) -> Result<Vec<XmlNode>, ParseError> {
        parse_supersigil_xml(content, 0, Path::new("test.md"))
    }

    fn parse_with_offset(content: &str, offset: usize) -> Result<Vec<XmlNode>, ParseError> {
        parse_supersigil_xml(content, offset, Path::new("test.md"))
    }

    // -- Valid fragments ---------------------------------------------------

    #[test]
    fn empty_input() {
        let nodes = parse("").unwrap();
        assert!(nodes.is_empty());
    }

    #[test]
    fn whitespace_only_input() {
        let nodes = parse("  \n  \n  ").unwrap();
        assert!(nodes.is_empty());
    }

    #[test]
    fn single_self_closing_element() {
        let nodes = parse(r#"<Spec id="s1" />"#).unwrap();
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            XmlNode::Element {
                name,
                attributes,
                children,
                offset,
            } => {
                assert_eq!(name, "Spec");
                assert_eq!(attributes, &[("id".to_owned(), "s1".to_owned())]);
                assert!(children.is_empty());
                assert_eq!(*offset, 0);
            }
            _ => panic!("expected Element"),
        }
    }

    #[test]
    fn self_closing_no_space_before_slash() {
        let nodes = parse(r#"<Spec id="s1"/>"#).unwrap();
        assert_eq!(nodes.len(), 1);
        if let XmlNode::Element { name, .. } = &nodes[0] {
            assert_eq!(name, "Spec");
        } else {
            panic!("expected Element");
        }
    }

    #[test]
    fn element_with_text_content() {
        let nodes = parse("<Title>Hello World</Title>").unwrap();
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            XmlNode::Element { name, children, .. } => {
                assert_eq!(name, "Title");
                assert_eq!(children.len(), 1);
                assert!(
                    matches!(&children[0], XmlNode::Text { content, .. } if content == "Hello World")
                );
            }
            _ => panic!("expected Element"),
        }
    }

    #[test]
    fn element_with_no_attributes() {
        let nodes = parse("<Container></Container>").unwrap();
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            XmlNode::Element {
                name, attributes, ..
            } => {
                assert_eq!(name, "Container");
                assert!(attributes.is_empty());
            }
            _ => panic!("expected Element"),
        }
    }

    // -- Nested elements ---------------------------------------------------

    #[test]
    fn nested_elements() {
        let input = r#"<Parent id="p1"><Child id="c1" /></Parent>"#;
        let nodes = parse(input).unwrap();
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            XmlNode::Element { name, children, .. } => {
                assert_eq!(name, "Parent");
                assert_eq!(children.len(), 1);
                match &children[0] {
                    XmlNode::Element {
                        name, attributes, ..
                    } => {
                        assert_eq!(name, "Child");
                        assert_eq!(attributes, &[("id".to_owned(), "c1".to_owned())]);
                    }
                    _ => panic!("expected nested Element"),
                }
            }
            _ => panic!("expected Element"),
        }
    }

    #[test]
    fn deeply_nested_elements() {
        let input = "<A><B><C>deep</C></B></A>";
        let nodes = parse(input).unwrap();
        assert_eq!(nodes.len(), 1);
        // A -> B -> C -> Text("deep")
        let a = &nodes[0];
        if let XmlNode::Element { children, .. } = a {
            let b = &children[0];
            if let XmlNode::Element { children, .. } = b {
                let c = &children[0];
                if let XmlNode::Element { children, .. } = c {
                    assert!(
                        matches!(&children[0], XmlNode::Text { content, .. } if content == "deep")
                    );
                } else {
                    panic!("expected C element");
                }
            } else {
                panic!("expected B element");
            }
        } else {
            panic!("expected A element");
        }
    }

    #[test]
    fn mixed_children_text_and_elements() {
        let input = "<Parent>before<Child />after</Parent>";
        let nodes = parse(input).unwrap();
        assert_eq!(nodes.len(), 1);
        if let XmlNode::Element { children, .. } = &nodes[0] {
            assert_eq!(children.len(), 3);
            assert!(matches!(&children[0], XmlNode::Text { content, .. } if content == "before"));
            assert!(matches!(&children[1], XmlNode::Element { name, .. } if name == "Child"));
            assert!(matches!(&children[2], XmlNode::Text { content, .. } if content == "after"));
        } else {
            panic!("expected Element");
        }
    }

    // -- Multiple top-level elements ---------------------------------------

    #[test]
    fn multiple_top_level_elements() {
        let input = r#"<A id="1" />
<B id="2" />"#;
        let nodes = parse(input).unwrap();
        assert_eq!(nodes.len(), 2);
        if let XmlNode::Element { name, .. } = &nodes[0] {
            assert_eq!(name, "A");
        }
        if let XmlNode::Element { name, .. } = &nodes[1] {
            assert_eq!(name, "B");
        }
    }

    // -- Attribute parsing -------------------------------------------------

    #[test]
    fn multiple_attributes() {
        let input = r#"<Criterion id="c1" strategy="tag" />"#;
        let nodes = parse(input).unwrap();
        if let XmlNode::Element { attributes, .. } = &nodes[0] {
            assert_eq!(
                attributes,
                &[
                    ("id".to_owned(), "c1".to_owned()),
                    ("strategy".to_owned(), "tag".to_owned()),
                ]
            );
        } else {
            panic!("expected Element");
        }
    }

    #[test]
    fn attribute_with_entity_in_value() {
        let input = r#"<Spec desc="a &amp; b" />"#;
        let nodes = parse(input).unwrap();
        if let XmlNode::Element { attributes, .. } = &nodes[0] {
            assert_eq!(attributes[0].1, "a & b");
        } else {
            panic!("expected Element");
        }
    }

    #[test]
    fn all_supported_entities_in_attribute() {
        let input = r#"<Spec val="&amp;&lt;&gt;&quot;" />"#;
        let nodes = parse(input).unwrap();
        if let XmlNode::Element { attributes, .. } = &nodes[0] {
            assert_eq!(attributes[0].1, "&<>\"");
        } else {
            panic!("expected Element");
        }
    }

    // -- Entity references in text content ---------------------------------

    #[test]
    fn entity_references_in_text() {
        let input = "<Note>a &lt; b &amp; c &gt; d &quot;e&quot;</Note>";
        let nodes = parse(input).unwrap();
        if let XmlNode::Element { children, .. } = &nodes[0] {
            assert!(
                matches!(&children[0], XmlNode::Text { content, .. } if content == r#"a < b & c > d "e""#)
            );
        } else {
            panic!("expected Element");
        }
    }

    // -- Position offsetting -----------------------------------------------

    #[test]
    fn offset_applied_to_elements() {
        let fence_offset = 100;
        let input = r#"<Spec id="s1" />"#;
        let nodes = parse_with_offset(input, fence_offset).unwrap();
        if let XmlNode::Element { offset, .. } = &nodes[0] {
            assert_eq!(*offset, 100);
        } else {
            panic!("expected Element");
        }
    }

    #[test]
    fn offset_applied_to_nested_element() {
        let fence_offset = 50;
        // "<A>" takes 3 bytes, so <B> starts at position 3.
        let input = "<A><B /></A>";
        let nodes = parse_with_offset(input, fence_offset).unwrap();
        if let XmlNode::Element {
            offset, children, ..
        } = &nodes[0]
        {
            assert_eq!(*offset, 50); // A at position 0 + 50
            if let XmlNode::Element { offset, .. } = &children[0] {
                assert_eq!(*offset, 53); // B at position 3 + 50
            } else {
                panic!("expected nested Element");
            }
        } else {
            panic!("expected Element");
        }
    }

    #[test]
    fn offset_with_multiple_top_level_elements() {
        let fence_offset = 200;
        let input = "<A />\n<B />";
        let nodes = parse_with_offset(input, fence_offset).unwrap();
        assert_eq!(nodes.len(), 2);
        if let XmlNode::Element { offset, .. } = &nodes[0] {
            assert_eq!(*offset, 200); // A at byte 0
        }
        if let XmlNode::Element { offset, .. } = &nodes[1] {
            assert_eq!(*offset, 206); // B at byte 6 (after "<A />\n")
        }
    }

    // -- Error cases: unclosed tags ----------------------------------------

    #[test]
    fn unclosed_element() {
        let err = parse("<Spec>content").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("closing tag"), "got: {msg}");
        assert!(msg.contains("Spec"), "got: {msg}");
    }

    #[test]
    fn mismatched_closing_tag() {
        let err = parse("<A>text</B>").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("mismatched"), "got: {msg}");
        assert!(msg.contains("A"), "got: {msg}");
        assert!(msg.contains("B"), "got: {msg}");
    }

    // -- Error cases: invalid attributes -----------------------------------

    #[test]
    fn single_quoted_attribute_value() {
        let err = parse("<Spec id='s1' />").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("double-quoted"), "got: {msg}");
    }

    #[test]
    fn single_quotes_inside_double_quoted_attribute_value() {
        // Single quotes inside a double-quoted attribute value are valid XML.
        let result = parse(r#"<Spec val="a='b'" />"#);
        assert!(result.is_ok(), "got: {}", result.unwrap_err());
    }

    #[test]
    fn missing_attribute_value() {
        let err = parse("<Spec id />").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("="), "got: {msg}");
    }

    // -- Error cases: unsupported XML features -----------------------------

    #[test]
    fn processing_instruction_rejected() {
        let err = parse("<?xml version=\"1.0\"?>").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("processing instruction"), "got: {msg}");
    }

    #[test]
    fn cdata_rejected() {
        let err = parse("<![CDATA[foo]]>").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("CDATA"), "got: {msg}");
    }

    #[test]
    fn doctype_rejected() {
        let err = parse("<!DOCTYPE html>").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("DTD") || msg.contains("DOCTYPE"), "got: {msg}");
    }

    #[test]
    fn comment_rejected() {
        let err = parse("<!-- comment -->").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("comment"), "got: {msg}");
    }

    #[test]
    fn namespace_in_element_rejected() {
        // The parser treats `:` as not part of a name, so `foo:Bar` would
        // parse `foo` as the name and then fail on `:`. That error is
        // acceptable — the point is it doesn't silently succeed.
        let err = parse("<foo:Bar />").unwrap_err();
        assert!(
            err.to_string().contains("test.md"),
            "error should include path"
        );
    }

    #[test]
    fn unsupported_entity_rejected() {
        let err = parse("<Spec>&apos;</Spec>").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unsupported entity"), "got: {msg}");
    }

    #[test]
    fn unterminated_entity_rejected() {
        let err = parse("<Spec>&amp</Spec>").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unterminated entity"), "got: {msg}");
    }

    // -- Lowercase elements are parsed successfully -------------------------

    #[test]
    fn lowercase_element_name_parsed_successfully() {
        let nodes = parse("<spec />").unwrap();
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            XmlNode::Element { name, .. } => assert_eq!(name, "spec"),
            _ => panic!("expected Element"),
        }
    }

    #[test]
    fn lowercase_element_inside_pascal_case_element() {
        let nodes = parse(r#"<Criterion id="c1">Use <em>fast</em> path</Criterion>"#).unwrap();
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            XmlNode::Element { name, children, .. } => {
                assert_eq!(name, "Criterion");
                // Children: Text("Use "), Element(em), Text(" path")
                assert_eq!(children.len(), 3);
                assert!(matches!(&children[0], XmlNode::Text { content, .. } if content == "Use "));
                match &children[1] {
                    XmlNode::Element { name, children, .. } => {
                        assert_eq!(name, "em");
                        assert_eq!(children.len(), 1);
                        assert!(
                            matches!(&children[0], XmlNode::Text { content, .. } if content == "fast")
                        );
                    }
                    _ => panic!("expected em Element"),
                }
                assert!(
                    matches!(&children[2], XmlNode::Text { content, .. } if content == " path")
                );
            }
            _ => panic!("expected Criterion Element"),
        }
    }

    // -- Error position information ----------------------------------------

    #[test]
    fn error_includes_line_and_column() {
        // Error on line 2, at the start of the line (column 1).
        // Use a namespaced element (still rejected) to trigger an error.
        let err = parse("<A>\n<ns:B /></A>").unwrap_err();
        if let ParseError::XmlSyntaxError { line, column, .. } = &err {
            assert_eq!(*line, 2);
            assert_eq!(*column, 1);
        } else {
            panic!("expected XmlSyntaxError");
        }
    }

    #[test]
    fn error_includes_file_path() {
        let err = parse_supersigil_xml("<?xml?>", 0, Path::new("/foo/bar.md")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("/foo/bar.md"), "got: {msg}");
    }

    // -- Closing tag at top level ------------------------------------------

    #[test]
    fn closing_tag_at_top_level_rejected() {
        let err = parse("</Orphan>").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unexpected closing tag"), "got: {msg}");
    }

    // -- Realistic example -------------------------------------------------

    #[test]
    fn realistic_component_example() {
        let input = r#"<Criterion id="perf-latency" strategy="tag">
  P99 latency must be under 100ms for API requests.
</Criterion>
<VerifiedBy strategy="tag" tag="perf-latency" />"#;
        let nodes = parse(input).unwrap();
        assert_eq!(nodes.len(), 2);

        // Criterion
        match &nodes[0] {
            XmlNode::Element {
                name,
                attributes,
                children,
                ..
            } => {
                assert_eq!(name, "Criterion");
                assert_eq!(attributes.len(), 2);
                assert_eq!(attributes[0], ("id".to_owned(), "perf-latency".to_owned()));
                assert_eq!(attributes[1], ("strategy".to_owned(), "tag".to_owned()));
                assert_eq!(children.len(), 1);
                if let XmlNode::Text { content, .. } = &children[0] {
                    assert!(content.contains("P99 latency"));
                } else {
                    panic!("expected Text child");
                }
            }
            _ => panic!("expected Element"),
        }

        // VerifiedBy
        match &nodes[1] {
            XmlNode::Element {
                name,
                attributes,
                children,
                ..
            } => {
                assert_eq!(name, "VerifiedBy");
                assert_eq!(attributes.len(), 2);
                assert!(children.is_empty());
            }
            _ => panic!("expected Element"),
        }
    }

    // -- UTF-8 text content ------------------------------------------------

    #[test]
    fn utf8_text_content_preserved() {
        let input = "<Note>cafe\u{0301} \u{1F600}</Note>";
        let nodes = parse(input).unwrap();
        if let XmlNode::Element { children, .. } = &nodes[0] {
            if let XmlNode::Text { content: t, .. } = &children[0] {
                assert!(t.contains("cafe\u{0301}"));
                assert!(t.contains('\u{1F600}'));
            } else {
                panic!("expected Text");
            }
        } else {
            panic!("expected Element");
        }
    }

    #[test]
    fn text_node_has_correct_offset() {
        // "<Title>" = 7 bytes, so text "Hello" starts at offset 7
        let input = "<Title>Hello</Title>";
        let nodes = parse(input).unwrap();
        if let XmlNode::Element { children, .. } = &nodes[0] {
            if let XmlNode::Text {
                content, offset, ..
            } = &children[0]
            {
                assert_eq!(content, "Hello");
                assert_eq!(*offset, 7, "text should start at byte 7 (after '<Title>')");
            } else {
                panic!("expected Text");
            }
        } else {
            panic!("expected Element");
        }
    }

    #[test]
    fn text_node_offset_with_fence_offset() {
        let fence_offset = 100;
        let input = "<Title>Hello</Title>";
        let nodes = parse_with_offset(input, fence_offset).unwrap();
        if let XmlNode::Element { children, .. } = &nodes[0] {
            if let XmlNode::Text {
                content, offset, ..
            } = &children[0]
            {
                assert_eq!(content, "Hello");
                assert_eq!(*offset, 107, "text should be fence_offset + 7");
            } else {
                panic!("expected Text");
            }
        } else {
            panic!("expected Element");
        }
    }

    #[test]
    fn text_node_end_offset_plain_text() {
        // "<Title>Hello</Title>" — text "Hello" starts at 7, ends at 12
        let input = "<Title>Hello</Title>";
        let nodes = parse(input).unwrap();
        if let XmlNode::Element { children, .. } = &nodes[0] {
            if let XmlNode::Text {
                content,
                offset,
                end_offset,
            } = &children[0]
            {
                assert_eq!(content, "Hello");
                assert_eq!(*offset, 7);
                assert_eq!(
                    *end_offset, 12,
                    "end_offset should be past the last byte of 'Hello'"
                );
                assert_eq!(
                    &input[*offset..*end_offset],
                    "Hello",
                    "offset..end_offset should span the raw text"
                );
            } else {
                panic!("expected Text");
            }
        } else {
            panic!("expected Element");
        }
    }

    #[test]
    fn text_node_end_offset_with_entities() {
        // "a &lt; b" in raw source → decoded "a < b"
        // Raw: a(1) space(1) &lt;(4) space(1) b(1) = 8 bytes
        let input = "<T>a &lt; b</T>";
        let nodes = parse(input).unwrap();
        if let XmlNode::Element { children, .. } = &nodes[0] {
            if let XmlNode::Text {
                content,
                offset,
                end_offset,
            } = &children[0]
            {
                assert_eq!(content, "a < b", "content should be entity-decoded");
                assert_eq!(*offset, 3, "text starts after '<T>'");
                assert_eq!(
                    *end_offset, 11,
                    "end_offset should be past the last byte of 'a &lt; b' in raw source"
                );
                assert_eq!(
                    &input[*offset..*end_offset],
                    "a &lt; b",
                    "offset..end_offset should span the raw source text"
                );
                // Decoded length (5) < raw length (8)
                assert!(content.len() < (*end_offset - *offset));
            } else {
                panic!("expected Text");
            }
        } else {
            panic!("expected Element");
        }
    }

    #[test]
    fn text_node_end_offset_with_fence_offset_and_entities() {
        let fence_offset = 50;
        let input = "<T>&amp;</T>";
        let nodes = parse_with_offset(input, fence_offset).unwrap();
        if let XmlNode::Element { children, .. } = &nodes[0] {
            if let XmlNode::Text {
                content,
                offset,
                end_offset,
            } = &children[0]
            {
                assert_eq!(content, "&", "decoded entity");
                assert_eq!(*offset, 53, "starts at fence_offset + 3 (after '<T>')");
                // &amp; = 5 bytes in raw source, starts at position 3 in XML content
                assert_eq!(
                    *end_offset, 58,
                    "end_offset = fence_offset + 3 + 5 (length of '&amp;')"
                );
            } else {
                panic!("expected Text");
            }
        } else {
            panic!("expected Element");
        }
    }

    #[test]
    fn text_node_end_offset_multiple_entities() {
        // "&lt;&gt;" → decoded "<>" (2 chars), raw = 8 bytes
        let input = "<T>&lt;&gt;</T>";
        let nodes = parse(input).unwrap();
        if let XmlNode::Element { children, .. } = &nodes[0] {
            if let XmlNode::Text {
                content,
                offset,
                end_offset,
            } = &children[0]
            {
                assert_eq!(content, "<>");
                assert_eq!(*offset, 3);
                assert_eq!(*end_offset, 11, "past '&lt;&gt;' in raw source");
                assert_eq!(&input[*offset..*end_offset], "&lt;&gt;");
            } else {
                panic!("expected Text");
            }
        } else {
            panic!("expected Element");
        }
    }
}
