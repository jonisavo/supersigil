//! Integration tests for the `supersigil-lsp --compatibility-info` preflight.

use std::process::Command;

use serde_json::Value;
use supersigil_rust_macros::verifies;

fn compatibility_info_output() -> (String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_supersigil-lsp"))
        .arg("--compatibility-info")
        .output()
        .expect("compatibility info command should run");

    assert!(
        output.status.success(),
        "expected success, got status {:?} with stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    (
        String::from_utf8(output.stdout).expect("stdout should be utf-8"),
        String::from_utf8(output.stderr).expect("stderr should be utf-8"),
    )
}

#[test]
#[verifies(
    "editor-server-compatibility/req#req-1-1",
    "editor-server-compatibility/req#req-1-2"
)]
fn compatibility_info_command_returns_json_with_versions() {
    let (stdout, stderr) = compatibility_info_output();
    let json: Value = serde_json::from_str(stdout.trim()).expect("stdout should be valid json");

    assert_eq!(json["compatibility_version"], 1);
    assert_eq!(json["server_version"], env!("CARGO_PKG_VERSION"));
    assert!(stderr.is_empty(), "stderr should be empty, got: {stderr}");
}

#[test]
#[verifies("editor-server-compatibility/req#req-1-3")]
fn compatibility_info_command_does_not_emit_lsp_transport_output() {
    let (stdout, _) = compatibility_info_output();

    assert!(
        !stdout.starts_with("Content-Length:"),
        "compatibility info should print JSON rather than LSP transport frames"
    );
}
