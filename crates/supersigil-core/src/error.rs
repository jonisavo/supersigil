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
    #[error("unknown verification rule: `{rule}`{}", format_suggestion(.suggestion.as_deref()))]
    UnknownRule {
        rule: String,
        suggestion: Option<String>,
    },
    #[error("invalid id_pattern `{pattern}`: {message}")]
    InvalidIdPattern { pattern: String, message: String },
    #[error("unknown ecosystem plugin: `{plugin}`{}", format_suggestion(.suggestion.as_deref()))]
    UnknownPlugin {
        plugin: String,
        suggestion: Option<String>,
    },
}

fn format_suggestion(suggestion: Option<&str>) -> String {
    match suggestion {
        Some(s) => format!("; did you mean `{s}`?"),
        None => String::new(),
    }
}

/// Find the closest match from `candidates` to `input` using Levenshtein distance.
///
/// Returns `Some(candidate)` if the best match has distance <= `threshold`.
#[must_use]
pub fn suggest_similar<'a>(
    input: &str,
    candidates: &[&'a str],
    threshold: usize,
) -> Option<&'a str> {
    candidates
        .iter()
        .map(|c| (*c, levenshtein(input, c)))
        .filter(|(_, d)| *d <= threshold)
        .min_by_key(|(_, d)| *d)
        .map(|(c, _)| c)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());

    let mut prev = (0..=n).collect::<Vec<_>>();
    let mut curr = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
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
    fn suggest_similar_finds_close_match() {
        assert_eq!(suggest_similar("rusts", &["rust", "js"], 2), Some("rust"));
        assert_eq!(suggest_similar("jss", &["rust", "js"], 2), Some("js"));
    }

    #[test]
    fn suggest_similar_returns_none_beyond_threshold() {
        assert_eq!(suggest_similar("python", &["rust", "js"], 2), None);
    }

    #[test]
    fn suggest_similar_exact_match() {
        assert_eq!(suggest_similar("rust", &["rust", "js"], 2), Some("rust"));
    }
}
