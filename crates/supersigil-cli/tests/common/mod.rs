//! Helpers shared by the CLI's integration tests. Each test binary uses a
//! different subset.
#![allow(dead_code, reason = "each test binary uses only some helpers")]

use std::path::{Path, PathBuf};
use std::process::Command;

use supersigil_session::checkout::canonical;
use supersigil_session::discover::encode_project_dir;

/// The `supersigil` binary under test.
pub fn supersigil_cmd() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin("supersigil"))
}

/// A fresh temporary directory and its canonical path, the spelling records
/// store.
pub fn canonical_tempdir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = canonical(dir.path()).unwrap();
    (dir, root)
}

/// Writes `text` as `<name>` in the Claude Code project directory of `cwd`
/// under `home`, and returns its path.
pub fn project_transcript(home: &Path, cwd: &Path, name: &str, text: &str) -> PathBuf {
    let dir = home.join("projects").join(encode_project_dir(cwd));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}
