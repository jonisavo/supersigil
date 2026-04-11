use std::fs::OpenOptions;
use std::io::{self, IsTerminal, Write};
use std::path::Path;

use crate::commands::InitArgs;
use crate::error::CliError;
use crate::format::{ColorConfig, Token};
use crate::prompt;
use crate::skills::{self, DEFAULT_SKILLS_PATH};

const DEFAULT_CONFIG: &str = r#"paths = ["specs/**/*.md"]

# Ecosystem plugins discover test evidence from language-native annotations.
# [ecosystem]
# plugins = ["rust", "js"]

# Override default severity for specific verification rules.
# [verify.rules]
# missing_verification_evidence = "warning"   # Downgrade from error
# stale_tracked_files = "off"                 # Disable staleness checks
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
                print_skill_chooser(color);
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

    print_next_steps(color);

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

fn print_next_steps(color: ColorConfig) {
    let err = io::stderr();
    let mut w = err.lock();
    let _ = writeln!(w);
    let _ = writeln!(w, "{}", color.paint(Token::Hint, "Next steps:"));
    let _ = writeln!(
        w,
        "  1. supersigil new requirements <feature>   Create your first spec"
    );
    let _ = writeln!(
        w,
        "  2. Edit the generated file                 Add criteria"
    );
    let _ = writeln!(
        w,
        "  3. supersigil verify                       Check everything"
    );
}

fn print_skill_chooser(color: ColorConfig) {
    let err = io::stderr();
    let mut w = err.lock();
    let _ = writeln!(w);
    let _ = writeln!(
        w,
        "  Build or fix with existing specs  -> {}",
        color.paint(Token::DocId, "feature-development")
    );
    let _ = writeln!(
        w,
        "  Write or repair specs             -> {}",
        color.paint(Token::DocId, "feature-specification")
    );
    let _ = writeln!(
        w,
        "  Existing code, no specs           -> {}",
        color.paint(Token::DocId, "retroactive-specification")
    );
    let _ = writeln!(
        w,
        "  Behavior-preserving cleanup       -> {}",
        color.paint(Token::DocId, "refactoring")
    );
    let _ = writeln!(
        w,
        "  CI / PR verification              -> {}",
        color.paint(Token::DocId, "ci-review")
    );
    let _ = writeln!(
        w,
        "  Full guided flow                  -> {}",
        color.paint(Token::DocId, "spec-driven-development")
    );
}
