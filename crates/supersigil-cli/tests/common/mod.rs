//! Helpers shared by the CLI's integration tests. Each test binary uses a
//! different subset.
#![allow(dead_code, reason = "each test binary uses only some helpers")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

use supersigil_session::checkout::canonical;
use supersigil_session::discover::encode_project_dir;

pub mod annotation;
pub mod fixture;
pub mod session;

/// Builds a command for the `supersigil` binary under test, clearing agent session
/// markers and the reviews directory override. Tests may run inside Claude Code
/// and choose their own reviews directory.
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

/// Runs `work` on another thread and returns its result, failing the test
/// when it takes longer than `limit` times [`time_limit_scale`]. A hang
/// becomes a failure instead of a stuck test run; the thread is left to the
/// process's exit.
pub fn finishes_within<T: Send + 'static>(
    limit: Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (done, result) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(work());
    });
    result
        .recv_timeout(limit * time_limit_scale())
        .expect("the work finishes within its time limit")
}

/// The whole number in `SUPERSIGIL_TIME_LIMIT_SCALE`, or 1 when it is unset,
/// so a slow CI runner can allow more time while others keep the limits.
///
/// # Panics
///
/// Panics if the variable is set to anything but a positive whole number.
fn time_limit_scale() -> u32 {
    std::env::var("SUPERSIGIL_TIME_LIMIT_SCALE").map_or(1, |scale| {
        scale
            .parse()
            .ok()
            .filter(|&scale| scale > 0)
            .expect("SUPERSIGIL_TIME_LIMIT_SCALE is a positive whole number")
    })
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
