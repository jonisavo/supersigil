//! The binary starts, prints help and version, and generates completions.

mod common;

use assert_cmd::prelude::*;
use predicates::prelude::*;

#[test]
fn help_lists_completions() {
    common::supersigil_cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("completions"));
}

#[test]
fn version_matches_the_crate() {
    common::supersigil_cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn bash_completions_mention_the_binary() {
    common::supersigil_cmd()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("supersigil"));
}

#[test]
fn unknown_command_fails_with_usage() {
    common::supersigil_cmd()
        .arg("verify")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}
