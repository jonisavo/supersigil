//! Finds a checkout's Claude Code transcripts by scanning its project directory.
//!
//! Searches `~/.claude/projects/<encoded checkout>/` for main transcripts and
//! `<session id>/subagents/` for subagent transcripts. Use this when no hook
//! supplied a `transcript_path`, such as when importing older sessions.

use std::path::{Path, PathBuf};

/// Converts a checkout path to a Claude Code project directory name.
///
/// Replaces every character outside ASCII letters and digits with `-`.
/// For example, `/work/my.repo` becomes `-work-my-repo`. Uses the supplied
/// path without resolving symlinks or making it absolute.
#[must_use]
pub fn encode_project_dir(checkout: &Path) -> String {
    checkout
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Lists the main and subagent transcripts for `checkout`.
///
/// `claude_home` is normally `~/.claude`. Searches its `projects` directory
/// using [`encode_project_dir`] on the supplied checkout path. Returns main
/// `.jsonl` paths first, then `<session id>/subagents/agent-*.jsonl` paths,
/// with each group sorted by path. Ignores other files and directories.
/// Returns an empty vector if the project directory is missing.
///
/// # Errors
///
/// Returns an I/O error if an existing project or subagent directory, or an
/// entry needed for the scan, cannot be read.
pub fn discover_transcripts(checkout: &Path, claude_home: &Path) -> std::io::Result<Vec<PathBuf>> {
    let dir = claude_home
        .join("projects")
        .join(encode_project_dir(checkout));
    let Some(entries) = read_dir_if_present(&dir)? else {
        return Ok(Vec::new());
    };
    let mut main = Vec::new();
    let mut subagents = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            subagents.extend(subagent_transcripts(&path.join("subagents"))?);
        } else if is_jsonl(&path) {
            main.push(path);
        }
    }
    main.sort();
    subagents.sort();
    main.append(&mut subagents);
    Ok(main)
}

/// Lists `agent-*.jsonl` paths in `dir`, or returns an empty vector if it is missing.
///
/// # Errors
///
/// Returns an I/O error if the directory or one of its entries cannot be read.
fn subagent_transcripts(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let Some(entries) = read_dir_if_present(dir)? else {
        return Ok(Vec::new());
    };
    let mut found = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let is_agent = path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("agent-"));
        if is_agent && is_jsonl(&path) {
            found.push(path);
        }
    }
    Ok(found)
}

/// Opens a directory for reading, returning `None` if it does not exist.
///
/// # Errors
///
/// Returns any error other than [`std::io::ErrorKind::NotFound`].
fn read_dir_if_present(dir: &Path) -> std::io::Result<Option<std::fs::ReadDir>> {
    match std::fs::read_dir(dir) {
        Ok(entries) => Ok(Some(entries)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn is_jsonl(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "jsonl")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_every_non_alphanumeric_byte_as_a_dash() {
        assert_eq!(
            encode_project_dir(Path::new("/home/joni/.local/src/supersigil")),
            "-home-joni--local-src-supersigil"
        );
        assert_eq!(encode_project_dir(Path::new("/work/repo")), "-work-repo");
        assert_eq!(
            encode_project_dir(Path::new(r"C:\Users\me\proj")),
            "C--Users-me-proj"
        );
        assert_eq!(
            encode_project_dir(Path::new("/tmp/my repo")),
            "-tmp-my-repo"
        );
    }

    #[test]
    fn discovers_sorted_jsonl_files_for_the_checkout_only() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("projects").join("-work-repo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.jsonl"), "").unwrap();
        std::fs::write(dir.join("a.jsonl"), "").unwrap();
        // A main transcript beside its session directory: main transcripts
        // come before subagent ones, although `s1` sorts before `s1.jsonl`
        // as a path.
        std::fs::write(dir.join("s1.jsonl"), "").unwrap();
        std::fs::write(dir.join("notes.txt"), "").unwrap();
        let subagents = dir.join("s1").join("subagents");
        std::fs::create_dir_all(&subagents).unwrap();
        std::fs::write(subagents.join("agent-x.jsonl"), "").unwrap();
        std::fs::write(subagents.join("agent-x.meta.json"), "").unwrap();
        let tool_results = dir.join("s1").join("tool-results");
        std::fs::create_dir_all(&tool_results).unwrap();
        std::fs::write(tool_results.join("foo.txt"), "").unwrap();
        let other = home.path().join("projects").join("-work-other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("c.jsonl"), "").unwrap();

        let found = discover_transcripts(Path::new("/work/repo"), home.path()).unwrap();
        assert_eq!(
            found,
            vec![
                dir.join("a.jsonl"),
                dir.join("b.jsonl"),
                dir.join("s1.jsonl"),
                subagents.join("agent-x.jsonl"),
            ]
        );
        assert!(
            discover_transcripts(Path::new("/nowhere"), home.path())
                .unwrap()
                .is_empty()
        );
    }
}
