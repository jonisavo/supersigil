//! Error types for the parser pipeline and config loader.

use std::path::PathBuf;

use crate::SourcePosition;

// ---------------------------------------------------------------------------
// ParseError
// ---------------------------------------------------------------------------

/// Errors produced by the parser pipeline.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("{path}: {source}")]
    IoError {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: unclosed front matter (missing closing `---`)")]
    UnclosedFrontMatter { path: PathBuf },
    #[error("{path}: invalid YAML: {message}")]
    InvalidYaml { path: PathBuf, message: String },
    #[error("{path}: missing required `id` field in supersigil front matter")]
    MissingId { path: PathBuf },
    #[error("{path}:{line}:{column}: MDX syntax error: {message}")]
    MdxSyntaxError {
        path: PathBuf,
        line: usize,
        column: usize,
        message: String,
    },
    #[error(
        "{path}:{}:{}: expression attribute `{attribute}` on `<{component}>` (only string literals supported)",
        position.line, position.column
    )]
    ExpressionAttribute {
        path: PathBuf,
        component: String,
        attribute: String,
        position: SourcePosition,
    },
    #[error(
        "{path}:{}:{}: unknown component `<{component}>`",
        position.line, position.column
    )]
    UnknownComponent {
        path: PathBuf,
        component: String,
        position: SourcePosition,
    },
    #[error(
        "{path}:{}:{}: missing required attribute `{attribute}` on `<{component}>`",
        position.line, position.column
    )]
    MissingRequiredAttribute {
        path: PathBuf,
        component: String,
        attribute: String,
        position: SourcePosition,
    },
}

// ---------------------------------------------------------------------------
// ConfigError
// ---------------------------------------------------------------------------

/// Errors produced by the config loader.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{path}: {source}")]
    IoError {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("TOML syntax error: {message}")]
    TomlSyntax { message: String },
    #[error("keys are mutually exclusive: {}", keys.join(", "))]
    MutualExclusivity { keys: Vec<String> },
    #[error("missing required config: {message}")]
    MissingRequired { message: String },
    #[error("unknown verification rule: `{rule}`")]
    UnknownRule { rule: String },
    #[error("invalid id_pattern `{pattern}`: {message}")]
    InvalidIdPattern { pattern: String, message: String },
    #[error("unknown ecosystem plugin: `{plugin}`")]
    UnknownPlugin { plugin: String },
    #[error(
        "runner `{runner}` has invalid placeholder `{placeholder}` (valid: {{file}}, {{dir}}, {{lang}}, {{name}})"
    )]
    InvalidRunnerPlaceholder { runner: String, placeholder: String },
}

// ---------------------------------------------------------------------------
// ComponentDefError
// ---------------------------------------------------------------------------

/// Errors produced when validating component definitions.
#[derive(Debug, thiserror::Error)]
pub enum ComponentDefError {
    #[error("component `{component}` is verifiable but not referenceable")]
    VerifiableNotReferenceable { component: String },
    #[error("component `{component}` is verifiable but has no required `id` attribute")]
    VerifiableMissingId { component: String },
}

// ---------------------------------------------------------------------------
// ListSplitError
// ---------------------------------------------------------------------------

/// Error returned by `split_list_attribute` when splitting produces empty items.
#[derive(Debug, thiserror::Error)]
#[error("invalid list value `{raw}`: {message}")]
pub struct ListSplitError {
    pub raw: String,
    pub message: String,
}

/// Split a raw comma-separated attribute value into trimmed, non-empty items.
///
/// # Errors
///
/// Returns [`ListSplitError`] if any item is empty after trimming (trailing
/// comma, consecutive commas, or empty input).
pub fn split_list_attribute(raw: &str) -> Result<Vec<&str>, ListSplitError> {
    if raw.is_empty() {
        return Err(ListSplitError {
            raw: raw.to_owned(),
            message: "empty input".to_owned(),
        });
    }

    let mut items = Vec::new();
    for item in raw.split(',').map(str::trim) {
        if item.is_empty() {
            return Err(ListSplitError {
                raw: raw.to_owned(),
                message: "empty item after trimming (trailing comma, consecutive commas, or whitespace-only item)".to_owned(),
            });
        }
        items.push(item);
    }

    Ok(items)
}
