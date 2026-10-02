//! The test isolation itself: configuration a parent process passes through
//! the environment must not reach a test's git.

mod common;

use common::{TestRepo, isolated};
use supersigil_git::Git;

#[test]
fn isolation_overrides_configuration_passed_through_the_environment() {
    let repo = TestRepo::new();
    let leak = repo.dir.path().join("leak.cfg");
    std::fs::write(&leak, "[core]\n\tleak = global\n").unwrap();
    // What a parent environment could hold, set before the isolation as an
    // inherited variable would be.
    let inherited = Git::new(&repo.root)
        .with_env("GIT_CONFIG_GLOBAL", &leak)
        .with_env("GIT_CONFIG_COUNT", "1")
        .with_env("GIT_CONFIG_KEY_0", "core.leak")
        .with_env("GIT_CONFIG_VALUE_0", "count")
        .with_env("GIT_CONFIG_PARAMETERS", "'core.leak=parameters'");
    let leaked = inherited.raw(["config", "--get-all", "core.leak"]).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&leaked.stdout),
        "global\ncount\nparameters\n",
        "the variables must leak without the isolation, or this test proves nothing"
    );

    let git = isolated(inherited, &repo.home());
    let output = git.raw(["config", "--get-all", "core.leak"]).unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
}
