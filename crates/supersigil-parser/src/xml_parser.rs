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
        /// Byte offset past the closing `>` relative to the source file.
        end_offset: usize,
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

                // After parse_children returns, reader is past the closing `>`.
                let end_in_content = content_offset(reader.buffer_position(), root_tag_len);
                let file_end_offset = fence_offset + end_in_content;

                nodes.push(XmlNode::Element {
                    name: tag_name,
                    attributes,
                    children,
                    offset: file_offset,
                    end_offset: file_end_offset,
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

                // After reading Empty event, reader is past the closing `/>`.
                let end_in_content = content_offset(reader.buffer_position(), root_tag_len);
                let file_end_offset = fence_offset + end_in_content;

                nodes.push(XmlNode::Element {
                    name: tag_name,
                    attributes,
                    children: vec![],
                    offset: file_offset,
                    end_offset: file_end_offset,
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
mod tests;
