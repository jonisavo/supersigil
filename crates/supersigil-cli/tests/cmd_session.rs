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
        .stdout(predicate::str::contains(
            "unknown record types: ai-title (1)",
        ));

    let records: Vec<_> = std::fs::read_dir(&e.records)
        .unwrap()
        .flatten()
        .filter(|d| d.file_name() != ".lock")
        .collect();
    assert_eq!(records.len(), 1);
    assert!(records[0].path().join("manifest.json").exists());
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
