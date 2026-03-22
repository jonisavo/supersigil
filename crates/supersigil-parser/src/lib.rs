//! Three-stage MDX parsing pipeline for supersigil spec documents.

mod extract;
mod frontmatter;
mod preprocess;

pub use extract::{extract_components, validate_components};
pub use frontmatter::{FrontMatterResult, deserialize_front_matter, extract_front_matter};
pub use preprocess::{normalize, preprocess};

use std::path::Path;

use markdown::mdast;
use supersigil_core::{ComponentDefs, ParseError, ParseResult, SpecDocument};

// ---------------------------------------------------------------------------
// Stage 2: MDX AST generation
// ---------------------------------------------------------------------------

/// Parse MDX body content into an AST using `markdown-rs` with MDX constructs
/// enabled.
///
/// # Errors
///
/// Returns `ParseError::MdxSyntaxError` if the body contains invalid MDX
/// syntax.
pub fn parse_mdx_body(body: &str, path: &Path) -> Result<mdast::Node, ParseError> {
    let options = markdown::ParseOptions {
        constructs: markdown::Constructs::mdx(),
        ..markdown::ParseOptions::default()
    };

    markdown::to_mdast(body, &options).map_err(|msg| {
        let (line, column) = msg
            .place
            .as_ref()
            .map_or((1, 1), |place| match place.as_ref() {
                markdown::message::Place::Point(pt) => (pt.line, pt.column),
                markdown::message::Place::Position(pos) => (pos.start.line, pos.start.column),
            });

        ParseError::MdxSyntaxError {
            path: path.to_path_buf(),
            line,
            column,
            message: msg.reason,
        }
    })
}

// ---------------------------------------------------------------------------
// parse_content — public API (Req 8-1)
// ---------------------------------------------------------------------------

/// Parse MDX content from an in-memory string into a [`ParseResult`].
///
/// This is the core of the parsing pipeline, operating on a content string
/// that has already been decoded and normalized (e.g. by the LSP buffer or
/// by [`parse_file`] after preprocessing). It performs:
///
/// 1. Front matter extraction and deserialization.
/// 2. MDX AST generation via `markdown-rs`.
/// 3. Component extraction + lint-time validation.
///
/// Stage 1 fatal errors prevent stages 2–3. Stage 2 errors prevent stage 3.
///
/// # Errors
///
/// Returns `Vec<ParseError>` containing all detected errors across stages.
pub fn parse_content(
    path: &Path,
    content: &str,
    component_defs: &ComponentDefs,
) -> Result<ParseResult, Vec<ParseError>> {
    // Stage 1: Extract front matter
    let (yaml, body) = match extract_front_matter(content, path) {
        Ok(Some((yaml, body))) => (yaml, body),
        Ok(None) => return Ok(ParseResult::NotSupersigil(path.to_path_buf())),
        Err(e) => return Err(vec![e]),
    };

    // Stage 1: Deserialize front matter
    let (frontmatter, extra) = match deserialize_front_matter(yaml, path) {
        Ok(FrontMatterResult::Supersigil { frontmatter, extra }) => (frontmatter, extra),
        Ok(FrontMatterResult::NotSupersigil) => {
            return Ok(ParseResult::NotSupersigil(path.to_path_buf()));
        }
        Err(e) => return Err(vec![e]),
    };

    // Compute body offset for source position adjustment.
    // body starts at content[body_offset..], so:
    let body_offset = content.len() - body.len();
    // Count frontmatter lines so component line numbers (which markdown-rs
    // reports relative to the body) can be adjusted to file-absolute.
    let frontmatter_lines = content[..body_offset].lines().count();

    // Stage 2: Parse MDX body
    let ast = parse_mdx_body(body, path).map_err(|e| {
        // Adjust body-relative line to file-absolute.
        vec![match e {
            ParseError::MdxSyntaxError {
                path,
                line,
                column,
                message,
            } => ParseError::MdxSyntaxError {
                path,
                line: line + frontmatter_lines,
                column,
                message,
            },
            other => other,
        }]
    })?;

    // Stage 3: Extract components
    let mut errors = Vec::new();
    let components = extract_components(
        &ast,
        body_offset,
        frontmatter_lines,
        path,
        &mut errors,
        component_defs,
    );

    // Stage 3: Lint-time validation
    validate_components(&components, component_defs, path, &mut errors);

    if errors.is_empty() {
        Ok(ParseResult::Document(SpecDocument {
            path: path.to_path_buf(),
            frontmatter,
            extra,
            components,
        }))
    } else {
        Err(errors)
    }
}

// ---------------------------------------------------------------------------
// parse_file — public API (Req 10)
// ---------------------------------------------------------------------------

/// Parse a single MDX file into a [`ParseResult`].
///
/// Implements the three-stage pipeline:
/// 1. Preprocess (UTF-8 decode, BOM strip, CRLF normalize) + front matter
///    extraction and deserialization.
/// 2. MDX AST generation via `markdown-rs`.
/// 3. Component extraction + lint-time validation.
///
/// Stage 1 fatal errors prevent stages 2–3. Stage 2 errors prevent stage 3.
/// Within each stage, all independent errors are collected.
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
