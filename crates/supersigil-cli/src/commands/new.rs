use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;

use supersigil_core::load_config;

use crate::commands::{BUILTIN_DOC_TYPES, NewArgs};
use crate::error::CliError;
use crate::format::{self, ColorConfig, Token};
use crate::loader;

/// Run the `new` command: scaffold a new spec document.
///
/// # Errors
///
/// Returns `CliError` if config loading or file writing fails.
pub fn run(args: &NewArgs, config_path: &Path, color: ColorConfig) -> Result<(), CliError> {
    let config = load_config(config_path).map_err(CliError::Config)?;

    // Validate doc_type exists in config or is a built-in type
    let is_known = BUILTIN_DOC_TYPES.contains(&args.doc_type.as_str())
        || config.documents.types.contains_key(&args.doc_type);

    if !is_known {
        let custom_types: Vec<&str> = config.documents.types.keys().map(String::as_str).collect();
        let all_types: Vec<&str> = BUILTIN_DOC_TYPES
            .iter()
            .copied()
            .chain(custom_types)
            .collect();
        eprintln!(
            "{} unknown document type '{}'. Known types: {}",
            color.paint(Token::Warning, "warning:"),
            args.doc_type,
            all_types.join(", ")
        );
    }

    let type_short = type_short_name(&args.doc_type);

    // Convention: ID = {feature}/{type_short}, path = specs/{feature}/{feature}.{type_short}.mdx
    let doc_id = format!("{}/{type_short}", args.id);
    let output_path = format!("specs/{}/{}.{type_short}.mdx", args.id, args.id);
    let output = Path::new(&output_path);

    // Ensure parent dir exists
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Check if a requirements file exists for this feature (used by design template)
    let project_root = loader::project_root(config_path);
    let req_path = project_root.join(format!("specs/{}/{}.req.mdx", args.id, args.id));
    let req_exists = req_path.is_file();

    let content = generate_template(&args.doc_type, &doc_id, &args.id, req_exists);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                CliError::CommandFailed(format!("{output_path} already exists"))
            } else {
                CliError::Io(e)
            }
        })?;
    file.write_all(content.as_bytes())?;

    let stdout = io::stdout();
    let mut out = stdout.lock();
    writeln!(
        out,
        "{} {}",
        color.paint(Token::Success, "Created"),
        color.paint(Token::Path, &output_path),
    )?;
    format::hint(color, "Run `supersigil lint` to validate the new document.");

    Ok(())
}

/// Map full type name to short name used in file conventions.
fn type_short_name(doc_type: &str) -> &str {
    match doc_type {
        "requirements" => "req",
        other => other,
    }
}

fn generate_template(doc_type: &str, id: &str, feature: &str, req_exists: bool) -> String {
    let status = "draft";

    let frontmatter = format!(
        r#"---
supersigil:
  id: {id}
  type: {doc_type}
  status: {status}
title: ""
---
"#
    );

    match doc_type {
        "requirements" => format!(
            r#"{frontmatter}
## Introduction

{{/* What problem does this feature solve? What is in scope and out of scope? */}}

## Definitions

{{/* Domain terms used in the requirements below. Use bold for the term name. */}}

- **Term**: Definition.

## Requirement 1: Title

As a [role], I want [capability], so that [benefit].

<AcceptanceCriteria>
  <Criterion id="req-1-1">
    WHEN [precondition], THE [component] SHALL [behavior].
  </Criterion>
</AcceptanceCriteria>
"#
        ),
        "design" => {
            let implements_line = if req_exists {
                format!(r#"<Implements refs="{feature}/req" />"#)
            } else {
                r#"{/* <Implements refs="" /> */}"#.to_owned()
            };
            format!(
                r#"{frontmatter}
{implements_line}

{{/* <DependsOn refs="" /> */}}
{{/* <TrackedFiles paths="" /> */}}

## Overview

{{/* High-level summary of the design approach. */}}

## Architecture

{{/* System structure, data flow, crate/module boundaries. Mermaid diagrams encouraged. */}}

## Key Types

{{/* Core data structures and their relationships. Rust type sketches encouraged. */}}

## Error Handling

{{/* Error types, failure modes, recovery strategies. */}}

## Testing Strategy

{{/* How correctness will be verified: property tests, unit tests, integration tests. */}}

## Alternatives Considered

{{/* Approaches that were evaluated and rejected, with rationale. */}}
"#
            )
        }
        "tasks" => format!(
            r#"{frontmatter}
## Overview

{{/* Brief description of the implementation sequence and approach. */}}

<Task id="task-1" status="draft">
  {{/* Describe the task. Use implements="{feature}/req#req-1-1" to link to criteria. */}}

  {{/* Subtasks are optional:
  <Task id="task-1-1" status="draft" implements="">
    Subtask description.
  </Task>

  <Task id="task-1-2" status="draft" depends="task-1-1">
    Subtask that depends on task-1-1.
  </Task>
  */}}
</Task>
"#
        ),
        _ => format!("{frontmatter}\n"),
    }
}
