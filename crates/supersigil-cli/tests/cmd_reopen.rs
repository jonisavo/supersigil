//! Tests `supersigil review` after completion. Reviews relate to earlier judged
//! reviews of their content or worktree, carry judgments with matching evidence,
//! and report changes since the most recent completion or that it carries over.

mod common;

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::time::{Duration, Instant};

use common::fixture::{Fixture, LIB_FINAL, NEW_RS, basis_id, gate_row, slice, slice_text, write};
use common::session::Session;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use supersigil_record::JudgmentId;
use supersigil_record::reviews::{self, Basis, Judgment, JudgmentKind, PinnedLog};
use supersigil_record::time::UtcTime;

/// The name of the person judging.
const NAME: &str = "Ada Lovelace";
/// Session id of the slice fixture.
const SLICE_SESSION: &str = "11111111-1111-4111-8111-111111111111";
/// The slice fixture's gate items, by name.
const SLICE_ITEMS: [&str; 4] = [
    "old.txt",
    "lines:src/lib.rs",
    "src/new.rs",
    "gap:src/lib.rs",
];

/// Runs `supersigil judge <args>` in the repository; panics unless it
/// succeeds.
fn judge(f: &Fixture, args: &[&str]) {
    let output = f
        .plain(&f.repo, &[&["judge"], args].concat())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The review `supersigil review --format json <args>` prints in the
/// repository.
fn review(f: &Fixture, args: &[&str]) -> Value {
    f.json(&f.repo, &[&["review", "--format", "json"], args].concat())
}

/// The summary `supersigil review --format terminal <args>` prints in the
/// repository.
fn summary(f: &Fixture, args: &[&str]) -> String {
    let output = f
        .supersigil(
            &f.repo,
            &[&["review", "--format", "terminal"], args].concat(),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// The status lines of `text`, a summary: from the one after the header to
/// the gate line.
fn status_lines(text: &str) -> Vec<&str> {
    text.lines()
        .skip(4)
        .take_while(|line| !line.starts_with("Gate: "))
        .collect()
}

/// Each item of `review`'s gate by name, with its state, in gate order.
fn gate_states(review: &Value) -> Vec<(&str, &str)> {
    review["gate"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["name"].as_str().unwrap(),
                item["state"].as_str().unwrap(),
            )
        })
        .collect()
}

/// The judgment recorded last in `f`'s review store.
fn last_judgment(f: &Fixture) -> Judgment {
    let store = reviews::find(&f.reviews(), &f.repo.join(".git"))
        .unwrap()
        .unwrap();
    store
        .snapshot()
        .unwrap()
        .judgments()
        .unwrap()
        .pop()
        .unwrap()
}

/// Reviews the slice fixture with a git identity, acknowledges each gate item,
/// and completes it. Returns the fixture, review, and completion.
fn completed() -> (Fixture, Value, Judgment) {
    let f = Fixture::new();
    slice(&f);
    f.git(&["config", "user.name", NAME]);
    f.git(&["config", "user.email", "ada@example.com"]);
    let completed = review(&f, &[]);
    judge(&f, &[&["acknowledge"][..], &SLICE_ITEMS].concat());
    judge(&f, &["complete"]);
    let completion = last_judgment(&f);
    assert_eq!(completion.kind, JudgmentKind::Complete);
    (f, completed, completion)
}

/// Appends `text` to the slice session's transcript.
fn append_to_slice(f: &Fixture, text: &str) {
    f.transcript(&f.repo, "slice.jsonl", &format!("{}{text}", slice_text()));
}

/// Formats a completion's date and review for a status line.
fn since(completed: &Value, completion: &Judgment) -> String {
    format!(
        "since your completion on {} (review {})",
        completion.at.date(),
        &basis_id(completed)[..8]
    )
}

#[test]
fn a_file_changed_after_completion_reopens_its_item_and_carries_the_others() {
    let (f, completed, completion) = completed();
    write(
        &f.repo,
        "src/lib.rs",
        &format!("{LIB_FINAL}// changed since\n"),
    );

    let review = review(&f, &[]);
    let text = summary(&f, &[]);

    assert_eq!(
        status_lines(&text),
        [
            format!(
                "Review not complete. Code changed {}:",
                since(&completed, &completion)
            ),
            "  src/lib.rs".to_owned(),
        ],
        "{text}"
    );
    assert_eq!(
        gate_states(&review),
        [
            ("old.txt", "carried"),
            ("lines:src/lib.rs", "reopened"),
            ("src/new.rs", "carried"),
            ("gap:src/lib.rs", "carried"),
        ]
    );
    let reasons = [
        "file content or mode changed",
        "evidence behind the lines changed",
        "line explanations changed",
    ];
    let carried = format!(
        "carried from review {}, by {NAME} on {}",
        &basis_id(&completed)[..8],
        completion.at.date()
    );
    assert!(
        text.contains(&format!(
            "\nGate: 1 open, 3 closed.\n\n  \
             old.txt           acknowledged  {carried}\n  \
             lines:src/lib.rs  reopened      {} since your acknowledgment\n  \
             src/new.rs        acknowledged  {carried}\n  \
             gap:src/lib.rs    acknowledged  {carried}\n",
            reasons.join("; ")
        )),
        "{text}"
    );
    assert!(
        text.contains("\nNext: supersigil judge acknowledge lines:src/lib.rs\n"),
        "{text}"
    );
    let related = &review["status"]["related"];
    assert_eq!(related.as_array().unwrap().len(), 1, "{related:#}");
    assert_eq!(related[0]["review"], basis_id(&completed));
    assert_eq!(related[0]["relation"], "code_changed");
    assert_eq!(related[0]["completion"]["id"], completion.id.as_str());
    assert_eq!(related[0]["changed_paths"], json!(["src/lib.rs"]));
    assert_eq!(
        related[0]["changed_evidence"],
        json!({"explanations": [], "new": [], "capture": [], "gone_or_rewritten": []})
    );
    assert_eq!(
        related[0]["uncovered"],
        json!([{
            "id": gate_row(&review, "lines:src/lib.rs")["id"],
            "name": "lines:src/lib.rs",
            "reasons": reasons,
        }])
    );
}

#[test]
fn committing_the_completed_working_tree_carries_the_completion_to_the_commit() {
    let (f, completed, completion) = completed();
    // The agent, asked to commit, adds turns and a command that names no
    // reviewed path; untracked `src/new.rs` stays out of the commit.
    let appended = Session::new(SLICE_SESSION, &f.repo)
        .prompt("Commit this.")
        .bash("toolu_commit", "git commit -qam 'Add greet'", None)
        .text();
    append_to_slice(&f, &appended);
    f.git(&["commit", "-qam", "Add greet"]);

    let review = review(&f, &["--target", "HEAD"]);
    let text = summary(&f, &["--target", "HEAD"]);

    let logs = |review: &Value| review["basis"]["document"]["records"][0]["logs"].clone();
    assert_ne!(
        logs(&review),
        logs(&completed),
        "the agent's turns were read"
    );
    assert_eq!(
        status_lines(&text),
        [format!(
            "Review complete: completed on {} by {NAME} for the same content (review {}).",
            completion.at.date(),
            &basis_id(&completed)[..8]
        )],
        "{text}"
    );
    assert_eq!(review["status"]["complete"], true);
    assert_eq!(review["status"]["completion"]["id"], completion.id.as_str());
    assert_eq!(
        gate_states(&review),
        [
            ("old.txt", "carried"),
            ("lines:src/lib.rs", "carried"),
            ("gap:src/lib.rs", "carried"),
        ]
    );
    assert!(!text.contains("Next:"), "{text}");
}

#[test]
fn a_completion_of_the_same_content_carries_over_whatever_was_completed_since() {
    // Review A of content X is completed; then `src/lib.rs` changes to Y
    // and review B is completed too.
    let (f, completed, completion) = completed();
    write(
        &f.repo,
        "src/lib.rs",
        &format!("{LIB_FINAL}// changed since\n"),
    );
    let changed = review(&f, &[]);
    judge(&f, &["acknowledge", "lines:src/lib.rs"]);
    judge(&f, &["complete"]);
    assert_ne!(last_judgment(&f).id, completion.id);
    // Back to X, after a turn that names nothing reviewed, so the review
    // is neither A nor B.
    write(&f.repo, "src/lib.rs", LIB_FINAL);
    let appended = Session::new(SLICE_SESSION, &f.repo)
        .prompt("Never mind, keep it as it was.")
        .text();
    append_to_slice(&f, &appended);

    let review = review(&f, &[]);
    let text = summary(&f, &[]);

    assert_ne!(basis_id(&review), basis_id(&completed));
    assert_ne!(basis_id(&review), basis_id(&changed));
    assert_eq!(
        status_lines(&text),
        [format!(
            "Review complete: completed on {} by {NAME} for the same content (review {}).",
            completion.at.date(),
            &basis_id(&completed)[..8]
        )],
        "{text}"
    );
    assert_eq!(review["status"]["completion"]["id"], completion.id.as_str());
    let related = &review["status"]["related"];
    assert_eq!(related[0]["review"], basis_id(&completed));
    assert_eq!(related[1]["review"], basis_id(&changed));
    assert_eq!(related[1]["relation"], "code_changed");
    // The reverted file's acknowledgment carries from A, not B.
    let lines = gate_row(&review, "lines:src/lib.rs");
    assert_eq!(lines["state"], "carried");
    assert_eq!(lines["judgment"]["review"], basis_id(&completed));
}

#[test]
fn a_new_recorded_edit_of_a_reviewed_file_is_changed_evidence() {
    let (f, completed, completion) = completed();
    // The agent changes a line and changes it back: the same content.
    let lib = f.repo.join("src/lib.rs");
    let tried = LIB_FINAL.replace("Hello, {name}!", "Hi, {name}!");
    let appended = Session::new(SLICE_SESSION, &f.repo)
        .prompt("Try another greeting.")
        .edit(
            "toolu_try",
            &lib,
            LIB_FINAL,
            "Hello, {name}!",
            "Hi, {name}!",
        )
        .edit("toolu_back", &lib, &tried, "Hi, {name}!", "Hello, {name}!")
        .text();
    append_to_slice(&f, &appended);

    let review = review(&f, &[]);
    let text = summary(&f, &[]);

    let related = &review["status"]["related"][0];
    assert_eq!(review["status"]["complete"], false);
    assert_eq!(related["relation"], "same_content");
    assert_eq!(
        related["changed_evidence"]["new"],
        json!([{"session": SLICE_SESSION, "paths": ["src/lib.rs"]}])
    );
    let lines = status_lines(&text);
    assert_eq!(
        lines[0],
        format!(
            "Review not complete. Evidence changed {}:",
            since(&completed, &completion)
        ),
        "{text}"
    );
    assert!(
        lines.contains(&format!("  new: session {SLICE_SESSION}: src/lib.rs").as_str()),
        "{text}"
    );
}

#[test]
fn only_commands_whose_change_report_lists_a_reviewed_path_are_new_evidence() {
    let (f, _, _) = completed();
    let lib = f.repo.join("src/lib.rs").to_string_lossy().into_owned();
    // A formatter whose report lists `src/lib.rs` without its text naming
    // it, a command whose text names untracked `src/new.rs`, and one that
    // names nothing reviewed.
    let report = json!({"changedFiles": [lib], "files": [{"filePath": lib, "hunks": [{
        "oldStart": 1, "oldLines": 1, "newStart": 1, "newLines": 1,
        "lines": ["-// manual", "+// manual"]}]}], "moreFiles": 0});
    let appended = Session::new(SLICE_SESSION, &f.repo)
        .prompt("Tidy up.")
        .bash("toolu_fmt", "cargo fmt", Some(report))
        .bash("toolu_cat", "cat src/new.rs", None)
        .bash("toolu_ls", "ls notes.txt", None)
        .text();
    append_to_slice(&f, &appended);

    let review = review(&f, &[]);

    assert_eq!(
        review["status"]["related"][0]["changed_evidence"]["new"],
        json!([{"session": SLICE_SESSION, "paths": ["src/lib.rs"]}])
    );
}

#[test]
fn a_change_report_on_a_later_sighting_of_a_command_is_new_evidence() {
    let (f, _, _) = completed();
    let other = "22222222-2222-4222-8222-222222222222";
    let lib = f.repo.join("src/lib.rs").to_string_lossy().into_owned();
    let report = json!({"changedFiles": [lib], "files": [{"filePath": lib, "hunks": [{
        "oldStart": 1, "oldLines": 1, "newStart": 1, "newLines": 1,
        "lines": ["-// manual", "+// manual"]}]}], "moreFiles": 0});
    let formatted = |report: Option<Value>| {
        Session::new(other, &f.repo)
            .prompt("Tidy up.")
            .bash("toolu_fmt", "cargo fmt", report)
            .text()
    };
    // Another session's formatter, first recorded without a change report.
    f.transcript(&f.repo, "other.jsonl", &formatted(None));
    assert_eq!(review(&f, &[])["status"]["complete"], true);
    // Its transcript rewritten with the report: read again from the start,
    // the command is recorded again under the same id.
    f.transcript(&f.repo, "other.jsonl", &formatted(Some(report)));

    let review = review(&f, &[]);

    assert_eq!(review["status"]["complete"], false);
    assert_eq!(
        review["status"]["related"][0]["changed_evidence"]["new"],
        json!([{"session": other, "paths": ["src/lib.rs"]}])
    );
}

#[test]
fn a_change_report_counts_whether_or_not_its_transcript_is_a_candidate() {
    // A nested worktree whose readme changed by hand.
    let f = Fixture::new();
    f.git(&["config", "user.name", NAME]);
    f.git(&["config", "user.email", "ada@example.com"]);
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    f.git(&["worktree", "add", "-q", "-b", "x", ".claude/worktrees/x"]);
    let nested = f.repo.join(".claude/worktrees/x");
    write(&nested, "README.md", "readme!\n");
    let readme = nested.join("README.md").to_string_lossy().into_owned();
    let report = json!({"changedFiles": [readme], "files": [{"filePath": readme, "hunks": [{
        "oldStart": 1, "oldLines": 1, "newStart": 1, "newLines": 1,
        "lines": ["-readme", "+readme!"]}]}], "moreFiles": 0});
    // A session in the main checkout whose formatter was first recorded
    // without a change report, so its transcript is not a candidate.
    let formatted = |report: Option<Value>| {
        Session::new("s-main", &f.repo)
            .prompt("Tidy up.")
            .bash("toolu_fmt", "cargo fmt --all", report)
            .text()
    };
    let transcript = f.transcript(&f.repo, "main.jsonl", &formatted(None));
    let candidate = |review: &Value| {
        review["evidence"]["candidate_transcripts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|candidate| candidate["transcript"] == transcript.to_string_lossy().as_ref())
    };
    let completed = f.json(&nested, &["review", "--format", "json"]);
    assert!(!candidate(&completed), "{:#}", completed["evidence"]);
    for args in [&["acknowledge", "README.md"][..], &["complete"]] {
        let output = f
            .plain(&nested, &[&["judge"][..], args].concat())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    // Its transcript rewritten with the report: read again from the start,
    // the command is recorded again under the same id, and candidates still
    // come from its first sighting.
    f.transcript(&f.repo, "main.jsonl", &formatted(Some(report)));

    let review = f.json(&nested, &["review", "--format", "json"]);

    assert!(!candidate(&review), "{:#}", review["evidence"]);
    assert_eq!(review["status"]["complete"], false);
    assert_eq!(
        review["status"]["related"][0]["changed_evidence"]["new"],
        json!([{"session": "s-main", "paths": ["README.md"]}])
    );
}

#[test]
fn a_later_sighting_that_changes_an_edit_of_a_gap_reopens_the_gap() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    f.git(&["config", "user.name", NAME]);
    f.git(&["config", "user.email", "ada@example.com"]);
    let gap = f.repo.join("src/gap.rs");
    // The file was gone again before the second write created it.
    let created = |second: &str| {
        Session::new("s-gap", &f.repo)
            .prompt("Write gap.rs twice.")
            .create("toolu_one", &gap, "one\n")
            .create("toolu_two", &gap, second)
            .text()
    };
    f.transcript(&f.repo, "gap.jsonl", &created("two\n"));
    write(&f.repo, "src/gap.rs", "two\n");
    let args = ["--include-untracked", "src/gap.rs"];
    assert_eq!(gate_states(&review(&f, &args)), [("src/gap.rs", "open")]);
    judge(&f, &["acknowledge", "src/gap.rs"]);
    // The second write's payload rewritten in its transcript: read again
    // from the start, it is recorded again under the same id, while the
    // file on disk stays as the first sighting wrote it.
    f.transcript(&f.repo, "gap.jsonl", &created("three\n"));

    let review = review(&f, &args);

    let row = gate_row(&review, "src/gap.rs");
    assert_eq!(row["state"], "reopened", "{row:#}");
    assert_eq!(row["reopen_reasons"], json!(["gaps changed"]));
}

#[test]
fn a_later_sighting_that_names_another_file_reopens_an_untracked_item() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    f.git(&["config", "user.name", NAME]);
    f.git(&["config", "user.email", "ada@example.com"]);
    let written = |file: &str| {
        Session::new("s-notes", &f.repo)
            .prompt("Write the notes.")
            .create("toolu_notes", &f.repo.join(file), "notes\n")
            .text()
    };
    f.transcript(&f.repo, "notes.jsonl", &written("notes.txt"));
    write(&f.repo, "notes.txt", "notes\n");
    assert_eq!(gate_states(&review(&f, &[])), [("notes.txt", "open")]);
    judge(&f, &["dismiss", "notes.txt", "--reason", "scratch notes"]);
    judge(&f, &["complete"]);
    // The write rewritten in its transcript to name a file that is not
    // there: read again from the start, it is recorded again under the
    // same id, while the record keeps its first sighting, which names the
    // untracked file.
    f.transcript(&f.repo, "notes.jsonl", &written("elsewhere.txt"));

    let review = review(&f, &[]);

    assert_eq!(review["status"]["complete"], false, "{review:#}");
    let row = gate_row(&review, "notes.txt");
    assert_eq!(row["state"], "reopened", "{row:#}");
    assert_eq!(row["reopen_reasons"], json!(["recorded edits changed"]));
}

#[test]
fn a_command_that_only_reads_a_reviewed_file_leaves_the_completion_carrying() {
    let (f, completed, completion) = completed();
    // The agent reads a file the review showed lines no edit explains, so
    // the review lists the command as a mention of it.
    let appended = Session::new(SLICE_SESSION, &f.repo)
        .prompt("Show me the library.")
        .bash("toolu_cat", "cat src/lib.rs", None)
        .text();
    append_to_slice(&f, &appended);

    let review = review(&f, &[]);
    let text = summary(&f, &[]);

    let lib = review["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "src/lib.rs")
        .unwrap();
    assert_eq!(lib["mentions"][0]["text"], "cat src/lib.rs", "{lib:#}");
    assert_eq!(review["status"]["complete"], true);
    assert_eq!(review["status"]["completion"]["id"], completion.id.as_str());
    assert_eq!(
        review["status"]["related"][0]["changed_evidence"],
        json!({"explanations": [], "new": [], "capture": [], "gone_or_rewritten": []})
    );
    assert_eq!(
        status_lines(&text),
        [format!(
            "Review complete: completed on {} by {NAME} for the same content (review {}).",
            completion.at.date(),
            &basis_id(&completed)[..8]
        )],
        "{text}"
    );
}

#[test]
fn a_changed_line_ending_conversion_changes_an_explanation() {
    let (f, completed, completion) = completed();
    f.git(&["config", "core.autocrlf", "true"]);

    let review = review(&f, &[]);
    let text = summary(&f, &[]);

    // The same content, explained otherwise.
    assert_eq!(review["target"]["tree"], completed["target"]["tree"]);
    let related = &review["status"]["related"][0];
    assert_eq!(related["relation"], "same_content");
    let explanations = related["changed_evidence"]["explanations"]
        .as_array()
        .unwrap();
    assert!(explanations.contains(&json!("src/lib.rs")), "{related:#}");
    let lines = status_lines(&text);
    assert_eq!(
        lines[..2],
        [
            format!(
                "Review not complete. Evidence changed {}:",
                since(&completed, &completion)
            ),
            format!(
                "  explanations differ: {}",
                explanations
                    .iter()
                    .map(|path| path.as_str().unwrap())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ],
        "{text}"
    );
}

#[test]
fn a_pruned_snapshot_leaves_the_changed_paths_unavailable_and_judgments_carrying() {
    let (f, completed, completion) = completed();
    write(
        &f.repo,
        "src/lib.rs",
        &format!("{LIB_FINAL}// changed since\n"),
    );
    // The person deletes the reviewed snapshot's ref; while git still has
    // the tree, the changed paths are listed.
    let reference = format!("refs/supersigil/reviews/{}", basis_id(&completed));
    f.git(&["update-ref", "-d", &reference]);
    let kept = review(&f, &[]);
    assert_eq!(
        kept["status"]["related"][0]["changed_paths"],
        json!(["src/lib.rs"])
    );
    // Then git prunes what only the ref kept.
    f.git(&["prune", "--expire=now"]);

    let review = review(&f, &[]);
    let text = summary(&f, &[]);

    assert_eq!(
        status_lines(&text),
        [
            format!(
                "Review not complete. Code changed {}:",
                since(&completed, &completion)
            ),
            "  changed paths unavailable: the reviewed snapshot is no longer in the repository"
                .to_owned(),
        ],
        "{text}"
    );
    assert_eq!(
        gate_states(&review),
        [
            ("old.txt", "carried"),
            ("lines:src/lib.rs", "reopened"),
            ("src/new.rs", "carried"),
            ("gap:src/lib.rs", "carried"),
        ]
    );
    let related = &review["status"]["related"][0];
    assert_eq!(related["review"], basis_id(&completed));
    assert_eq!(related["changed_paths"], Value::Null);
    // Without the earlier tree, a changed explanation cannot be told from
    // changed code.
    assert_eq!(related["changed_evidence"]["explanations"], json!([]));
}

/// The id of the record the slice fixture's review read.
fn slice_record(completed: &Value) -> String {
    let records = completed["records"].as_array().unwrap();
    assert_eq!(records.len(), 1, "{records:?}");
    records[0]["id"].as_str().unwrap().to_owned()
}

#[test]
fn a_record_gone_since_the_completion_is_changed_evidence() {
    let (f, completed, completion) = completed();
    let record = slice_record(&completed);
    // Its transcript is synced again into a new record.
    std::fs::remove_dir_all(f.records.join(&record)).unwrap();

    let review = review(&f, &[]);
    let text = summary(&f, &[]);

    assert_eq!(
        review["status"]["related"][0]["changed_evidence"]["gone_or_rewritten"],
        json!([record])
    );
    let lines = status_lines(&text);
    assert_eq!(
        lines[0],
        format!(
            "Review not complete. Evidence changed {}:",
            since(&completed, &completion)
        ),
        "{text}"
    );
    assert!(
        lines.contains(&format!("  gone or rewritten: {record}").as_str()),
        "{text}"
    );
}

#[test]
fn a_log_rewritten_to_the_same_length_is_changed_evidence() {
    let (f, completed, completion) = completed();
    let record = slice_record(&completed);
    let log = f.records.join(&record).join("observations/0.jsonl");
    // The person's first message, as the record holds it.
    let before = std::fs::read_to_string(&log).unwrap();
    let after = before.replacen("clean up", "tidy up!", 1);
    assert_ne!(after, before);
    std::fs::write(&log, after).unwrap();

    let review = review(&f, &[]);
    let text = summary(&f, &[]);

    assert_eq!(
        review["status"]["related"][0]["changed_evidence"]["gone_or_rewritten"],
        json!([record])
    );
    let lines = status_lines(&text);
    assert_eq!(
        lines[0],
        format!(
            "Review not complete. Evidence changed {}:",
            since(&completed, &completion)
        ),
        "{text}"
    );
    assert!(
        lines.contains(&format!("  gone or rewritten: {record}").as_str()),
        "{text}"
    );
}

#[test]
fn a_capture_limitation_appended_to_a_candidate_transcript_is_changed_evidence() {
    let (f, completed, completion) = completed();
    let appended = Session::new(SLICE_SESSION, &f.repo)
        .prompt("Edit the notebook.")
        .notebook_edit("toolu_nb")
        .text();
    append_to_slice(&f, &appended);

    let review = review(&f, &[]);
    let text = summary(&f, &[]);

    let transcript = review["evidence"]["candidate_transcripts"][0]["transcript"]
        .as_str()
        .unwrap();
    assert_eq!(
        review["status"]["related"][0]["changed_evidence"],
        json!({"explanations": [], "new": [], "capture": [transcript], "gone_or_rewritten": []})
    );
    assert_eq!(
        status_lines(&text),
        [
            format!(
                "Review not complete. Evidence changed {}:",
                since(&completed, &completion)
            ),
            format!("  capture limitations changed: {transcript}"),
            "No open gate items. This snapshot has not been marked complete.".to_owned(),
        ],
        "{text}"
    );
    assert!(
        text.ends_with("\n\nNext: supersigil judge complete\n"),
        "{text}"
    );
}

#[test]
fn a_transcript_that_became_a_candidate_since_brings_no_earlier_capture_limitation() {
    // A nested worktree, whose one change a session there explains.
    let f = Fixture::new();
    f.git(&["config", "user.name", NAME]);
    f.git(&["config", "user.email", "ada@example.com"]);
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    f.git(&["worktree", "add", "-q", "-b", "x", ".claude/worktrees/x"]);
    let nested = f.repo.join(".claude/worktrees/x");
    let edit = Session::new("s-nested", &nested)
        .prompt("Stress the readme.")
        .edit(
            "toolu_edit",
            &nested.join("README.md"),
            "readme\n",
            "readme",
            "readme!",
        )
        .text();
    f.transcript(&nested, "nested.jsonl", &edit);
    write(&nested, "README.md", "readme!\n");
    // A session in the main checkout whose capture missed a tool use, and
    // which has done nothing in the nested worktree yet.
    let mut main = Session::new("s-main", &f.repo);
    main.prompt("Edit the notebook.").notebook_edit("toolu_nb");
    let transcript = f.transcript(&f.repo, "main.jsonl", &main.text());
    let completed = f.json(&nested, &["review", "--format", "json"]);
    let output = f.plain(&nested, &["judge", "complete"]).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Then the session runs a command in the nested worktree, missing
    // nothing more.
    main.cd(&nested).bash("toolu_ls", "ls", None);
    f.transcript(&f.repo, "main.jsonl", &main.text());

    let review = f.json(&nested, &["review", "--format", "json"]);

    // Its transcript is a candidate now, with the limitation it had.
    let candidate = |review: &Value| {
        review["evidence"]["candidate_transcripts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|candidate| candidate["transcript"] == transcript.to_string_lossy().as_ref())
            .cloned()
    };
    assert_eq!(candidate(&completed), None);
    let now = candidate(&review).unwrap_or_else(|| panic!("{:#}", review["evidence"]));
    assert_ne!(now["capture_limitations"], json!([]));
    assert_eq!(review["target"]["tree"], completed["target"]["tree"]);
    let related = &review["status"]["related"][0];
    assert_eq!(related["review"], basis_id(&completed));
    assert_eq!(
        related["changed_evidence"],
        json!({"explanations": [], "new": [], "capture": [], "gone_or_rewritten": []})
    );
    assert_eq!(review["status"]["complete"], true);
}

#[test]
fn a_completion_does_not_cover_a_gate_item_it_saw_in_another_state() {
    // Review A acknowledges every item and is not completed; review B, the
    // same content with `src/new.rs` changed on disk, acknowledges it again
    // and is completed. With `src/new.rs` restored, the review is A again:
    // every item is closed, yet B's completion did not see `src/new.rs` as
    // it is.
    let f = Fixture::new();
    slice(&f);
    f.git(&["config", "user.name", NAME]);
    f.git(&["config", "user.email", "ada@example.com"]);
    let first = review(&f, &[]);
    judge(&f, &[&["acknowledge"][..], &SLICE_ITEMS].concat());
    write(&f.repo, "src/new.rs", "pub const VERSION: &str = \"2\";\n");
    let second = review(&f, &[]);
    assert_eq!(gate_row(&second, "src/new.rs")["state"], "reopened");
    judge(&f, &["acknowledge", "src/new.rs"]);
    judge(&f, &["complete"]);
    let completion = last_judgment(&f);
    write(&f.repo, "src/new.rs", NEW_RS);

    let review = review(&f, &[]);
    let text = summary(&f, &[]);

    assert_eq!(basis_id(&review), basis_id(&first));
    assert_eq!(review["gate"]["open"], 0);
    assert_eq!(
        status_lines(&text),
        [
            format!(
                "Review not complete. Completed on {} for the same content, but these gate items \
                 differ from that review:",
                completion.at.date()
            ),
            "  src/new.rs: file on disk changed".to_owned(),
            "No open gate items. This snapshot has not been marked complete.".to_owned(),
        ],
        "{text}"
    );
    assert_eq!(review["status"]["related"][0]["review"], basis_id(&second));
    assert!(
        text.ends_with("\n\nNext: supersigil judge complete\n"),
        "{text}"
    );
}

#[test]
fn an_unreconciled_item_keeps_a_completion_from_carrying_over() {
    let f = Fixture::new();
    slice(&f);
    f.git(&["config", "user.name", NAME]);
    f.git(&["config", "user.email", "ada@example.com"]);
    // Another writer holds the records directory throughout.
    std::fs::create_dir_all(&f.records).unwrap();
    let guard = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(f.records.join(".lock"))
        .unwrap();
    guard.lock().unwrap();
    let first = review(&f, &[]);
    let names: Vec<&str> = gate_states(&first).iter().map(|(name, _)| *name).collect();
    let unreconciled = *names
        .iter()
        .find(|name| name.starts_with("unreconciled:"))
        .unwrap_or_else(|| panic!("{names:?}"));
    judge(&f, &[&["acknowledge"][..], &names].concat());
    judge(&f, &["complete"]);
    let completion = last_judgment(&f);

    let review = review(&f, &[]);
    let text = summary(&f, &[]);
    drop(guard);

    // Never the same saved review, and the evidence it did not read never
    // matches.
    assert_ne!(basis_id(&review), basis_id(&first));
    assert_eq!(gate_row(&review, unreconciled)["state"], "reopened");
    assert_eq!(review["status"]["complete"], false);
    assert_eq!(
        status_lines(&text),
        [
            format!(
                "Review not complete. Completed on {} for the same content, but these gate items \
                 differ from that review:",
                completion.at.date()
            ),
            format!("  {unreconciled}: evidence not read"),
        ],
        "{text}"
    );
}

#[test]
fn many_judged_reviews_of_the_worktree_keep_review_prompt() {
    // The completed review and 199 judged reviews of the same worktree over
    // earlier commits, whose trees are gone, written through the store.
    let (f, completed, completion) = completed();
    let mut earlier: Basis =
        serde_json::from_value(completed["basis"]["document"].clone()).unwrap();
    // Each pins the slice record's log partway into its first observation,
    // with that prefix's hash: reading its log tail would fail the review.
    let log = "observations/0.jsonl";
    let bytes = std::fs::read(f.records.join(slice_record(&completed)).join(log)).unwrap();
    let partway = PinnedLog {
        length: 10,
        sha256: hex::encode(Sha256::digest(&bytes[..10])),
    };
    assert!(
        earlier.records[0]
            .logs
            .insert(log.to_owned(), partway)
            .is_some()
    );
    let store = reviews::find(&f.reviews(), &f.repo.join(".git"))
        .unwrap()
        .unwrap();
    let at: UtcTime = "2026-10-01T12:00:00Z".parse().unwrap();
    let mut tx = store.begin().unwrap();
    for k in 0..199 {
        let mut basis = earlier.clone();
        basis.base.tree = format!("{k:040x}");
        basis.target.tree = format!("{:040x}", k + 1000);
        let id = tx.save_basis(&basis, at).unwrap();
        let judgment = Judgment {
            id: JudgmentId::generate(),
            basis: id,
            kind: JudgmentKind::Acknowledge,
            items: basis.gate.iter().map(|item| item.id.clone()).collect(),
            reason: None,
            text: None,
            decision: None,
            correction: None,
            closes: BTreeMap::new(),
            withdraws: Vec::new(),
            who: completion.who.clone(),
            at,
        };
        tx.append_judgment(&judgment).unwrap();
    }
    tx.commit(UtcTime::now()).unwrap();
    write(
        &f.repo,
        "src/lib.rs",
        &format!("{LIB_FINAL}// changed since\n"),
    );

    let started = Instant::now();
    let review = review(&f, &[]);
    let text = summary(&f, &[]);

    assert!(started.elapsed() < Duration::from_secs(30));
    // Only the completed review is related, and only its tails were read.
    let related = review["status"]["related"].as_array().unwrap();
    assert_eq!(related.len(), 1, "{related:#?}");
    assert_eq!(related[0]["review"], basis_id(&completed));
    assert_eq!(related[0]["changed_paths"], json!(["src/lib.rs"]));
    assert_eq!(
        status_lines(&text)[0],
        format!(
            "Review not complete. Code changed {}:",
            since(&completed, &completion)
        ),
        "{text}"
    );
}
