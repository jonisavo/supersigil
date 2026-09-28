use std::process::Command;

/// The `supersigil` binary under test.
pub fn supersigil_cmd() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin("supersigil"))
}
