//! Error types for the parser pipeline and config loader.

use std::path::PathBuf;

use crate::SourcePosition;

// ---------------------------------------------------------------------------
// ParseError
// ---------------------------------------------------------------------------

/// Errors produced by the parser pipeline.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// An I/O error occurred while reading a spec file.
    #[error("{path}: {source}")]
    IoError {
        /// Path to the file that caused the error.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Front matter block was opened but never closed.
    #[error("{path}: unclosed front matter (missing closing `---`)")]
    UnclosedFrontMatter {
        /// Path to the file with unclosed front matter.
        path: PathBuf,
    },
    /// YAML front matter failed to parse.
    #[error("{path}: invalid YAML: {message}")]
    InvalidYaml {
        /// Path to the file with invalid YAML.
        path: PathBuf,
        /// Description of the YAML error.
        message: String,
    },
    /// The required `id` field is missing from supersigil front matter.
    #[error("{path}: missing required `id` field in supersigil front matter")]
    MissingId {
        /// Path to the file missing an ID.
        path: PathBuf,
    },
    /// XML syntax error in a supersigil code block.
    #[error("{path}:{line}:{column}: XML syntax error: {message}")]
    XmlSyntaxError {
        /// Path to the file containing the error.
        path: PathBuf,
        /// Line number of the error.
        line: usize,
        /// Column number of the error.
        column: usize,
        /// Description of the XML syntax error.
        message: String,
    },
    /// A required attribute is missing on a component.
    #[error(
        "{path}:{}:{}: missing required attribute `{attribute}` on `<{component}>`",
        position.line, position.column
    )]
    MissingRequiredAttribute {
        /// Path to the file containing the component.
        path: PathBuf,
        /// The component name that is missing the attribute.
        component: String,
        /// The name of the missing required attribute.
        attribute: String,
        /// Source position of the component.
        position: SourcePosition,
    },
}

// ---------------------------------------------------------------------------
// ConfigError
// ---------------------------------------------------------------------------

/// Errors produced by the config loader.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// An I/O error occurred while reading the config file.
    #[error("{path}: {source}")]
    IoError {
        /// Path to the config file.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The TOML content failed to parse.
    #[error("{}: {message}", path.display())]
    TomlSyntax {
        /// Path to the config file.
        path: PathBuf,
        /// Description of the TOML syntax error.
        message: String,
    },
    /// Mutually exclusive config keys were both present.
    #[error("{}: keys are mutually exclusive: {}", path.display(), keys.join(", "))]
    MutualExclusivity {
        /// Path to the config file.
        path: PathBuf,
        /// The conflicting key names.
        keys: Vec<String>,
    },
    /// A required config entry is missing.
    #[error("{}: missing required config: {message}", path.display())]
    MissingRequired {
        /// Path to the config file.
        path: PathBuf,
        /// Description of what is missing.
        message: String,
    },
    /// An unrecognized verification rule name was used.
    #[error("{}: unknown verification rule: `{rule}`{}", path.display(), format_suggestion(.suggestion.as_deref()))]
    UnknownRule {
        /// Path to the config file.
        path: PathBuf,
        /// The unrecognized rule name.
        rule: String,
        /// A similar known rule name, if one exists.
        suggestion: Option<String>,
    },
    /// The `id_pattern` regex failed to compile.
    #[error("{}: invalid id_pattern `{pattern}`: {message}", path.display())]
    InvalidIdPattern {
        /// Path to the config file.
        path: PathBuf,
        /// The invalid regex pattern.
        pattern: String,
        /// Description of the regex compilation error.
        message: String,
    },
    /// An unrecognized ecosystem plugin name was used.
    #[error("{}: unknown ecosystem plugin: `{plugin}`{}", path.display(), format_suggestion(.suggestion.as_deref()))]
    UnknownPlugin {
        /// Path to the config file.
        path: PathBuf,
        /// The unrecognized plugin name.
        plugin: String,
        /// A similar known plugin name, if one exists.
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
    /// A component is marked verifiable but not referenceable.
    #[error("component `{component}` is verifiable but not referenceable")]
    VerifiableNotReferenceable {
        /// The component name.
        component: String,
    },
    /// A verifiable component lacks a required `id` attribute.
    #[error("component `{component}` is verifiable but has no required `id` attribute")]
    VerifiableMissingId {
        /// The component name.
        component: String,
    },
}

// ---------------------------------------------------------------------------
// ListSplitError
// ---------------------------------------------------------------------------

/// Error returned by `split_list_attribute` when splitting produces empty items.
#[derive(Debug, thiserror::Error)]
#[error("invalid list value `{raw}`: {message}")]
pub struct ListSplitError {
    /// The raw input string that failed to split.
    pub raw: String,
    /// Description of why splitting failed.
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
