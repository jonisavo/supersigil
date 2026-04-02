use super::*;

// ---------------------------------------------------------------------------
// -j / --parallelism flag
// ---------------------------------------------------------------------------

#[verifies("executable-examples/req#req-6-5")]
#[test]
fn verify_parallelism_flags_are_accepted() {
    for args in [["-j", "2"], ["--parallelism", "1"]] {
        let tmp = TempDir::new().unwrap();
        setup_clean_example_fixture(tmp.path());

        let output = cargo_bin_cmd!("supersigil")
            .args(["verify", "--format", "json"])
            .args(args)
            .current_dir(tmp.path())
            .output()
            .unwrap();

        assert_eq!(
            output.status.code(),
            Some(0),
            "verify {} {} should succeed, stderr: {}",
            args[0],
            args[1],
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

#[verifies("executable-examples/req#req-6-5")]
#[test]
fn verify_parallelism_flag_overrides_config() {
    let tmp = TempDir::new().unwrap();
    write_config(
        tmp.path(),
        r#"paths = ["specs/**/*.md"]
[examples]
parallelism = 8
"#,
    );
    common::write_spec_doc(
        tmp.path(),
        "specs/demo.md",
        "demo/req",
        Some("requirements"),
        Some("approved"),
        r#"```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="d-1">demo</Criterion>
</AcceptanceCriteria>

<Example id="par-test" lang="sh" runner="sh" verifies="demo/req#d-1">
  <Expected status="0" contains="ok" />
</Example>
```

```sh supersigil-ref=par-test
echo ok
```"#,
    );

    // -j 1 forces sequential even though config says 8
    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json", "-j", "1"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "verify -j 1 overriding config parallelism=8 should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

// ---------------------------------------------------------------------------
// Post-verify hooks receive interim report with example findings
// ---------------------------------------------------------------------------

#[verifies("executable-examples/req#req-4-6")]
#[test]
fn verify_hooks_receive_interim_report_with_example_results() {
    let tmp = TempDir::new().unwrap();

    // Write a hook script that reads the interim report from stdin and
    // echoes the number of findings back as a hook finding.
    let hook_script = tmp.path().join("check-hook.sh");
    fs::write(
        &hook_script,
        r#"#!/bin/sh
# Read stdin (interim report JSON), count findings, emit hook finding
REPORT=$(cat)
COUNT=$(echo "$REPORT" | grep -o '"rule"' | wc -l)
echo "[[\"info\", \"hook saw $COUNT finding rules\"]]"
"#,
    )
    .unwrap();
    #[cfg(unix)]
    #[allow(
        clippy::semicolon_outside_block,
        reason = "conflicts with semicolon_if_nothing_returned"
    )]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook_script, fs::Permissions::from_mode(0o755)).unwrap();
    }

    // Config with the hook and an example
    fs::write(
        tmp.path().join("supersigil.toml"),
        format!(
            r#"paths = ["specs/**/*.md"]

[hooks]
post_verify = ["{hook}"]
"#,
            hook = hook_script.to_string_lossy()
        ),
    )
    .unwrap();
    fs::create_dir_all(tmp.path().join("specs")).unwrap();

    common::write_spec_doc(
        tmp.path(),
        "specs/hook-test.md",
        "hook-test/req",
        Some("requirements"),
        Some("approved"),
        r#"```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="h-1">hook test</Criterion>
</AcceptanceCriteria>

<Example id="hook-ex" lang="sh" runner="sh" verifies="hook-test/req#h-1">
  <Expected status="0" contains="hello" />
</Example>
```

```sh supersigil-ref=hook-ex
echo hello
```"#,
    );

    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json", "-j", "1"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "verify with hook should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    let findings = report["findings"]
        .as_array()
        .expect("findings should be an array");

    // The hook should have produced a finding mentioning the finding count
    let hook_findings: Vec<_> = findings
        .iter()
        .filter(|f| {
            f["message"]
                .as_str()
                .is_some_and(|m| m.contains("hook saw"))
        })
        .collect();

    assert!(
        !hook_findings.is_empty(),
        "hook should produce a finding with interim report data: {report}",
    );
}

/// Verify that an empty project (0 documents) produces a warning finding and
/// exits with code 2 (warnings only).
#[test]
fn verify_empty_project_warns() {
    let dir = TempDir::new().unwrap();
    write_config(dir.path(), "paths = [\"specs/**/*.md\"]\n");

    // JSON output: should contain the empty_project finding as a warning
    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(2),
        "expected exit code 2 (warnings only)"
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let findings = json["findings"].as_array().unwrap();
    assert!(
        findings.iter().any(|f| {
            f["rule"].as_str() == Some("empty_project")
                && f["effective_severity"].as_str() == Some("warning")
        }),
        "verify should include an empty_project warning, got: {findings:?}",
    );

    // Terminal output: should render the warning (not "Clean: no findings")
    let terminal_output = cargo_bin_cmd!("supersigil")
        .args(["verify"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&terminal_output.stdout);
    assert!(
        stdout.contains("no documents found"),
        "terminal output should show empty_project warning, got: {stdout}",
    );
    assert!(
        !stdout.contains("Clean: no findings"),
        "terminal should not say Clean when there are warnings, got: {stdout}",
    );
}

/// Verify that the binary does not panic (exit 101) when stdout is a broken pipe.
///
/// Agents commonly pipe through `head` or `2>&1 | head`, which closes the pipe
/// early. The binary should exit cleanly, not panic from a `BrokenPipe` error in
/// the error handler.
///
/// To trigger the bug, the JSON output must exceed the OS pipe buffer (`~64KB`),
/// so we generate many documents with evidence to produce a large report.
#[test]
fn broken_pipe_does_not_panic() {
    use std::fmt::Write;
    use std::process::{Command, Stdio};

    let dir = TempDir::new().unwrap();

    // Generate enough documents and evidence to produce >64KB of JSON output.
    let mut config = String::from("paths = [\"specs/**/*.md\"]\n");
    config.push_str("\n[ecosystem.rust]\n");
    fs::write(dir.path().join("supersigil.toml"), &config).unwrap();
    fs::create_dir_all(dir.path().join("specs")).unwrap();
    fs::create_dir_all(dir.path().join("tests")).unwrap();

    // Create 30 requirement documents, each with 5 criteria and file-glob evidence.
    for i in 0..30 {
        let feature = format!("feat-{i:03}");
        let feature_dir = dir.path().join("specs").join(&feature);
        fs::create_dir_all(&feature_dir).unwrap();

        let mut criteria = String::new();
        for j in 0..5 {
            write!(
                criteria,
                "  <Criterion id=\"ac-{j}\">\n    \
                 Acceptance criterion {j} for feature {i}\n    \
                 <VerifiedBy strategy=\"file-glob\" paths=\"tests/{feature}_test.rs\" />\n  \
                 </Criterion>\n"
            )
            .unwrap();
        }

        common::write_spec_doc(
            dir.path(),
            &format!("specs/{feature}/{feature}.md"),
            &format!("{feature}/req"),
            Some("requirements"),
            Some("approved"),
            &format!("<AcceptanceCriteria>\n{criteria}</AcceptanceCriteria>"),
        );

        // Create a matching test file so evidence is discovered
        fs::write(
            dir.path().join("tests").join(format!("{feature}_test.rs")),
            format!("fn test_{feature}() {{}}\n"),
        )
        .unwrap();
    }

    let bin = assert_cmd::cargo::cargo_bin("supersigil");
    let output = Command::new("bash")
        .arg("-c")
        .arg(format!(
            "{} verify --format json 2>&1 | head -1; exit ${{PIPESTATUS[0]}}",
            bin.display()
        ))
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to run pipeline");

    let code = output.status.code().unwrap_or(-1);

    assert_ne!(
        code, 101,
        "binary panicked (exit 101) on broken pipe — should exit cleanly"
    );
}

// -----------------------------------------------------------------------
// code_ref_conflict rule
// -----------------------------------------------------------------------

#[test]
fn verify_orphan_code_ref_emits_warning_and_exits_two() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    // Write a document with an orphan supersigil-ref fence (targets no component).
    // Using status: approved so the warning is not downgraded to info.
    let content = "\
---
supersigil:
  id: ref-test/doc
  status: approved
---

# A spec with an orphan ref

```sh supersigil-ref=nonexistent-example
echo hello
```
";
    fs::write(tmp.path().join("specs/orphan.md"), content).unwrap();

    cargo_bin_cmd!("supersigil")
        .args(["verify", "--skip-examples"])
        .current_dir(tmp.path())
        .assert()
        .code(2)
        .stdout(predicate::str::contains("code_ref_conflict"))
        .stdout(predicate::str::contains("orphan"));
}
