//! Finds Claude Code transcripts for a checkout without a hook.
//!
//! Claude Code stores transcripts under `~/.claude/projects/<encoded cwd>/`,
//! where the encoding replaces path separators and dots with dashes. Hooks
//! hand over `transcript_path` directly; this module is the fallback for
//! sessions recorded before the hook existed.

use std::path::{Path, PathBuf};

/// Encodes a checkout path the way Claude Code names its project directories.
#[must_use]
pub fn encode_project_dir(checkout: &Path) -> String {
    checkout
        .to_string_lossy()
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c == '.' {
                '-'
            } else {
                c
            }
        })
        .collect()
}

/// Sorted `.jsonl` transcripts for `checkout` under `claude_home`
/// (normally `~/.claude`). Empty when the project directory does not exist.
///
/// # Errors
///
/// Returns the I/O error if the project directory exists but cannot be read,
/// or if one of its entries cannot be read.
pub fn discover_transcripts(checkout: &Path, claude_home: &Path) -> std::io::Result<Vec<PathBuf>> {
    let dir = claude_home
        .join("projects")
        .join(encode_project_dir(checkout));
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut found = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "jsonl") {
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_separators_and_dots_as_dashes() {
        assert_eq!(
            encode_project_dir(Path::new("/home/joni/.local/src/supersigil")),
            "-home-joni--local-src-supersigil"
        );
        assert_eq!(encode_project_dir(Path::new("/work/repo")), "-work-repo");
    }

    #[test]
    fn discovers_sorted_jsonl_files_for_the_checkout_only() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("projects").join("-work-repo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.jsonl"), "").unwrap();
        std::fs::write(dir.join("a.jsonl"), "").unwrap();
        std::fs::write(dir.join("notes.txt"), "").unwrap();
        let other = home.path().join("projects").join("-work-other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("c.jsonl"), "").unwrap();

        let found = discover_transcripts(Path::new("/work/repo"), home.path()).unwrap();
        assert_eq!(found, vec![dir.join("a.jsonl"), dir.join("b.jsonl")]);
        assert!(
            discover_transcripts(Path::new("/nowhere"), home.path())
                .unwrap()
                .is_empty()
        );
    }
}
