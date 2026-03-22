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
    #[error("{path}:{line}:{column}: XML syntax error: {message}")]
    XmlSyntaxError {
        path: PathBuf,
        line: usize,
        column: usize,
        message: String,
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
    #[error("{path}: orphan supersigil-ref `{target}` (no matching component)")]
    OrphanCodeRef {
        path: PathBuf,
        target: String,
        content_offset: usize,
    },
    #[error("{path}: duplicate supersigil-ref fences targeting `{target}`")]
    DuplicateCodeRef { path: PathBuf, target: String },
    #[error(
        "{path}: dual-source conflict for `{target}` (both inline text and supersigil-ref fence)"
    )]
    DualSourceConflict {
        path: PathBuf,
        target: String,
        content_offset: usize,
    },
}

impl TryFrom<ParseError> for ParseWarning {
    type Error = ParseError;

    /// Convert a code-ref `ParseError` into a [`ParseWarning`].
    ///
    /// Returns `Err(original)` if the error is not a code-ref warning variant.
    fn try_from(err: ParseError) -> Result<Self, Self::Error> {
        match err {
            ParseError::OrphanCodeRef {
                path,
                target,
                content_offset,
            } => Ok(ParseWarning::OrphanCodeRef {
                path,
                target,
                content_offset,
            }),
            ParseError::DuplicateCodeRef { path, target } => {
                Ok(ParseWarning::DuplicateCodeRef { path, target })
            }
            ParseError::DualSourceConflict {
                path,
                target,
                content_offset,
            } => Ok(ParseWarning::DualSourceConflict {
                path,
                target,
                content_offset,
            }),
            other => Err(other),
        }
    }
}

// ---------------------------------------------------------------------------
// ParseWarning
// ---------------------------------------------------------------------------

/// Non-fatal warnings produced by the code-ref resolution stage.
///
/// These are structurally identical to their `ParseError` counterparts but
/// stored separately on `SpecDocument` to indicate they do not prevent
/// graph construction.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseWarning {
    #[error("{path}: orphan supersigil-ref `{target}` (no matching component)")]
    OrphanCodeRef {
        path: PathBuf,
        target: String,
        content_offset: usize,
    },
    #[error("{path}: duplicate supersigil-ref fences targeting `{target}`")]
    DuplicateCodeRef { path: PathBuf, target: String },
    #[error(
        "{path}: dual-source conflict for `{target}` (both inline text and supersigil-ref fence)"
    )]
    DualSourceConflict {
        path: PathBuf,
        target: String,
        content_offset: usize,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orphan_code_ref_converts_to_warning() {
        let err = ParseError::OrphanCodeRef {
            path: PathBuf::from("test.md"),
            target: "t".into(),
            content_offset: 0,
        };
        ParseWarning::try_from(err).expect("should convert to warning");
    }

    #[test]
    fn duplicate_code_ref_converts_to_warning() {
        let err = ParseError::DuplicateCodeRef {
            path: PathBuf::from("test.md"),
            target: "t".into(),
        };
        ParseWarning::try_from(err).expect("should convert to warning");
    }

    #[test]
    fn dual_source_conflict_converts_to_warning() {
        let err = ParseError::DualSourceConflict {
            path: PathBuf::from("test.md"),
            target: "t".into(),
            content_offset: 0,
        };
        ParseWarning::try_from(err).expect("should convert to warning");
    }

    #[test]
    fn xml_syntax_error_does_not_convert_to_warning() {
        let err = ParseError::XmlSyntaxError {
            path: PathBuf::from("test.md"),
            line: 1,
            column: 1,
            message: "err".into(),
        };
        ParseWarning::try_from(err).expect_err("should not convert to warning");
    }

    #[test]
    fn missing_required_attribute_does_not_convert_to_warning() {
        let err = ParseError::MissingRequiredAttribute {
            path: PathBuf::from("test.md"),
            component: "Criterion".into(),
            attribute: "id".into(),
            position: crate::SourcePosition {
                byte_offset: 0,
                line: 1,
                column: 1,
            },
        };
        ParseWarning::try_from(err).expect_err("should not convert to warning");
    }

    #[test]
    fn missing_id_does_not_convert_to_warning() {
        let err = ParseError::MissingId {
            path: PathBuf::from("test.md"),
        };
        ParseWarning::try_from(err).expect_err("should not convert to warning");
    }
}
