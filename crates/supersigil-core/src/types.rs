//! Core data model types for supersigil spec documents.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Frontmatter
// ---------------------------------------------------------------------------

/// Parsed `supersigil:` namespace from YAML front matter.
/// The YAML `type` field maps to `doc_type` (reserved keyword in Rust).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frontmatter {
    /// The document identifier.
    pub id: String,
    /// The document type (e.g. `"requirements"`, `"design"`).
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub doc_type: Option<String>,
    /// The document status (e.g. `"draft"`, `"approved"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

// ---------------------------------------------------------------------------
// SourcePosition
// ---------------------------------------------------------------------------

/// File-relative source position for editor integration.
///
/// Offsets from `markdown-rs` are relative to the Markdown body; the
/// `byte_offset` here is adjusted by the front matter byte length so
/// that it refers to the original file content (after BOM stripping).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SourcePosition {
    /// Byte offset from the start of the file.
    pub byte_offset: usize,
    /// One-based line number.
    pub line: usize,
    /// One-based column number.
    pub column: usize,
}

// ---------------------------------------------------------------------------
// SpanKind
// ---------------------------------------------------------------------------

/// Indicates where a code block's content span points in the source file.
///
/// This is relevant for snapshot rewriting: [`XmlInline`](Self::XmlInline)
/// spans require XML entity escaping when replacing content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    /// Content comes from inline XML body text (may contain entity-encoded chars).
    XmlInline,
}

// ---------------------------------------------------------------------------
// CodeBlock
// ---------------------------------------------------------------------------

/// A fenced code block extracted from component body content.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CodeBlock {
    /// The language tag of the fenced code block, if present.
    pub lang: Option<String>,
    /// The text content of the code block.
    pub content: String,
    /// Byte offset of the content start in the raw source file.
    pub content_offset: usize,
    /// Byte offset of the end of the content in the raw source file.
    ///
    /// For ref fences this equals `content_offset + content.len()` because
    /// content is stored verbatim. For inline fallback from XML body text,
    /// this is the raw source end offset (which can differ from
    /// `content_offset + content.len()` when entity references like `&lt;`
    /// are decoded).
    pub content_end_offset: usize,
    /// Whether this code block came from an inline XML body or a ref fence.
    pub span_kind: SpanKind,
}

// ---------------------------------------------------------------------------
// ExtractedComponent
// ---------------------------------------------------------------------------

/// A single component extracted from the AST, with its name, attributes,
/// children, optional body text, and source position.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExtractedComponent {
    /// The component tag name (e.g. `"Criterion"`, `"Task"`).
    pub name: String,
    /// Attribute key-value pairs from the component tag.
    pub attributes: HashMap<String, String>,
    /// Nested child components.
    pub children: Vec<ExtractedComponent>,
    /// Trimmed concatenation of non-component text nodes.
    /// `None` if self-closing or no text content.
    pub body_text: Option<String>,
    /// Byte offset of the body text start in the normalized source file.
    /// `None` if there is no body text.
    pub body_text_offset: Option<usize>,
    /// Byte offset of the body text end in the raw source file.
    /// `None` if there is no body text.
    /// This can differ from `body_text_offset + body_text.len()` when the
    /// raw source contains entity references (e.g. `&lt;`) that are decoded.
    pub body_text_end_offset: Option<usize>,
    /// Fenced code blocks extracted from the component body.
    pub code_blocks: Vec<CodeBlock>,
    /// Source position of the opening tag of this component.
    pub position: SourcePosition,
    /// Source position of the end of this component (past the closing `>`).
    pub end_position: SourcePosition,
}

// ---------------------------------------------------------------------------
// SpecDocument
// ---------------------------------------------------------------------------

/// A parsed supersigil document containing front matter, extra metadata,
/// and extracted components.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpecDocument {
    /// File path of the spec document.
    pub path: PathBuf,
    /// Parsed supersigil front matter.
    pub frontmatter: Frontmatter,
    /// All YAML keys outside the `supersigil:` namespace, preserved as-is.
    pub extra: HashMap<String, yaml_serde::Value>,
    /// Top-level extracted components from the document body.
    pub components: Vec<ExtractedComponent>,
}

// ---------------------------------------------------------------------------
// ParseResult
// ---------------------------------------------------------------------------

/// The return type of the parser. Either a valid supersigil document or a
/// signal that the file is not a supersigil spec.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseResult {
    /// The file was successfully parsed as a supersigil document.
    Document(SpecDocument),
    /// The file is not a supersigil spec (no supersigil front matter).
    NotSupersigil(PathBuf),
}
