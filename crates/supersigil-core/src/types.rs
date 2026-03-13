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
/// Offsets from `markdown-rs` are relative to the MDX body; the
/// `byte_offset` here is adjusted by the front matter byte length so
/// that it refers to the original file content (after BOM stripping).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SourcePosition {
    pub byte_offset: usize,
    pub line: usize,
    pub column: usize,
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
}

// ---------------------------------------------------------------------------
// ExtractedComponent
// ---------------------------------------------------------------------------

/// A single MDX component extracted from the AST, with its name, attributes,
/// children, optional body text, and source position.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExtractedComponent {
    pub name: String,
    pub attributes: HashMap<String, String>,
    pub children: Vec<ExtractedComponent>,
    /// Trimmed concatenation of non-component text nodes.
    /// `None` if self-closing or no text content.
    pub body_text: Option<String>,
    /// Fenced code blocks extracted from the component body.
    pub code_blocks: Vec<CodeBlock>,
    pub position: SourcePosition,
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
