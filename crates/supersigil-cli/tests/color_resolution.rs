//! Integration tests for terminal color resolution.

mod common;

use common::supersigil_cmd;
use supersigil_rust::verifies;
use tempfile::TempDir;

/// Helper: set up a minimal project that produces styled terminal output.
fn setup_styled_project(tmp: &std::path::Path) {
    common::setup_project(tmp);
    common::write_spec(tmp, "a", "doc/a", "requirements", "draft");
}

/// `--color always` forces ANSI escape codes in output even without a TTY.
#[verifies("cli-runtime/req#req-3-1")]
#[test]
fn color_always_flag_emits_ansi_codes() {
    let tmp = TempDir::new().unwrap();
    setup_styled_project(tmp.path());

    let output = supersigil_cmd()
        .args(["status", "--color", "always"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\x1b["),
        "stdout should contain ANSI escape codes with --color always, got: {stdout}"
    );
}

/// `--color never` suppresses ANSI escape codes regardless of env vars.
#[verifies("cli-runtime/req#req-3-1")]
#[test]
fn color_never_flag_suppresses_ansi_codes() {
    let tmp = TempDir::new().unwrap();
    setup_styled_project(tmp.path());

    let output = supersigil_cmd()
        .args(["status", "--color", "never"])
        .env("FORCE_COLOR", "1")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("\x1b["),
        "stdout should not contain ANSI escape codes with --color never, got: {stdout}"
    );
}

/// `FORCE_COLOR` env var enables ANSI codes when `--color auto` (default).
/// In a test harness stdout is not a TTY, so without `FORCE_COLOR` auto would
/// resolve to no-color. With `FORCE_COLOR`, color should be forced on.
#[verifies("cli-runtime/req#req-3-2")]
#[test]
fn force_color_env_enables_ansi_codes() {
    let tmp = TempDir::new().unwrap();
    setup_styled_project(tmp.path());

    let output = supersigil_cmd()
        .args(["status"])
        .env("FORCE_COLOR", "1")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\x1b["),
        "stdout should contain ANSI escape codes with FORCE_COLOR=1, got: {stdout}"
    );
}

/// `NO_COLOR` env var suppresses ANSI codes when `--color auto` (default).
#[verifies("cli-runtime/req#req-3-2")]
#[test]
fn no_color_env_suppresses_ansi_codes() {
    let tmp = TempDir::new().unwrap();
    setup_styled_project(tmp.path());

    let output = supersigil_cmd()
        .args(["status"])
        .env("NO_COLOR", "1")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("\x1b["),
        "stdout should not contain ANSI escape codes with NO_COLOR=1, got: {stdout}"
    );
}

/// `FORCE_COLOR` takes precedence over `NO_COLOR` when both are set.
#[verifies("cli-runtime/req#req-3-2")]
#[test]
fn force_color_beats_no_color() {
    let tmp = TempDir::new().unwrap();
    setup_styled_project(tmp.path());

    let output = supersigil_cmd()
        .args(["status"])
        .env("FORCE_COLOR", "1")
        .env("NO_COLOR", "1")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\x1b["),
        "FORCE_COLOR should take precedence over NO_COLOR, got: {stdout}"
    );
}

/// `--color never` takes precedence over `FORCE_COLOR` env var.
#[verifies("cli-runtime/req#req-3-1")]
#[test]
fn color_flag_beats_force_color_env() {
    let tmp = TempDir::new().unwrap();
    setup_styled_project(tmp.path());

    let output = supersigil_cmd()
        .args(["status", "--color", "never"])
        .env("FORCE_COLOR", "1")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("\x1b["),
        "--color never should override FORCE_COLOR, got: {stdout}"
    );
}
