use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;

use supersigil_core::scaffold::{
    BUILTIN_DOC_TYPES, generate_template, is_known_doc_type, type_short_name,
};
use supersigil_core::{glob_prefix, load_config};

use crate::commands::NewArgs;
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
    if !is_known_doc_type(&args.doc_type, &config) {
        let custom_types: Vec<&str> = config.documents.types.keys().map(String::as_str).collect();
        let all_types: Vec<&str> = BUILTIN_DOC_TYPES
            .iter()
            .copied()
            .chain(custom_types)
            .collect();
        return Err(CliError::CommandFailed(format!(
            "unknown document type '{}'. Known types: {}",
            args.doc_type,
            all_types.join(", ")
        )));
    }

    let type_short = type_short_name(&args.doc_type);

    // Determine the spec directory prefix based on project selection.
    let spec_dir = resolve_spec_dir(&config, args.project.as_deref())?;

    // Convention: ID = {feature}/{type_short}, path = {spec_dir}{feature}/{feature}.{type_short}.md
    let doc_id = format!("{}/{type_short}", args.id);
    let output_path = format!("{spec_dir}{}/{}.{type_short}.md", args.id, args.id);
    let output = Path::new(&output_path);

    // Ensure parent dir exists
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Check if a requirements file exists for this feature (used by design template)
    let project_root = loader::project_root(config_path);
    let req_path = project_root.join(format!("{}{}/{}.req.md", spec_dir, args.id, args.id));
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

/// Resolve the spec directory prefix for the new document.
///
/// - No `--project` in single-project mode: defaults to `specs/`.
/// - No `--project` in multi-project mode: error (must specify).
/// - `--project` in single-project mode: error.
/// - `--project` with unknown name: error.
/// - `--project` with known name: derives prefix from the project's first glob pattern.
fn resolve_spec_dir(
    config: &supersigil_core::Config,
    project: Option<&str>,
) -> Result<String, CliError> {
    let is_multi_project = config.projects.is_some();

    let Some(project_name) = project else {
        if is_multi_project {
            return Err(CliError::CommandFailed(
                "--project is required in multi-project mode".to_owned(),
            ));
        }
        return Ok("specs/".to_owned());
    };

    let projects = config.projects.as_ref().ok_or_else(|| {
        CliError::CommandFailed(
            "--project requires multi-project mode (use [projects] in supersigil.toml)".to_owned(),
        )
    })?;

    let project_config = projects.get(project_name).ok_or_else(|| {
        let available: Vec<&str> = projects.keys().map(String::as_str).collect();
        CliError::CommandFailed(format!(
            "unknown project '{}'. Available projects: {}",
            project_name,
            available.join(", ")
        ))
    })?;

    // Derive the spec directory from the first glob pattern.
    let prefix = project_config
        .paths
        .first()
        .map_or_else(|| "specs/".to_owned(), |p| glob_prefix(p));

    // Ensure prefix ends with a separator for path joining.
    if prefix.is_empty() {
        Ok("specs/".to_owned())
    } else {
        Ok(prefix)
    }
}
