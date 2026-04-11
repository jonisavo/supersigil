//! Embedded agent skills and write logic.

use std::io::{self, Write};
use std::path::Path;

use include_dir::{Dir, include_dir};

use crate::format::{ColorConfig, Token};

/// Default directory for installed skills.
pub const DEFAULT_SKILLS_PATH: &str = ".agents/skills";

/// The supersigil agent skills, embedded at compile time.
const SKILL_DIRS: &[(&str, Dir<'_>)] = &[
    (
        "ss-ci-review",
        include_dir!("$CARGO_MANIFEST_DIR/skills/ss-ci-review"),
    ),
    (
        "ss-feature-development",
        include_dir!("$CARGO_MANIFEST_DIR/skills/ss-feature-development"),
    ),
    (
        "ss-feature-specification",
        include_dir!("$CARGO_MANIFEST_DIR/skills/ss-feature-specification"),
    ),
    (
        "ss-refactoring",
        include_dir!("$CARGO_MANIFEST_DIR/skills/ss-refactoring"),
    ),
    (
        "ss-retroactive-specification",
        include_dir!("$CARGO_MANIFEST_DIR/skills/ss-retroactive-specification"),
    ),
    (
        "ss-spec-driven-development",
        include_dir!("$CARGO_MANIFEST_DIR/skills/ss-spec-driven-development"),
    ),
];

/// Write all embedded skills to `dir`, creating directories as needed.
///
/// Returns the number of skills written.
///
/// # Errors
///
/// Returns `io::Error` on file system failures.
pub fn write_skills(dir: &Path) -> io::Result<usize> {
    for (name, embedded) in SKILL_DIRS {
        write_dir_recursive(&dir.join(name), embedded)?;
    }
    Ok(SKILL_DIRS.len())
}

/// Print the skill chooser guide to stderr.
pub fn print_chooser(color: ColorConfig) {
    let err = io::stderr();
    let mut w = err.lock();
    let _ = writeln!(w);
    let _ = writeln!(
        w,
        "  Build or fix with existing specs  -> {}",
        color.paint(Token::DocId, "ss-feature-development")
    );
    let _ = writeln!(
        w,
        "  Write or repair specs             -> {}",
        color.paint(Token::DocId, "ss-feature-specification")
    );
    let _ = writeln!(
        w,
        "  Existing code, no specs           -> {}",
        color.paint(Token::DocId, "ss-retroactive-specification")
    );
    let _ = writeln!(
        w,
        "  Behavior-preserving cleanup       -> {}",
        color.paint(Token::DocId, "ss-refactoring")
    );
    let _ = writeln!(
        w,
        "  CI / PR verification              -> {}",
        color.paint(Token::DocId, "ss-ci-review")
    );
    let _ = writeln!(
        w,
        "  Full guided flow                  -> {}",
        color.paint(Token::DocId, "ss-spec-driven-development")
    );
}

fn write_dir_recursive(target: &Path, dir: &Dir<'_>) -> io::Result<()> {
    std::fs::create_dir_all(target)?;

    for file in dir.files() {
        let file_path = target.join(file.path().file_name().unwrap_or(file.path().as_os_str()));
        std::fs::write(&file_path, file.contents())?;
    }

    for subdir in dir.dirs() {
        let subdir_name = subdir
            .path()
            .file_name()
            .unwrap_or(subdir.path().as_os_str());
        write_dir_recursive(&target.join(subdir_name), subdir)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use supersigil_rust::verifies;
    use tempfile::TempDir;

    #[verifies("skills-install/req#req-1-1")]
    #[test]
    fn write_skills_creates_expected_structure() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".agents/skills");
        let count = write_skills(&dir).unwrap();

        assert_eq!(count, 6, "should write 6 skills");
        assert!(dir.join("ss-ci-review/SKILL.md").exists());
        assert!(dir.join("ss-feature-development/SKILL.md").exists());
        assert!(dir.join("ss-feature-specification/SKILL.md").exists());
        assert!(dir.join("ss-refactoring/SKILL.md").exists());
        assert!(dir.join("ss-retroactive-specification/SKILL.md").exists());
        assert!(dir.join("ss-spec-driven-development/SKILL.md").exists());
    }

    #[verifies("skills-install/req#req-1-1")]
    #[test]
    fn write_skills_includes_companion_files() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("skills");
        write_skills(&dir).unwrap();

        assert!(
            dir.join("ss-feature-development/references/implementation-loop.md")
                .exists()
        );
        assert!(
            dir.join("ss-feature-specification/references/templates.md")
                .exists()
        );
        assert!(
            dir.join("ss-feature-development/agents/openai.yaml")
                .exists()
        );
    }

    #[test]
    fn write_skills_overwrites_existing() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("skills");

        write_skills(&dir).unwrap();
        let original =
            std::fs::read_to_string(dir.join("ss-feature-development/SKILL.md")).unwrap();

        std::fs::write(dir.join("ss-feature-development/SKILL.md"), "tampered").unwrap();

        write_skills(&dir).unwrap();
        let restored =
            std::fs::read_to_string(dir.join("ss-feature-development/SKILL.md")).unwrap();

        assert_eq!(original, restored);
        assert_ne!(restored, "tampered");
    }
}
