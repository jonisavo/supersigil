//! Helpers shared by the CLI's integration tests. Each test binary uses a
//! different subset.
#![allow(dead_code, reason = "each test binary uses only some helpers")]

use std::path::{Path, PathBuf};
use std::process::Command;

use supersigil_session::checkout::canonical;
use supersigil_session::discover::encode_project_dir;

pub mod fixture;

/// The `supersigil` binary under test, outside any agent session and with
/// the reviews directory left to the test: the suite itself often runs
/// inside Claude Code.
pub fn supersigil_cmd() -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin("supersigil"));
    for key in [
        "CLAUDECODE",
        "CLAUDE_CODE_SESSION_ID",
        "SUPERSIGIL_REVIEW_DIR",
    ] {
        command.env_remove(key);
    }
    command
}

/// A fresh temporary directory and its canonical path, the spelling records
/// store.
pub fn canonical_tempdir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = canonical(dir.path()).unwrap();
    (dir, root)
}

/// Writes `text` as `<name>` in the Claude Code project directory of `cwd`
/// under `home`, and returns its path. `name` may hold subdirectories.
pub fn project_transcript(home: &Path, cwd: &Path, name: &str, text: &str) -> PathBuf {
    let path = home
        .join("projects")
        .join(encode_project_dir(cwd))
        .join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
    path
}
