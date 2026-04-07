mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use supersigil_rust::verifies;
use tempfile::TempDir;

#[verifies("work-queries/req#req-1-2")]
#[test]
fn plan_all_shows_outstanding_targets() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/req.md",
        "auth/req/login",
        Some("requirements"),
        Some("approved"),
        r#"# Login

<AcceptanceCriteria>
  <Criterion id="valid-creds">
    WHEN valid creds THEN return token.
  </Criterion>
</AcceptanceCriteria>
"#,
    );

    cargo_bin_cmd!("supersigil")
        .args(["plan"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("valid-creds"));
}

#[verifies("work-queries/req#req-1-2")]
#[test]
fn plan_exact_id() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/req.md",
        "auth/req/login",
        Some("requirements"),
        None,
        "# Login\n\n<AcceptanceCriteria>\n  <Criterion id=\"c1\">\n    Test criterion.\n  </Criterion>\n</AcceptanceCriteria>\n",
    );

    cargo_bin_cmd!("supersigil")
        .args(["plan", "auth/req/login"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("c1"));
}

#[verifies("work-queries/req#req-1-2")]
#[test]
fn plan_prefix_match() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/a.md",
        "auth/req/login",
        Some("requirements"),
        None,
        "# A\n\n<AcceptanceCriteria>\n  <Criterion id=\"c1\">\n    Test.\n  </Criterion>\n</AcceptanceCriteria>\n",
    );
    common::write_spec_doc(
        tmp.path(),
        "specs/b.md",
        "billing/req/pay",
        Some("requirements"),
        None,
        "# B\n\n<AcceptanceCriteria>\n  <Criterion id=\"c2\">\n    Test.\n  </Criterion>\n</AcceptanceCriteria>\n",
    );

    cargo_bin_cmd!("supersigil")
        .args(["plan", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("c1"))
        .stdout(predicate::str::contains("c2").not());
}

#[verifies("work-queries/req#req-1-2", "work-queries/req#req-1-3")]
#[test]
fn plan_no_match_exits_one() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(tmp.path(), "specs/req.md", "test/doc", None, None, "");

    cargo_bin_cmd!("supersigil")
        .args(["plan", "nonexistent"])
        .current_dir(tmp.path())
        .assert()
        .failure();
}

#[verifies("work-queries/req#req-4-1")]
#[test]
fn plan_shows_dependency_graph() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/req.md",
        "test/req",
        Some("requirements"),
        Some("approved"),
        r#"# Test

<AcceptanceCriteria>
  <Criterion id="c1">
    First criterion.
  </Criterion>
  <Criterion id="c2">
    Second criterion.
  </Criterion>
</AcceptanceCriteria>
"#,
    );
    common::write_spec_doc(
        tmp.path(),
        "specs/tasks.md",
        "test/tasks",
        Some("tasks"),
        None,
        r#"# Tasks

<Task id="task-1-1" implements="test/req#c1">
  First task.
</Task>
<Task id="task-1-2" implements="test/req#c2" depends="task-1-1">
  Second task depends on first.
</Task>
"#,
    );

    cargo_bin_cmd!("supersigil")
        .args(["plan", "test"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Dependency graph"))
        .stdout(predicate::str::contains("→"));
}

#[verifies("work-queries/req#req-4-1")]
#[test]
fn plan_default_shows_actionable_work() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/req.md",
        "test/req",
        Some("requirements"),
        Some("approved"),
        r#"# Test

<AcceptanceCriteria>
  <Criterion id="c1">
    First criterion.
  </Criterion>
  <Criterion id="c2">
    Second criterion.
  </Criterion>
</AcceptanceCriteria>
"#,
    );
    common::write_spec_doc(
        tmp.path(),
        "specs/tasks.md",
        "test/tasks",
        Some("tasks"),
        None,
        r#"# Tasks

<Task id="task-1-1" implements="test/req#c1">
  First task.
</Task>
<Task id="task-1-2" implements="test/req#c2" depends="task-1-1">
  Second task depends on first.
</Task>
"#,
    );

    // Default mode: only c1 is actionable (task-1-1 has no deps).
    // c2 is blocked (task-1-2 depends on task-1-1).
    cargo_bin_cmd!("supersigil")
        .args(["plan", "test"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Actionable work"))
        .stdout(predicate::str::contains("c1"))
        .stdout(predicate::str::contains("1 more targets blocked"));
}

#[verifies("work-queries/req#req-4-2")]
#[test]
fn plan_full_shows_all_targets_and_task_list() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/req.md",
        "test/req",
        Some("requirements"),
        Some("approved"),
        r#"# Test

<AcceptanceCriteria>
  <Criterion id="c1">
    First criterion.
  </Criterion>
  <Criterion id="c2">
    Second criterion.
  </Criterion>
</AcceptanceCriteria>
"#,
    );
    common::write_spec_doc(
        tmp.path(),
        "specs/tasks.md",
        "test/tasks",
        Some("tasks"),
        None,
        r#"# Tasks

<Task id="task-1-1" implements="test/req#c1">
  First task.
</Task>
<Task id="task-1-2" implements="test/req#c2" depends="task-1-1">
  Second task depends on first.
</Task>
"#,
    );

    // Full mode: all criteria shown + task list with implements refs.
    cargo_bin_cmd!("supersigil")
        .args(["plan", "test", "--full"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Outstanding work"))
        .stdout(predicate::str::contains("c1"))
        .stdout(predicate::str::contains("c2"))
        .stdout(predicate::str::contains("Pending tasks"))
        .stdout(predicate::str::contains("implements:"));
}

#[verifies("work-queries/req#req-3-2")]
#[test]
fn plan_json_format() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/req.md",
        "test/doc",
        Some("requirements"),
        None,
        "# Test\n",
    );

    let output = cargo_bin_cmd!("supersigil")
        .args(["plan", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    assert!(json.get("outstanding_targets").is_some());
}

/// When the Rust plugin is enabled but finds Rust files with zero test items,
/// the plugin failure warning must appear on stderr while stdout stays clean.
#[test]
fn plan_plugin_failure_warning_on_stderr() {
    let tmp = TempDir::new().unwrap();
    common::setup_project_with_rust_plugin(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/req.md",
        "test/req",
        Some("requirements"),
        None,
        "# Test\n\n<AcceptanceCriteria>\n  <Criterion id=\"c1\">\n    Test.\n  </Criterion>\n</AcceptanceCriteria>\n",
    );

    // Create a Rust source file with no test items to trigger the plugin failure.
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("src/lib.rs"), "pub fn hello() {}\n").unwrap();

    cargo_bin_cmd!("supersigil")
        .args(["plan"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("plugin"))
        .stderr(predicate::str::contains("zero supported Rust test items"));
}

/// With --format json, stdout must be valid JSON even when plugin warnings
/// are emitted on stderr.
#[verifies("cli-runtime/req#req-3-4")]
#[test]
fn plan_json_stdout_clean_despite_plugin_warning() {
    let tmp = TempDir::new().unwrap();
    common::setup_project_with_rust_plugin(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/req.md",
        "test/req",
        Some("requirements"),
        None,
        "# Test\n",
    );

    // Rust file with no tests triggers the plugin failure path.
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("src/lib.rs"), "pub fn helper() {}\n").unwrap();

    let output = cargo_bin_cmd!("supersigil")
        .args(["plan", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());

    // stderr has the plugin warning.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("zero supported Rust test items"),
        "stderr should contain plugin warning, got: {stderr}",
    );

    // stdout is valid JSON.
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    assert!(json.get("outstanding_targets").is_some());
}

/// Terminal plan prints "No outstanding work." when there are no targets, tasks, or completed tasks.
/// When completed tasks exist, a completed-task summary is appended.
#[verifies("work-queries/req#req-4-3")]
#[test]
fn plan_no_work_message_and_completed_summary() {
    // Case 1: No criteria, no tasks at all -> "No outstanding work."
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/req.md",
        "test/req",
        Some("requirements"),
        Some("draft"),
        "# Test\n",
    );

    cargo_bin_cmd!("supersigil")
        .args(["plan"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No outstanding work."));

    // Case 2: A done task exists -> completed-task summary is appended.
    let tmp2 = TempDir::new().unwrap();
    common::setup_project(tmp2.path());
    common::write_spec_doc(
        tmp2.path(),
        "specs/req.md",
        "test/req",
        Some("requirements"),
        Some("approved"),
        r#"# Test

<AcceptanceCriteria>
  <Criterion id="c1">
    Some criterion.
  </Criterion>
</AcceptanceCriteria>
"#,
    );
    common::write_spec_doc(
        tmp2.path(),
        "specs/tasks.md",
        "test/tasks",
        Some("tasks"),
        None,
        r#"# Tasks

<Task id="task-1" status="done" implements="test/req#c1">
  Completed task.
</Task>
"#,
    );

    let output = cargo_bin_cmd!("supersigil")
        .args(["plan", "test"])
        .current_dir(tmp2.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should have completed summary (the done task is present).
    assert!(
        stdout.contains("task-1"),
        "stdout should mention the completed task, got: {stdout}"
    );
}
