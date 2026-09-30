//! Tests transcript parsing with synthetic sessions and tool results.

mod common;

use std::path::PathBuf;

use common::{SESSION, fixture, line_starts};

use serde_json::{Value, json};
use supersigil_record::observations::{
    CommandCategory, Content, EditOperation, FileState, Material, Observation, Outcome, Role,
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
