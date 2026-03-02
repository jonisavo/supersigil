//! Stage 1b–1c: Front matter extraction and deserialization.

use std::collections::HashMap;
use std::path::Path;

use supersigil_core::{Frontmatter, ParseError};

/// Result of deserializing YAML front matter.
#[derive(Debug, Clone, PartialEq)]
pub enum FrontMatterResult {
    /// The YAML contained a `supersigil:` key with a valid `Frontmatter`.
    Supersigil {
        frontmatter: Frontmatter,
        extra: HashMap<String, yaml_serde::Value>,
    },
    /// The YAML did not contain a `supersigil:` key.
    NotSupersigil,
}

/// Check whether a line is a `---` delimiter (optionally followed by trailing
/// whitespace).
fn is_delimiter(line: &str) -> bool {
    let trimmed = line.trim_end();
    trimmed == "---"
}

/// Detect and extract YAML front matter between `---` delimiters.
///
/// Returns `Ok(Some((yaml_str, body_str)))` when front matter is found,
/// `Ok(None)` when the file does not start with `---`.
///
/// # Errors
///
/// Returns `ParseError::UnclosedFrontMatter` when the opening `---` has no
/// matching closing delimiter.
pub fn extract_front_matter<'a>(
    content: &'a str,
    path: &Path,
) -> Result<Option<(&'a str, &'a str)>, ParseError> {
    // Check if the first line is a `---` delimiter.
    let first_newline = content.find('\n');
    let first_line = match first_newline {
        Some(pos) => &content[..pos],
        None => content,
    };

    if !is_delimiter(first_line) {
        return Ok(None);
    }

    // Start of YAML content is right after the first line + newline.
    let yaml_start = match first_newline {
        Some(pos) => pos + 1,
        // Content is just "---" with no newline — unclosed.
        None => {
            return Err(ParseError::UnclosedFrontMatter {
                path: path.to_path_buf(),
            });
        }
    };

    // Scan remaining lines for the closing `---` delimiter.
    let rest = &content[yaml_start..];
    let mut offset = 0;
    for line in rest.split('\n') {
        if is_delimiter(line) {
            let yaml = &content[yaml_start..yaml_start + offset];
            let body_start = yaml_start + offset + line.len();
            // Skip the newline after the closing delimiter if present.
            let body_start = if content.as_bytes().get(body_start) == Some(&b'\n') {
                body_start + 1
            } else {
                body_start
            };
            let body = &content[body_start..];
            return Ok(Some((yaml, body)));
        }
        // +1 for the \n that split consumed
        offset += line.len() + 1;
    }

    Err(ParseError::UnclosedFrontMatter {
        path: path.to_path_buf(),
    })
}

/// Deserialize YAML front matter, extracting the `supersigil:` namespace and
/// preserving extra metadata keys.
///
/// Returns `FrontMatterResult::Supersigil` when a `supersigil:` key is found,
/// `FrontMatterResult::NotSupersigil` when it is absent.
///
/// # Errors
///
/// Returns `ParseError::InvalidYaml` for malformed YAML, or
/// `ParseError::MissingId` when the `supersigil:` key is present but `id` is
/// missing.
pub fn deserialize_front_matter(yaml: &str, path: &Path) -> Result<FrontMatterResult, ParseError> {
    // Empty YAML → no supersigil key.
    if yaml.trim().is_empty() {
        return Ok(FrontMatterResult::NotSupersigil);
    }

    // Parse into a generic YAML mapping first.
    let mut mapping: HashMap<String, yaml_serde::Value> =
        yaml_serde::from_str(yaml).map_err(|e| ParseError::InvalidYaml {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

    // Remove the `supersigil:` key, moving the value out without cloning.
    let Some(supersigil_value) = mapping.remove("supersigil") else {
        return Ok(FrontMatterResult::NotSupersigil);
    };

    // Extract fields from the supersigil mapping structurally to detect
    // missing `id` without relying on brittle error-message string matching.
    let yaml_serde::Value::Mapping(supersigil_map) = supersigil_value else {
        return Err(ParseError::InvalidYaml {
            path: path.to_path_buf(),
            message: "supersigil value must be a mapping".to_owned(),
        });
    };

    let get_optional_string = |map: &yaml_serde::Mapping, key: &str| -> Option<String> {
        map.get(key)
            .and_then(yaml_serde::Value::as_str)
            .map(str::to_owned)
    };

    let id = get_optional_string(&supersigil_map, "id").ok_or_else(|| ParseError::MissingId {
        path: path.to_path_buf(),
    })?;

    let frontmatter = Frontmatter {
        id,
        doc_type: get_optional_string(&supersigil_map, "type"),
        status: get_optional_string(&supersigil_map, "status"),
    };

    // The remaining keys are all non-supersigil extra metadata.
    Ok(FrontMatterResult::Supersigil {
        frontmatter,
        extra: mapping,
    })
}
