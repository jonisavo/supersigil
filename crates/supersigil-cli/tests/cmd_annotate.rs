//! Tests `supersigil annotate` against the slice fixture's record. An import
//! is checked against the session's observations and stored as its next
//! revision; a refused import writes nothing. `list` shows each session's
//! current annotation and the corrections that annotation has not seen.

mod common;

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Output, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::Command;
use common::fixture::{Fixture, review_store, slice};
use common::session::Session;
use serde_json::{Value, json};
use supersigil_record::interpretations::{AnnotationDocument, AnnotationRevision};
use supersigil_record::reviews::{
    BasisId, CorrectionKind, DecisionRef, Judgment, JudgmentKind, Who, Withdrawal,
};
use supersigil_record::store::{Association, Manifest, Store, stores_in};
use supersigil_record::time::UtcTime;
use supersigil_record::{EventId, JudgmentId, RecordId, SessionId};

/// The slice fixture's session.
const SESSION: &str = "11111111-1111-4111-8111-111111111111";

/// What importing [`document`] the first time prints.
const IMPORTED: &str = "Imported revision 1 of session 11111111 (1 decision, 1 open question).\n";

/// `supersigil annotate <args>` in `dir`, isolated as `f` isolates every
/// command.
fn annotate(f: &Fixture, dir: &Path, args: &[&str]) -> Command {
    Command::from_std(f.plain(dir, &[&["annotate"], args].concat()))
}

/// The slice fixture with its session synced into the repository's record.
fn synced() -> Fixture {
    let f = Fixture::new();
    slice(&f);
    sync(&f, &f.repo);
    f
}

/// Syncs the transcripts `f`'s Claude home holds for `dir` into its record.
fn sync(f: &Fixture, dir: &Path) {
    let output = f
        .plain(dir, &["session", "sync", "--claude-home"])
        .arg(&f.claude)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The one record in `records`.
fn record_in(records: &Path) -> Store {
    let stores: Vec<Store> = stores_in(records, Store::open)
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let [store] = <[Store; 1]>::try_from(stores).unwrap();
    store
}

/// The manifest of every record in `f`'s records directory.
fn manifests(f: &Fixture) -> Vec<Manifest> {
    stores_in(&f.records, Store::open)
        .unwrap()
        .map(|store| store.unwrap().manifest().unwrap())
        .collect()
}

/// The id of the slice session's edit made by tool use `tool`.
fn edit(tool: &str) -> String {
    EventId::derive("edit", &SessionId::new(SESSION), tool)
        .as_str()
        .to_owned()
}

/// A first annotation of the slice session, prepared from `record` as it is
/// now: one decision citing the first edit, the person's request quoted as
/// its rationale, and one open question about the same edit.
fn document(record: &Store) -> Value {
    let snapshot = record.snapshot().unwrap();
    let observed = snapshot
        .observations(&SessionId::new(SESSION))
        .unwrap()
        .len();
    json!({
        "schema": "supersigil.annotation/1",
        "session": SESSION,
        "based_on": null,
        "prepared": {
            "record": snapshot.manifest().record_id,
            "observation_revision": snapshot.revision(),
            "observed": {"count": observed, "last": null},
        },
        "produced_by": "claude-code/supersigil-annotate",
        "decisions": [{
            "id": "d1",
            "title": "Add a greeting function",
            "cites": [edit("toolu_01")],
            "rationale": [{
                "text": "The person asked for one.",
                "speaker": "human",
                "explicit": true,
                "source": {"turn": "u1", "quote": "Add a greeting function"},
            }],
        }],
        "open_questions": [
            {"id": "q1", "text": "Should greet live in lib.rs?", "about": [edit("toolu_01")]},
        ],
    })
}

/// `document` without its decisions and open questions.
fn without_lists(mut document: Value) -> Value {
    let fields = document.as_object_mut().unwrap();
    fields.remove("decisions");
    fields.remove("open_questions");
    document
}

/// Writes `bytes` as `name` beside `f`'s repository and returns its path.
fn written(f: &Fixture, name: &str, bytes: &[u8]) -> String {
    let path = f.root.join(name);
    std::fs::write(&path, bytes).unwrap();
    path.to_str().unwrap().to_owned()
}

/// Writes `document` as JSON with [`written`].
fn written_json(f: &Fixture, name: &str, document: &Value) -> String {
    written(f, name, &serde_json::to_vec(document).unwrap())
}

/// Runs `command`, which must succeed, and returns its standard output.
fn ran(command: &mut Command) -> String {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// Runs `command` and checks that it exits 1, prints nothing to standard
/// output, and leaves every record of `f` as it was. Returns its standard
/// error.
fn refused(f: &Fixture, command: &mut Command) -> String {
    let before = manifests(f);
    let output = command.output().unwrap();
    refusal(f, &before, &output)
}

/// Checks that `output` is a command's exit 1 with nothing on standard
/// output, and that every record of `f` is still as `before` has it. Returns
/// the standard error.
fn refusal(f: &Fixture, before: &[Manifest], output: &Output) -> String {
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stdout.is_empty());
    assert_eq!(manifests(f), before);
    std::str::from_utf8(&output.stderr).unwrap().to_owned()
}

/// Waits for `child` to exit and returns its output, killing it and failing
/// the test when it runs on for 30 seconds.
fn finished(mut child: Child) -> Output {
    let deadline = Instant::now() + Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("the command was still reading its input after 30 seconds");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    child.wait_with_output().unwrap()
}

/// Starts `command` with its output piped, as [`finished`] expects.
fn started(command: &mut std::process::Command) -> Child {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

#[test]
fn each_import_stores_the_next_revision_from_a_file_or_standard_input() {
    let f = synced();
    let record = record_in(&f.records);
    let first = document(&record);
    let mut second = document(&record);
    second["based_on"] = json!(1);
    second["decisions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "d2", "title": "Leave the notes alone"}));
    second.as_object_mut().unwrap().remove("open_questions");
    let mut third = document(&record);
    third["based_on"] = json!(2);
    let first_file = written_json(&f, "first.json", &first);
    let second_file = written_json(&f, "second.json", &second);

    // Agents may import.
    let singular = ran(annotate(
        &f,
        &f.repo,
        &["import", &first_file, "--format", "terminal"],
    )
    .env("CLAUDECODE", "1"));
    let plural = ran(&mut annotate(
        &f,
        &f.repo,
        &["import", &second_file, "--format", "terminal"],
    ));
    let piped = ran(annotate(&f, &f.repo, &["import", "-", "--format", "json"])
        .write_stdin(serde_json::to_vec(&third).unwrap()));

    assert_eq!(singular, IMPORTED);
    assert_eq!(
        plural,
        "Imported revision 2 of session 11111111 (2 decisions, 0 open questions).\n"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&piped).unwrap(),
        json!({
            "schema": "supersigil.annotate-import/1",
            "record": record.manifest().unwrap().record_id,
            "session": SESSION,
            "revision": 3,
            "decisions": 1,
            "open_questions": 1,
        })
    );
    let snapshot = record.snapshot().unwrap();
    let session = SessionId::new(SESSION);
    assert_eq!(
        snapshot.annotation_revisions(&session),
        [1, 2, 3].map(AnnotationRevision::new)
    );
    let stored = snapshot
        .annotation(&session, AnnotationRevision::FIRST)
        .unwrap();
    assert_eq!(
        stored.document,
        serde_json::from_value::<AnnotationDocument>(first).unwrap()
    );
}

#[test]
fn bytes_that_are_not_an_annotation_document_are_refused_from_a_file_or_standard_input() {
    let f = synced();
    let mut unknown = document(&record_in(&f.records));
    unknown["decisions"][0]["weight"] = json!(1);
    let unknown = serde_json::to_vec(&unknown).unwrap();
    let not_json = b"Imported nothing.".to_vec();
    let shape = serde_json::from_slice::<AnnotationDocument>(&unknown).unwrap_err();
    let syntax = serde_json::from_slice::<AnnotationDocument>(&not_json).unwrap_err();
    let cases = [
        (
            vec![b'{', 0xff, b'}'],
            "The annotation is not UTF-8.\n".to_owned(),
        ),
        (
            not_json,
            format!("The annotation is not a valid document: {syntax}\n"),
        ),
        (
            unknown,
            format!("The annotation is not a valid document: decisions[0].weight: {shape}\n"),
        ),
    ];

    for (bytes, message) in cases {
        let file = written(&f, "bad.json", &bytes);
        let from_file = refused(&f, &mut annotate(&f, &f.repo, &["import", &file]));
        let from_stdin = refused(
            &f,
            annotate(&f, &f.repo, &["import", "-"]).write_stdin(bytes),
        );
        assert_eq!(from_file, message);
        assert_eq!(from_stdin, message);
    }
    // serde names the field and where it is.
    let shape = shape.to_string();
    assert!(shape.starts_with("unknown field `weight`"), "{shape}");
    assert!(shape.contains(" at line 1 column "), "{shape}");
    let missing = f.root.join("missing.json");
    let unreadable = refused(
        &f,
        &mut annotate(&f, &f.repo, &["import", missing.to_str().unwrap()]),
    );
    assert!(
        unreadable.starts_with(&format!("error: cannot read {}: ", missing.display())),
        "{unreadable}"
    );
}

#[test]
fn a_structural_problem_is_refused_at_its_json_path_one_at_a_time() {
    let f = synced();
    let mut deep = document(&record_in(&f.records));
    deep["decisions"][0]["rationale"][0]["source"]["weight"] = json!(1);
    deep["open_questions"][0]["weight"] = json!(1);
    let bytes = serde_json::to_vec(&deep).unwrap();
    let first = serde_json::from_slice::<AnnotationDocument>(&bytes).unwrap_err();
    let file = written(&f, "deep.json", &bytes);

    let refusal = refused(&f, &mut annotate(&f, &f.repo, &["import", &file]));

    // Reading stops at the first problem, so the question's is not listed.
    assert_eq!(
        refusal,
        format!(
            "The annotation is not a valid document: decisions[0].rationale[0].source.weight: \
             {first}\n"
        )
    );
}

#[test]
fn a_document_over_16_mib_is_refused_and_one_of_16_mib_is_read() {
    let f = synced();
    let mut bytes = serde_json::to_vec(&document(&record_in(&f.records))).unwrap();
    // Whitespace after the document keeps it valid JSON.
    bytes.resize(16 * 1024 * 1024 + 1, b' ');
    let file = written(&f, "large.json", &bytes);
    let message = "The annotation is over 16 MiB.\n";

    let from_file = refused(&f, &mut annotate(&f, &f.repo, &["import", &file]));
    let from_stdin = refused(
        &f,
        annotate(&f, &f.repo, &["import", "-"]).write_stdin(bytes.clone()),
    );
    bytes.pop();
    let file = written(&f, "large.json", &bytes);
    let at_the_limit = ran(&mut annotate(
        &f,
        &f.repo,
        &["import", &file, "--format", "terminal"],
    ));

    assert_eq!(from_file, message);
    assert_eq!(from_stdin, message);
    assert_eq!(at_the_limit, IMPORTED);
}

#[test]
fn standard_input_that_never_ends_is_refused_once_it_is_over_16_mib() {
    let f = synced();
    let before = manifests(&f);
    let mut command = f.plain(&f.repo, &["annotate", "import", "-"]);
    let mut child = started(&mut command);
    // The producer never closes the pipe; it stops when the command does.
    let mut stdin = child.stdin.take().unwrap();
    let producer = std::thread::spawn(move || {
        let chunk = vec![b' '; 64 * 1024];
        while stdin.write_all(&chunk).is_ok() {}
    });

    let output = finished(child);
    producer.join().unwrap();

    assert_eq!(
        refusal(&f, &before, &output),
        "The annotation is over 16 MiB.\n"
    );
}

#[cfg(unix)]
#[test]
fn a_file_that_never_ends_is_refused_once_it_is_over_16_mib() {
    let f = synced();
    let before = manifests(&f);
    let mut command = f.plain(&f.repo, &["annotate", "import", "/dev/zero"]);

    let output = finished(started(&mut command));

    assert_eq!(
        refusal(&f, &before, &output),
        "The annotation is over 16 MiB.\n"
    );
}

#[test]
fn a_refusal_lists_every_problem_at_its_path() {
    let f = synced();
    let mut document = document(&record_in(&f.records));
    document["produced_by"] = json!("");
    document["decisions"][0]["title"] = json!(" ");
    document["decisions"][0]["cites"] = json!(["edit:0000000000000000"]);
    let file = written_json(&f, "annotation.json", &document);

    let message = refused(&f, &mut annotate(&f, &f.repo, &["import", &file]));

    assert_eq!(
        message,
        "Annotation refused:\n  \
         produced_by: is blank\n  \
         decisions[0].title: is blank\n  \
         decisions[0].cites[0]: \"edit:0000000000000000\" is not an edit or command of this \
         session\n"
    );
}

#[test]
fn a_refusal_lists_100_problems_and_counts_the_rest() {
    let f = synced();
    let mut document = document(&record_in(&f.records));
    document["decisions"] = (0..150)
        .map(|k| json!({"id": format!("d{k}"), "title": " "}))
        .collect();
    let file = written_json(&f, "annotation.json", &document);

    let message = refused(&f, &mut annotate(&f, &f.repo, &["import", &file]));

    let mut lines = vec!["Annotation refused:".to_owned()];
    lines.extend((0..100).map(|k| format!("  decisions[{k}].title: is blank")));
    lines.push("  … and 50 more problems.".to_owned());
    assert_eq!(message, lines.join("\n") + "\n");
}

#[test]
fn a_document_built_on_another_revision_says_to_prepare_again() {
    let f = synced();
    let record = record_in(&f.records);
    let first = written_json(&f, "first.json", &document(&record));
    ran(&mut annotate(&f, &f.repo, &["import", &first]));
    let mut second = document(&record);
    second["based_on"] = json!(1);
    let second = written_json(&f, "second.json", &second);

    // Another import stored a revision after this document's brief.
    let unannotated = refused(&f, &mut annotate(&f, &f.repo, &["import", &first]));
    ran(&mut annotate(&f, &f.repo, &["import", &second]));
    let annotated = refused(&f, &mut annotate(&f, &f.repo, &["import", &second]));

    assert_eq!(
        unannotated,
        "Annotation refused:\n  \
         based_on: none, but the current revision is 1\n\
         Annotation based on no revision, but revision 1 is current; run annotate prepare \
         again.\n"
    );
    assert_eq!(
        annotated,
        "Annotation refused:\n  \
         based_on: 1, but the current revision is 2\n\
         Annotation based on revision 1, but revision 2 is current; run annotate prepare \
         again.\n"
    );
}

#[test]
fn a_session_the_record_does_not_hold_is_refused() {
    let f = synced();
    let mut document = without_lists(document(&record_in(&f.records)));
    document["session"] = json!("22222222-2222-4222-8222-222222222222");
    document["prepared"]["observed"]["count"] = json!(0);
    let file = written_json(&f, "annotation.json", &document);

    let message = refused(&f, &mut annotate(&f, &f.repo, &["import", &file]));

    assert_eq!(
        message,
        "Annotation refused:\n  \
         session: session 22222222-2222-4222-8222-222222222222 has no observations in this \
         record\n"
    );
}

#[test]
fn without_a_record_for_the_checkout_nothing_is_imported_or_listed() {
    let f = synced();
    let file = written_json(&f, "annotation.json", &document(&record_in(&f.records)));

    // The record belongs to the repository, not the directory above it.
    let import = refused(&f, &mut annotate(&f, &f.root, &["import", &file]));
    let list = refused(&f, &mut annotate(&f, &f.root, &["list"]));

    let message = format!(
        "error: no record for {}; run `supersigil session sync` first\n",
        f.root.display()
    );
    assert_eq!(import, message);
    assert_eq!(list, message);
}

#[test]
fn control_bytes_from_the_document_print_escaped() {
    let f = synced();
    let record = record_in(&f.records);
    let mut session = without_lists(document(&record));
    session["session"] = json!("s\u{1b}[2J");
    session["prepared"]["observed"]["count"] = json!(0);
    let mut field = document(&record);
    field["\u{1b}[2J"] = json!(1);
    let mut id = document(&record);
    id["decisions"][0]["id"] = json!("d\u{1b}");

    let refusal = |name: &str, document: &Value| {
        let file = written_json(&f, name, document);
        refused(&f, &mut annotate(&f, &f.repo, &["import", &file]))
    };
    let session = refusal("session.json", &session);
    let field = refusal("field.json", &field);
    let id = refusal("id.json", &id);

    assert_eq!(
        session,
        "Annotation refused:\n  \
         session: session s\\x1b[2J has no observations in this record\n"
    );
    assert!(
        field.starts_with(
            "The annotation is not a valid document: \\x1b[2J: unknown field `\\x1b[2J`"
        ),
        "{field}"
    );
    // A quoted value is already written with Rust's escapes.
    assert_eq!(
        id,
        "Annotation refused:\n  \
         decisions[0].id: \"d\\u{1b}\" holds '\\u{1b}'; an id holds only A-Z, a-z, 0-9, _ and \
         -\n"
    );
    for printed in [&session, &field, &id] {
        assert!(!printed.contains('\u{1b}'), "{printed:?}");
    }
}

#[test]
fn a_busy_record_imports_nothing() {
    let f = synced();
    let record = record_in(&f.records);
    let file = written_json(&f, "annotation.json", &document(&record));
    // A sync, say, holding the record's writer lock.
    let held = record.begin().unwrap();

    let started = Instant::now();
    let message = refused(&f, &mut annotate(&f, &f.repo, &["import", &file]));

    assert!(started.elapsed() < Duration::from_secs(30));
    drop(held);
    assert_eq!(message, "The record is busy; try again.\n");
}

/// The slice fixture with its session in the repository's record, and the
/// repository's directory `sub` with a record of its own holding session
/// `s-sub`, synced first so that the repository gets a record apart.
/// Returns the fixture and `sub`.
fn with_nested_record() -> (Fixture, PathBuf) {
    let f = Fixture::new();
    slice(&f);
    let sub = f.repo.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let text = Session::new("s-sub", &sub).prompt("Hello.").text();
    f.transcript(&sub, "sub.jsonl", &text);
    sync(&f, &sub);
    sync(&f, &f.repo);
    (f, sub)
}

/// The record that owns `checkout` in `f`'s records directory, found as
/// `session show` finds it.
fn record_of(f: &Fixture, checkout: &Path) -> Store {
    supersigil_cli::record_dir::find_record(&f.records, checkout)
        .unwrap()
        .unwrap()
}

#[test]
fn a_document_naming_an_ancestor_checkouts_record_imports_into_that_record() {
    let (f, sub) = with_nested_record();
    let (outer, own) = (record_of(&f, &f.repo), record_of(&f, &sub));
    let file = written_json(&f, "annotation.json", &document(&outer));

    // From the directory, whose own record lacks the session.
    let imported = ran(&mut annotate(
        &f,
        &sub,
        &["import", &file, "--format", "terminal"],
    ));
    let listed = ran(&mut annotate(&f, &sub, &["list", "--format", "json"]));

    assert_eq!(imported, IMPORTED);
    let session = SessionId::new(SESSION);
    assert_eq!(
        outer.snapshot().unwrap().annotation_revisions(&session),
        [AnnotationRevision::FIRST]
    );
    assert!(
        own.snapshot()
            .unwrap()
            .annotation_revisions(&session)
            .is_empty()
    );
    // `list` shows the directory's own record alone.
    let sessions: Vec<Value> = serde_json::from_str::<Value>(&listed).unwrap()["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|listed| listed["session"].clone())
        .collect();
    assert_eq!(sessions, ["s-sub"]);
}

#[test]
fn a_document_names_its_record_when_several_own_the_checkout_at_one_distance() {
    let f = synced();
    let named = record_in(&f.records);
    let twin = Store::create(
        &f.records.join("twin"),
        Association {
            checkout: f.repo.clone(),
        },
    )
    .unwrap();
    let file = written_json(&f, "annotation.json", &document(&named));

    let imported = ran(&mut annotate(
        &f,
        &f.repo,
        &["import", &file, "--format", "terminal"],
    ));
    // `list` picks the record itself, so it still needs there to be one.
    let listed = refused(&f, &mut annotate(&f, &f.repo, &["list"]));

    assert_eq!(imported, IMPORTED);
    let session = SessionId::new(SESSION);
    assert_eq!(
        named.snapshot().unwrap().annotation_revisions(&session),
        [AnnotationRevision::FIRST]
    );
    assert!(
        twin.snapshot()
            .unwrap()
            .annotation_revisions(&session)
            .is_empty()
    );
    assert!(
        listed.contains("is associated with several records"),
        "{listed}"
    );
}

#[test]
fn a_document_naming_a_record_of_no_ancestor_or_none_is_refused() {
    let (f, sub) = with_nested_record();
    let inner = record_of(&f, &sub).manifest().unwrap().record_id;
    // The directory's record lies below the repository, not above it.
    let mut nested = document(&record_of(&f, &f.repo));
    nested["prepared"]["record"] = json!(inner);
    let mut missing = nested.clone();
    missing["prepared"]["record"] = json!("r\u{1b}[2J");
    let nested = written_json(&f, "nested.json", &nested);
    let missing = written_json(&f, "missing.json", &missing);

    let unrelated = refused(&f, &mut annotate(&f, &f.repo, &["import", &nested]));
    let absent = refused(&f, &mut annotate(&f, &f.repo, &["import", &missing]));

    assert_eq!(
        unrelated,
        format!(
            "The annotation names record {inner}, which is not this checkout's or an \
             ancestor's.\n"
        )
    );
    assert_eq!(
        absent,
        "The annotation names record r\\x1b[2J, which does not exist.\n"
    );
}

/// The slice fixture and a second session, `s-other`, synced into the
/// repository's record, the slice session annotated once.
fn annotated() -> Fixture {
    let f = Fixture::new();
    slice(&f);
    let other = Session::new("s-other", &f.repo)
        .prompt("Tidy the notes.")
        .text();
    f.transcript(&f.repo, "other.jsonl", &other);
    sync(&f, &f.repo);
    let file = written_json(&f, "annotation.json", &document(&record_in(&f.records)));
    ran(&mut annotate(&f, &f.repo, &["import", &file]));
    f
}

/// One row of `annotate list`'s table.
fn row(cells: [&str; 6]) -> String {
    let [session, revision, imported, decisions, questions, unseen] = cells;
    format!(
        "{session:<36}  {revision:<8}  {imported:<16}  {decisions:<9}  {questions:<14}  {unseen}\n"
    )
}

#[test]
fn list_shows_each_session_with_its_current_annotation() {
    let f = annotated();
    let record = record_in(&f.records);
    let at = record
        .snapshot()
        .unwrap()
        .current_annotation(&SessionId::new(SESSION))
        .unwrap()
        .unwrap()
        .at;

    // Agents may list.
    let text = ran(annotate(&f, &f.repo, &["list", "--format", "terminal"]).env("CLAUDECODE", "1"));
    let listed = ran(&mut annotate(&f, &f.repo, &["list"]));

    let minute = at.to_string()[..16].replace('T', " ");
    let header = [
        "session",
        "revision",
        "imported (UTC)",
        "decisions",
        "open questions",
        "unseen corrections",
    ];
    assert_eq!(
        text,
        [
            row(header),
            row([SESSION, "1", &minute, "1", "1", "0"]),
            row(["s-other", "-", "-", "0", "0", "0"]),
        ]
        .concat()
    );
    assert_eq!(
        serde_json::from_str::<Value>(&listed).unwrap(),
        json!({
            "schema": "supersigil.annotate-list/1",
            "record": record.manifest().unwrap().record_id,
            "sessions": [
                {"session": SESSION, "revision": 1, "at": at, "decisions": 1,
                 "open_questions": 1, "unseen_corrections": 0},
                {"session": "s-other", "revision": null, "at": null, "decisions": 0,
                 "open_questions": 0, "unseen_corrections": 0},
            ],
        })
    );
}

/// A correction on review `basis` saying decision `d1` of revision 1 of
/// `session` in `record` is explained wrongly.
fn correction(basis: &BasisId, record: &RecordId, session: &str) -> Judgment {
    Judgment {
        id: JudgmentId::generate(),
        basis: basis.clone(),
        kind: JudgmentKind::Correct,
        items: Vec::new(),
        reason: Some("The person never said why.".to_owned()),
        text: None,
        decision: Some(DecisionRef {
            record: record.clone(),
            session: SessionId::new(session),
            revision: AnnotationRevision::FIRST,
            id: "d1".to_owned(),
        }),
        correction: Some(CorrectionKind::Wrong),
        closes: BTreeMap::new(),
        withdraws: Vec::new(),
        who: Who {
            name: "Ada Lovelace".to_owned(),
            email: "ada@example.com".to_owned(),
        },
        at: UtcTime::now(),
    }
}

#[test]
fn list_counts_the_standing_corrections_the_current_revision_has_not_seen() {
    let f = annotated();
    let record = record_in(&f.records);
    let record_id = record.manifest().unwrap().record_id;
    // Corrections go to a review store only `--review-dir` names.
    let reviews = f.root.join("scratch-reviews");
    let reviews_flag = reviews.to_str().unwrap();
    let review = f.json(
        &f.repo,
        &["review", "--format", "json", "--review-dir", reviews_flag],
    );
    let basis: BasisId = serde_json::from_value(review["basis"]["id"].clone()).unwrap();
    let unseen = correction(&basis, &record_id, SESSION);
    let seen = correction(&basis, &record_id, SESSION);
    let withdrawn = correction(&basis, &record_id, SESSION);
    let undo = Judgment {
        kind: JudgmentKind::Undo,
        reason: None,
        decision: None,
        correction: None,
        withdraws: vec![Withdrawal {
            judgment: withdrawn.id.clone(),
            item: None,
        }],
        ..correction(&basis, &record_id, SESSION)
    };
    // A nested checkout's record can hold the same session.
    let in_another_record = correction(&basis, &RecordId::new("r-elsewhere"), SESSION);
    let of_another_session = correction(&basis, &record_id, "s-other");
    let store = review_store(&f, &reviews);
    let mut tx = store.begin().unwrap();
    for judgment in [
        &unseen,
        &seen,
        &withdrawn,
        &undo,
        &in_another_record,
        &of_another_session,
    ] {
        tx.append_judgment(judgment).unwrap();
    }
    tx.commit(UtcTime::now()).unwrap();
    let mut revised = document(&record);
    revised["based_on"] = json!(1);
    revised["corrections_seen"] = json!([seen.id]);
    let file = written_json(&f, "revised.json", &revised);
    ran(&mut annotate(&f, &f.repo, &["import", &file]));

    let unseen_counts = |args: &[&str]| -> Vec<Value> {
        let listed: Value = serde_json::from_str(&ran(&mut annotate(&f, &f.repo, args))).unwrap();
        listed["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["unseen_corrections"].clone())
            .collect()
    };
    let named = unseen_counts(&["list", "--review-dir", reviews_flag]);
    let default = unseen_counts(&["list"]);

    assert_eq!(named, [json!(2), json!(1)]);
    // The default reviews directory holds no review store.
    assert_eq!(default, [json!(0), json!(0)]);
}

#[test]
fn a_record_without_sessions_lists_none() {
    let f = Fixture::new();
    let record = Store::create(
        &f.records.join("empty"),
        Association {
            checkout: f.repo.clone(),
        },
    )
    .unwrap();

    let text = ran(&mut annotate(
        &f,
        &f.repo,
        &["list", "--format", "terminal"],
    ));
    let listed = ran(&mut annotate(&f, &f.repo, &["list"]));

    assert_eq!(text, "No sessions.\n");
    assert_eq!(
        serde_json::from_str::<Value>(&listed).unwrap(),
        json!({
            "schema": "supersigil.annotate-list/1",
            "record": record.manifest().unwrap().record_id,
            "sessions": [],
        })
    );
}

#[test]
fn outside_a_git_repository_no_correction_is_counted() {
    let f = Fixture::new();
    let plain = f.root.join("plain");
    std::fs::create_dir(&plain).unwrap();
    let text = Session::new("s-plain", &plain).prompt("Hello.").text();
    f.transcript(&plain, "plain.jsonl", &text);
    sync(&f, &plain);

    let listed = ran(&mut annotate(&f, &plain, &["list"]));

    assert_eq!(
        serde_json::from_str::<Value>(&listed).unwrap()["sessions"],
        json!([{"session": "s-plain", "revision": null, "at": null, "decisions": 0,
                "open_questions": 0, "unseen_corrections": 0}])
    );
}

#[test]
fn the_store_options_and_their_variable_choose_the_record() {
    let f = Fixture::new();
    slice(&f);
    let records = f.root.join("other-records");
    let records_flag = records.to_str().unwrap();
    let output = f
        .plain(
            &f.repo,
            &[
                "session",
                "--record-dir",
                records_flag,
                "sync",
                "--claude-home",
            ],
        )
        .arg(&f.claude)
        .output()
        .unwrap();
    assert!(output.status.success());
    let file = written_json(&f, "annotation.json", &document(&record_in(&records)));
    let repo = f.repo.to_str().unwrap();

    // From outside the checkout, naming it.
    let imported = ran(&mut annotate(
        &f,
        &f.root,
        &[
            "import",
            &file,
            "--checkout",
            repo,
            "--record-dir",
            records_flag,
            "--format",
            "terminal",
        ],
    ));
    // Options before the subcommand, `--claude-home` among them.
    let by_flag = f.json(&f.repo, &["annotate", "--record-dir", records_flag, "list"]);
    let by_variable =
        ran(annotate(&f, &f.repo, &["list"]).env("SUPERSIGIL_RECORD_DIR", records_flag));
    // The fixture's variable names a records directory without the record.
    let without = refused(&f, &mut annotate(&f, &f.repo, &["list"]));

    assert_eq!(imported, IMPORTED);
    assert_eq!(by_flag["sessions"][0]["revision"], 1);
    assert_eq!(
        serde_json::from_str::<Value>(&by_variable).unwrap(),
        by_flag
    );
    assert!(without.starts_with("error: no record for "), "{without}");
}

#[test]
fn help_describes_each_command_and_who_may_run_them() {
    let help = |args: &[&str]| {
        let output = common::supersigil_cmd().args(args).output().unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    };

    let top = help(&["--help"]);
    let group = help(&["annotate", "--help"]);
    let import = help(&["annotate", "import", "--help"]);

    assert!(
        top.contains("Import annotations of recorded sessions and list them"),
        "{top}"
    );
    for text in [
        "import  Check an annotation document against the record it names, this checkout's or \
         an ancestor's, and store it as the session's next revision",
        "list    List this checkout's sessions with their current annotations",
        "Agents may run these commands.",
    ] {
        assert!(group.contains(text), "{text:?} not in:\n{group}");
    }
    for text in [
        "<FILE>",
        "or - to read standard input",
        "--checkout",
        "--record-dir",
        "--review-dir",
        "--claude-home",
        "--format",
    ] {
        assert!(import.contains(text), "{text:?} not in:\n{import}");
    }
}
