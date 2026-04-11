//! `supersigil skills` subcommand group.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::commands::{SkillsArgs, SkillsCommand};
use crate::error::CliError;
use crate::format::{ColorConfig, Token};
use crate::skills::{self, DEFAULT_SKILLS_PATH};

/// Run the `skills` subcommand group.
///
/// # Errors
///
/// Returns `CliError` on failures.
pub fn run(args: &SkillsArgs, color: ColorConfig) -> Result<(), CliError> {
    match args.command {
        SkillsCommand::Install(ref install_args) => run_install(install_args, color),
    }
}

fn run_install(
    args: &crate::commands::SkillsInstallArgs,
    color: ColorConfig,
) -> Result<(), CliError> {
    let dir = resolve_skills_dir(args.path.as_deref());
    let count = skills::write_skills(&dir)?;

    let stdout = io::stdout();
    let mut out = stdout.lock();
    writeln!(
        out,
        "{} to {}",
        color.paint(Token::Success, &format!("Installed {count} skills")),
        color.paint(Token::Path, &dir.display().to_string()),
    )?;

    skills::print_chooser(color);

    Ok(())
}

/// Minimal config struct to extract just the skills path without coupling to
/// the full `Config` schema.
#[derive(Deserialize)]
struct SkillsOnlyConfig {
    #[serde(default)]
    skills: SkillsSection,
}

#[derive(Default, Deserialize)]
struct SkillsSection {
    path: Option<String>,
}

/// Resolve the skills directory: `--path` flag > config > default.
fn resolve_skills_dir(flag_path: Option<&Path>) -> PathBuf {
    if let Some(path) = flag_path {
        return path.to_path_buf();
    }

    if let Ok(content) = std::fs::read_to_string(supersigil_core::CONFIG_FILENAME)
        && let Ok(config) = toml::from_str::<SkillsOnlyConfig>(&content)
        && let Some(path) = config.skills.path
    {
        return PathBuf::from(path);
    }

    PathBuf::from(DEFAULT_SKILLS_PATH)
}
