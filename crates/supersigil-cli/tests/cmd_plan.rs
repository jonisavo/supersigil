//! Integration tests for the `plan` command.

mod common;

use assert_cmd::assert::OutputAssertExt;
use common::supersigil_cmd;
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

    supersigil_cmd()
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

    supersigil_cmd()
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

    supersigil_cmd()
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

    supersigil_cmd()
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

    supersigil_cmd()
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
    supersigil_cmd()
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
    supersigil_cmd()
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

    let output = supersigil_cmd()
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

    supersigil_cmd()
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

    let output = supersigil_cmd()
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

    supersigil_cmd()
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

    let output = supersigil_cmd()
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

#[verifies("work-queries/req#req-5-1", "work-queries/req#req-5-2")]
#[test]
fn plan_json_qualified_task_refs() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/req.md",
        "test/req",
        Some("requirements"),
        Some("approved"),
        r#"# Req

<AcceptanceCriteria>
  <Criterion id="c1">First criterion.</Criterion>
  <Criterion id="c2">Second criterion.</Criterion>
</AcceptanceCriteria>
"#,
    );
    common::write_spec_doc(
        tmp.path(),
        "specs/tasks-a.md",
        "test/tasks-a",
        Some("tasks"),
        None,
        r#"# Tasks A

<Task id="task-1" implements="test/req#c1">
  First task in doc A.
</Task>
"#,
    );
    common::write_spec_doc(
        tmp.path(),
        "specs/tasks-b.md",
        "test/tasks-b",
        Some("tasks"),
        None,
        r#"# Tasks B

<Task id="task-1" implements="test/req#c2" depends="task-2">
  First task in doc B, depends on task-2.
</Task>

<Task id="task-2" implements="test/req#c2">
  Second task in doc B.
</Task>
"#,
    );

    let output = supersigil_cmd()
        .args(["plan", "test/", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");

    // actionable_tasks and blocked_tasks should use qualified refs.
    let actionable = json["actionable_tasks"]
        .as_array()
        .expect("actionable_tasks should be array");
    let blocked = json["blocked_tasks"]
        .as_array()
        .expect("blocked_tasks should be array");

    // All entries should be qualified (contain '#').
    for entry in actionable.iter().chain(blocked.iter()) {
        let s = entry.as_str().expect("should be string");
        assert!(s.contains('#'), "expected qualified ref, got bare: {s}");
    }

    // test/tasks-a#task-1 should be actionable (no deps).
    assert!(
        actionable.iter().any(|e| e == "test/tasks-a#task-1"),
        "tasks-a#task-1 should be actionable: {actionable:?}"
    );

    // depends_on should also be qualified.
    let pending = json["pending_tasks"]
        .as_array()
        .expect("pending_tasks should be array");
    let task_b1 = pending
        .iter()
        .find(|t| t["task_id"] == "task-1" && t["tasks_doc_id"] == "test/tasks-b")
        .expect("should find tasks-b/task-1");
    let deps = task_b1["depends_on"]
        .as_array()
        .expect("depends_on should be array");
    assert!(
        deps.iter().any(|d| d == "test/tasks-b#task-2"),
        "depends_on should be qualified: {deps:?}"
    );
}

/// Set up a fixture with both completed and pending tasks for compact/full testing.
fn setup_plan_compact_fixture(root: &std::path::Path) {
    common::setup_project(root);
    common::write_spec_doc(
        root,
        "specs/req.md",
        "test/req",
        Some("requirements"),
        Some("approved"),
        r#"# Req

<AcceptanceCriteria>
  <Criterion id="c1">First criterion.</Criterion>
  <Criterion id="c2">Second criterion.</Criterion>
</AcceptanceCriteria>
"#,
    );
    common::write_spec_doc(
        root,
        "specs/tasks.md",
        "test/tasks",
        Some("tasks"),
        None,
        r#"# Tasks

<Task id="task-1" status="done" implements="test/req#c1">
  Completed task body text.
</Task>

<Task id="task-2" implements="test/req#c2">
  Pending task body text.
</Task>
"#,
    );
}

#[verifies("work-queries/req#req-6-5")]
#[test]
fn plan_json_compact_omits_completed_and_body_text() {
    let tmp = TempDir::new().unwrap();
    setup_plan_compact_fixture(tmp.path());

    let output = supersigil_cmd()
        .args(["plan", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");

    // completed_tasks should be absent in compact mode.
    assert!(
        json.get("completed_tasks").is_none(),
        "compact plan should omit completed_tasks key, got: {json}",
    );

    // pending_tasks should exist but without body_text.
    let pending = json["pending_tasks"]
        .as_array()
        .expect("pending_tasks should be array");
    assert!(!pending.is_empty(), "should have pending tasks");
    for task in pending {
        assert!(
            task.get("body_text").is_none(),
            "compact plan should omit body_text from pending tasks, got: {task}",
        );
    }

    // outstanding_targets should exist but without body_text.
    let targets = json["outstanding_targets"]
        .as_array()
        .expect("outstanding_targets should be array");
    for target in targets {
        assert!(
            target.get("body_text").is_none(),
            "compact plan should omit body_text from targets, got: {target}",
        );
    }

    // Structural fields should still be present.
    assert!(json.get("actionable_tasks").is_some());
    assert!(json.get("blocked_tasks").is_some());
}

#[verifies("work-queries/req#req-6-6")]
#[test]
fn plan_json_detail_full_includes_completed_and_body_text() {
    let tmp = TempDir::new().unwrap();
    setup_plan_compact_fixture(tmp.path());

    let output = supersigil_cmd()
        .args(["plan", "--format", "json", "--detail", "full"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");

    // completed_tasks should be present and non-empty.
    let completed = json["completed_tasks"]
        .as_array()
        .expect("completed_tasks should be array");
    assert!(
        !completed.is_empty(),
        "full detail plan should have non-empty completed_tasks",
    );

    // completed_tasks should have body_text.
    assert!(
        completed[0].get("body_text").is_some(),
        "full detail plan should include body_text on completed tasks",
    );

    // pending_tasks should have body_text.
    let pending = json["pending_tasks"]
        .as_array()
        .expect("pending_tasks should be array");
    assert!(!pending.is_empty(), "should have pending tasks");
    assert!(
        pending[0].get("body_text").is_some(),
        "full detail plan should include body_text on pending tasks",
    );
}
