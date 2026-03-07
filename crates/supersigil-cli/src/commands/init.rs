use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;

use crate::error::CliError;
use crate::format::{self, ColorConfig, Token};

const DEFAULT_CONFIG: &str = r#"paths = ["specs/**/*.mdx"]
"#;

/// Run the `init` command: create a minimal `supersigil.toml`.
///
/// # Errors
///
/// Returns `CliError::Io` on file system errors.
pub fn run(color: ColorConfig) -> Result<(), CliError> {
    let config_path = Path::new("supersigil.toml");

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(config_path)
        .map_err(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                CliError::CommandFailed("supersigil.toml already exists".into())
            } else {
                CliError::Io(e)
            }
        })?;
    file.write_all(DEFAULT_CONFIG.as_bytes())?;

    let stdout = io::stdout();
    let mut out = stdout.lock();
    writeln!(
        out,
        "{} {}",
        color.paint(Token::Success, "Created"),
        color.paint(Token::Path, "supersigil.toml"),
    )?;
    format::hint(
        color,
        "Add spec files under specs/ and run `supersigil lint` to validate them.",
    );

    Ok(())
}
