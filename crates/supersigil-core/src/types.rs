//! Core data model types for supersigil spec documents.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ParseWarning;

// ---------------------------------------------------------------------------
// Frontmatter
// ---------------------------------------------------------------------------

/// Parsed `supersigil:` namespace from YAML front matter.
/// The YAML `type` field maps to `doc_type` (reserved keyword in Rust).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frontmatter {
    pub id: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub doc_type: Option<String>,
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
    pub byte_offset: usize,
    pub line: usize,
    pub column: usize,
}

// ---------------------------------------------------------------------------
// SpanKind
// ---------------------------------------------------------------------------

/// Indicates where a code block's content span points in the source file.
///
/// This is relevant for snapshot rewriting: [`XmlInline`](Self::XmlInline)
/// spans require XML entity escaping when replacing content, while
/// [`RefFence`](Self::RefFence) spans are written verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    /// Content comes from inline XML body text (may contain entity-encoded chars).
    XmlInline,
    /// Content comes from a `supersigil-ref` fenced code block (verbatim).
    RefFence,
}

// ---------------------------------------------------------------------------
// CodeBlock
// ---------------------------------------------------------------------------

/// A fenced code block extracted from component body content.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CodeBlock {
    pub lang: Option<String>,
    pub content: String,
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
    pub name: String,
    pub attributes: HashMap<String, String>,
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
    pub path: PathBuf,
    pub frontmatter: Frontmatter,
    /// All YAML keys outside the `supersigil:` namespace, preserved as-is.
    pub extra: HashMap<String, yaml_serde::Value>,
    pub components: Vec<ExtractedComponent>,
    /// Non-fatal parse warnings (e.g. orphan code refs, duplicate code refs).
    ///
    /// These issues do not prevent the document from being loaded into the
    /// graph but should be surfaced by linting and LSP diagnostics.
    #[serde(skip)]
    pub warnings: Vec<ParseWarning>,
}

// ---------------------------------------------------------------------------
// ParseResult
// ---------------------------------------------------------------------------

/// The return type of the parser. Either a valid supersigil document or a
/// signal that the file is not a supersigil spec.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseResult {
    Document(SpecDocument),
    NotSupersigil(PathBuf),
}
