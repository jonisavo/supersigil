//! `supersigil session` against the slice fixture.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::assert::OutputAssertExt;
use predicates::prelude::*;

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../supersigil-session/tests/fixtures/slice.jsonl")
        .canonicalize()
        .unwrap()
}

struct Env {
    _dir: tempfile::TempDir,
    records: PathBuf,
    checkout: PathBuf,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let records = dir.path().join("records");
    let checkout = dir.path().join("checkout");
    std::fs::create_dir_all(&checkout).unwrap();
    Env {
        _dir: dir,
        records,
        checkout,
    }
}

fn session_cmd(env: &Env) -> Command {
    let mut cmd = common::supersigil_cmd();
    cmd.env("SUPERSIGIL_RECORD_DIR", &env.records)
        .env("HOME", env.checkout.parent().unwrap())
        .env_remove("XDG_DATA_HOME")
        .current_dir(&env.checkout)
        .arg("session");
    cmd
}

#[test]
fn sync_with_explicit_transcript_creates_a_record() {
    let e = env();
    session_cmd(&e)
        .args(["sync", "--transcript"])
        .arg(fixture_path())
        .assert()
        .success()
        .stdout(predicate::str::contains("25 observations"))
        .stdout(predicate::str::contains("revision 1"))
        .stdout(predicate::str::contains("unknown record types").not());

    let records: Vec<_> = std::fs::read_dir(&e.records)
        .unwrap()
        .flatten()
        .filter(|d| d.file_name() != ".lock")
        .collect();
    assert_eq!(records.len(), 1);
    assert!(records[0].path().join("manifest.json").exists());
}

#[test]
fn sync_reports_unknown_record_types() {
    let e = env();
    let mut bytes = std::fs::read(fixture_path()).unwrap();
    bytes.extend_from_slice(
        br#"{"type":"totally-new","sessionId":"11111111-1111-4111-8111-111111111111"}"#,
    );
    bytes.push(b'\n');
    let transcript = e.checkout.join("slice.jsonl");
    std::fs::write(&transcript, bytes).unwrap();
    session_cmd(&e)
        .args(["sync", "--transcript"])
        .arg(&transcript)
        .assert()
        .success()
        .stdout(predicate::str::contains("26 observations"))
        .stdout(predicate::str::contains(
            "unknown record types: totally-new (1)",
        ));
}

#[test]
fn sync_twice_reports_nothing_new() {
    let e = env();
    session_cmd(&e)
        .args(["sync", "--transcript"])
        .arg(fixture_path())
        .assert()
        .success();
    session_cmd(&e)
        .args(["sync", "--transcript"])
        .arg(fixture_path())
        .assert()
        .success()
        .stdout(predicate::str::contains("0 observations"))
        .stdout(predicate::str::contains("revision 1"));
}

#[test]
fn sync_json_prints_the_report() {
    let e = env();
    let output = session_cmd(&e)
        .args(["sync", "--format", "json", "--transcript"])
        .arg(fixture_path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["new_observations"], 25);
    assert_eq!(report["revision"], 1);
    assert_eq!(report["transcripts"][0]["trailing_partial"], false);
}

#[test]
fn sync_without_transcripts_hints_and_succeeds() {
    let e = env();
    session_cmd(&e)
        .args(["sync", "--claude-home"])
        .arg(e.checkout.join("no-such-claude-home"))
        .assert()
        .success()
        .stderr(predicate::str::contains("no transcripts found"))
        .stderr(predicate::str::contains("--transcript"));
}

#[test]
fn list_before_any_sync_says_so() {
    let e = env();
    session_cmd(&e)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("no record for"));
}

#[test]
fn list_json_summarizes_sessions() {
    let e = env();
    session_cmd(&e)
        .args(["sync", "--transcript"])
        .arg(fixture_path())
        .assert()
        .success();
    let output = session_cmd(&e)
        .args(["list", "--format", "json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let list: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    let entry = &list[0];
    assert_eq!(entry["session"], "11111111-1111-4111-8111-111111111111");
    assert_eq!(entry["started"], "2026-09-28T10:00:00.000Z");
    assert_eq!(entry["branch"], "main");
    assert_eq!(entry["checkout"], "/work/repo");
    assert_eq!(entry["turns"], 17);
    assert_eq!(entry["edits"], 5);
    assert_eq!(entry["commands"], 2);
    assert_eq!(entry["restores"], 1);
    assert_eq!(entry["discontinuities"], 1);
}

#[test]
fn list_terminal_prints_one_row_per_session() {
    let e = env();
    session_cmd(&e)
        .args(["sync", "--transcript"])
        .arg(fixture_path())
        .assert()
        .success();
    session_cmd(&e)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("11111111"))
        .stdout(predicate::str::contains("17 turns"))
        .stdout(predicate::str::contains("5 edits"));
}

#[test]
fn show_accepts_a_unique_prefix_and_prints_everything() {
    let e = env();
    session_cmd(&e)
        .args(["sync", "--transcript"])
        .arg(fixture_path())
        .assert()
        .success();
    let output = session_cmd(&e).args(["show", "1111"]).output().unwrap();
    assert!(output.status.success());
    let shown: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(shown["session"], "11111111-1111-4111-8111-111111111111");
    assert_eq!(shown["revision"], 1);
    assert_eq!(shown["observations"].as_array().unwrap().len(), 25);
    assert_eq!(shown["observations"][0]["kind"], "session_start");
    assert_eq!(
        shown["derivations"]["restores"].as_array().unwrap().len(),
        1
    );
}

#[test]
fn show_unknown_session_fails() {
    let e = env();
    session_cmd(&e)
        .args(["sync", "--transcript"])
        .arg(fixture_path())
        .assert()
        .success();
    session_cmd(&e)
        .args(["show", "zzz"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no session matching"));
}

#[test]
fn session_commands_need_no_config_file() {
    let e = env();
    assert!(!e.checkout.join("supersigil.toml").exists());
    session_cmd(&e).arg("list").assert().success();
}

#[test]
fn two_records_for_one_checkout_is_an_error_that_names_both() {
    use supersigil_record::store::{Association, Store};
    let e = env();
    session_cmd(&e)
        .args(["sync", "--transcript"])
        .arg(fixture_path())
        .assert()
        .success();
    let checkout = e.checkout.canonicalize().unwrap();
    Store::create(&e.records.join("second"), Association { checkout }).unwrap();
    let first = std::fs::read_dir(&e.records)
        .unwrap()
        .flatten()
        .map(|d| d.file_name().to_string_lossy().into_owned())
        .find(|n| n != "second" && n != ".lock")
        .unwrap();
    session_cmd(&e)
        .arg("list")
        .assert()
        .failure()
        .stderr(predicate::str::contains("several records"))
        .stderr(predicate::str::contains(&first))
        .stderr(predicate::str::contains("second"));
}

#[test]
fn list_json_before_any_sync_prints_an_empty_array() {
    let e = env();
    let output = session_cmd(&e)
        .args(["list", "--format", "json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let list: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(list, serde_json::json!([]));
    assert!(String::from_utf8_lossy(&output.stderr).contains("no record for"));
}

#[test]
fn sync_json_without_transcripts_prints_an_empty_report() {
    let e = env();
    let output = session_cmd(&e)
        .args(["sync", "--format", "json", "--claude-home"])
        .arg(e.checkout.join("no-such-claude-home"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["new_observations"], 0);
    assert_eq!(report["transcripts"], serde_json::json!([]));
    assert_eq!(report["revision"], 0);
    assert!(String::from_utf8_lossy(&output.stderr).contains("no transcripts found"));
}

#[cfg(unix)]
#[test]
fn unreadable_records_dir_is_an_error() {
    use std::os::unix::fs::PermissionsExt;
    let e = env();
    std::fs::create_dir_all(&e.records).unwrap();
    std::fs::set_permissions(&e.records, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_dir(&e.records).is_ok() {
        // Permissions are ignored (running as root); nothing to test.
        std::fs::set_permissions(&e.records, std::fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    let output = session_cmd(&e).arg("list").output().unwrap();
    std::fs::set_permissions(&e.records, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("error:"));
}

#[test]
fn terminal_output_escapes_control_bytes() {
    let e = env();
    let session = "evil\u{1b}[2Kid";
    let user = serde_json::json!({
        "type": "user", "uuid": "u1", "parentUuid": null, "sessionId": session,
        "cwd": "/work/repo", "gitBranch": "b\u{7}",
        "timestamp": "2026-09-28T10:00:00.000Z", "isSidechain": false, "isMeta": false,
        "message": {"role": "user", "content": "hello"}
    });
    let agent = serde_json::json!({
        "type": "assistant", "uuid": "a1", "parentUuid": "u1", "sessionId": session,
        "cwd": "/work/repo", "gitBranch": "b\u{7}",
        "timestamp": "2026-09-28T10:00:01.000Z", "isSidechain": false,
        "message": {"role": "assistant", "content": [{"type": "text", "text": "hi"}]}
    });
    let transcript = e.checkout.join("evil.jsonl");
    std::fs::write(&transcript, format!("{user}\n{agent}\n")).unwrap();

    let synced = session_cmd(&e)
        .args(["sync", "--transcript"])
        .arg(&transcript)
        .output()
        .unwrap();
    assert!(synced.status.success());
    let sync_out = String::from_utf8(synced.stdout).unwrap();
    assert!(sync_out.contains(r"evil\x1b[2Kid"), "{sync_out}");
    assert!(!sync_out.contains('\u{1b}'));

    let listed = session_cmd(&e).arg("list").output().unwrap();
    assert!(listed.status.success());
    let stdout = String::from_utf8(listed.stdout).unwrap();
    assert!(stdout.contains(r"\x1b[2K"), "{stdout}");
    assert!(stdout.contains(r"\x07"), "{stdout}");
    assert!(!stdout.contains('\u{1b}'));
    assert!(!stdout.contains('\u{7}'));

    // JSON keeps the original value.
    let json = session_cmd(&e)
        .args(["list", "--format", "json"])
        .output()
        .unwrap();
    let list: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(list[0]["session"], session);
}

#[test]
fn ambiguous_session_error_escapes_control_bytes() {
    let e = env();
    let record = |session: &str| {
        serde_json::json!({
            "type": "user", "uuid": format!("u-{session}"), "parentUuid": null,
            "sessionId": session, "cwd": "/work/repo", "gitBranch": "main",
            "timestamp": "2026-09-28T10:00:00.000Z", "isSidechain": false, "isMeta": false,
            "message": {"role": "user", "content": "hello"}
        })
    };
    for (name, session) in [
        ("one.jsonl", "x\u{1b}]0;one"),
        ("two.jsonl", "x\u{1b}]0;two"),
    ] {
        let transcript = e.checkout.join(name);
        std::fs::write(&transcript, format!("{}\n", record(session))).unwrap();
        session_cmd(&e)
            .args(["sync", "--transcript"])
            .arg(&transcript)
            .assert()
            .success();
    }
    let output = session_cmd(&e).args(["show", "x"]).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("matches several sessions"), "{stderr}");
    assert!(stderr.contains(r"x\x1b]0;one"), "{stderr}");
    assert!(!stderr.contains('\u{1b}'));
}

#[cfg(unix)]
#[test]
fn unreadable_child_record_is_an_error() {
    use std::os::unix::fs::PermissionsExt;
    let e = env();
    session_cmd(&e)
        .args(["sync", "--transcript"])
        .arg(fixture_path())
        .assert()
        .success();
    let record_dirs = || -> Vec<PathBuf> {
        std::fs::read_dir(&e.records)
            .unwrap()
            .flatten()
            .filter(|d| d.file_name() != ".lock")
            .map(|d| d.path())
            .collect()
    };
    let records = record_dirs();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    std::fs::set_permissions(record, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::metadata(record.join("manifest.json")).is_ok() {
        // Permissions are ignored (running as root); nothing to test.
        std::fs::set_permissions(record, std::fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    let listed = session_cmd(&e).arg("list").output().unwrap();
    let synced = session_cmd(&e)
        .args(["sync", "--transcript"])
        .arg(fixture_path())
        .output()
        .unwrap();
    std::fs::set_permissions(record, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!listed.status.success());
    assert!(String::from_utf8_lossy(&listed.stderr).contains("error:"));
    assert!(!synced.status.success());
    assert!(String::from_utf8_lossy(&synced.stderr).contains("error:"));
    assert_eq!(record_dirs().len(), 1);
}

#[cfg(unix)]
#[test]
fn checkout_paths_are_escaped_in_terminal_messages() {
    let dir = tempfile::tempdir().unwrap();
    let checkout = dir.path().join("check\u{1b}[2Kout");
    std::fs::create_dir_all(&checkout).unwrap();
    let e = Env {
        records: dir.path().join("records"),
        checkout,
        _dir: dir,
    };
    let assert_escaped = |label: &str, text: &[u8]| {
        let text = String::from_utf8(text.to_vec()).unwrap();
        assert!(text.contains(r"check\x1b[2Kout"), "{label}: {text}");
        assert!(!text.contains('\u{1b}'), "{label}: {text}");
    };

    let listed = session_cmd(&e)
        .args(["list", "--format", "json"])
        .output()
        .unwrap();
    assert!(listed.status.success());
    assert_escaped("list --format json stderr", &listed.stderr);

    let listed = session_cmd(&e).arg("list").output().unwrap();
    assert!(listed.status.success());
    assert_escaped("list stdout", &listed.stdout);

    let shown = session_cmd(&e).args(["show", "x"]).output().unwrap();
    assert!(!shown.status.success());
    assert_escaped("show stderr", &shown.stderr);

    let synced = session_cmd(&e)
        .args(["sync", "--claude-home"])
        .arg(e.checkout.join("no-such-claude-home"))
        .output()
        .unwrap();
    assert!(synced.status.success());
    assert_escaped("sync stderr", &synced.stderr);
}

#[test]
fn sync_reports_conflicting_and_unmatched_tool_results() {
    let e = env();
    let issue = serde_json::json!({
        "type": "assistant", "uuid": "a1", "parentUuid": null, "sessionId": "s",
        "cwd": "/work/repo", "gitBranch": "main",
        "timestamp": "2026-09-28T10:00:00.000Z", "isSidechain": false,
        "message": {"role": "assistant", "content": [{"type": "tool_use", "id": "t1", "name": "Write",
            "input": {"file_path": "/work/repo/a.txt", "content": "x"}}]}
    });
    let result = serde_json::json!({
        "type": "user", "uuid": "u1", "parentUuid": "a1", "sessionId": "s",
        "cwd": "/work/repo", "gitBranch": "main",
        "timestamp": "2026-09-28T10:00:01.000Z", "isSidechain": false, "isMeta": false,
        "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]},
        "toolUseResult": {"type": "update", "filePath": "/work/repo/b.txt", "content": "x", "structuredPatch": []}
    });
    let stray = serde_json::json!({
        "type": "user", "uuid": "u2", "parentUuid": "u1", "sessionId": "s",
        "cwd": "/work/repo", "gitBranch": "main",
        "timestamp": "2026-09-28T10:00:02.000Z", "isSidechain": false, "isMeta": false,
        "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "never-issued", "content": "ok"}]}
    });
    let transcript = e.checkout.join("t.jsonl");
    std::fs::write(&transcript, format!("{issue}\n{result}\n{stray}\n")).unwrap();
    session_cmd(&e)
        .args(["sync", "--transcript"])
        .arg(&transcript)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "edits whose input and result name different files dropped: 1",
        ))
        .stdout(predicate::str::contains(
            "tool results without a matching tool use dropped: 1",
        ));
}

#[cfg(unix)]
#[test]
fn duplicate_record_error_escapes_paths() {
    use supersigil_record::store::{Association, Store};
    let dir = tempfile::tempdir().unwrap();
    let checkout = dir.path().join("dup\u{1b}[2Kout");
    std::fs::create_dir_all(&checkout).unwrap();
    let e = Env {
        records: dir.path().join("records"),
        checkout,
        _dir: dir,
    };
    let canonical = e.checkout.canonicalize().unwrap();
    for name in ["first\u{1b}]0;a", "second"] {
        Store::create(
            &e.records.join(name),
            Association {
                checkout: canonical.clone(),
            },
        )
        .unwrap();
    }
    let output = session_cmd(&e).arg("list").output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("several records"), "{stderr}");
    assert!(stderr.contains(r"dup\x1b[2Kout"), "{stderr}");
    assert!(stderr.contains(r"first\x1b]0;a"), "{stderr}");
    assert!(!stderr.contains('\u{1b}'), "{stderr}");
}
