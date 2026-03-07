use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;

use supersigil_core::load_config;

use crate::commands::{BUILTIN_DOC_TYPES, NewArgs};
use crate::error::CliError;
use crate::format::{self, ColorConfig, Token};

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

    let content = generate_template(&args.doc_type, &doc_id);
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
        "requirement" => "req",
        other => other,
    }
}

fn generate_template(doc_type: &str, id: &str) -> String {
    let status = "draft";

    let mut content = format!(
        r#"---
supersigil:
  id: {id}
  type: {doc_type}
  status: {status}
title: ""
---

"#
    );

    // Add type-appropriate placeholder components.
    // - Use MDX comments ({/* */}) not HTML comments (<!-- -->)
    // - Never emit empty refs="" (causes BrokenRef graph error)
    // - Include all required attributes for each component
    match doc_type {
        "requirement" => {
            content.push_str(
                r#"<AcceptanceCriteria>
  <Criterion id="req-1">
    {/* Describe the acceptance criterion */}
  </Criterion>
</AcceptanceCriteria>
"#,
            );
        }
        "tasks" => {
            content.push_str(
                r#"<Task id="task-1-1" status="draft">
  {/* Describe the task */}
</Task>
"#,
            );
        }
        _ => {}
    }

    content
}
