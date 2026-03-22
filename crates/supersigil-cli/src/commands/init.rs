use std::fs::OpenOptions;
use std::io::{self, IsTerminal, Write};
use std::path::Path;

use crate::commands::InitArgs;
use crate::error::CliError;
use crate::format::{self, ColorConfig, Token};
use crate::prompt;
use crate::skills::{self, DEFAULT_SKILLS_PATH};

const DEFAULT_CONFIG: &str = r#"paths = ["specs/**/*.md"]
"#;

/// Resolved skills installation decision.
enum SkillsResolution {
    /// Do not install skills.
    Skip,
    /// Install skills to the default path (`.agents/skills`).
    InstallDefault,
    /// Install skills to a custom path that should be persisted to config.
    InstallAt(String),
}

/// Run the `init` command: create a minimal `supersigil.toml` and optionally
/// install agent skills.
///
/// # Errors
///
/// Returns `CliError` on file system or I/O errors.
pub fn run(args: &InitArgs, color: ColorConfig) -> Result<(), CliError> {
    let config_path = Path::new(supersigil_core::CONFIG_FILENAME);

    let is_tty = io::stdin().is_terminal();
    let non_interactive = args.yes || !is_tty;

    let resolution = resolve_skills(args, non_interactive)?;

    // Build config content
    let config_content = match resolution {
        SkillsResolution::InstallAt(ref dir) => {
            format!("{DEFAULT_CONFIG}\n[skills]\npath = \"{dir}\"\n")
        }
        _ => DEFAULT_CONFIG.to_owned(),
    };

    // Write config file
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
    file.write_all(config_content.as_bytes())?;

    let stdout = io::stdout();
    let mut out = stdout.lock();
    writeln!(
        out,
        "{} {}",
        color.paint(Token::Success, "Created"),
        color.paint(Token::Path, "supersigil.toml"),
    )?;

    // Install skills
    let skills_dir = match resolution {
        SkillsResolution::Skip => None,
        SkillsResolution::InstallDefault => Some(DEFAULT_SKILLS_PATH),
        SkillsResolution::InstallAt(ref dir) => Some(dir.as_str()),
    };

    if let Some(dir) = skills_dir {
        match skills::write_skills(Path::new(dir)) {
            Ok(count) => {
                writeln!(
                    out,
                    "{} to {}",
                    color.paint(Token::Success, &format!("Installed {count} skills")),
                    color.paint(Token::Path, dir),
                )?;
            }
            Err(e) => {
                let _ = writeln!(
                    io::stderr(),
                    "{} failed to install skills: {e}",
                    color.warn(),
                );
            }
        }
    }

    format::hint(
        color,
        "Run `supersigil new <type> <name>` to create spec documents, then `supersigil lint` to validate them.",
    );

    Ok(())
}

fn resolve_skills(args: &InitArgs, non_interactive: bool) -> Result<SkillsResolution, CliError> {
    if args.no_skills {
        return Ok(SkillsResolution::Skip);
    }

    if let Some(ref path) = args.skills_path {
        return Ok(SkillsResolution::InstallAt(path.display().to_string()));
    }

    if args.skills {
        if non_interactive {
            return Ok(SkillsResolution::InstallDefault);
        }
        return prompt_for_path();
    }

    if non_interactive {
        return Ok(SkillsResolution::InstallDefault);
    }

    // Interactive: prompt for both
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stderr = io::stderr();
    let mut output = stderr.lock();

    let install = prompt::confirm(&mut input, &mut output, "Install agent skills?", true, true)?;

    if !install {
        return Ok(SkillsResolution::Skip);
    }

    let path = prompt::input_with_default(
        &mut input,
        &mut output,
        "Skills directory",
        DEFAULT_SKILLS_PATH,
        true,
    )?;

    Ok(as_resolution(path))
}

fn prompt_for_path() -> Result<SkillsResolution, CliError> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stderr = io::stderr();
    let mut output = stderr.lock();

    let path = prompt::input_with_default(
        &mut input,
        &mut output,
        "Skills directory",
        DEFAULT_SKILLS_PATH,
        true,
    )?;

    Ok(as_resolution(path))
}

fn as_resolution(path: String) -> SkillsResolution {
    if path == DEFAULT_SKILLS_PATH {
        SkillsResolution::InstallDefault
    } else {
        SkillsResolution::InstallAt(path)
    }
}
