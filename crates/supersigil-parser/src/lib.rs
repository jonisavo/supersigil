//! Parsing pipeline for supersigil spec documents.
//!
//! Documents use standard Markdown with `supersigil-xml` fenced code blocks
//! for component markup.

mod frontmatter;
mod markdown_fences;
mod preprocess;
pub mod util;
mod xml_extract;
mod xml_parser;

pub use frontmatter::{FrontMatterResult, deserialize_front_matter, extract_front_matter};
pub use markdown_fences::{MarkdownFences, XmlFence, extract_markdown_fences};
pub use preprocess::{normalize, preprocess};
pub use xml_extract::extract_components_from_xml;
pub use xml_parser::{XmlNode, parse_supersigil_xml};

use std::path::Path;

use supersigil_core::{ComponentDefs, ExtractedComponent, ParseError, ParseResult, SpecDocument};

/// A recovered parse result that may include a partial document alongside
/// fatal validation errors.
#[derive(Debug)]
pub struct RecoveredParse {
    /// The parse result. `Document` may be present even when `fatal_errors`
    /// is non-empty, allowing best-effort local features to use the partial
    /// component tree.
    pub result: ParseResult,
    /// Fatal errors produced after enough structure was recovered to build a
    /// partial `SpecDocument`.
    pub fatal_errors: Vec<ParseError>,
}

// ---------------------------------------------------------------------------
// Lint-time validation (format-agnostic)
// ---------------------------------------------------------------------------

/// Validate extracted components against the known component definitions.
///
/// Checks missing required attributes -> `MissingRequiredAttribute` error.
///
/// Only known components reach this point (unknown `PascalCase` elements are
/// filtered out during extraction), so every component here has a definition.
/// Recurses into children.
pub fn validate_components(
    components: &[ExtractedComponent],
    component_defs: &ComponentDefs,
    path: &Path,
    errors: &mut Vec<ParseError>,
) {
    for comp in components {
        if let Some(def) = component_defs.get(&comp.name) {
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
        }
        validate_components(&comp.children, component_defs, path, errors);
    }
}

// ---------------------------------------------------------------------------
// parse_content — public API (Req 8-1)
// ---------------------------------------------------------------------------

/// Parse a spec document from an in-memory string into a [`ParseResult`].
///
/// This is the core of the parsing pipeline, operating on a content string
/// that has already been decoded and normalized (e.g. by the LSP buffer or
/// by [`parse_file`] after preprocessing). It performs:
///
/// 1. **Front matter** — extraction and deserialization (fatal on error).
/// 2. **Markdown fence extraction** — parse the body as standard Markdown
///    and collect `supersigil-xml` fenced code blocks.
/// 3. **XML parsing** — parse each `supersigil-xml` fence into structured
///    XML nodes. Errors in one fence do not prevent parsing of others.
/// 4. **Component extraction** — walk XML nodes and extract known components.
/// 5. **Lint-time validation** — check required attributes, etc.
///
/// Stage 1 errors are fatal and prevent all later stages.
///
/// # Errors
///
/// Returns `Vec<ParseError>` containing all detected errors across stages.
pub fn parse_content_recovering(
    path: &Path,
    content: &str,
    component_defs: &ComponentDefs,
) -> Result<RecoveredParse, Vec<ParseError>> {
    // Stage 1: Extract front matter
    let (yaml, body) = match extract_front_matter(content, path) {
        Ok(Some((yaml, body))) => (yaml, body),
        Ok(None) => {
            return Ok(RecoveredParse {
                result: ParseResult::NotSupersigil(path.to_path_buf()),
                fatal_errors: Vec::new(),
            });
        }
        Err(e) => return Err(vec![e]),
    };

    // Stage 1: Deserialize front matter
    let (frontmatter, extra) = match deserialize_front_matter(yaml, path) {
        Ok(FrontMatterResult::Supersigil { frontmatter, extra }) => (frontmatter, extra),
        Ok(FrontMatterResult::NotSupersigil) => {
            return Ok(RecoveredParse {
                result: ParseResult::NotSupersigil(path.to_path_buf()),
                fatal_errors: Vec::new(),
            });
        }
        Err(e) => return Err(vec![e]),
    };

    // Compute body offset for source position adjustment.
    // body starts at content[body_offset..], so:
    let body_offset = content.len() - body.len();

    // Stage 2: Parse Markdown body and extract supersigil-xml fences
    let fences = extract_markdown_fences(body, body_offset);

    // Stage 3: Parse XML content from each supersigil-xml fence
    let mut errors = Vec::new();
    let mut all_components = Vec::new();
    for fence in &fences.xml_fences {
        match parse_supersigil_xml(&fence.content, fence.content_offset, path) {
            Ok(nodes) => {
                let mut comps = extract_components_from_xml(&nodes, content, component_defs);
                all_components.append(&mut comps);
            }
            Err(e) => {
                // Adjust fence-relative line to file-absolute
                let adjusted = match e {
                    ParseError::XmlSyntaxError {
                        path,
                        line,
                        column,
                        message,
                    } => {
                        // Compute the line number where this fence starts in the file
                        let fence_start_line = content[..fence.content_offset]
                            .chars()
                            .filter(|&c| c == '\n')
                            .count();
                        ParseError::XmlSyntaxError {
                            path,
                            line: line + fence_start_line,
                            column,
                            message,
                        }
                    }
                    other => other,
                };
                errors.push(adjusted);
            }
        }
    }

    // Stage 4: Lint-time validation
    validate_components(&all_components, component_defs, path, &mut errors);

    Ok(RecoveredParse {
        result: ParseResult::Document(SpecDocument {
            path: path.to_path_buf(),
            frontmatter,
            extra,
            components: all_components,
        }),
        fatal_errors: errors,
    })
}

/// Parse a spec document from an in-memory string into a [`ParseResult`].
///
/// This returns only fully valid documents. Call
/// [`parse_content_recovering`] when the caller needs best-effort access to
/// partially valid component trees.
///
/// # Errors
///
/// Returns `Vec<ParseError>` when front matter, XML parsing, or validation
/// prevents the document from being considered fully valid.
pub fn parse_content(
    path: &Path,
    content: &str,
    component_defs: &ComponentDefs,
) -> Result<ParseResult, Vec<ParseError>> {
    let recovered = parse_content_recovering(path, content, component_defs)?;
    if recovered.fatal_errors.is_empty() {
        Ok(recovered.result)
    } else {
        Err(recovered.fatal_errors)
    }
}

// ---------------------------------------------------------------------------
// parse_file — public API (Req 10)
// ---------------------------------------------------------------------------

/// Parse a single spec file into a [`ParseResult`].
///
/// Implements the full parsing pipeline:
/// 1. Preprocess (UTF-8 decode, BOM strip, CRLF normalize).
/// 2. Front matter extraction and deserialization.
/// 3. Markdown fence extraction (`supersigil-xml`).
/// 4. XML parsing and component extraction.
/// 5. Lint-time validation.
///
/// Stage 1 fatal errors prevent later stages. XML parse errors in one fence
/// do not prevent other fences from being parsed.
///
/// # Errors
///
/// Returns `Vec<ParseError>` containing all detected errors across stages.
pub fn parse_file(
    path: impl AsRef<Path>,
    component_defs: &ComponentDefs,
) -> Result<ParseResult, Vec<ParseError>> {
    let path = path.as_ref();
    // Read file
    let raw = std::fs::read(path).map_err(|e| {
        vec![ParseError::IoError {
            path: path.to_path_buf(),
            source: e,
        }]
    })?;

    // Stage 1: Preprocess
    let content = preprocess(&raw, path).map_err(|e| vec![e])?;

    parse_content(path, &content, component_defs)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_error_positions_are_file_absolute_not_fence_relative() {
        // Front matter (3 lines) + blank line + prose line + blank line = 6 lines
        // Then the fence marker is on line 7, fence content starts on line 8.
        let content = "\
---
supersigil:
  id: test/err
  type: requirements
  status: approved
---

Some prose here.

```supersigil-xml
<Criterion id=\"c1\">
  <?bad processing instruction?>
</Criterion>
```
";
        let defs = ComponentDefs::defaults();
        let errors = parse_content(Path::new("test.md"), content, &defs).unwrap_err();
        assert!(!errors.is_empty(), "should have at least one error");

        // Find the XML syntax error
        let xml_err = errors
            .iter()
            .find(|e| matches!(e, ParseError::XmlSyntaxError { .. }))
            .expect("should have an XmlSyntaxError");

        if let ParseError::XmlSyntaxError { line, .. } = xml_err {
            // The processing instruction is on line 2 within the fence,
            // but the fence content starts on line 11 of the file.
            // So the error line should be > 10 (file-absolute).
            assert!(
                *line > 2,
                "error line should be file-absolute (got {line}, fence-relative would be 2)"
            );
        }
    }

    #[test]
    fn xml_syntax_error_remains_fatal() {
        let content = "\
---
supersigil:
  id: test/fatal
  type: requirements
  status: approved
---

```supersigil-xml
<Criterion id=\"c1\">
  <?bad processing instruction?>
</Criterion>
```
";
        let defs = ComponentDefs::defaults();
        let result = parse_content(Path::new("test.md"), content, &defs);

        assert!(result.is_err(), "XML syntax error should still be fatal");
        let errors = result.unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ParseError::XmlSyntaxError { .. })),
            "should contain XmlSyntaxError"
        );
    }

    #[test]
    fn missing_required_attribute_remains_fatal() {
        // A Criterion component without the required `id` attribute.
        let content = "\
---
supersigil:
  id: test/missing-attr
  type: requirements
  status: approved
---

```supersigil-xml
<Criterion>
  some text
</Criterion>
```
";
        let defs = ComponentDefs::defaults();
        let result = parse_content(Path::new("test.md"), content, &defs);

        assert!(
            result.is_err(),
            "MissingRequiredAttribute should still be fatal"
        );
        let errors = result.unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ParseError::MissingRequiredAttribute { .. })),
            "should contain MissingRequiredAttribute"
        );
    }

    #[test]
    fn parse_content_recovering_keeps_partial_document_on_validation_error() {
        let content = "\
---
supersigil:
  id: test/partial
  type: requirements
  status: draft
---

```supersigil-xml
<AcceptanceCriteria>
  <Criterion>broken</Criterion>
  <Criterion id=\"ok-1\">ok</Criterion>
</AcceptanceCriteria>
```
";
        let defs = ComponentDefs::defaults();
        let recovered = parse_content_recovering(Path::new("test.md"), content, &defs)
            .expect("recovering parse should return a partial document");

        assert_eq!(recovered.fatal_errors.len(), 1);
        assert!(matches!(
            recovered.fatal_errors[0],
            ParseError::MissingRequiredAttribute { .. }
        ));

        let ParseResult::Document(doc) = recovered.result else {
            panic!("expected partial document");
        };
        assert_eq!(doc.components.len(), 1);
        assert_eq!(doc.components[0].name, "AcceptanceCriteria");
        assert_eq!(doc.components[0].children.len(), 2);
        assert_eq!(doc.components[0].children[0].name, "Criterion");
        assert_eq!(
            doc.components[0].children[1]
                .attributes
                .get("id")
                .map(String::as_str),
            Some("ok-1")
        );
    }
}
