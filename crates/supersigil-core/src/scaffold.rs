//! Document scaffolding templates.
//!
//! Shared template generation used by both the CLI `new` command and the LSP
//! code actions.

use crate::Config;

/// Built-in document type names recognized by supersigil.
pub const BUILTIN_DOC_TYPES: &[&str] = &["requirements", "design", "tasks", "adr"];

/// Check whether a full document type name is known (built-in or custom).
#[must_use]
pub fn is_known_doc_type(doc_type: &str, config: &Config) -> bool {
    BUILTIN_DOC_TYPES.contains(&doc_type) || config.documents.types.contains_key(doc_type)
}

/// Map a full document type name to the short name used in file conventions.
///
/// `"requirements"` maps to `"req"`; all other types pass through unchanged.
#[must_use]
pub fn type_short_name(doc_type: &str) -> &str {
    match doc_type {
        "requirements" => "req",
        other => other,
    }
}

/// Map short type name back to full type name used in frontmatter.
#[must_use]
pub fn type_full_name(short: &str) -> &str {
    match short {
        "req" => "requirements",
        other => other,
    }
}

/// Convert a feature slug like `"user-auth"` to a title like `"User Auth"`.
///
/// The result is safe for embedding in a double-quoted YAML scalar: any `\`
/// or `"` characters in the slug are escaped.
fn slug_to_title(slug: &str) -> String {
    slug.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => {
                    let mut s = c.to_uppercase().to_string();
                    s.push_str(chars.as_str());
                    s
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

/// Generate the full template content for a new spec document.
///
/// The returned string includes YAML frontmatter and type-specific body
/// sections with placeholder comments.
///
/// # Arguments
///
/// * `doc_type` — full document type (e.g. `"requirements"`, `"design"`)
/// * `id` — the document ID (e.g. `"auth/req"`)
/// * `feature` — the feature slug (e.g. `"auth"`)
/// * `req_exists` — whether a sibling requirements doc exists (affects design
///   and ADR templates)
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "template literals dominate line count"
)]
pub fn generate_template(doc_type: &str, id: &str, feature: &str, req_exists: bool) -> String {
    let status = "draft";
    let title = slug_to_title(feature);

    let frontmatter = format!(
        r#"---
supersigil:
  id: {id}
  type: {doc_type}
  status: {status}
title: "{title}"
---
"#
    );

    match doc_type {
        "requirements" => format!(
            r#"{frontmatter}
## Introduction

<!-- What problem does this feature solve? What is in scope and out of scope? -->

## Definitions

<!-- Domain terms used in the requirements below. Use bold for the term name. -->

- **Term**: Definition.

## Requirement 1: Title

As a [role], I want [capability], so that [benefit].

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    WHEN [precondition], THE [component] SHALL [behavior].
  </Criterion>
</AcceptanceCriteria>
```

<!-- To link criteria to test evidence, nest a VerifiedBy inside the Criterion:

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-2">
    Criterion description.
    <VerifiedBy strategy="tag" tag="{feature}/req#req-1-2" />
  </Criterion>
</AcceptanceCriteria>
```
-->
"#
        ),
        "design" => {
            let implements_block = if req_exists {
                format!("```supersigil-xml\n<Implements refs=\"{feature}/req\" />\n```")
            } else {
                "<!-- ```supersigil-xml\n<Implements refs=\"\" />\n``` -->".to_owned()
            };
            format!(
                r#"{frontmatter}
{implements_block}

<!-- ```supersigil-xml
<DependsOn refs="" />
``` -->

<!-- ```supersigil-xml
<TrackedFiles paths="" />
``` -->

## Overview

<!-- High-level summary of the design approach. -->

## Architecture

<!-- System structure, data flow, crate/module boundaries. Mermaid diagrams encouraged. -->

## Key Types

<!-- Core data structures and their relationships. Rust type sketches encouraged. -->

## Error Handling

<!-- Error types, failure modes, recovery strategies. -->

## Testing Strategy

<!-- How correctness will be verified: property tests, unit tests, integration tests. -->

## Alternatives Considered

<!-- Approaches that were evaluated and rejected, with rationale. -->
"#
            )
        }
        "tasks" => format!(
            r#"{frontmatter}
## Overview

<!-- Brief description of the implementation sequence and approach. -->

```supersigil-xml
<Task id="task-1" status="draft">
  Describe the task. Use implements="{feature}/req#req-1-1" to link to criteria.
</Task>
```

<!-- Subtasks are optional — nest them inside the parent Task:

```supersigil-xml
<Task id="task-1" status="draft">
  <Task id="task-1-1" status="draft" implements="">
    Subtask description.
  </Task>
  <Task id="task-1-2" status="draft" depends="task-1-1">
    Subtask that depends on task-1-1.
  </Task>
</Task>
```
-->
"#
        ),
        "adr" => {
            let references_block = if req_exists {
                format!("```supersigil-xml\n<References refs=\"{feature}/req\" />\n```")
            } else {
                "<!-- ```supersigil-xml\n<References refs=\"\" />\n``` -->".to_owned()
            };
            format!(
                r#"{frontmatter}
{references_block}

## Context

<!-- What is the situation that motivates this decision? What forces are at play? -->

## Decision

<!-- What decision was made? State it clearly and directly. -->

```supersigil-xml
<Decision id="decision-1">
  One-line summary of the decision.

  <Rationale>
    Why was this decision made? What tradeoffs were accepted?
  </Rationale>
</Decision>
```

<!-- Add alternatives inside the Decision element:

```supersigil-xml
<Decision id="decision-1">
  ...
  <Alternative id="alt-1" status="rejected">
    Describe the alternative and why it was not chosen.
  </Alternative>
</Decision>
```
-->

## Consequences

<!-- What are the expected outcomes, positive and negative, of this decision? -->
"#
            )
        }
        _ => format!("{frontmatter}\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::{generate_template, type_full_name, type_short_name};

    #[test]
    fn type_short_name_maps_requirements() {
        assert_eq!(type_short_name("requirements"), "req");
    }

    #[test]
    fn type_short_name_passes_through_others() {
        assert_eq!(type_short_name("design"), "design");
        assert_eq!(type_short_name("tasks"), "tasks");
        assert_eq!(type_short_name("adr"), "adr");
    }

    #[test]
    fn type_full_name_maps_req() {
        assert_eq!(type_full_name("req"), "requirements");
    }

    #[test]
    fn type_full_name_passes_through_others() {
        assert_eq!(type_full_name("design"), "design");
        assert_eq!(type_full_name("tasks"), "tasks");
        assert_eq!(type_full_name("adr"), "adr");
    }

    #[test]
    fn title_derived_from_feature_slug() {
        let content = generate_template("requirements", "auth/req", "auth", false);
        assert!(
            content.contains("title: \"Auth\""),
            "should title-case single word, got:\n{content}",
        );

        let content = generate_template("design", "user-auth/design", "user-auth", false);
        assert!(
            content.contains("title: \"User Auth\""),
            "should title-case hyphenated slug, got:\n{content}",
        );
    }

    #[test]
    fn title_escapes_yaml_special_chars() {
        let content = generate_template("requirements", "test/req", "a\"b\\c", false);
        assert!(
            content.contains(r#"title: "A\"b\\c""#),
            "should escape quotes and backslashes, got:\n{content}",
        );
    }

    #[test]
    fn template_requirements() {
        let content = generate_template("requirements", "auth/req", "auth", false);
        assert!(content.starts_with("---\n"));
        assert!(content.contains("type: requirements"));
        assert!(content.contains("id: auth/req"));
        assert!(content.contains("<AcceptanceCriteria>"));
    }

    #[test]
    fn template_design_without_req() {
        let content = generate_template("design", "auth/design", "auth", false);
        assert!(content.contains("type: design"));
        assert!(content.contains("<!-- ```supersigil-xml\n<Implements refs=\"\" />\n``` -->"));
    }

    #[test]
    fn template_design_with_req() {
        let content = generate_template("design", "auth/design", "auth", true);
        assert!(content.contains("type: design"));
        assert!(content.contains("<Implements refs=\"auth/req\" />"));
    }

    #[test]
    fn template_tasks() {
        let content = generate_template("tasks", "auth/tasks", "auth", false);
        assert!(content.contains("type: tasks"));
        assert!(content.contains("<Task id=\"task-1\""));
    }

    #[test]
    fn template_adr_without_req() {
        let content = generate_template("adr", "auth/adr", "auth", false);
        assert!(content.contains("type: adr"));
        assert!(content.contains("<!-- ```supersigil-xml\n<References refs=\"\" />\n``` -->"));
    }

    #[test]
    fn template_adr_with_req() {
        let content = generate_template("adr", "auth/adr", "auth", true);
        assert!(content.contains("type: adr"));
        assert!(content.contains("<References refs=\"auth/req\" />"));
    }

    #[test]
    fn template_unknown_type() {
        let content = generate_template("custom", "auth/custom", "auth", false);
        assert!(content.contains("type: custom"));
        assert!(content.contains("id: auth/custom"));
        // Unknown types get just the frontmatter + blank line.
        assert!(content.ends_with("---\n\n"));
    }
}
