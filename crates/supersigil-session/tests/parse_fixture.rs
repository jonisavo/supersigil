//! Tests transcript parsing with synthetic sessions and tool results.

mod common;

use std::collections::BTreeSet;
use std::path::PathBuf;

use common::{SESSION, fixture, line_starts};

use serde_json::{Value, json};
use supersigil_record::observations::{
    ChangeKind, ChangeReport, CommandCategory, Content, Edit, EditOperation, EditOrigin, FileState,
    Hunk, Material, Observation, Outcome, Role,
};
use supersigil_record::{ContentId, EventId, SessionId, Timestamp, TurnId};
use supersigil_session::claude_code::{ParseOutcome, parse_transcript};

fn edits(outcome: &ParseOutcome) -> Vec<&supersigil_record::observations::Edit> {
    outcome
        .observations
        .iter()
        .filter_map(|o| match o {
            Observation::Edit(e) => Some(e),
            _ => None,
        })
        .collect()
}

fn commands(outcome: &ParseOutcome) -> Vec<&supersigil_record::observations::Command> {
    outcome
        .observations
        .iter()
        .filter_map(|o| match o {
            Observation::Command(c) => Some(c),
            _ => None,
        })
        .collect()
}

fn turns(outcome: &ParseOutcome) -> Vec<&supersigil_record::observations::Turn> {
    outcome
        .observations
        .iter()
        .filter_map(|o| match o {
            Observation::Turn(t) => Some(t),
            _ => None,
        })
        .collect()
}

fn known(text: &str) -> FileState {
    FileState::known(ContentId::of(text.as_bytes()))
}

fn unavailable(reason: &str) -> Material<String> {
    Material::Unavailable {
        reason: reason.to_owned(),
    }
}

/// Creates a two-line transcript containing tool call `t1` and its result.
/// The result includes `result_block` and the optional structured `toolUseResult`.
fn tool_exchange(
    name: &str,
    input: Value,
    result_block: Value,
    tool_use_result: Option<Value>,
) -> String {
    let mut issue = json!({
        "type": "assistant", "uuid": "a1", "parentUuid": null, "sessionId": "s",
        "cwd": "/work/repo", "gitBranch": "main",
        "timestamp": "2026-09-28T10:00:00.000Z", "isSidechain": false,
        "message": {"role": "assistant", "content": [
            {"type": "tool_use", "id": "t1", "name": name}
        ]}
    });
    issue["message"]["content"][0]["input"] = input;
    let mut result = json!({
        "type": "user", "uuid": "u1", "parentUuid": "a1", "sessionId": "s",
        "cwd": "/work/repo", "gitBranch": "main",
        "timestamp": "2026-09-28T10:00:01.000Z", "isSidechain": false, "isMeta": false,
        "message": {"role": "user"}
    });
    result["message"]["content"] = Value::Array(vec![result_block]);
    if let Some(structured) = tool_use_result {
        result["toolUseResult"] = structured;
    }
    format!("{issue}\n{result}\n")
}

fn ok_block() -> Value {
    json!({"type": "tool_result", "tool_use_id": "t1", "content": "ok"})
}

/// Parses a tool exchange and asserts that it produced exactly one edit.
fn only_edit(
    name: &str,
    input: Value,
    tool_use_result: Value,
) -> supersigil_record::observations::Edit {
    let lines = tool_exchange(name, input, ok_block(), Some(tool_use_result));
    let outcome = parse_transcript(lines.as_bytes(), 0);
    let edits = edits(&outcome);
    assert_eq!(edits.len(), 1, "expected one edit from {lines}");
    edits[0].clone()
}

/// Parses a tool exchange and asserts that it produced exactly one command.
fn only_command(
    result_block: Value,
    tool_use_result: Option<Value>,
) -> supersigil_record::observations::Command {
    let lines = tool_exchange(
        "Bash",
        json!({"command": "ls"}),
        result_block,
        tool_use_result,
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    let commands = commands(&outcome);
    assert_eq!(commands.len(), 1, "expected one command from {lines}");
    commands[0].clone()
}

#[test]
fn whole_fixture_is_consumed_with_session_metadata() {
    let bytes = fixture();
    let outcome = parse_transcript(&bytes, 0);
    assert_eq!(outcome.consumed, bytes.len() as u64);
    assert_eq!(outcome.next_ordinal, 19);
    assert_eq!(outcome.session, Some(SessionId::new(SESSION)));
    assert_eq!(outcome.checkout, Some(PathBuf::from("/work/repo")));
    assert_eq!(outcome.branch.as_deref(), Some("main"));
    assert_eq!(
        outcome.first_time.as_ref().map(Timestamp::as_str),
        Some("2026-09-28T10:00:00.000Z")
    );
    assert!(!outcome.trailing_partial);
    assert!(!outcome.sidechain);
    assert_eq!(outcome.counts.malformed_lines, 0);
    assert!(outcome.counts.unknown_records.is_empty());
    assert_eq!(outcome.ignored_records.get("ai-title"), Some(&1));
    assert_eq!(
        outcome.ignored_records.get("file-history-snapshot"),
        Some(&1)
    );
    assert_eq!(outcome.counts.outside_checkout, 0);
    assert_eq!(outcome.counts.abandoned_tool_uses, 0);
}

#[test]
fn turns_are_classified_and_ordered() {
    let outcome = parse_transcript(&fixture(), 0);
    let turns = turns(&outcome);
    assert_eq!(turns.len(), 17);
    assert_eq!(turns[0].role, Role::Human);
    assert_eq!(
        turns[0].excerpt.retained().map(String::as_str),
        Some("Add a greeting function and clean up.")
    );
    assert_eq!(turns[0].source_ordinal, 0);
    assert_eq!(turns[1].role, Role::Agent);
    assert_eq!(turns[1].parent.as_ref().map(TurnId::as_str), Some("u1"));
    assert_eq!(
        turns[1].excerpt.retained().map(String::as_str),
        Some("I'll add greet to lib.rs.")
    );
    assert_eq!(turns[2].role, Role::Tool);
    assert!(turns[2].excerpt.retained().is_none());
    // Line 10 is a file-history record and line 17 an unknown record: the
    // meta turn on line 18 has ordinal 17.
    let meta = turns.iter().find(|t| t.id.as_str() == "u9").unwrap();
    assert_eq!(meta.role, Role::Meta);
    assert_eq!(meta.source_ordinal, 17);
    assert_eq!(turns.last().unwrap().id.as_str(), "a8");
}

#[test]
fn tool_result_and_meta_records_are_not_human() {
    let outcome = parse_transcript(&fixture(), 0);
    let humans: Vec<_> = turns(&outcome)
        .into_iter()
        .filter(|t| t.role == Role::Human)
        .collect();
    assert_eq!(humans.len(), 1);
    assert_eq!(humans[0].id.as_str(), "u1");
}

#[test]
fn edits_carry_both_sides_and_computed_after_states() {
    let outcome = parse_transcript(&fixture(), 0);
    let session = SessionId::new(SESSION);
    let edits = edits(&outcome);
    assert_eq!(edits.len(), 5);

    let original = "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n";
    let with_greet = "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\npub fn greet(name: &str) -> String {\n    format!(\"hello {name}\")\n}\n";
    let first = edits[0];
    assert_eq!(first.id, EventId::derive("edit", &session, "toolu_01"));
    assert_eq!(first.turn.as_str(), "a1");
    assert_eq!(first.path, PathBuf::from("src/lib.rs"));
    assert_eq!(first.checkout, PathBuf::from("/work/repo"));
    assert_eq!(first.before, known(original));
    assert_eq!(first.after, known(with_greet));
    assert_eq!(first.patch.retained().map(Vec::len), Some(1));
    assert_eq!(
        first.old_text.retained().map(String::as_str),
        Some(original)
    );
    assert_eq!(first.time.as_str(), "2026-09-28T10:00:06.000Z");
    assert_eq!(first.source_ordinal, 1);

    let created = edits[1];
    assert_eq!(created.path, PathBuf::from("src/new.rs"));
    assert_eq!(created.before, FileState::Absent);
    assert_eq!(created.after, known("pub const VERSION: &str = \"1\";\n"));
    assert!(matches!(created.old_text, Material::Unavailable { .. }));

    assert_eq!(edits[2].before, known("draft\n"));
    assert_eq!(edits[2].after, known("final\n"));
    assert_eq!(edits[3].before, known("final\n"));
    assert_eq!(edits[3].after, known("draft\n"));

    let manual = format!("// manual\n{with_greet}");
    let capitalised = manual.replace("format!(\"hello {name}\")", "format!(\"Hello, {name}!\")");
    assert_eq!(edits[4].before, known(&manual));
    assert_eq!(edits[4].after, known(&capitalised));
}

#[test]
fn fixture_edits_record_their_operation() {
    let outcome = parse_transcript(&fixture(), 0);
    let operations: Vec<EditOperation> = edits(&outcome).iter().map(|e| e.operation).collect();
    assert_eq!(
        operations,
        [
            EditOperation::Replace,
            EditOperation::Write,
            EditOperation::Replace,
            EditOperation::Replace,
            EditOperation::Replace,
        ]
    );
}

#[test]
fn multi_edit_operation_is_unknown() {
    let path = "/work/repo/a.txt";
    let multi = only_edit(
        "MultiEdit",
        json!({"file_path": path, "edits": [{"old_string": "a", "new_string": "c"}]}),
        json!({"filePath": path, "originalFile": "a b\n", "structuredPatch": []}),
    );
    assert_eq!(multi.after, known("c b\n"));
    assert_eq!(multi.operation, EditOperation::Unknown);
}

#[test]
fn commands_are_classified_with_outcome_and_end_time() {
    let outcome = parse_transcript(&fixture(), 0);
    let commands = commands(&outcome);
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].cmd, "rm old.txt");
    assert_eq!(commands[0].category, CommandCategory::Other);
    assert!(!commands[0].reported_error);
    assert_eq!(commands[0].outcome, None);
    assert_eq!(commands[0].started.as_str(), "2026-09-28T10:00:50.000Z");
    assert_eq!(
        commands[0].ended.as_ref().map(Timestamp::as_str),
        Some("2026-09-28T10:00:51.000Z")
    );
    assert_eq!(commands[1].cmd, "cargo nextest run");
    assert_eq!(commands[1].category, CommandCategory::TestRun);
    assert_eq!(commands[1].outcome, Some(Outcome::Passed));
    assert!(
        commands[1]
            .stdout_tail
            .retained()
            .unwrap()
            .contains("3 passed")
    );
}

#[test]
fn edit_with_missing_old_string_has_unknown_after_state() {
    let lines = concat!(
        r#"{"type":"assistant","uuid":"a1","parentUuid":null,"sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/work/repo/a.txt","old_string":"gone","new_string":"there"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"u1","parentUuid":"a1","sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:01.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]},"toolUseResult":{"filePath":"/work/repo/a.txt","oldString":"gone","newString":"there","originalFile":"something else\n","structuredPatch":[],"userModified":true,"replaceAll":false}}"#,
        "\n",
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    let edits = edits(&outcome);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].before, known("something else\n"));
    assert_eq!(
        edits[0].after,
        FileState::Present {
            content: Content::Unknown
        }
    );
}

#[test]
fn edit_without_original_file_has_unknown_before_state() {
    let lines = concat!(
        r#"{"type":"assistant","uuid":"a1","parentUuid":null,"sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/work/repo/a.txt","old_string":"x","new_string":"y"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"u1","parentUuid":"a1","sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:01.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]},"toolUseResult":{"filePath":"/work/repo/a.txt","oldString":"x","newString":"y","structuredPatch":[],"userModified":false,"replaceAll":false}}"#,
        "\n",
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    let edits = edits(&outcome);
    assert_eq!(edits[0].before, FileState::unknown());
    assert_eq!(edits[0].after, FileState::unknown());
}

#[test]
fn trailing_partial_line_is_not_consumed() {
    let bytes = fixture();
    let starts = line_starts(&bytes);
    let cut = &bytes[..starts[18] + 40];
    let outcome = parse_transcript(cut, 0);
    assert!(outcome.trailing_partial);
    assert_eq!(outcome.consumed, starts[18] as u64);
    assert_eq!(outcome.next_ordinal, 18);
    assert_eq!(turns(&outcome).last().unwrap().id.as_str(), "u9");
}

#[test]
fn pending_tool_use_at_end_is_not_consumed() {
    let bytes = fixture();
    let starts = line_starts(&bytes);
    // Everything through line 15 (the assistant record issuing `cargo nextest run`).
    let cut = &bytes[..starts[15]];
    let outcome = parse_transcript(cut, 0);
    assert!(!outcome.trailing_partial);
    assert_eq!(outcome.consumed, starts[14] as u64);
    assert_eq!(outcome.next_ordinal, 14);
    assert_eq!(commands(&outcome).len(), 1);
    assert_eq!(turns(&outcome).last().unwrap().id.as_str(), "u7");
    assert_eq!(outcome.counts.abandoned_tool_uses, 0);
}

#[test]
fn abandoned_tool_use_is_counted_and_command_emitted_without_result() {
    let lines = concat!(
        r#"{"type":"assistant","uuid":"a1","parentUuid":null,"sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"u1","parentUuid":"a1","sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:09.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":"never mind"}}"#,
        "\n",
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert_eq!(outcome.consumed, lines.len() as u64);
    assert_eq!(outcome.counts.abandoned_tool_uses, 1);
    let commands = commands(&outcome);
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].ended, None);
    assert!(matches!(
        commands[0].stdout_tail,
        Material::Unavailable { .. }
    ));
    assert_eq!(commands[0].category, CommandCategory::TestRun);
    assert!(!commands[0].reported_error);
}

#[test]
fn split_assistant_message_keeps_both_tool_uses() {
    // Claude Code writes each content block of one assistant message as its
    // own record with the same `message.id`.
    let lines = concat!(
        r#"{"type":"assistant","uuid":"a1","parentUuid":null,"sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"message":{"id":"m1","role":"assistant","content":[{"type":"text","text":"Editing and testing."}]}}"#,
        "\n",
        r#"{"type":"assistant","uuid":"a2","parentUuid":"a1","sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.100Z","isSidechain":false,"message":{"id":"m1","role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/work/repo/a.txt","old_string":"a","new_string":"b","replace_all":false}}]}}"#,
        "\n",
        r#"{"type":"assistant","uuid":"a3","parentUuid":"a2","sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.200Z","isSidechain":false,"message":{"id":"m1","role":"assistant","content":[{"type":"tool_use","id":"t2","name":"Bash","input":{"command":"ls"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"u1","parentUuid":"a3","sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:01.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]},"toolUseResult":{"filePath":"/work/repo/a.txt","oldString":"a","newString":"b","originalFile":"a\n","structuredPatch":[],"replaceAll":false}}"#,
        "\n",
        r#"{"type":"user","uuid":"u2","parentUuid":"u1","sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:02.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"a.txt"}]},"toolUseResult":{"stdout":"a.txt","stderr":"","interrupted":false}}"#,
        "\n",
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert_eq!(outcome.consumed, lines.len() as u64);
    assert_eq!(outcome.next_ordinal, 5);
    assert_eq!(outcome.counts.abandoned_tool_uses, 0);
    assert_eq!(outcome.counts.unmatched_tool_results, 0);
    let edits = edits(&outcome);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].after, known("b\n"));
    let commands = commands(&outcome);
    assert_eq!(commands.len(), 1);
    assert_eq!(
        commands[0].ended,
        Some(Timestamp::new("2026-09-28T10:00:02.000Z"))
    );
}

#[test]
fn edit_outside_checkout_is_counted_and_dropped() {
    let lines = concat!(
        r#"{"type":"assistant","uuid":"a1","parentUuid":null,"sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Write","input":{"file_path":"/etc/hosts","content":"x"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"u1","parentUuid":"a1","sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:01.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]},"toolUseResult":{"type":"update","filePath":"/etc/hosts","content":"x","structuredPatch":[]}}"#,
        "\n",
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert_eq!(outcome.counts.outside_checkout, 1);
    assert!(edits(&outcome).is_empty());
}

#[test]
fn malformed_line_is_counted_and_skipped() {
    let bytes = fixture();
    let starts = line_starts(&bytes);
    let mut broken = bytes[..starts[1]].to_vec();
    broken.extend_from_slice(b"{not json\n");
    broken.extend_from_slice(&bytes[starts[1]..]);
    let outcome = parse_transcript(&broken, 0);
    assert_eq!(outcome.counts.malformed_lines, 1);
    assert_eq!(outcome.next_ordinal, 20);
    assert_eq!(edits(&outcome).len(), 5);
}

#[test]
fn parsing_from_an_ordinal_returns_only_what_follows_it() {
    let bytes = fixture();
    let starts = line_starts(&bytes);
    let whole = parse_transcript(&bytes, 0);
    let first = parse_transcript(&bytes[..starts[15]], 0);
    let rest = parse_transcript(&bytes, first.next_ordinal);

    let chunked: Vec<_> = first
        .observations
        .iter()
        .chain(&rest.observations)
        .cloned()
        .collect();
    assert_eq!(chunked, whole.observations);
    assert_eq!(rest.consumed, whole.consumed);
    assert_eq!(rest.next_ordinal, whole.next_ordinal);
    // What the session is, and where it ran, comes from the whole file.
    assert_eq!(rest.session, whole.session);
    assert_eq!(rest.checkout, whole.checkout);
    assert_eq!(rest.first_time, whole.first_time);
}

#[test]
fn failed_write_is_not_an_edit() {
    let lines = concat!(
        r#"{"type":"assistant","uuid":"a1","parentUuid":null,"sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Write","input":{"file_path":"/work/repo/locked.txt","content":"never written\n"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"u1","parentUuid":"a1","sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:01.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","is_error":true,"content":"EACCES: permission denied, open '/work/repo/locked.txt'"}]}}"#,
        "\n",
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert!(edits(&outcome).is_empty());
    assert_eq!(outcome.counts.failed_tool_uses, 1);
    assert_eq!(outcome.counts.outside_checkout, 0);
    assert_eq!(turns(&outcome).len(), 2);
}

#[test]
fn errored_bash_records_reported_error_and_failed_tests() {
    let lines = concat!(
        r#"{"type":"assistant","uuid":"a1","parentUuid":null,"sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo nextest run"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"u1","parentUuid":"a1","sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:09.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","is_error":true,"content":"error: could not compile"}]},"toolUseResult":{"stdout":"","stderr":"error: could not compile `supersigil`","interrupted":false}}"#,
        "\n",
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    let commands = commands(&outcome);
    assert_eq!(commands.len(), 1);
    assert!(commands[0].reported_error);
    assert_eq!(commands[0].outcome, Some(Outcome::Failed));
    assert_eq!(outcome.counts.failed_tool_uses, 0);
}

#[test]
fn traversal_path_is_outside_checkout() {
    let lines = concat!(
        r#"{"type":"assistant","uuid":"a1","parentUuid":null,"sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Write","input":{"file_path":"/work/repo/../private/secret","content":"x"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"u1","parentUuid":"a1","sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:01.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]},"toolUseResult":{"type":"create","filePath":"/work/repo/../private/secret","content":"x","structuredPatch":[]}}"#,
        "\n",
        r#"{"type":"assistant","uuid":"a2","parentUuid":"u1","sessionId":"s","cwd":"","gitBranch":"main","timestamp":"2026-09-28T10:00:02.000Z","isSidechain":false,"message":{"role":"assistant","content":[{"type":"tool_use","id":"t2","name":"Write","input":{"file_path":"a.txt","content":"x"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"u2","parentUuid":"a2","sessionId":"s","cwd":"","gitBranch":"main","timestamp":"2026-09-28T10:00:03.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"ok"}]},"toolUseResult":{"type":"create","filePath":"a.txt","content":"x","structuredPatch":[]}}"#,
        "\n",
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert!(edits(&outcome).is_empty());
    assert_eq!(outcome.counts.outside_checkout, 2);
}

#[test]
fn mixed_cargo_test_output_is_failed() {
    use supersigil_session::claude_code::test_outcome;
    let stdout = "running 3 tests\ntest result: ok. 3 passed; 0 failed\n\nrunning 2 tests\ntest a ... FAILED\ntest result: FAILED. 1 passed; 1 failed\n";
    assert_eq!(test_outcome(stdout, ""), Some(Outcome::Failed));
    assert_eq!(
        test_outcome("test result: ok. 3 passed; 0 failed\n", ""),
        Some(Outcome::Passed)
    );
    assert_eq!(test_outcome("compiling...", ""), None);
}

#[test]
fn nextest_failures_are_failed() {
    use supersigil_session::claude_code::test_outcome;
    let stdout = "     Summary [   0.512s] 3 tests run: 2 passed, 1 failed, 0 skipped\n        FAIL [   0.010s] crate::tests::a\n";
    assert_eq!(test_outcome(stdout, ""), Some(Outcome::Failed));
    assert_eq!(
        test_outcome(
            "     Summary [   0.512s] 3 tests run: 3 passed, 0 skipped\n",
            ""
        ),
        Some(Outcome::Passed)
    );
    assert_eq!(
        test_outcome(
            "     Summary [   0.512s] 3 tests run: 3 passed, 0 failed, 0 skipped\n",
            ""
        ),
        Some(Outcome::Passed)
    );
}

#[test]
fn counts_stop_at_the_consumption_cutoff() {
    let pending = concat!(
        r#"{"type":"assistant","uuid":"a1","parentUuid":null,"sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test"}}]}}"#,
        "\n",
        r#"{"type":"totally-new","sessionId":"s"}"#,
        "\n",
        "{not json\n",
    );
    let outcome = parse_transcript(pending.as_bytes(), 0);
    assert_eq!(outcome.consumed, 0);
    assert_eq!(outcome.next_ordinal, 0);
    assert!(outcome.counts.unknown_records.is_empty());
    assert_eq!(outcome.counts.malformed_lines, 0);
    assert_eq!(outcome.counts.abandoned_tool_uses, 0);

    let full = format!(
        "{pending}{}\n",
        r#"{"type":"user","uuid":"u1","parentUuid":"a1","sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:01.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]},"toolUseResult":{"stdout":"","stderr":"","interrupted":false}}"#
    );
    let outcome = parse_transcript(full.as_bytes(), 0);
    assert_eq!(outcome.counts.unknown_records.get("totally-new"), Some(&1));
    assert_eq!(outcome.counts.malformed_lines, 1);
}

#[test]
fn missing_write_content_is_unknown() {
    let path = "/work/repo/a.txt";
    let missing = only_edit(
        "Write",
        json!({"file_path": path}),
        json!({"type": "update", "filePath": path, "originalFile": "old\n", "structuredPatch": []}),
    );
    assert_eq!(missing.before, known("old\n"));
    assert_eq!(missing.after, FileState::unknown());
    assert_eq!(missing.new_text, unavailable("field missing"));

    // Input and result disagree: the result's text is kept, but the
    // conflicting evidence yields no content hash.
    let conflicting = only_edit(
        "Write",
        json!({"file_path": path, "content": "asked\n"}),
        json!({"type": "update", "filePath": path, "content": "written\n", "originalFile": "old\n", "structuredPatch": []}),
    );
    assert_eq!(conflicting.after, FileState::unknown());
    assert_eq!(
        conflicting.new_text,
        Material::Retained("written\n".to_owned())
    );

    // Input and result agree: the written content is known.
    let agreeing = only_edit(
        "Write",
        json!({"file_path": path, "content": "written\n"}),
        json!({"type": "update", "filePath": path, "content": "written\n", "originalFile": "old\n", "structuredPatch": []}),
    );
    assert_eq!(agreeing.after, known("written\n"));
    assert_eq!(
        agreeing.new_text,
        Material::Retained("written\n".to_owned())
    );

    // Only the result carries it.
    let from_result = only_edit(
        "Write",
        json!({"file_path": path}),
        json!({"type": "update", "filePath": path, "content": "written\n", "originalFile": "old\n", "structuredPatch": []}),
    );
    assert_eq!(from_result.after, known("written\n"));

    // An explicitly empty file is known content, not a missing field.
    let empty = only_edit(
        "Write",
        json!({"file_path": path, "content": ""}),
        json!({"type": "update", "filePath": path, "content": "", "originalFile": "old\n", "structuredPatch": []}),
    );
    assert_eq!(empty.after, known(""));
    assert_eq!(empty.new_text, Material::Retained(String::new()));
}

#[test]
fn missing_edit_fields_are_unknown() {
    let path = "/work/repo/a.txt";
    let no_new = only_edit(
        "Edit",
        json!({"file_path": path, "old_string": "a "}),
        json!({"filePath": path, "oldString": "a ", "originalFile": "a b\n", "structuredPatch": [], "replaceAll": false}),
    );
    assert_eq!(no_new.before, known("a b\n"));
    assert_eq!(no_new.after, FileState::unknown());
    assert_eq!(no_new.old_text, Material::Retained("a ".to_owned()));
    assert_eq!(no_new.new_text, unavailable("field missing"));

    let no_old = only_edit(
        "Edit",
        json!({"file_path": path, "new_string": "c"}),
        json!({"filePath": path, "newString": "c", "originalFile": "a b\n", "structuredPatch": [], "replaceAll": false}),
    );
    assert_eq!(no_old.after, FileState::unknown());
    assert_eq!(no_old.old_text, unavailable("field missing"));

    let no_replace_all = only_edit(
        "Edit",
        json!({"file_path": path, "old_string": "a ", "new_string": ""}),
        json!({"filePath": path, "oldString": "a ", "newString": "", "originalFile": "a b\n", "structuredPatch": []}),
    );
    assert_eq!(no_replace_all.after, FileState::unknown());

    // An explicitly empty replacement is a deletion the evidence supports.
    let deletion = only_edit(
        "Edit",
        json!({"file_path": path, "old_string": "a ", "new_string": "", "replace_all": false}),
        json!({"filePath": path, "oldString": "a ", "newString": "", "originalFile": "a b\n", "structuredPatch": [], "replaceAll": false}),
    );
    assert_eq!(deletion.after, known("b\n"));
    assert_eq!(deletion.new_text, Material::Retained(String::new()));

    // Fields only the input carries are used when the result lacks them.
    let from_input = only_edit(
        "Edit",
        json!({"file_path": path, "old_string": "a", "new_string": "c", "replace_all": false}),
        json!({"filePath": path, "originalFile": "a b\n", "structuredPatch": []}),
    );
    assert_eq!(from_input.after, known("c b\n"));

    // A multi-edit without edits says nothing about the file afterwards.
    let no_edits = only_edit(
        "MultiEdit",
        json!({"file_path": path, "edits": []}),
        json!({"filePath": path, "originalFile": "a b\n", "structuredPatch": []}),
    );
    assert_eq!(no_edits.after, FileState::unknown());
    let absent_edits = only_edit(
        "MultiEdit",
        json!({"file_path": path}),
        json!({"filePath": path, "originalFile": "a b\n", "structuredPatch": []}),
    );
    assert_eq!(absent_edits.after, FileState::unknown());
}

#[test]
fn conflicting_edit_fields_are_unknown() {
    let path = "/work/repo/a.txt";
    let new_differs = only_edit(
        "Edit",
        json!({"file_path": path, "old_string": "a", "new_string": "asked", "replace_all": false}),
        json!({"filePath": path, "oldString": "a", "newString": "applied", "originalFile": "a\n", "structuredPatch": [], "replaceAll": false}),
    );
    assert_eq!(new_differs.before, known("a\n"));
    assert_eq!(new_differs.after, FileState::unknown());
    assert_eq!(
        new_differs.new_text,
        Material::Retained("applied".to_owned())
    );

    let replace_all_differs = only_edit(
        "Edit",
        json!({"file_path": path, "old_string": "a", "new_string": "b", "replace_all": true}),
        json!({"filePath": path, "oldString": "a", "newString": "b", "originalFile": "a a\n", "structuredPatch": [], "replaceAll": false}),
    );
    assert_eq!(replace_all_differs.after, FileState::unknown());
}

#[test]
fn missing_command_streams_fall_back_to_result_content_or_unavailable() {
    let from_string = only_command(
        json!({"type": "tool_result", "tool_use_id": "t1", "content": "file1\nfile2"}),
        None,
    );
    assert_eq!(
        from_string.stdout_tail,
        Material::Retained("file1\nfile2".to_owned())
    );
    assert_eq!(from_string.stderr_tail, unavailable("not captured"));

    let from_blocks = only_command(
        json!({"type": "tool_result", "tool_use_id": "t1", "content": [
            {"type": "text", "text": "one"}, {"type": "text", "text": "two"}
        ]}),
        None,
    );
    assert_eq!(
        from_blocks.stdout_tail,
        Material::Retained("one\ntwo".to_owned())
    );

    let nothing = only_command(json!({"type": "tool_result", "tool_use_id": "t1"}), None);
    assert_eq!(nothing.stdout_tail, unavailable("not captured"));
    assert_eq!(nothing.stderr_tail, unavailable("not captured"));

    // Explicitly empty streams are retained as empty, not replaced.
    let empty = only_command(
        json!({"type": "tool_result", "tool_use_id": "t1", "content": "ignored"}),
        Some(json!({"stdout": "", "stderr": "", "interrupted": false})),
    );
    assert_eq!(empty.stdout_tail, Material::Retained(String::new()));
    assert_eq!(empty.stderr_tail, Material::Retained(String::new()));
}

#[test]
fn mixed_text_and_tool_result_record_keeps_the_result() {
    let path = "/work/repo/a.txt";
    let lines = tool_exchange(
        "Edit",
        json!({"file_path": path, "old_string": "a", "new_string": "b"}),
        json!({"type": "tool_result", "tool_use_id": "t1", "content": "ok"}),
        Some(
            json!({"filePath": path, "oldString": "a", "newString": "b", "originalFile": "a\n", "structuredPatch": [], "replaceAll": false}),
        ),
    );
    // Put a typed message next to the tool result in the same user record.
    let lines = lines.replacen(
        r#""content":[{"content":"ok""#,
        r#""content":[{"type":"text","text":"also rename it"},{"content":"ok""#,
        1,
    );
    assert!(
        lines.contains("also rename it"),
        "fixture edit failed: {lines}"
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert_eq!(outcome.counts.abandoned_tool_uses, 0);
    let edits = edits(&outcome);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].after, known("b\n"));
    let user = turns(&outcome)
        .into_iter()
        .find(|t| t.id.as_str() == "u1")
        .unwrap();
    assert_eq!(user.role, Role::Human);
}

#[test]
fn every_nextest_summary_is_inspected() {
    use supersigil_session::claude_code::test_outcome;
    let stdout = concat!(
        "     Summary [   0.512s] 3 tests run: 3 passed, 0 skipped\n",
        "     Summary [   0.210s] 2 tests run: 1 passed, 1 failed, 0 skipped\n",
    );
    assert_eq!(test_outcome(stdout, ""), Some(Outcome::Failed));
}

#[test]
fn harness_error_overrides_a_passing_test_result() {
    let lines = tool_exchange(
        "Bash",
        json!({"command": "cargo test"}),
        json!({"type": "tool_result", "tool_use_id": "t1", "is_error": true, "content": "exit 101"}),
        Some(json!({
            "stdout": "running 3 tests\ntest result: ok. 3 passed; 0 failed\n",
            "stderr": "error: could not compile `later`",
            "interrupted": false
        })),
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    let commands = commands(&outcome);
    assert_eq!(commands.len(), 1);
    assert!(commands[0].reported_error);
    assert_eq!(commands[0].outcome, Some(Outcome::Failed));
}

#[test]
fn conflicting_edit_paths_are_dropped_and_counted() {
    let edit_input = |path: &str| json!({"file_path": path, "old_string": "a", "new_string": "b", "replace_all": false});
    let edit_result = |path: &str| json!({"filePath": path, "oldString": "a", "newString": "b", "originalFile": "a\n", "structuredPatch": [], "replaceAll": false});
    for (input_path, result_path) in [
        // The input names a file in the checkout; the result one outside it.
        ("/work/repo/a.txt", "/etc/hosts"),
        // The input names a file outside; the result one inside.
        ("/etc/hosts", "/work/repo/a.txt"),
        // Both inside, but not the same file.
        ("/work/repo/a.txt", "/work/repo/b.txt"),
        // A relative input cannot be shown to name the result's file.
        ("a.txt", "/work/repo/a.txt"),
    ] {
        let lines = tool_exchange(
            "Edit",
            edit_input(input_path),
            ok_block(),
            Some(edit_result(result_path)),
        );
        let outcome = parse_transcript(lines.as_bytes(), 0);
        assert!(
            edits(&outcome).is_empty(),
            "an edit was emitted for {input_path:?} vs {result_path:?}"
        );
        assert_eq!(
            outcome.counts.conflicting_tool_results, 1,
            "{input_path:?} vs {result_path:?}"
        );
        assert_eq!(outcome.counts.outside_checkout, 0);
    }

    // Agreement, or a path on one side only, is not a conflict.
    let agreed = only_edit(
        "Edit",
        edit_input("/work/repo/a.txt"),
        edit_result("/work/repo/a.txt"),
    );
    assert_eq!(agreed.path, PathBuf::from("a.txt"));
    let result_only = only_edit(
        "Edit",
        json!({"old_string": "a", "new_string": "b", "replace_all": false}),
        edit_result("/work/repo/a.txt"),
    );
    assert_eq!(result_only.path, PathBuf::from("a.txt"));
    let input_only = only_edit(
        "Edit",
        edit_input("/work/repo/a.txt"),
        json!({"oldString": "a", "newString": "b", "originalFile": "a\n", "structuredPatch": [], "replaceAll": false}),
    );
    assert_eq!(input_only.path, PathBuf::from("a.txt"));

    // Two paths that both lie outside are counted as outside, not conflicting.
    let lines = tool_exchange(
        "Edit",
        edit_input("/etc/hosts"),
        ok_block(),
        Some(edit_result("/etc/passwd")),
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert!(edits(&outcome).is_empty());
    assert_eq!(outcome.counts.outside_checkout, 1);
    assert_eq!(outcome.counts.conflicting_tool_results, 0);
}

#[test]
fn unknown_records_before_any_turn_still_yield_the_session() {
    let lines = concat!(
        r#"{"type":"totally-new","sessionId":"s"}"#,
        "\n",
        r#"{"type":"totally-new","sessionId":"s"}"#,
        "\n",
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert_eq!(outcome.session, Some(SessionId::new("s")));
    assert_eq!(outcome.counts.unknown_records.get("totally-new"), Some(&2));
    assert_eq!(outcome.consumed, lines.len() as u64);

    // Metadata missing from the first record is taken from a later one of the
    // same session.
    let later = format!(
        "{lines}{}\n",
        r#"{"type":"user","uuid":"u1","parentUuid":null,"sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":"hi"}}"#
    );
    let outcome = parse_transcript(later.as_bytes(), 0);
    assert_eq!(outcome.session, Some(SessionId::new("s")));
    assert_eq!(outcome.checkout, Some(PathBuf::from("/work/repo")));
    assert_eq!(outcome.branch.as_deref(), Some("main"));
    assert_eq!(
        outcome.first_time,
        Some(Timestamp::new("2026-09-28T10:00:00.000Z"))
    );
    assert_eq!(turns(&outcome).len(), 1);
}

#[test]
fn unmatched_tool_result_is_counted() {
    let lines = concat!(
        r#"{"type":"user","uuid":"u0","parentUuid":null,"sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":"hi"}}"#,
        "\n",
        r#"{"type":"user","uuid":"u1","parentUuid":"u0","sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:01.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"never-issued","content":"ok"}]},"toolUseResult":{"type":"update","filePath":"/work/repo/a.txt","content":"x","structuredPatch":[]}}"#,
        "\n",
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert_eq!(outcome.counts.unmatched_tool_results, 1);
    assert!(edits(&outcome).is_empty());
    assert!(!outcome.counts.is_empty());
    assert_eq!(outcome.consumed, lines.len() as u64);
}

#[test]
fn empty_session_id_is_treated_as_absent() {
    let lines = concat!(
        r#"{"type":"user","uuid":"u1","parentUuid":null,"sessionId":"","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":"hi"}}"#,
        "\n",
        r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","sessionId":"","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:01.000Z","isSidechain":false,"message":{"role":"assistant","content":[{"type":"text","text":"hello"}]}}"#,
        "\n",
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert_eq!(outcome.session, None);
    assert_eq!(outcome.counts.unknown_records.get("no-session"), Some(&2));
    assert!(outcome.observations.is_empty());
}

#[test]
fn claude_code_ui_state_records_are_ignored() {
    let kinds = [
        "ai-title",
        "atis-latch",
        "bridge-session",
        "cost-state",
        "pr-link",
        "queue-operation",
        "relocated",
        "worktree-state",
    ];
    let mut lines = String::new();
    for kind in kinds {
        lines.push_str(&json!({"type": kind, "sessionId": "s"}).to_string());
        lines.push('\n');
    }
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert!(outcome.counts.is_empty(), "{:?}", outcome.counts);
    assert!(outcome.observations.is_empty());
    assert_eq!(outcome.consumed, lines.len() as u64);
    for kind in kinds {
        assert_eq!(outcome.ignored_records.get(kind), Some(&1), "{kind}");
    }
}

#[test]
fn session_metadata_from_a_sidechain_record_is_flagged() {
    let sidechain = concat!(
        r#"{"type":"user","uuid":"su1","parentUuid":null,"sessionId":"s","agentId":"agent1","cwd":"/work/repo/.claude/worktrees/x","gitBranch":"side","timestamp":"2026-09-28T11:00:00.000Z","isSidechain":true,"isMeta":false,"message":{"role":"user","content":"do the side task"}}"#,
        "\n",
    );
    let outcome = parse_transcript(sidechain.as_bytes(), 0);
    assert_eq!(outcome.session, Some(SessionId::new("s")));
    assert!(outcome.sidechain);

    let main = sidechain.replace(r#""isSidechain":true"#, r#""isSidechain":false"#);
    assert!(!parse_transcript(main.as_bytes(), 0).sidechain);
}

#[test]
fn metadata_beyond_the_consumption_cutoff_is_not_learned() {
    // The only record issues a tool use still waiting for its result, so
    // nothing is consumed and nothing it names is known yet.
    let pending = concat!(
        r#"{"type":"assistant","uuid":"a1","parentUuid":null,"sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":true,"message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test"}}]}}"#,
        "\n",
    );
    let outcome = parse_transcript(pending.as_bytes(), 0);
    assert_eq!(outcome.consumed, 0);
    assert_eq!(outcome.session, None);
    assert_eq!(outcome.checkout, None);
    assert_eq!(outcome.branch, None);
    assert_eq!(outcome.first_time, None);
    assert!(!outcome.sidechain);

    // Metadata first carried below the cutoff is kept; a field first carried
    // beyond it is not.
    let partly = format!(
        "{}\n{pending}",
        r#"{"type":"user","uuid":"u0","parentUuid":null,"sessionId":"s","timestamp":"2026-09-28T09:59:59.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":"hi"}}"#
    );
    let outcome = parse_transcript(partly.as_bytes(), 0);
    assert_eq!(outcome.consumed, (partly.len() - pending.len()) as u64);
    assert_eq!(outcome.session, Some(SessionId::new("s")));
    assert_eq!(
        outcome.first_time,
        Some(Timestamp::new("2026-09-28T09:59:59.000Z"))
    );
    assert_eq!(outcome.checkout, None);
    assert_eq!(outcome.branch, None);
}

#[test]
fn malformed_lines_before_any_session_are_not_consumed() {
    let lines = "{not json\n";
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert_eq!(outcome.session, None);
    assert_eq!(outcome.consumed, 0);
    assert_eq!(outcome.next_ordinal, 0);
    // What was seen is still reported.
    assert_eq!(outcome.counts.malformed_lines, 1);
}

/// Creates a human message record for `session`.
fn human(uuid: &str, session: &str) -> Value {
    json!({
        "type": "user", "uuid": uuid, "parentUuid": null, "sessionId": session,
        "cwd": "/work/repo", "gitBranch": "main",
        "timestamp": "2026-09-28T10:00:00.000Z", "isSidechain": false, "isMeta": false,
        "message": {"role": "user", "content": "hi"}
    })
}

/// Creates an assistant message record with one text block for `session`.
fn agent(uuid: &str, session: &str) -> Value {
    json!({
        "type": "assistant", "uuid": uuid, "parentUuid": null, "sessionId": session,
        "cwd": "/work/repo", "gitBranch": "main",
        "timestamp": "2026-09-28T10:00:01.000Z", "isSidechain": false,
        "message": {"role": "assistant", "content": [{"type": "text", "text": "hello"}]}
    })
}

#[test]
fn a_typed_message_of_another_session_abandons_pending_tool_uses() {
    // Session `a` issues a Bash call that never gets a result, then a person
    // types in session `b`, as in two transcripts written into one file.
    let issue = json!({
        "type": "assistant", "uuid": "a1", "parentUuid": null, "sessionId": "a",
        "cwd": "/work/repo", "gitBranch": "main",
        "timestamp": "2026-09-28T10:00:00.000Z", "isSidechain": false,
        "message": {"role": "assistant", "content": [
            {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls"}}
        ]}
    });
    let lines = format!("{issue}\n{}\n", human("ub", "b"));
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert_eq!(outcome.consumed, lines.len() as u64);
    assert_eq!(outcome.counts.abandoned_tool_uses, 1);
    assert_eq!(outcome.counts.session_mismatch, 1);
    assert_eq!(commands(&outcome).len(), 1);
}

#[test]
fn a_bash_call_without_a_command_is_counted_not_recorded() {
    let streams = json!({"stdout": "x\n", "stderr": "", "interrupted": false});
    for input in [json!({"description": "list"}), json!({"command": 42})] {
        let lines = tool_exchange("Bash", input.clone(), ok_block(), Some(streams.clone()));
        let outcome = parse_transcript(lines.as_bytes(), 0);
        assert!(commands(&outcome).is_empty(), "{input}");
        assert_eq!(outcome.counts.unsupported_tool_uses, 1, "{input}");
    }

    // Abandoned before any result, it is counted as abandoned only.
    let issue = tool_exchange("Bash", json!({}), ok_block(), None)
        .lines()
        .next()
        .unwrap()
        .to_owned();
    let lines = format!("{issue}\n{}\n", human("u2", "s"));
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert!(commands(&outcome).is_empty());
    assert_eq!(outcome.counts.abandoned_tool_uses, 1);
}

#[test]
fn records_of_another_session_are_counted_not_staged() {
    let lines = format!(
        "{}\n{}\n{}\n{}\n",
        human("ua", "a"),
        agent("aa", "a"),
        human("ub", "b"),
        agent("ab", "b")
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert_eq!(outcome.session, Some(SessionId::new("a")));
    let ids: Vec<&str> = turns(&outcome).iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["ua", "aa"]);
    assert_eq!(outcome.counts.session_mismatch, 2);
    assert!(!outcome.counts.is_empty());
    assert_eq!(outcome.consumed, lines.len() as u64);

    // Parsed from an ordinal, the session is still the one the file names
    // first.
    let appended = format!("{lines}{}\n", human("uc", "b"));
    let outcome = parse_transcript(appended.as_bytes(), 4);
    assert!(turns(&outcome).is_empty());
    assert_eq!(outcome.counts.session_mismatch, 1);
}

#[test]
fn tool_uses_and_results_without_an_id_are_not_paired() {
    for id in [None, Some("")] {
        let mut tool_use = json!({"type": "tool_use", "name": "Bash", "input": {"command": "ls"}});
        let mut result = json!({"type": "tool_result", "content": "ok"});
        if let Some(id) = id {
            tool_use["id"] = json!(id);
            result["tool_use_id"] = json!(id);
        }
        let issue = json!({
            "type": "assistant", "uuid": "a1", "parentUuid": null, "sessionId": "s",
            "cwd": "/work/repo", "gitBranch": "main",
            "timestamp": "2026-09-28T10:00:00.000Z", "isSidechain": false,
            "message": {"role": "assistant", "content": [tool_use]}
        });
        let answer = json!({
            "type": "user", "uuid": "u1", "parentUuid": "a1", "sessionId": "s",
            "cwd": "/work/repo", "gitBranch": "main",
            "timestamp": "2026-09-28T10:00:01.000Z", "isSidechain": false, "isMeta": false,
            "message": {"role": "user", "content": [result]},
            "toolUseResult": {"stdout": "", "stderr": "", "interrupted": false}
        });
        let lines = format!("{issue}\n{answer}\n");
        let outcome = parse_transcript(lines.as_bytes(), 0);
        assert!(commands(&outcome).is_empty(), "{id:?}");
        assert_eq!(outcome.counts.unnamed_tool_uses, 1, "{id:?}");
        assert_eq!(outcome.counts.unmatched_tool_results, 1, "{id:?}");
        // The unnamed tool use is never awaited, so it does not hold the
        // cursor back.
        assert_eq!(outcome.consumed, lines.len() as u64, "{id:?}");
    }
}

#[test]
fn unsupported_editing_tools_are_counted() {
    let lines = tool_exchange(
        "NotebookEdit",
        json!({"notebook_path": "/work/repo/a.ipynb", "new_source": "x = 1"}),
        ok_block(),
        Some(json!({"notebook_path": "/work/repo/a.ipynb", "new_source": "x = 1"})),
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert!(edits(&outcome).is_empty());
    assert_eq!(outcome.counts.unsupported_tool_uses, 1);
}

#[test]
fn command_categories_come_from_command_positions() {
    use supersigil_session::claude_code::classify_command;
    for (cmd, category) in [
        ("cat nextest-results.txt", CommandCategory::Other),
        ("echo \"cargo test\"", CommandCategory::Other),
        ("grep -r 'cargo test' docs", CommandCategory::Other),
        ("cargo fmt && cargo nextest run", CommandCategory::TestRun),
        ("RUST_LOG=debug cargo test", CommandCategory::TestRun),
        ("cargo nextest run", CommandCategory::TestRun),
        ("cargo test -p foo 2>&1 | tail -5", CommandCategory::TestRun),
        ("cd crates/x; go test ./...", CommandCategory::TestRun),
        ("pnpm test", CommandCategory::TestRun),
        ("npm test", CommandCategory::TestRun),
        ("pytest -q", CommandCategory::TestRun),
        ("vitest run", CommandCategory::TestRun),
        ("jest", CommandCategory::TestRun),
        ("cargo clippy --workspace", CommandCategory::Build),
        ("cargo build && git status", CommandCategory::Build),
        ("tsc --noEmit", CommandCategory::Build),
        ("pnpm build", CommandCategory::Build),
        ("npm run build", CommandCategory::Build),
        ("git status", CommandCategory::Git),
        ("cargo fmt || git diff", CommandCategory::Git),
        ("gh pr view 14", CommandCategory::Git),
        ("cargo fmt --all", CommandCategory::Other),
        ("echo git", CommandCategory::Other),
        ("rm old.txt", CommandCategory::Other),
    ] {
        assert_eq!(classify_command(cmd), category, "{cmd}");
    }
}

#[test]
fn results_without_a_timestamp_have_no_end_time() {
    let without_time = |lines: String| {
        let mut records: Vec<Value> = lines
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        records[1].as_object_mut().unwrap().remove("timestamp");
        let mut text = String::new();
        for record in &records {
            text.push_str(&record.to_string());
            text.push('\n');
        }
        text
    };

    let lines = without_time(tool_exchange(
        "Bash",
        json!({"command": "ls"}),
        ok_block(),
        Some(json!({"stdout": "a\n", "stderr": "", "interrupted": false})),
    ));
    let outcome = parse_transcript(lines.as_bytes(), 0);
    let commands = commands(&outcome);
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].ended, None);
    assert_eq!(commands[0].started.as_str(), "2026-09-28T10:00:00.000Z");

    // An edit keeps the issuing record's time instead.
    let lines = without_time(tool_exchange(
        "Write",
        json!({"file_path": "/work/repo/a.txt", "content": "x\n"}),
        ok_block(),
        Some(
            json!({"type": "create", "filePath": "/work/repo/a.txt", "content": "x\n", "structuredPatch": []}),
        ),
    ));
    let outcome = parse_transcript(lines.as_bytes(), 0);
    let edits = edits(&outcome);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].time.as_str(), "2026-09-28T10:00:00.000Z");
}

/// Creates four records in order: `Write` call `ta`, `Bash` call `tb`, then
/// the results of `ta` and `tb`.
fn interleaved_tool_uses() -> Vec<String> {
    let issue = |uuid: &str, block: Value| {
        json!({
            "type": "assistant", "uuid": uuid, "parentUuid": null, "sessionId": "s",
            "cwd": "/work/repo", "gitBranch": "main",
            "timestamp": "2026-09-28T10:00:00.000Z", "isSidechain": false,
            "message": {"role": "assistant", "content": [block]}
        })
        .to_string()
    };
    let result = |uuid: &str, id: &str, structured: Value| {
        json!({
            "type": "user", "uuid": uuid, "parentUuid": null, "sessionId": "s",
            "cwd": "/work/repo", "gitBranch": "main",
            "timestamp": "2026-09-28T10:00:01.000Z", "isSidechain": false, "isMeta": false,
            "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": id, "content": "ok"}
            ]},
            "toolUseResult": structured
        })
        .to_string()
    };
    vec![
        issue(
            "a1",
            json!({"type": "tool_use", "id": "ta", "name": "Write",
                "input": {"file_path": "/work/repo/a.txt", "content": "x\n"}}),
        ),
        issue(
            "a2",
            json!({"type": "tool_use", "id": "tb", "name": "Bash", "input": {"command": "ls"}}),
        ),
        result(
            "u1",
            "ta",
            json!({"type": "create", "filePath": "/work/repo/a.txt", "content": "x\n", "structuredPatch": []}),
        ),
        result(
            "u2",
            "tb",
            json!({"stdout": "a.txt\n", "stderr": "", "interrupted": false}),
        ),
    ]
}

#[test]
fn the_cutoff_never_splits_a_tool_use_from_its_result() {
    let lines = interleaved_tool_uses();
    // Cut after `ta`'s result while `tb` is still pending: consuming `ta`'s
    // issuing record without its result would lose the edit.
    let joined = |lines: &[String]| {
        let mut text = String::new();
        for line in lines {
            text.push_str(line);
            text.push('\n');
        }
        text
    };
    let cut = joined(&lines[..3]);
    let outcome = parse_transcript(cut.as_bytes(), 0);
    assert_eq!(outcome.consumed, 0);
    assert_eq!(outcome.next_ordinal, 0);
    assert!(outcome.observations.is_empty());

    let whole = joined(&lines);
    let outcome = parse_transcript(whole.as_bytes(), 0);
    assert_eq!(outcome.consumed, whole.len() as u64);
    assert_eq!(edits(&outcome).len(), 1);
    assert_eq!(commands(&outcome).len(), 1);
    assert_eq!(outcome.counts.unmatched_tool_results, 0);
}

#[test]
fn a_parent_agents_prompt_in_a_sidechain_is_a_delegation() {
    let prompt = "Implement the task. ".repeat(20);
    let lines = format!(
        "{}\n",
        json!({
            "type": "user", "uuid": "su1", "parentUuid": null, "sessionId": "s",
            "agentId": "agent1", "cwd": "/work/repo", "gitBranch": "main",
            "timestamp": "2026-09-28T10:00:00.000Z", "isSidechain": true, "isMeta": false,
            "message": {"role": "user", "content": prompt}
        })
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    let turns = turns(&outcome);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].role, Role::Delegation);
    assert_eq!(
        turns[0].excerpt.retained().map(|t| t.chars().count()),
        Some(240)
    );
    assert!(turns.iter().all(|t| t.role != Role::Human));
    assert_eq!(
        serde_json::to_value(Role::Delegation).unwrap(),
        json!("delegation")
    );
}

#[test]
fn heredoc_bodies_and_comments_are_not_commands() {
    use supersigil_session::claude_code::classify_command;
    let report =
        "cat <<'EOF' > report.md\nRan cargo test:\ncargo test\ntest result: ok. 3 passed\nEOF";
    for (cmd, category) in [
        (report, CommandCategory::Other),
        (
            "cargo nextest run <<EOF\ninput\nEOF",
            CommandCategory::TestRun,
        ),
        ("# cargo test\nls", CommandCategory::Other),
        ("ls # cargo test", CommandCategory::Other),
        // Unterminated: the rest is opaque.
        ("cat <<EOF\ncargo test", CommandCategory::Other),
        // `<<-` strips leading tabs from the terminator; after it, commands
        // count again.
        (
            "cat > f <<-\"END\"\n\tcargo test\n\tEND\ncargo test",
            CommandCategory::TestRun,
        ),
        (
            "git commit -F - <<EOF\ncargo test\nEOF\n",
            CommandCategory::Git,
        ),
        // A here-string is not a heredoc.
        ("cat <<< 'x'\ncargo test", CommandCategory::TestRun),
    ] {
        assert_eq!(classify_command(cmd), category, "{cmd:?}");
    }

    // Printed test output does not become a test verdict.
    let lines = tool_exchange(
        "Bash",
        json!({"command": report}),
        ok_block(),
        Some(json!({"stdout": "test result: ok. 3 passed\n", "stderr": "", "interrupted": false})),
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    let commands = commands(&outcome);
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].category, CommandCategory::Other);
    assert_eq!(commands[0].outcome, None);
}

#[test]
fn quoted_heredoc_delimiters_may_contain_spaces() {
    use supersigil_session::claude_code::classify_command;
    for (cmd, category) in [
        (
            "cat <<'END NOTE'\ncargo test\nEND NOTE\ncargo nextest run",
            CommandCategory::TestRun,
        ),
        (
            "cat <<'END NOTE'\ncargo test\nEND NOTE",
            CommandCategory::Other,
        ),
        (
            "cat <<-\"END NOTE\"\n\tcargo test\n\tEND NOTE\ngit status",
            CommandCategory::Git,
        ),
        // `\EOF` is the word `EOF`.
        (
            "cat <<\\EOF\ncargo test\nEOF\ngit status",
            CommandCategory::Git,
        ),
        // The terminator must match exactly.
        (
            "cat <<'END NOTE'\ncargo test\nEND\ngit status",
            CommandCategory::Other,
        ),
        // An unterminated quote leaves the rest opaque.
        ("cat <<'END NOTE\ncargo test", CommandCategory::Other),
    ] {
        assert_eq!(classify_command(cmd), category, "{cmd:?}");
    }
}

#[test]
fn a_tool_result_in_a_user_record_without_a_uuid_still_resolves() {
    let lines = tool_exchange(
        "Write",
        json!({"file_path": "/work/repo/a.txt", "content": "x\n"}),
        ok_block(),
        Some(
            json!({"type": "create", "filePath": "/work/repo/a.txt", "content": "x\n", "structuredPatch": []}),
        ),
    );
    let mut records: Vec<Value> = lines
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    records[1].as_object_mut().unwrap().remove("uuid");
    let mut text = String::new();
    for record in &records {
        text.push_str(&record.to_string());
        text.push('\n');
    }
    let outcome = parse_transcript(text.as_bytes(), 0);
    let edits = edits(&outcome);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].turn.as_str(), "a1");
    assert_eq!(outcome.consumed, text.len() as u64);
    assert_eq!(outcome.counts.abandoned_tool_uses, 0);
    assert_eq!(
        outcome.counts.unknown_records.get("user-without-uuid"),
        Some(&1)
    );
    // No turn is staged for the record without a uuid.
    assert_eq!(turns(&outcome).len(), 1);
}

#[test]
fn records_without_cwd_inherit_the_first_cwd() {
    let exchange = tool_exchange(
        "Write",
        json!({"file_path": "/work/repo/x.rs", "content": "x\n"}),
        ok_block(),
        Some(json!({"type": "create", "filePath": "/work/repo/x.rs", "content": "x\n", "structuredPatch": []})),
    )
    .replace(r#""cwd":"/work/repo","#, "");
    assert!(!exchange.contains("\"cwd\""));
    let lines = format!("{}\n{exchange}", human("u0", "s"));

    let outcome = parse_transcript(lines.as_bytes(), 0);
    let found = edits(&outcome);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].checkout, PathBuf::from("/work/repo"));
    assert_eq!(found[0].path, PathBuf::from("x.rs"));
    assert_eq!(outcome.counts.outside_checkout, 0);

    // With no record naming a working directory, the edit cannot be placed.
    let outcome = parse_transcript(exchange.as_bytes(), 0);
    assert!(edits(&outcome).is_empty());
    assert_eq!(outcome.counts.outside_checkout, 1);
}

/// Parses a Bash call with `command` and structured `result`, returning its
/// one command.
fn bash(command: &str, result: Value) -> supersigil_record::observations::Command {
    let lines = tool_exchange(
        "Bash",
        json!({"command": command}),
        ok_block(),
        Some(result),
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    let commands = commands(&outcome);
    assert_eq!(commands.len(), 1, "expected one command from {lines}");
    commands[0].clone()
}

/// A structured Bash result carrying `report` as its `bashEditDiff`.
fn with_report(report: &Value) -> Value {
    json!({"stdout": "", "stderr": "", "interrupted": false, "isImage": false,
        "noOutputExpected": false, "bashEditDiff": report})
}

/// The change report of a Bash call whose result carries `report`.
fn report_of(report: &Value) -> ChangeReport {
    match bash("make", with_report(report)).changes {
        Material::Retained(report) => report,
        other => panic!("no report: {other:?}"),
    }
}

fn hunk_json() -> Value {
    json!({"oldStart": 1, "oldLines": 1, "newStart": 1, "newLines": 2,
        "lines": [" a", "+\tb"]})
}

fn hunk() -> Hunk {
    Hunk {
        old_start: 1,
        old_lines: 1,
        new_start: 1,
        new_lines: 2,
        lines: vec![" a".to_owned(), "+\tb".to_owned()],
    }
}

#[test]
fn a_change_report_lists_every_changed_file_with_what_the_harness_stated() {
    let report = report_of(&json!({
        "changedFiles": ["/work/repo/src/new.rs", "/work/repo/src/lib.rs",
            "/work/repo/old.txt", "/work/repo/Cargo.lock"],
        "files": [
            {"filePath": "/work/repo/src/new.rs", "created": true, "hunks": [hunk_json()]},
            {"filePath": "/work/repo/src/lib.rs", "hunks": [hunk_json()]},
            {"filePath": "/work/repo/old.txt", "deleted": true, "hunks": []},
        ],
        "moreFiles": 1,
    }));
    let listed: Vec<(PathBuf, ChangeKind)> = report
        .files
        .iter()
        .map(|f| (f.path.clone(), f.kind))
        .collect();
    assert_eq!(
        listed,
        vec![
            (PathBuf::from("src").join("new.rs"), ChangeKind::Created),
            (PathBuf::from("src").join("lib.rs"), ChangeKind::Modified),
            (PathBuf::from("old.txt"), ChangeKind::Deleted),
            (PathBuf::from("Cargo.lock"), ChangeKind::NotStated),
        ]
    );
    // Hunks are kept as given: display text, with the harness's literal tab.
    assert_eq!(report.files[0].patch, Material::Retained(vec![hunk()]));
    assert_eq!(report.files[2].patch, Material::Retained(Vec::new()));
    assert_eq!(
        report.files[3].patch,
        Material::unavailable("no entry in the harness report")
    );
    // `moreFiles` counted the one listed path without an entry.
    assert_eq!(report.unlisted, 0);
    assert_eq!(report.outside, 0);
    assert!(report.flags.is_empty());
}

#[test]
fn a_result_without_a_change_report_says_nothing_about_changes() {
    let plain = json!({"stdout": "", "stderr": "", "interrupted": false});
    assert_eq!(
        bash("ls", plain).changes,
        Material::unavailable("no change report")
    );
    // A failed call's structured result is a string.
    let failed = only_command(
        json!({"type": "tool_result", "tool_use_id": "t1", "is_error": true, "content": "boom"}),
        Some(json!("Error: boom")),
    );
    assert_eq!(failed.changes, Material::unavailable("no change report"));
    assert_eq!(
        only_command(ok_block(), None).changes,
        Material::unavailable("no change report")
    );
}

#[test]
fn an_abandoned_command_has_no_change_report() {
    let issue = json!({
        "type": "assistant", "uuid": "a1", "parentUuid": null, "sessionId": "s",
        "cwd": "/work/repo", "timestamp": "2026-09-28T10:00:00.000Z", "isSidechain": false,
        "message": {"role": "assistant", "content": [
            {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "make"}}
        ]}
    });
    let typed = human("u1", "s");
    let lines = format!("{issue}\n{typed}\n");
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert_eq!(
        commands(&outcome)[0].changes,
        Material::unavailable("no result recorded")
    );
}

#[test]
fn reported_paths_outside_the_working_directory_are_counted_not_kept() {
    let report = report_of(&json!({
        "changedFiles": ["/work/other/a.rs", "/work/repo/../secret", "/work/repo",
            "relative.rs", "/work/repo/kept.rs"],
        "files": [{"filePath": "/work/other/a.rs", "hunks": []}],
        "moreFiles": 4,
    }));
    assert_eq!(report.outside, 4);
    assert_eq!(report.files.len(), 1);
    assert_eq!(report.files[0].path, PathBuf::from("kept.rs"));
}

#[test]
fn changed_files_the_harness_did_not_name_are_counted() {
    // 200 listed paths, five entries, and 726 files without an entry: 531
    // changed files have no name in the report.
    let listed: Vec<String> = (0..200).map(|i| format!("/work/repo/f{i}.rs")).collect();
    let entries: Vec<Value> = listed[..5]
        .iter()
        .map(|path| json!({"filePath": path, "hunks": []}))
        .collect();
    let report = report_of(&json!({"changedFiles": listed, "files": entries, "moreFiles": 726}));
    assert_eq!(report.files.len(), 200);
    assert_eq!(report.unlisted, 531);
    // A count below the listed paths never goes negative.
    let short =
        report_of(&json!({"changedFiles": ["/work/repo/a.rs"], "files": [], "moreFiles": 0}));
    assert_eq!(short.unlisted, 0);
}

#[test]
fn an_entry_the_harness_did_not_list_is_still_a_changed_file() {
    let report = report_of(&json!({
        "changedFiles": ["/work/repo/a.rs"],
        "files": [{"filePath": "/work/repo/b.rs", "hunks": []},
            {"filePath": "/work/repo/a.rs", "hunks": []}],
        "moreFiles": 0,
    }));
    let paths: Vec<PathBuf> = report.files.iter().map(|f| f.path.clone()).collect();
    assert_eq!(paths, vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")]);
    // Without `changedFiles`, the entries are the list.
    let entries_only = report_of(&json!({"files": [{"filePath": "/work/repo/b.rs", "hunks": []}]}));
    assert_eq!(entries_only.files.len(), 1);
    // An empty `files` array names no file.
    assert_eq!(report_of(&json!({"files": []})), ChangeReport::default());
}

#[test]
fn a_path_reported_twice_is_one_changed_file() {
    let report = report_of(&json!({
        "changedFiles": ["/work/repo/a.rs", "/work/repo/a.rs", "/work/repo/b.rs"],
        "files": [{"filePath": "/work/repo/b.rs", "created": true, "hunks": []},
            {"filePath": "/work/repo/b.rs", "deleted": true, "hunks": []}],
        "moreFiles": 3,
    }));
    let listed: Vec<(PathBuf, ChangeKind)> = report
        .files
        .iter()
        .map(|f| (f.path.clone(), f.kind))
        .collect();
    // The first entry for a path is kept.
    assert_eq!(
        listed,
        vec![
            (PathBuf::from("a.rs"), ChangeKind::NotStated),
            (PathBuf::from("b.rs"), ChangeKind::Created)
        ]
    );
    // One distinct listed path has no entry.
    assert_eq!(report.unlisted, 2);
}

#[test]
fn two_spellings_of_one_path_are_one_changed_file() {
    // The first entry determines the change. Another spelling cannot
    // change deleted to created.
    let report = report_of(&json!({
        "changedFiles": ["/work/repo/src/a.rs", "/work/repo/src//a.rs", "/work/repo/./src/a.rs"],
        "files": [{"filePath": "/work/repo/src/a.rs", "deleted": true, "hunks": []},
            {"filePath": "/work/repo/src//a.rs", "created": true, "hunks": []}],
        "moreFiles": 0,
    }));
    let listed: Vec<(PathBuf, ChangeKind)> = report
        .files
        .iter()
        .map(|f| (f.path.clone(), f.kind))
        .collect();
    assert_eq!(
        listed,
        vec![(PathBuf::from("src/a.rs"), ChangeKind::Deleted)]
    );
    // Outside paths are told apart as written.
    let outside = report_of(&json!({
        "changedFiles": ["/other/a.rs", "/other/a.rs", "/other/b.rs"],
        "files": [{"filePath": "/other/b.rs", "hunks": []}],
        "moreFiles": 1,
    }));
    assert_eq!(outside.outside, 2);
    assert_eq!(outside.unlisted, 0);
}

#[test]
fn a_huge_change_report_is_read_in_time_proportional_to_its_size() {
    let count = 100_000;
    let listed: Vec<String> = (0..count).map(|i| format!("/work/repo/f{i}.rs")).collect();
    let entries: Vec<Value> = listed
        .iter()
        .map(|path| json!({"filePath": path, "hunks": []}))
        .collect();
    let result = with_report(&json!({"changedFiles": listed, "files": entries, "moreFiles": 0}));
    let started = std::time::Instant::now();
    let Material::Retained(report) = bash("make", result).changes else {
        panic!("no report");
    };
    assert_eq!(report.files.len(), count);
    assert!(report.files.iter().all(|f| f.kind == ChangeKind::Modified));
    // Scanning entries per listed path would take minutes here.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn flags_the_harness_set_are_kept_by_name() {
    let report = report_of(&json!({"files": [], "moreFiles": 0, "shared": true,
        "unavailable": true, "later": false}));
    assert_eq!(
        report.flags.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["shared", "unavailable"]
    );
    assert!(report.files.is_empty());
}

#[test]
fn an_entry_states_no_kind_when_marked_both_created_and_deleted() {
    let report = report_of(&json!({
        "changedFiles": ["/work/repo/a.rs", "/work/repo/b.rs", "/work/repo/c.rs"],
        "files": [
            {"filePath": "/work/repo/a.rs", "created": true, "deleted": true, "hunks": []},
            {"filePath": "/work/repo/b.rs", "hunks": [{"oldStart": "one"}]},
            {"filePath": "/work/repo/c.rs"},
        ],
        "moreFiles": 0,
    }));
    assert_eq!(report.files[0].kind, ChangeKind::NotStated);
    assert_eq!(
        report.files[1].patch,
        Material::unavailable("unreadable hunks")
    );
    assert_eq!(
        report.files[2].patch,
        Material::unavailable("no hunks in the entry")
    );
}

#[test]
fn a_change_report_of_another_shape_is_unreadable() {
    for report in [
        json!("changed"),
        json!(["/work/repo/a.rs"]),
        // An object without `files` is another layout, not an empty report.
        json!({}),
        json!({"unexpected": "another layout"}),
        json!({"changedFiles": ["/work/repo/a.rs"], "moreFiles": 1}),
        json!({"changedFiles": null, "files": []}),
        // The report's own flags are booleans.
        json!({"files": [], "shared": "true"}),
        json!({"files": [], "unavailable": {}}),
        // A flag of another type states nothing about the file.
        json!({"files": [{"filePath": "/work/repo/a.rs", "created": true, "deleted": "true",
            "hunks": []}]}),
        json!({"files": [{"filePath": "/work/repo/a.rs", "created": 1, "hunks": []}]}),
        json!({"files": [{"filePath": "/work/repo/a.rs", "created": null, "hunks": []}]}),
        json!({"changedFiles": "/work/repo/a.rs"}),
        json!({"changedFiles": [1]}),
        json!({"files": {"filePath": "/work/repo/a.rs"}}),
        json!({"files": [{"hunks": []}]}),
        json!({"files": [{"filePath": 7, "hunks": []}]}),
        json!({"files": [], "moreFiles": "many"}),
        json!({"files": [], "moreFiles": -1}),
    ] {
        assert_eq!(
            bash("make", with_report(&report)).changes,
            Material::unavailable("unreadable change report"),
            "{report}"
        );
    }
}

/// Parses a Bash `command` whose result reports `entries` as `files`,
/// each also listed as changed.
fn shell(command: &str, entries: &Value) -> ParseOutcome {
    let listed: Vec<Value> = entries
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["filePath"].clone())
        .collect();
    let report = json!({"changedFiles": listed, "files": entries, "moreFiles": 0});
    shell_with(command, ok_block(), Some(with_report(&report)))
}

/// Parses a Bash call running `command` with the given result.
fn shell_with(command: &str, result_block: Value, result: Option<Value>) -> ParseOutcome {
    let lines = tool_exchange("Bash", json!({"command": command}), result_block, result);
    parse_transcript(lines.as_bytes(), 0)
}

fn created(path: &str) -> Value {
    json!({"filePath": path, "created": true, "hunks": []})
}

fn modified(path: &str) -> Value {
    json!({"filePath": path, "hunks": []})
}

fn shell_edit_id(index: usize) -> EventId {
    EventId::derive("edit", &SessionId::new("s"), &format!("t1#{index}"))
}

#[test]
fn a_confirmed_heredoc_write_is_an_edit_beside_its_command() {
    let command = "mkdir -p src && cat > src/new.rs <<'EOF'\nfn a() {}\n\t$x\nEOF\ncargo fmt\n";
    let outcome = shell(command, &json!([created("/work/repo/src/new.rs")]));
    let recorded = commands(&outcome)[0].clone();
    assert_eq!(recorded.cmd, command);
    assert_eq!(
        edits(&outcome),
        vec![&Edit {
            id: shell_edit_id(0),
            turn: TurnId::new("a1"),
            session: SessionId::new("s"),
            path: PathBuf::from("src/new.rs"),
            before: FileState::Absent,
            after: FileState::unknown(),
            patch: Material::unavailable("the change report describes the whole command"),
            old_text: unavailable("write replaces the whole file"),
            new_text: Material::Retained("fn a() {}\n\t$x\n".to_owned()),
            replace_all: false,
            operation: EditOperation::Write,
            origin: EditOrigin::Shell {
                command: recorded.id.clone(),
                index: 0,
            },
            checkout: PathBuf::from("/work/repo"),
            time: Timestamp::new("2026-09-28T10:00:01.000Z"),
            source_ordinal: 0,
            agent_id: None,
            transcript: None,
        }]
    );
    // The command comes first, at the same transcript position.
    let kinds: Vec<&str> = outcome
        .observations
        .iter()
        .filter_map(|o| match o {
            Observation::Command(_) => Some("command"),
            Observation::Edit(_) => Some("edit"),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, vec!["command", "edit"]);
    assert_eq!(recorded.source_ordinal, 0);
    assert!(outcome.counts.is_empty(), "{:?}", outcome.counts);
}

#[test]
fn the_writes_of_one_command_keep_their_place_in_it() {
    // Three writes to two files, plus one the report does not confirm.
    // Each edit keeps its position among all writes, so `a.rs`'s writes
    // stay ordered despite the writes between them.
    let command = "cat >> a.rs <<'E'\nx\nE\ncat > b.rs <<'E'\ny\nE\n\
        cat >> gone.rs <<'E'\nz\nE\ncat >> a.rs <<'E'\nx\nE\n";
    let outcome = shell(
        command,
        &json!([modified("/work/repo/a.rs"), created("/work/repo/b.rs")]),
    );
    let placed: Vec<(PathBuf, u64, u64)> = edits(&outcome)
        .iter()
        .map(|edit| {
            let EditOrigin::Shell { index, .. } = edit.origin else {
                panic!("{:?}", edit.origin);
            };
            (edit.path.clone(), index, edit.source_ordinal)
        })
        .collect();
    assert_eq!(
        placed,
        vec![
            (PathBuf::from("a.rs"), 0, 0),
            (PathBuf::from("b.rs"), 1, 0),
            (PathBuf::from("a.rs"), 3, 0),
        ]
    );
    assert_eq!(edits(&outcome)[2].id, shell_edit_id(3));
    assert_eq!(outcome.counts.unconfirmed_shell_writes, 1);
}

#[test]
fn a_heredoc_over_an_existing_file_has_an_unknown_before_state() {
    let outcome = shell(
        "cat > src/lib.rs <<'EOF'\nnew\nEOF\n",
        &json!([modified("/work/repo/src/lib.rs")]),
    );
    let edit = edits(&outcome)[0];
    assert_eq!(edit.operation, EditOperation::Write);
    assert_eq!(edit.before, FileState::unknown());
    assert_eq!(edit.after, FileState::unknown());
}

#[test]
fn a_heredoc_append_is_an_append_edit() {
    let outcome = shell(
        "cat >> notes.txt <<'EOF'\nmore\nEOF\n",
        &json!([modified("/work/repo/notes.txt")]),
    );
    let edit = edits(&outcome)[0];
    assert_eq!(edit.operation, EditOperation::Append);
    assert_eq!(edit.before, FileState::unknown());
    assert_eq!(edit.new_text, Material::Retained("more\n".to_owned()));
    assert_eq!(
        edit.old_text,
        unavailable("append adds to the end of the file")
    );
    // An append that created the file starts from nothing.
    let outcome = shell(
        "cat >> notes.txt <<'EOF'\nmore\nEOF\n",
        &json!([created("/work/repo/notes.txt")]),
    );
    assert_eq!(edits(&outcome)[0].before, FileState::Absent);
}

#[test]
fn only_the_first_write_of_a_command_finds_a_created_file_absent() {
    let command = "cat > a.rs <<'A'\n1\nA\ncat >> a.rs <<'B'\n2\nB\ncat > b.rs <<'C'\n3\nC\n";
    let outcome = shell(
        command,
        &json!([created("/work/repo/a.rs"), created("/work/repo/b.rs")]),
    );
    let found: Vec<(&EventId, &str, EditOperation, &FileState)> = edits(&outcome)
        .into_iter()
        .map(|e| (&e.id, e.path.to_str().unwrap(), e.operation, &e.before))
        .collect();
    assert_eq!(
        found,
        vec![
            (
                &shell_edit_id(0),
                "a.rs",
                EditOperation::Write,
                &FileState::Absent
            ),
            (
                &shell_edit_id(1),
                "a.rs",
                EditOperation::Append,
                &FileState::unknown()
            ),
            (
                &shell_edit_id(2),
                "b.rs",
                EditOperation::Write,
                &FileState::Absent
            ),
        ]
    );
}

#[test]
fn an_empty_append_is_no_edit_but_still_creates_the_file() {
    let command = "cat >> a.rs <<'A'\nA\ncat >> a.rs <<'B'\n2\nB\n";
    let outcome = shell(command, &json!([created("/work/repo/a.rs")]));
    let recorded = edits(&outcome);
    assert_eq!(recorded.len(), 1);
    // The id counts all command writes, recorded or not.
    assert_eq!(recorded[0].id, shell_edit_id(1));
    assert_eq!(recorded[0].before, FileState::unknown());
    assert!(outcome.counts.is_empty());
    // An empty `>` is an edit: it truncates.
    let outcome = shell(
        "cat > a.rs <<'A'\nA\n",
        &json!([modified("/work/repo/a.rs")]),
    );
    assert_eq!(
        edits(&outcome)[0].new_text,
        Material::Retained(String::new())
    );
}

#[test]
fn a_heredoc_write_resolves_against_the_working_directory_and_earlier_cds() {
    let outcome = shell(
        "cd ./crates/a && cat > src/x.rs <<'EOF'\nx\nEOF\n",
        &json!([created("/work/repo/crates/a/src/x.rs")]),
    );
    assert_eq!(edits(&outcome)[0].path, PathBuf::from("crates/a/src/x.rs"));
    // The same text is unconfirmed when the harness reports another file.
    let outcome = shell(
        "cd ./crates/a && cat > src/x.rs <<'EOF'\nx\nEOF\n",
        &json!([created("/work/repo/src/x.rs")]),
    );
    assert!(edits(&outcome).is_empty());
    assert_eq!(outcome.counts.unconfirmed_shell_writes, 1);
}

#[test]
fn a_heredoc_write_outside_the_working_directory_is_counted_as_outside() {
    let command = "cat > /elsewhere/x <<'A'\n1\nA\ncd .. && cat > y <<'B'\n2\nB\n";
    let outcome = shell(
        command,
        &json!([created("/elsewhere/x"), created("/work/y")]),
    );
    assert!(edits(&outcome).is_empty());
    assert_eq!(outcome.counts.outside_checkout, 2);
    assert_eq!(outcome.counts.unconfirmed_shell_writes, 0);
}

#[test]
fn a_heredoc_write_without_confirmation_is_counted_not_recorded() {
    let command = "cat > a.rs <<'EOF'\nx\nEOF\n";
    let entry = json!([created("/work/repo/a.rs")]);
    let report = |entries: &Value, listed: Value| json!({"changedFiles": listed, "files": entries, "moreFiles": 0});
    let confirmed = with_report(&report(&entry, json!(["/work/repo/a.rs"])));
    let mut interrupted = confirmed.clone();
    interrupted["interrupted"] = json!(true);
    let mut background = confirmed.clone();
    background["backgroundTaskId"] = json!("b1");
    let mut unstated = confirmed.clone();
    unstated.as_object_mut().unwrap().remove("interrupted");
    let mut misstated = confirmed.clone();
    misstated["interrupted"] = json!("false");
    let error_block =
        json!({"type": "tool_result", "tool_use_id": "t1", "is_error": true, "content": "x"});
    let cases: Vec<(&str, Value, Option<Value>)> = vec![
        ("no structured result", ok_block(), None),
        (
            "no report",
            ok_block(),
            Some(json!({"stdout": "", "stderr": "", "interrupted": false})),
        ),
        (
            "a failed call",
            error_block.clone(),
            Some(json!("Error: x")),
        ),
        (
            "a failed call that still has a report",
            error_block,
            Some(confirmed.clone()),
        ),
        ("an interrupted call", ok_block(), Some(interrupted)),
        ("a background call", ok_block(), Some(background)),
        (
            "a result that does not say whether it was interrupted",
            ok_block(),
            Some(unstated),
        ),
        (
            "a result whose interrupted field is not a boolean",
            ok_block(),
            Some(misstated),
        ),
        (
            "a report with an entry for the file but no listing of it",
            ok_block(),
            Some(with_report(&report(&entry, json!([])))),
        ),
        (
            "a report listing another spelling's file as deleted first",
            ok_block(),
            Some(with_report(&report(
                &json!([{"filePath": "/work/repo/a.rs", "deleted": true, "hunks": []},
                    created("/work/repo//a.rs")]),
                json!(["/work/repo/a.rs"]),
            ))),
        ),
        (
            "a report naming another file",
            ok_block(),
            Some(with_report(&report(
                &json!([created("/work/repo/b.rs")]),
                json!(["/work/repo/b.rs"]),
            ))),
        ),
        (
            "a report naming the file without an entry",
            ok_block(),
            Some(with_report(&report(&json!([]), json!(["/work/repo/a.rs"])))),
        ),
        (
            "a report marking the file deleted",
            ok_block(),
            Some(with_report(&report(
                &json!([{"filePath": "/work/repo/a.rs", "deleted": true, "hunks": []}]),
                json!(["/work/repo/a.rs"]),
            ))),
        ),
        (
            "an unreadable report",
            ok_block(),
            Some(with_report(&json!({"changedFiles": "/work/repo/a.rs"}))),
        ),
        (
            "a report whose own flag is not a boolean",
            ok_block(),
            Some(with_report(&json!({"changedFiles": ["/work/repo/a.rs"],
                "files": [created("/work/repo/a.rs")], "shared": "true"}))),
        ),
    ];
    for (case, block, result) in cases {
        let outcome = shell_with(command, block, result);
        assert!(edits(&outcome).is_empty(), "{case}");
        assert_eq!(outcome.counts.unconfirmed_shell_writes, 1, "{case}");
        assert_eq!(commands(&outcome).len(), 1, "{case}");
    }
    // Control: a confirming report turns the same call into an edit.
    let outcome = shell_with(command, ok_block(), Some(confirmed));
    assert_eq!(edits(&outcome).len(), 1);
    assert_eq!(outcome.counts.unconfirmed_shell_writes, 0);
}

#[test]
fn a_flagged_report_is_kept_and_confirms_no_heredoc_write() {
    // Report flags (`shared`, `unavailable`, or unknown) have
    // unestablished meanings. Flagged reports remain command observations
    // and confirm nothing.
    let command = "cat > a.rs <<'EOF'\nx\nEOF\n";
    for flag in ["shared", "unavailable", "partial"] {
        let mut report = json!({"changedFiles": ["/work/repo/a.rs"],
            "files": [created("/work/repo/a.rs")]});
        report[flag] = json!(true);
        let outcome = shell_with(command, ok_block(), Some(with_report(&report)));
        assert!(edits(&outcome).is_empty(), "{flag}");
        assert_eq!(outcome.counts.unconfirmed_shell_writes, 1, "{flag}");
        let recorded = commands(&outcome)[0].changes.retained().unwrap();
        assert_eq!(recorded.flags, BTreeSet::from([flag.to_owned()]), "{flag}");
        assert_eq!(recorded.files.len(), 1, "{flag}");
        // The same report with the flag unset confirms the write.
        report[flag] = json!(false);
        let outcome = shell_with(command, ok_block(), Some(with_report(&report)));
        assert_eq!(edits(&outcome).len(), 1, "{flag}");
    }
}

#[test]
fn a_command_of_many_heredoc_writes_is_read_in_time_proportional_to_its_size() {
    use std::fmt::Write as _;

    let count = 20_000;
    let mut command = String::new();
    let mut entries = Vec::new();
    for i in 0..count {
        write!(command, "cat >> f{}.rs <<'E'\n{i}\nE\n", i % 1000).unwrap();
    }
    for i in 0..1000 {
        entries.push(created(&format!("/work/repo/f{i}.rs")));
    }
    let started = std::time::Instant::now();
    let outcome = shell(&command, &Value::Array(entries));
    let recorded = edits(&outcome);
    assert_eq!(recorded.len(), count);
    // Each file's first append finds it absent; the nineteen after it do not.
    let absent = recorded
        .iter()
        .filter(|e| e.before == FileState::Absent)
        .count();
    assert_eq!(absent, 1000);
    // Scanning the report or prior writes per write would take minutes
    // here.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
}

/// A file entry with one hunk at `start` on both sides, holding
/// `lines`.
fn entry(path: &str, is_created: bool, start: u32, lines: &[&str]) -> Value {
    let count = |markers: &[char]| {
        lines
            .iter()
            .filter(|line| line.starts_with(markers))
            .count()
    };
    let mut entry = json!({"filePath": path, "hunks": [{"oldStart": start,
        "oldLines": count(&[' ', '-']), "newStart": start, "newLines": count(&[' ', '+']),
        "lines": lines}]});
    if is_created {
        entry["created"] = json!(true);
    }
    entry
}

/// The edits and the unconfirmed count of `command` with one file entry.
fn confirmed_with(command: &str, entry: &Value) -> (usize, u64) {
    let outcome = shell(command, &json!([entry]));
    (
        edits(&outcome).len(),
        outcome.counts.unconfirmed_shell_writes,
    )
}

#[test]
fn a_reported_diff_that_contradicts_a_heredoc_write_leaves_it_unconfirmed() {
    // `false &&` skips the heredoc; `echo` creates the file. The harness
    // reports it created with `echo`'s line.
    let skipped = "false && cat > a.rs <<'EOF'\nx\nEOF\necho y > a.rs\n";
    let reported = entry("/work/repo/a.rs", true, 1, &["+y"]);
    assert_eq!(confirmed_with(skipped, &reported), (0, 1));
    // A statement after the heredoc changed what it wrote.
    let reformatted = "cat > a.rs <<'EOF'\nfn  a(){}\nEOF\nrustfmt a.rs\n";
    let reported = entry("/work/repo/a.rs", true, 1, &["+fn a() {}"]);
    assert_eq!(confirmed_with(reformatted, &reported), (0, 1));
    // The diff shows more of the file than the heredoc wrote.
    let write = "cat > a.rs <<'EOF'\nx\nEOF\n";
    let longer = entry("/work/repo/a.rs", true, 1, &["+x", "+z"]);
    assert_eq!(confirmed_with(write, &longer), (0, 1));
    // An agreeing diff alone neither confirms nor rejects a write.
    let agreeing = entry("/work/repo/a.rs", true, 1, &["+x"]);
    assert_eq!(confirmed_with(write, &agreeing), (1, 0));
}

#[test]
fn an_overwrite_is_checked_at_the_lines_the_diff_shows() {
    let command = "cat > a.rs <<'EOF'\none\ntwo\nthree\nEOF\n";
    // Context and added lines are the file after the command; removed
    // lines are not, nor is a marker that follows one.
    let agreeing = entry(
        "/work/repo/a.rs",
        false,
        2,
        &["-old", "\\ No newline at end of file", "+two", " three"],
    );
    assert_eq!(confirmed_with(command, &agreeing), (1, 0));
    // A new-side no-newline marker contradicts heredoc text, which ends
    // with a newline.
    for lines in [
        &["+two", "+three", "\\ No newline at end of file"][..],
        &["+two", " three", "\\ No newline at end of file"],
    ] {
        let unterminated = entry("/work/repo/a.rs", false, 2, lines);
        assert_eq!(confirmed_with(command, &unterminated), (0, 1), "{lines:?}");
    }
    for lines in [
        &["+two", " four"][..],
        &["+three"],
        &[" two", " three", "+four"],
    ] {
        let contradicting = entry("/work/repo/a.rs", false, 2, lines);
        assert_eq!(confirmed_with(command, &contradicting), (0, 1), "{lines:?}");
    }
    // A hunk with an impossible start also contradicts the write.
    let misplaced = entry("/work/repo/a.rs", false, 0, &["+one"]);
    assert_eq!(confirmed_with(command, &misplaced), (0, 1));
}

#[test]
fn an_append_must_end_the_file_the_diff_shows_the_end_of() {
    let command = "cat >> a.rs <<'EOF'\ny\nz\nEOF\n";
    // Less context after the last change than before the first marks the
    // file's end: the file ended before the context did.
    for lines in [
        &[" x", "+y", "+z"][..],
        // An appended line may appear as context aligned with an equal
        // old line.
        &[" a", "+w", " y", "+z"],
    ] {
        let agreeing = entry("/work/repo/a.rs", false, 1, lines);
        assert_eq!(confirmed_with(command, &agreeing), (1, 0), "{lines:?}");
    }
    for lines in [
        // Something follows the appended text, or it is not there.
        &[" x", "+y", "+z", "+later"][..],
        &[" w", " x", "+y", "+z", " kept"],
        &[" x", "+y"],
        &[" w", "-x"],
    ] {
        let contradicting = entry("/work/repo/a.rs", false, 1, lines);
        assert_eq!(confirmed_with(command, &contradicting), (0, 1), "{lines:?}");
    }
    // If fewer lines are shown than appended, compare only those shown.
    let command = "cat >> a.rs <<'EOF'\na\nb\nc\nd\nEOF\n";
    let shorter = entry("/work/repo/a.rs", false, 7, &[" b", " c", "-x", "+d"]);
    assert_eq!(confirmed_with(command, &shorter), (1, 0));
    let other = entry("/work/repo/a.rs", false, 7, &[" b", " q", "-x", "+d"]);
    assert_eq!(confirmed_with(command, &other), (0, 1));
}

#[test]
fn an_append_longer_than_the_file_the_diff_shows_contradicts() {
    // Another statement left a two-line file: the hunk starts at line 1
    // and reaches the end, so the file cannot hold four appended lines.
    let command = "cat >> a.rs <<'EOF'\none\ntwo\nthree\nfour\nEOF\n";
    let lines = &[" three", "-old", "+four"];
    let short = entry("/work/repo/a.rs", false, 1, lines);
    assert_eq!(confirmed_with(command, &short), (0, 1));
    // The same hunk farther down leaves room for unseen lines.
    let room = entry("/work/repo/a.rs", false, 3, lines);
    assert_eq!(confirmed_with(command, &room), (1, 0));
}

#[test]
fn a_hunk_that_may_stop_before_the_end_of_the_file_does_not_speak_against_an_append() {
    let command = "cat >> a.rs <<'EOF'\ny\nz\nEOF\n";
    // Equal context before and after leaves the end unknown. Appended
    // lines may follow beyond the shown lines.
    for lines in [
        &[" x", "+y", "+z", " kept"][..],
        &["+top", " x"],
        &["-x"],
        &["-w", "+later"],
        &[" a", " b", " c", "+w", " d", " e", " f"],
    ] {
        let silent = entry("/work/repo/a.rs", false, 1, lines);
        assert_eq!(confirmed_with(command, &silent), (1, 0), "{lines:?}");
    }
    // Unknown lines or fewer old lines than the header counts also
    // leave the file's end unknown.
    let mut unknown = entry("/work/repo/a.rs", false, 1, &[" x", "+w"]);
    assert_eq!(confirmed_with(command, &unknown), (0, 1));
    unknown["hunks"][0]["lines"]
        .as_array_mut()
        .unwrap()
        .push(json!("? unknown"));
    assert_eq!(confirmed_with(command, &unknown), (1, 0));
    let mut cut = entry("/work/repo/a.rs", false, 1, &[" x", "+w"]);
    cut["hunks"][0]["oldLines"] = json!(4);
    assert_eq!(confirmed_with(command, &cut), (1, 0));
    // `sed` replaces the first of four `x` lines with four `y` lines;
    // the heredoc appends four `x` lines. The diff aligns three appended
    // lines with old ones and stops one line before the end.
    let command = "sed -i '1s/x/y\\ny\\ny\\ny/' a.rs\ncat >> a.rs <<'E'\nx\nx\nx\nx\nE\n";
    let aligned = entry(
        "/work/repo/a.rs",
        false,
        1,
        &["+y", "+y", "+y", "+y", " x", " x", " x"],
    );
    assert_eq!(confirmed_with(command, &aligned), (1, 0));
}

#[test]
fn a_file_the_diff_shows_without_a_final_newline_contradicts_a_heredoc() {
    // `printf x` created the file; the skipped heredoc would have written
    // `x` and a newline.
    let skipped = "false && cat > a.rs <<'E'\nx\nE\nprintf x > a.rs\n";
    let reported = entry(
        "/work/repo/a.rs",
        true,
        1,
        &["+x", "\\ No newline at end of file"],
    );
    assert_eq!(confirmed_with(skipped, &reported), (0, 1));
    let appended = "cat >> a.rs <<'E'\ny\nE\n";
    let reported = entry(
        "/work/repo/a.rs",
        false,
        1,
        &[" x", "+y", "\\ No newline at end of file"],
    );
    assert_eq!(confirmed_with(appended, &reported), (0, 1));
    // An empty file has no line to lack a newline, and no hunk lines.
    let emptied = "cat > a.rs <<'E'\nE\n";
    let reported = entry("/work/repo/a.rs", false, 1, &["-old"]);
    assert_eq!(confirmed_with(emptied, &reported), (1, 0));
}

#[test]
fn an_append_onto_a_line_without_a_newline_completes_that_line() {
    // Appending to unterminated `x` leaves `xy`; the diff removes the
    // old line and adds the new one.
    let command = "cat >> a.rs <<'E'\ny\nz\nE\n";
    let completing = entry(
        "/work/repo/a.rs",
        false,
        1,
        &[" a", "-x", "\\ No newline at end of file", "+xy", "+z"],
    );
    assert_eq!(confirmed_with(command, &completing), (1, 0));
    // An earlier statement may leave the unterminated line: the hunk shows
    // the pre-command file, not the pre-heredoc file.
    let command = "printf w > a.rs\ncat >> a.rs <<'E'\ny\nz\nE\n";
    let rewritten = entry("/work/repo/a.rs", false, 1, &[" a", "-x", "+wy", "+z"]);
    assert_eq!(confirmed_with(command, &rewritten), (1, 0));
    let whole = entry(
        "/work/repo/a.rs",
        false,
        1,
        &["-x", "\\ No newline at end of file", "+wy", "+z"],
    );
    assert_eq!(confirmed_with(command, &whole), (1, 0));
    // The line must end with the first appended line; later lines must
    // match the remaining appended lines.
    for lines in [&[" a", "-x", "+yw", "+z"][..], &[" a", "-x", "+wy", "+wz"]] {
        let other = entry("/work/repo/a.rs", false, 1, lines);
        assert_eq!(confirmed_with(command, &other), (0, 1), "{lines:?}");
    }
}

#[test]
fn an_appended_line_the_diff_shows_as_context_is_still_at_the_end() {
    // `sed` turned the file's `x` into `y`, then the heredoc appended `x`:
    // the diff aligns the appended line with the old one.
    let command = "sed -i 's/x/y/' a.rs\ncat >> a.rs <<'E'\nx\nE\n";
    let aligned = entry("/work/repo/a.rs", false, 1, &["+y", " x"]);
    assert_eq!(confirmed_with(command, &aligned), (1, 0));
    // Only the last hunk can reach the file's end, regardless of report
    // order. Real diffs never end earlier hunks short of context; this
    // case tests which hunk is read.
    let mut two = entry("/work/repo/a.rs", false, 9, &[" w", "+x"]);
    let first = entry("/work/repo/a.rs", false, 1, &[" c", "-a", "+b"]);
    let hunks = two["hunks"].as_array_mut().unwrap();
    hunks.push(first["hunks"][0].clone());
    assert_eq!(confirmed_with("cat >> a.rs <<'E'\nx\nE\n", &two), (1, 0));
    // A last hunk that reaches the end with another line contradicts.
    let other = entry("/work/repo/a.rs", false, 1, &[" v", " w", "+x", " y"]);
    assert_eq!(confirmed_with("cat >> a.rs <<'E'\nx\nE\n", &other), (0, 1));
}

#[test]
fn a_skipped_heredoc_whose_text_another_statement_wrote_is_recorded() {
    // The file ends with the heredoc's expected text. This is the
    // check's limit: no hunk contradicts it.
    let command = "false && cat > a.rs <<'E'\nx\nE\nprintf 'x\\n' > a.rs\n";
    let reported = entry("/work/repo/a.rs", true, 1, &["+x"]);
    assert_eq!(confirmed_with(command, &reported), (1, 0));
}

#[test]
fn only_the_diff_marker_says_a_line_has_no_newline() {
    let command = "cat > a.rs <<'E'\nx\nE\n";
    for other in ["\\ truncated", "\\", "\\ No newline", "? unknown"] {
        let mut reported = entry("/work/repo/a.rs", true, 1, &["+x"]);
        reported["hunks"][0]["lines"]
            .as_array_mut()
            .unwrap()
            .push(json!(other));
        assert_eq!(confirmed_with(command, &reported), (1, 0), "{other}");
    }
}

#[test]
fn a_hunk_with_a_line_the_reader_does_not_know_shows_nothing_by_position() {
    let command = "cat > a.rs <<'E'\na\nb\nc\nE\n";
    for created in [true, false] {
        // The unknown line accounts for the header's second new-side
        // line. Nothing places `c` on line 2.
        let mut unknown = entry("/work/repo/a.rs", created, 1, &["+a", "? omitted", "+c"]);
        unknown["hunks"][0]["newLines"] = json!(3);
        assert_eq!(confirmed_with(command, &unknown), (1, 0), "{created}");
        // A known line in its place establishes the position.
        let wrong = entry("/work/repo/a.rs", created, 1, &["+a", "+x", "+c"]);
        assert_eq!(confirmed_with(command, &wrong), (0, 1), "{created}");
    }
    // Unknown lines also leave a created file's length unknown, even when
    // the header counts all shown lines.
    let longer = "cat > a.rs <<'E'\na\nb\nc\nd\nE\n";
    let unknown = entry("/work/repo/a.rs", true, 1, &["+a", "+b", "? omitted", "+c"]);
    assert_eq!(confirmed_with(longer, &unknown), (1, 0));
}

#[test]
fn a_hunk_cut_short_does_not_speak_against_an_append() {
    let command = "cat >> a.rs <<'E'\ny\nE\n";
    // The header counts three new-side lines; the hunk shows two.
    let mut cut = entry("/work/repo/a.rs", false, 1, &[" w", "+x"]);
    cut["hunks"][0]["newLines"] = json!(3);
    assert_eq!(confirmed_with(command, &cut), (1, 0));
    // With full counts, these lines show the file ending with other
    // text.
    let whole = entry("/work/repo/a.rs", false, 1, &[" w", "+x"]);
    assert_eq!(confirmed_with(command, &whole), (0, 1));
}

#[test]
fn a_write_replaced_within_its_command_cannot_be_contradicted() {
    // The first heredoc did not run; the final file cannot reveal this.
    // That is a whole-command diff's limit.
    let command = "false && cat > a.rs <<'A'\nbad\nA\ncat > a.rs <<'B'\ngood\nB\n";
    let reported = entry("/work/repo/a.rs", true, 1, &["+good"]);
    assert_eq!(confirmed_with(command, &reported), (2, 0));
}

#[test]
fn the_writes_of_one_command_to_one_file_are_checked_together() {
    let command = "cat > a.rs <<'A'\n1\nA\ncat >> a.rs <<'B'\n2\nB\ncat >> b.rs <<'C'\n3\nC\n";
    let b = entry("/work/repo/b.rs", false, 1, &["+3"]);
    let agreeing = json!([entry("/work/repo/a.rs", true, 1, &["+1", "+2"]), b]);
    let outcome = shell(command, &agreeing);
    assert_eq!(edits(&outcome).len(), 3);
    // A contradiction leaves all writes to that file unconfirmed and the
    // other file's write unchanged.
    let contradicting = json!([entry("/work/repo/a.rs", true, 1, &["+1"]), b]);
    let outcome = shell(command, &contradicting);
    let recorded: Vec<&str> = edits(&outcome)
        .iter()
        .map(|e| e.path.to_str().unwrap())
        .collect();
    assert_eq!(recorded, vec!["b.rs"]);
    assert_eq!(outcome.counts.unconfirmed_shell_writes, 2);
    // An append that created the file is the whole file.
    let created_by_append = entry("/work/repo/b.rs", true, 2, &["+3"]);
    assert_eq!(
        confirmed_with("cat >> b.rs <<'C'\n3\nC\n", &created_by_append),
        (0, 1)
    );
}

#[test]
fn a_created_file_shorter_than_its_writes_contradicts_them() {
    // The second heredoc did not run: the harness shows a one-line file.
    let command = "cat > a.rs <<'A'\n1\nA\nfalse && cat >> a.rs <<'B'\n2\nB\ntrue\n";
    let one_line = entry("/work/repo/a.rs", true, 1, &["+1"]);
    assert_eq!(confirmed_with(command, &one_line), (0, 2));
    // A header counting unseen lines means the hunk is cut short and
    // leaves the file's length unknown.
    let mut cut = entry("/work/repo/a.rs", true, 1, &["+1"]);
    cut["hunks"][0]["newLines"] = json!(2);
    assert_eq!(confirmed_with(command, &cut), (2, 0));
    // An overwritten file's diff shows only what changed.
    let overwrite = "cat > a.rs <<'A'\n1\n2\nA\n";
    let partial = entry("/work/repo/a.rs", false, 2, &["+2"]);
    assert_eq!(confirmed_with(overwrite, &partial), (1, 0));
}

#[test]
fn a_heredoc_statement_that_did_not_run_is_recorded_when_the_harness_shows_no_diff() {
    // The harness confirms commands, not statements. Without hunks,
    // nothing contradicts the write. The edit attributes only where file
    // bytes reproduce its text.
    let command = "false && cat > a.rs <<'EOF'\nx\nEOF\necho y > a.rs\n";
    let outcome = shell(command, &json!([created("/work/repo/a.rs")]));
    let recorded = edits(&outcome);
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].new_text, Material::Retained("x\n".to_owned()));
    assert_eq!(recorded[0].after, FileState::unknown());
    // The same holds when the entry's hunks cannot be read.
    let unreadable = json!([{"filePath": "/work/repo/a.rs", "created": true,
        "hunks": [{"oldStart": "one"}]}]);
    assert_eq!(edits(&shell(command, &unreadable)).len(), 1);
}

#[test]
fn an_abandoned_command_counts_its_heredoc_writes_as_unconfirmed() {
    let issue = json!({
        "type": "assistant", "uuid": "a1", "parentUuid": null, "sessionId": "s",
        "cwd": "/work/repo", "timestamp": "2026-09-28T10:00:00.000Z", "isSidechain": false,
        "message": {"role": "assistant", "content": [
            {"type": "tool_use", "id": "t1", "name": "Bash",
             "input": {"command": "cat > a.rs <<'EOF'\nx\nEOF\n"}}
        ]}
    });
    let typed = human("u1", "s");
    let lines = format!("{issue}\n{typed}\n");
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert!(edits(&outcome).is_empty());
    assert_eq!(commands(&outcome).len(), 1);
    assert_eq!(outcome.counts.abandoned_tool_uses, 1);
    assert_eq!(outcome.counts.unconfirmed_shell_writes, 1);
}

#[test]
fn a_command_without_a_heredoc_write_yields_no_edit_whatever_it_changed() {
    for command in [
        "cargo fmt --all",
        "python3 - <<'EOF'\nopen('a.rs', 'w').write('x')\nEOF\n",
        "sed -i 's/a/b/' a.rs",
        "echo x > a.rs",
        "cat > a.rs <<EOF\nx\nEOF\n",
        "tee a.rs <<'EOF'\nx\nEOF\n",
    ] {
        let outcome = shell(command, &json!([modified("/work/repo/a.rs")]));
        assert!(edits(&outcome).is_empty(), "{command}");
        assert!(outcome.counts.is_empty(), "{command}");
        assert!(
            matches!(commands(&outcome)[0].changes, Material::Retained(_)),
            "{command}"
        );
    }
}

#[test]
fn editing_tool_calls_are_edits_of_tool_origin() {
    let outcome = parse_transcript(&fixture(), 0);
    let recorded = edits(&outcome);
    assert!(!recorded.is_empty());
    assert!(recorded.iter().all(|e| e.origin == EditOrigin::Tool));
}
