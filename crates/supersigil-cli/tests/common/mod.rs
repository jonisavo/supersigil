#![allow(
    dead_code,
    reason = "shared test helpers; not every test file uses every function"
)]

use std::process::Command;

const GIT_ENV_VARS_TO_CLEAR: [&str; 5] = [
    "GIT_COMMON_DIR",
    "GIT_DIR",
    "GIT_INDEX_FILE",
    "GIT_PREFIX",
    "GIT_WORK_TREE",
];

/// Clears git environment variables inherited from a hook or an editor so a
/// test runs against the repository it sets up.
pub fn sanitize_git_env(cmd: &mut Command) -> &mut Command {
    for name in GIT_ENV_VARS_TO_CLEAR {
        cmd.env_remove(name);
    }
    cmd
}

/// The `supersigil` binary under test with a clean git environment.
pub fn supersigil_cmd() -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin("supersigil"));
    sanitize_git_env(&mut cmd);
    cmd
}
