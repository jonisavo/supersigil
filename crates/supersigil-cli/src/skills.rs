//! Embedded agent skills and write logic.

use std::io;
use std::path::Path;

use include_dir::{Dir, include_dir};

/// Default directory for installed skills.
pub const DEFAULT_SKILLS_PATH: &str = ".agents/skills";

/// The four supersigil agent skills, embedded at compile time.
const SKILL_DIRS: &[(&str, Dir<'_>)] = &[
    (
        "feature-development",
        include_dir!("$CARGO_MANIFEST_DIR/../../.agents/skills/feature-development"),
    ),
    (
        "feature-specification",
        include_dir!("$CARGO_MANIFEST_DIR/../../.agents/skills/feature-specification"),
    ),
    (
        "retroactive-specification",
        include_dir!("$CARGO_MANIFEST_DIR/../../.agents/skills/retroactive-specification"),
    ),
    (
        "spec-driven-development",
        include_dir!("$CARGO_MANIFEST_DIR/../../.agents/skills/spec-driven-development"),
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
    use tempfile::TempDir;

    #[test]
    fn write_skills_creates_expected_structure() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".agents/skills");
        let count = write_skills(&dir).unwrap();

        assert_eq!(count, 4, "should write 4 skills");
        assert!(dir.join("feature-development/SKILL.md").exists());
        assert!(dir.join("feature-specification/SKILL.md").exists());
        assert!(dir.join("retroactive-specification/SKILL.md").exists());
        assert!(dir.join("spec-driven-development/SKILL.md").exists());
    }

    #[test]
    fn write_skills_includes_companion_files() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("skills");
        write_skills(&dir).unwrap();

        assert!(
            dir.join("feature-development/references/implementation-loop.md")
                .exists()
        );
        assert!(
            dir.join("feature-specification/references/templates.md")
                .exists()
        );
        assert!(dir.join("feature-development/agents/openai.yaml").exists());
    }

    #[test]
    fn write_skills_overwrites_existing() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("skills");

        write_skills(&dir).unwrap();
        let original = std::fs::read_to_string(dir.join("feature-development/SKILL.md")).unwrap();

        std::fs::write(dir.join("feature-development/SKILL.md"), "tampered").unwrap();

        write_skills(&dir).unwrap();
        let restored = std::fs::read_to_string(dir.join("feature-development/SKILL.md")).unwrap();

        assert_eq!(original, restored);
        assert_ne!(restored, "tampered");
    }
}
