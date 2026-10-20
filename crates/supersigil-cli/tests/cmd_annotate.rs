//! Tests `supersigil annotate` against the slice fixture's record. `prepare`
//! writes the brief an annotator reads; an import is checked against the
//! session's observations and stored as its next revision; a refused import
//! writes nothing. `list` shows each session's current annotation and the
//! corrections that annotation has not seen.

mod common;

use std::collections::BTreeMap;
use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Output, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::Command;
use common::annotation::{
    self, decision, edit_of, record_id, record_judgments, record_of, reference, saved_basis,
    synced as sync_into,
};
use common::finishes_within;
use common::fixture::{Fixture, review_store, slice, slice_text, write};
use common::session::Session;
use serde::de::{DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Value, json};
use supersigil_record::interpretations::{AnnotationDocument, AnnotationRevision, Observed};
use supersigil_record::reviews::{
    BasisId, CorrectionKind, Judgment, JudgmentKind, Who, Withdrawal,
};
use supersigil_record::store::{Association, Manifest, Store, stores_in};
use supersigil_record::time::UtcTime;
use supersigil_record::{EventId, Observation, RecordId, SessionId, TurnId};

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
    sync_into(&f, &f.repo, &[]);
    f
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
    edit_of(SESSION, tool)
}

/// A first annotation of the slice session, prepared from `record` as it is
/// now: one decision citing the first edit, the person's request quoted as
/// its rationale, and one open question about the same edit.
fn document(record: &Store) -> Value {
    let snapshot = record.snapshot().unwrap();
    let observed = Observed::of(&snapshot.observations(&SessionId::new(SESSION)).unwrap());
    json!({
        "schema": "supersigil.annotation/1",
        "session": SESSION,
        "based_on": null,
        "prepared": {
            "record": snapshot.manifest().record_id,
            "observation_revision": snapshot.revision(),
            "observed": observed,
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
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
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
    document["prepared"]["observed"] = json!({"count": 0, "last": null});
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
    session["prepared"]["observed"] = json!({"count": 0, "last": null});
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
/// Returns the fixture, `sub`, and the path of `s-sub`'s transcript.
fn with_nested_record() -> (Fixture, PathBuf, PathBuf) {
    let f = Fixture::new();
    slice(&f);
    let sub = f.repo.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let text = Session::new("s-sub", &sub).prompt("Hello.").text();
    let transcript = f.transcript(&sub, "sub.jsonl", &text);
    sync_into(&f, &sub, &[]);
    sync_into(&f, &f.repo, &[]);
    (f, sub, transcript)
}

#[test]
fn prepare_reads_a_session_held_by_several_records_from_the_one_a_review_reads() {
    let (f, sub, transcript) = with_nested_record();
    // The repository's record holds `s-sub` too, and annotates it.
    let outer = sync_into(&f, &f.repo, &[&transcript]);
    annotation::annotate(
        &outer,
        "s-sub",
        &json!({"decisions": [decision("d1", "Say hello", &[])]}),
    );

    let brief = brief(&f, &sub, &["s-sub"]);

    // Not the directory's own record, which is nearer but unannotated: a
    // review from the directory reads the annotated one, so a revision
    // must build on it.
    assert_eq!(brief["record"], json!(record_id(&outer)));
    assert_eq!(brief["current"]["revision"], json!(1));
    assert_eq!(brief["checkout"], json!(sub));
}

#[test]
fn a_document_naming_an_ancestor_checkouts_record_imports_into_that_record() {
    let (f, sub, _) = with_nested_record();
    let (outer, own) = (record_of(&f.records, &f.repo), record_of(&f.records, &sub));
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
    assert_eq!(own.snapshot().unwrap().annotation_revisions(&session), []);
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
    assert_eq!(twin.snapshot().unwrap().annotation_revisions(&session), []);
    assert!(
        listed.contains("is associated with several records"),
        "{listed}"
    );
}

#[test]
fn a_document_naming_a_record_of_no_ancestor_or_none_is_refused() {
    let (f, sub, _) = with_nested_record();
    let inner = record_of(&f.records, &sub).manifest().unwrap().record_id;
    // The directory's record lies below the repository, not above it.
    let mut nested = document(&record_of(&f.records, &f.repo));
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
    sync_into(&f, &f.repo, &[]);
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
        who: Who {
            name: "Ada Lovelace".to_owned(),
            email: "ada@example.com".to_owned(),
        },
        at: UtcTime::now(),
        ..annotation::correction(
            basis,
            reference(record, session, 1, "d1"),
            CorrectionKind::Wrong,
            "The person never said why.",
            "2026-01-01T00:00:00Z",
        )
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
    let basis = saved_basis(&review);
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
    record_judgments(
        &review_store(&f, &reviews),
        [
            &unseen,
            &seen,
            &withdrawn,
            &undo,
            &in_another_record,
            &of_another_session,
        ],
    );
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
    sync_into(&f, &plain, &[]);

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
    let prepare = help(&["annotate", "prepare", "--help"]);
    let import = help(&["annotate", "import", "--help"]);

    assert!(
        top.contains("Prepare, import and list annotations of recorded sessions"),
        "{top}"
    );
    for text in [
        "prepare  Write the brief an annotator reads for a session of this checkout's record or \
         an ancestor's",
        "import   Check an annotation document against the record it names, this checkout's or \
         an ancestor's, and store it as the session's next revision",
        "list     List this checkout's sessions with their current annotations",
        "Agents may run these commands.",
    ] {
        assert!(group.contains(text), "{text:?} not in:\n{group}");
    }
    let store_options = [
        "--checkout",
        "--record-dir",
        "--review-dir",
        "--claude-home",
    ];
    for text in ["<SESSION>", "a unique prefix is enough", "--out <FILE>"]
        .into_iter()
        .chain(store_options)
    {
        assert!(prepare.contains(text), "{text:?} not in:\n{prepare}");
    }
    // The brief is always JSON.
    assert!(!prepare.contains("--format"), "{prepare}");
    for text in ["<FILE>", "or - to read standard input", "--format"]
        .into_iter()
        .chain(store_options)
    {
        assert!(import.contains(text), "{text:?} not in:\n{import}");
    }
}

/// Runs `annotate prepare <args>` in `dir`, which must succeed, and parses
/// the brief it prints.
fn brief(f: &Fixture, dir: &Path, args: &[&str]) -> Value {
    serde_json::from_str(&ran(&mut annotate(f, dir, &[&["prepare"], args].concat()))).unwrap()
}

/// The `id` of each entry of `list`.
fn ids(list: &Value) -> Vec<&Value> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|entry| &entry["id"])
        .collect()
}

/// Each of `transcript`'s turns as `[id, sighting, role, ordinal, edit ids,
/// command ids]`.
fn turns(transcript: &Value) -> Vec<Value> {
    transcript["turns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|turn| {
            json!([
                turn["id"],
                turn["sighting"],
                turn["role"],
                turn["ordinal"],
                ids(&turn["edits"]),
                ids(&turn["commands"]),
            ])
        })
        .collect()
}

/// The id of session `session`'s edit or command made by tool use `tool`.
fn event(kind: &str, session: &str, tool: &str) -> String {
    EventId::derive(kind, &SessionId::new(session), tool)
        .as_str()
        .to_owned()
}

#[test]
fn the_brief_names_the_record_the_session_and_what_was_observed() {
    let f = synced();
    let record = record_in(&f.records);
    let snapshot = record.snapshot().unwrap();
    let session = SessionId::new(SESSION);
    let observations = snapshot.observations(&session).unwrap();
    let derivations = snapshot.derivations(&session).unwrap().unwrap();

    // Agents may prepare, naming the session by a prefix.
    let printed = ran(annotate(&f, &f.repo, &["prepare", "1111"]).env("CLAUDECODE", "1"));

    let mut brief: Value = serde_json::from_str(&printed).unwrap();
    let fields = brief.as_object_mut().unwrap();
    let transcripts = fields.remove("transcripts").unwrap();
    assert_eq!(
        brief,
        json!({
            "schema": "supersigil.annotation-brief/1",
            "record": snapshot.manifest().record_id,
            "observation_revision": snapshot.revision(),
            "session": SESSION,
            "checkout": f.repo,
            "started": "2026-09-28T10:00:00Z",
            "branch": "main",
            // The latest time any observation carries, without its fraction.
            "observed": {"count": observations.len(), "last": "2026-09-28T10:01:10Z"},
            "reconciled": true,
            // The default reviews directory holds no review store.
            "repository": null,
            "current": null,
            "corrections": [],
            "delegations": [],
            "restores": derivations.restores,
            "discontinuities": derivations.discontinuities,
        })
    );
    let [main] = transcripts.as_array().unwrap().as_slice() else {
        panic!("{transcripts:#}");
    };
    assert_eq!(main["agent_id"], Value::Null);
    assert_eq!(main["sidechain"], false);
    assert_eq!(
        main["turns"][0]["text"],
        json!({"state": "retained", "value": "Add a greeting function and clean up."})
    );
    assert_eq!(main["turns"][1]["parent"], "u1");
    assert_eq!(main["turns"][1]["time"], "2026-09-28T10:00:05.000Z");
}

/// Session `s-split`: its main transcript dispatches agent `a1`, which
/// writes the notes and edits a notebook the capture cannot read; the main
/// transcript learns that it stopped, then writes a greeting and runs the
/// tests.
fn split() -> Fixture {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    let main = Session::new("s-split", &f.repo)
        .prompt("Add a greeting and notes.")
        .dispatch("toolu_d1", "a1")
        .stopped("toolu_d1", "a1")
        .create("toolu_m1", &f.repo.join("greet.rs"), "pub fn greet() {}\n")
        .bash("toolu_m2", "cargo test", None)
        .text();
    // Named to sort after the agent's transcript by path.
    f.transcript(&f.repo, "z-split.jsonl", &main);
    let agent = Session::agent("s-split", &f.repo, "a1")
        .prompt("Write the notes.")
        .create("toolu_a1", &f.repo.join("notes.txt"), "notes\n")
        .notebook_edit("toolu_a2")
        .text();
    f.agent_transcript(&f.repo, "s-split", "a1", &agent);
    sync_into(&f, &f.repo, &[]);
    f
}

#[test]
fn each_transcript_lists_its_turns_in_order_with_their_edits_and_commands() {
    let f = split();
    let observed = record_in(&f.records)
        .snapshot()
        .unwrap()
        .observations(&SessionId::new("s-split"))
        .unwrap()
        .len();

    let brief = brief(&f, &f.repo, &["s-split"]);

    let [main, agent] = brief["transcripts"].as_array().unwrap().as_slice() else {
        panic!("{:#}", brief["transcripts"]);
    };
    // The main transcript first, then agent transcripts.
    assert!(
        main["transcript"]
            .as_str()
            .unwrap()
            .ends_with("z-split.jsonl"),
        "{main:#}"
    );
    assert_eq!(
        (&main["agent_id"], &main["sidechain"]),
        (&json!(null), &json!(false))
    );
    assert!(
        agent["transcript"]
            .as_str()
            .unwrap()
            .ends_with("agent-a1.jsonl"),
        "{agent:#}"
    );
    assert_eq!(
        (&agent["agent_id"], &agent["sidechain"]),
        (&json!("a1"), &json!(true))
    );
    // Each turn carries the edits and commands it issued. The agent's turn
    // ids repeat the main transcript's, which never confuses them.
    let (edit, command) = (
        event("edit", "s-split", "toolu_m1"),
        event("command", "s-split", "toolu_m2"),
    );
    assert_eq!(
        turns(main),
        [
            json!(["s-split-0", 1, "human", 0, [], []]),
            json!(["s-split-1", 1, "agent", 1, [], []]),
            json!(["s-split-2", 1, "tool", 2, [], []]),
            json!(["s-split-3", 1, "report", 3, [], []]),
            json!(["s-split-4", 1, "agent", 4, [edit], []]),
            json!(["s-split-5", 1, "tool", 5, [], []]),
            json!(["s-split-6", 1, "agent", 6, [], [command]]),
            json!(["s-split-7", 1, "tool", 7, [], []]),
        ]
    );
    assert_eq!(
        turns(agent),
        [
            json!(["s-split-0", 1, "delegation", 0, [], []]),
            json!([
                "s-split-1",
                1,
                "agent",
                1,
                [event("edit", "s-split", "toolu_a1")],
                []
            ]),
            json!(["s-split-2", 1, "tool", 2, [], []]),
            json!(["s-split-3", 1, "agent", 3, [], []]),
            json!(["s-split-4", 1, "tool", 4, [], []]),
        ]
    );
    // The notebook edit is a gap in the agent's transcript alone.
    assert_eq!(main["capture_limitations"], json!([]));
    let [gap] = agent["capture_limitations"].as_array().unwrap().as_slice() else {
        panic!("{agent:#}");
    };
    assert_eq!(gap["unsupported_tool_uses"], 1);
    assert_eq!(gap["transcript"], agent["transcript"]);
    let steps: Vec<&Value> = brief["delegations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|delegation| &delegation["step"]["type"])
        .collect();
    assert_eq!(steps, ["start", "stop", "receive"]);
    assert_eq!(
        brief["observed"],
        json!({"count": observed, "last": "2026-09-29T10:00:07Z"})
    );
    assert_eq!(brief["started"], "2026-09-29T10:00:00Z");
    // The session's transcripts named no branch.
    assert_eq!(brief["branch"], Value::Null);
}

#[test]
fn a_turn_sighted_again_in_a_rewritten_transcript_is_listed_once_per_sighting() {
    let f = synced();
    // The transcript rewritten shorter is read again from the start: its
    // first three records are sighted again under the same ids.
    let first_three: String = slice_text().split_inclusive('\n').take(3).collect();
    f.transcript(&f.repo, "slice.jsonl", &first_three);
    sync_into(&f, &f.repo, &[]);

    let brief = brief(&f, &f.repo, &[SESSION]);

    let listed = turns(&brief["transcripts"][0]);
    let first = edit("toolu_01");
    assert_eq!(
        listed[..7],
        [
            json!(["u1", 1, "human", 0, [], []]),
            json!(["u1", 2, "human", 0, [], []]),
            json!(["a1", 1, "agent", 1, [first], []]),
            json!(["a1", 2, "agent", 1, [first], []]),
            json!(["u2", 1, "tool", 2, [], []]),
            json!(["u2", 2, "tool", 2, [], []]),
            json!(["a2", 1, "agent", 3, [edit("toolu_02")], []]),
        ]
    );
}

#[test]
fn an_edit_whose_turn_its_transcript_lacks_is_listed_under_a_final_entry() {
    let f = synced();
    let record = record_in(&f.records);
    let session = SessionId::new(SESSION);
    let mut orphan = record
        .snapshot()
        .unwrap()
        .observations(&session)
        .unwrap()
        .into_iter()
        .find_map(|observation| match observation {
            Observation::Edit(edit) => Some(edit),
            _ => None,
        })
        .unwrap();
    orphan.id = EventId::new("edit:00000000deadbeef");
    orphan.turn = TurnId::new("gone");
    let mut tx = record.begin().unwrap();
    tx.append_observations(&[Observation::Edit(orphan)])
        .unwrap();
    tx.commit().unwrap();

    let brief = brief(&f, &f.repo, &[SESSION]);

    let listed = turns(&brief["transcripts"][0]);
    assert_eq!(listed.len(), 18);
    assert_eq!(
        listed[17],
        json!([null, null, null, null, ["edit:00000000deadbeef"], []])
    );
    let last = &brief["transcripts"][0]["turns"][17];
    for field in ["parent", "time", "text"] {
        assert_eq!(last[field], Value::Null, "{field}");
    }
}

#[test]
fn edits_under_a_turn_sighted_two_thousand_times_are_listed_promptly() {
    let f = synced();
    let record = record_in(&f.records);
    let observations = record
        .snapshot()
        .unwrap()
        .observations(&SessionId::new(SESSION))
        .unwrap();
    let turn = observations
        .iter()
        .find_map(|observation| match observation {
            Observation::Turn(turn) if turn.id.as_str() == "a1" => Some(turn.clone()),
            _ => None,
        })
        .unwrap();
    let first = observations
        .iter()
        .find_map(|observation| match observation {
            Observation::Edit(edit) => Some(edit.clone()),
            _ => None,
        })
        .unwrap();
    // The transcript rewritten 2,000 times, each time sighting turn `a1`
    // again and recording an edit under it.
    let rewritten: Vec<Observation> = (0..2_000)
        .flat_map(|k| {
            let mut edit = first.clone();
            edit.id = EventId::new(format!("edit:{k:016x}"));
            [Observation::Turn(turn.clone()), Observation::Edit(edit)]
        })
        .collect();
    let mut tx = record.begin().unwrap();
    tx.append_observations(&rewritten).unwrap();
    tx.commit().unwrap();

    let brief = finishes_within(Duration::from_secs(20), move || {
        brief(&f, &f.repo, &[SESSION])
    });

    let sightings: Vec<Value> = turns(&brief["transcripts"][0])
        .into_iter()
        .filter(|turn| turn[0] == "a1")
        .collect();
    assert_eq!(sightings.len(), 2_001);
    assert_eq!(
        sightings[0],
        json!(["a1", 1, "agent", 1, [edit("toolu_01")], []])
    );
    // Each edit sits under the sighting logged last before it.
    for (k, sighting) in sightings[1..].iter().enumerate() {
        assert_eq!(
            *sighting,
            json!(["a1", k + 2, "agent", 1, [format!("edit:{k:016x}")], []])
        );
    }
}

#[test]
fn an_edit_carries_its_hunks_or_else_its_new_text_cut_at_2000_characters() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    // Two bytes a character, so a cut by bytes would show.
    let long = "é".repeat(3_000);
    let text = Session::new("s-write", &f.repo)
        .prompt("Write it out.")
        .create("toolu_long", &f.repo.join("long.txt"), &long)
        .create("toolu_short", &f.repo.join("short.txt"), "short\n")
        .edit(
            "toolu_edit",
            &f.repo.join("README.md"),
            "readme\n",
            "readme",
            "read me",
        )
        .text();
    f.transcript(&f.repo, "s-write.jsonl", &text);
    sync_into(&f, &f.repo, &[]);
    let replaced = event("edit", "s-write", "toolu_edit");
    let hunks = record_in(&f.records)
        .snapshot()
        .unwrap()
        .observations(&SessionId::new("s-write"))
        .unwrap()
        .into_iter()
        .find_map(|observation| match observation {
            Observation::Edit(edit) if edit.id.as_str() == replaced => {
                Some(edit.patch.retained().cloned().unwrap())
            }
            _ => None,
        })
        .unwrap();
    assert!(!hunks.is_empty(), "the edit retains its display patch");

    let brief = brief(&f, &f.repo, &["s-write"]);

    let edits: Vec<&Value> = brief["transcripts"][0]["turns"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|turn| turn["edits"].as_array().unwrap())
        .collect();
    assert_eq!(
        edits,
        [
            &json!({"id": event("edit", "s-write", "toolu_long"), "path": "long.txt",
                    "operation": "write", "patch": null, "new_text": "é".repeat(2_000),
                    "truncated": true}),
            &json!({"id": event("edit", "s-write", "toolu_short"), "path": "short.txt",
                    "operation": "write", "patch": null, "new_text": "short\n",
                    "truncated": false}),
            &json!({"id": replaced, "path": "README.md", "operation": "replace",
                    "patch": hunks, "new_text": null, "truncated": false}),
        ]
    );
}

#[test]
fn the_current_annotation_is_given_as_the_record_stores_it() {
    let f = synced();
    let record = record_in(&f.records);
    let unannotated = brief(&f, &f.repo, &[SESSION]);
    let file = written_json(&f, "annotation.json", &document(&record));
    ran(&mut annotate(&f, &f.repo, &["import", &file]));

    let annotated = brief(&f, &f.repo, &[SESSION]);

    let stored = record
        .snapshot()
        .unwrap()
        .current_annotation(&SessionId::new(SESSION))
        .unwrap()
        .unwrap();
    assert_eq!(unannotated["current"], Value::Null);
    assert_eq!(annotated["current"], serde_json::to_value(&stored).unwrap());
    assert_eq!(annotated["current"]["revision"], 1);
}

/// The slice session annotated twice, with corrections recorded in a
/// review store only `--review-dir` names. Revision 1's `d1` cites the
/// first edit; revision 2 splits it into `d2`, citing the same edit, and
/// `d3`, citing the fifth, and has seen the correction of `d1`.
///
/// Returns the fixture, the reviews directory, the review the corrections
/// were recorded on, and the standing corrections of the session: of `d1`,
/// of a decision in a record that does not exist, and of one in a revision
/// this record lacks. A withdrawn correction and one of another session are
/// recorded as well.
fn corrected() -> (Fixture, String, BasisId, [Judgment; 3]) {
    let f = annotated();
    let record = record_in(&f.records);
    let record_id = record.manifest().unwrap().record_id;
    let reviews = f.root.join("scratch-reviews").to_str().unwrap().to_owned();
    let review = f.json(
        &f.repo,
        &["review", "--format", "json", "--review-dir", &reviews],
    );
    let basis = saved_basis(&review);
    let of_d1 = correction(&basis, &record_id, SESSION);
    let gone = correction(&basis, &RecordId::new("r-gone"), SESSION);
    let mut unrevised = correction(&basis, &record_id, SESSION);
    unrevised.decision.as_mut().unwrap().revision = AnnotationRevision::new(9);
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
    let of_another_session = correction(&basis, &record_id, "s-other");
    record_judgments(
        &review_store(&f, Path::new(&reviews)),
        [
            &of_d1,
            &gone,
            &withdrawn,
            &undo,
            &of_another_session,
            &unrevised,
        ],
    );
    let mut revised = document(&record);
    revised["based_on"] = json!(1);
    revised["corrections_seen"] = json!([of_d1.id]);
    revised["decisions"] = json!([
        {"id": "d2", "title": "Add a greeting function", "cites": [edit("toolu_01")]},
        {"id": "d3", "title": "Capitalise the greeting", "cites": [edit("toolu_05")]},
    ]);
    let file = written_json(&f, "revised.json", &revised);
    ran(&mut annotate(&f, &f.repo, &["import", &file]));
    (f, reviews, basis, [of_d1, gone, unrevised])
}

#[test]
fn corrections_carry_what_they_addressed_and_the_decisions_they_anchor_to() {
    let (f, reviews, basis, [of_d1, gone, unrevised]) = corrected();
    let record_id = record_in(&f.records).manifest().unwrap().record_id;

    let named = brief(&f, &f.repo, &[SESSION, "--review-dir", &reviews]);
    let default = brief(&f, &f.repo, &[SESSION]);

    let listed = |judgment: &Judgment, seen_by: Value, addressed: Value, anchors_to: Value| {
        let mut listed = json!({
            "judgment": judgment.id,
            "kind": "wrong",
            "decision": judgment.decision,
            "reason": "The person never said why.",
            "text": null,
            "who": {"name": "Ada Lovelace", "email": "ada@example.com"},
            "at": judgment.at,
            "seen_by": seen_by,
            "review": basis,
            "addressed": addressed,
            "anchors_to": anchors_to,
        });
        if let Value::String(_) = &listed["addressed"] {
            listed["addressed_unavailable"] = listed["addressed"].take();
        }
        listed
    };
    assert_eq!(named["current"]["revision"], 2);
    assert_eq!(named["repository"], json!(f.repo.join(".git")));
    assert_eq!(
        named["corrections"],
        json!([
            // Read from this record's revision 1; revision 2's d2 cites its edit.
            listed(
                &of_d1,
                json!([2]),
                json!({
                    "title": "Add a greeting function",
                    "rationale": [{
                        "text": "The person asked for one.",
                        "speaker": "human",
                        "explicit": true,
                        "source": {"turn": "u1", "quote": "Add a greeting function"},
                    }],
                    "cites": [edit("toolu_01")],
                }),
                json!(["d2"]),
            ),
            listed(&gone, json!([]), json!("record not found"), json!([])),
            listed(
                &unrevised,
                json!([]),
                json!("revision not found"),
                json!([])
            ),
        ])
    );
    assert_eq!(gone.decision.as_ref().unwrap().record.as_str(), "r-gone");
    assert_eq!(unrevised.decision.as_ref().unwrap().record, record_id);
    // Without the store, no correction is listed.
    assert_eq!(default["repository"], Value::Null);
    assert_eq!(default["corrections"], json!([]));
}

#[test]
fn corrections_of_one_decision_share_what_it_addressed() {
    let f = annotated();
    let record = record_in(&f.records);
    let record_id = record.manifest().unwrap().record_id;
    let reviews = f.root.join("scratch-reviews").to_str().unwrap().to_owned();
    let review = f.json(
        &f.repo,
        &["review", "--format", "json", "--review-dir", &reviews],
    );
    let basis = saved_basis(&review);
    // Three corrections of revision 1's d1, which revision 2 replaces.
    let corrections = [(); 3].map(|()| correction(&basis, &record_id, SESSION));
    record_judgments(&review_store(&f, Path::new(&reviews)), &corrections);
    let mut revised = document(&record);
    revised["based_on"] = json!(1);
    revised["decisions"] = json!([
        {"id": "d2", "title": "Add a greeting function", "cites": [edit("toolu_01")]},
    ]);
    let file = written_json(&f, "revised.json", &revised);
    ran(&mut annotate(&f, &f.repo, &["import", &file]));

    let brief = brief(&f, &f.repo, &[SESSION, "--review-dir", &reviews]);

    let listed = brief["corrections"].as_array().unwrap();
    let judgments: Vec<Value> = listed
        .iter()
        .map(|listed| listed["judgment"].clone())
        .collect();
    let recorded: Vec<Value> = corrections
        .iter()
        .map(|correction| json!(correction.id))
        .collect();
    assert_eq!(judgments, recorded);
    // Each says what d1 said in revision 1.
    let addressed = json!({
        "title": "Add a greeting function",
        "rationale": [{
            "text": "The person asked for one.",
            "speaker": "human",
            "explicit": true,
            "source": {"turn": "u1", "quote": "Add a greeting function"},
        }],
        "cites": [edit("toolu_01")],
    });
    for listed in listed {
        assert_eq!(listed["addressed"], addressed);
        assert!(listed.get("addressed_unavailable").is_none(), "{listed:#}");
        assert_eq!(listed["anchors_to"], json!(["d2"]));
    }
}

#[test]
fn a_revision_that_cannot_be_read_is_left_out_of_the_brief() {
    let (f, reviews, _, [of_d1, ..]) = corrected();
    let record = record_in(&f.records);
    // Revision 3 lists the correction, as revision 2 does.
    let mut third = document(&record);
    third["based_on"] = json!(2);
    third["corrections_seen"] = json!([of_d1.id]);
    let file = written_json(&f, "third.json", &third);
    ran(&mut annotate(&f, &f.repo, &["import", &file]));
    // Damaged store state: the documents of revisions 1 and 2 are gone.
    let manifest = record.manifest().unwrap();
    let number = manifest.sessions[&SessionId::new(SESSION)];
    for revision in [1, 2] {
        let name = &manifest.documents[&format!("interpretations/{number}/{revision}")];
        std::fs::remove_file(record.root().join(name)).unwrap();
    }

    let brief = brief(&f, &f.repo, &[SESSION, "--review-dir", &reviews]);

    assert_eq!(brief["current"]["revision"], 3);
    let listed = &brief["corrections"][0];
    assert_eq!(listed["judgment"], json!(of_d1.id));
    // It addressed revision 1, which cannot be read.
    assert_eq!(listed["addressed"], Value::Null);
    let unavailable = listed["addressed_unavailable"].as_str().unwrap();
    assert!(
        unavailable.starts_with("revision not readable: ") && unavailable.contains("is missing"),
        "{unavailable}"
    );
    assert_eq!(listed["anchors_to"], json!([]));
    // Revision 2 listed it too, but cannot be read.
    assert_eq!(listed["seen_by"], json!([3]));
}

/// The keys of every object in the JSON `text`, in the order written, by
/// the object's path with list positions written `*`: each distinct key
/// list once, in the order first seen.
fn shapes(text: &str) -> BTreeMap<String, Vec<Vec<String>>> {
    let mut shapes = BTreeMap::new();
    Shape {
        path: String::new(),
        shapes: &mut shapes,
    }
    .deserialize(&mut serde_json::Deserializer::from_str(text))
    .unwrap();
    shapes
}

/// Collects [`shapes`] below `path`.
struct Shape<'s> {
    path: String,
    shapes: &'s mut BTreeMap<String, Vec<Vec<String>>>,
}

impl<'de> DeserializeSeed<'de> for Shape<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Shape<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON")
    }

    fn visit_bool<E>(self, _: bool) -> Result<(), E> {
        Ok(())
    }

    fn visit_i64<E>(self, _: i64) -> Result<(), E> {
        Ok(())
    }

    fn visit_u64<E>(self, _: u64) -> Result<(), E> {
        Ok(())
    }

    fn visit_f64<E>(self, _: f64) -> Result<(), E> {
        Ok(())
    }

    fn visit_str<E>(self, _: &str) -> Result<(), E> {
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let path = format!("{}/*", self.path);
        while seq
            .next_element_seed(Shape {
                path: path.clone(),
                shapes: &mut *self.shapes,
            })?
            .is_some()
        {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut keys = Vec::new();
        while let Some(key) = map.next_key::<String>()? {
            map.next_value_seed(Shape {
                path: format!("{}/{key}", self.path),
                shapes: &mut *self.shapes,
            })?;
            keys.push(key);
        }
        let seen = self.shapes.entry(self.path).or_default();
        if !seen.contains(&keys) {
            seen.push(keys);
        }
        Ok(())
    }
}

#[test]
fn every_object_of_the_brief_writes_its_fields_in_the_documented_order() {
    let (f, reviews, ..) = corrected();
    let text = ran(&mut annotate(
        &f,
        &f.repo,
        &["prepare", SESSION, "--review-dir", &reviews],
    ));

    let shapes = shapes(&text);

    let correction = [
        "judgment",
        "kind",
        "decision",
        "reason",
        "text",
        "who",
        "at",
        "seen_by",
        "review",
        "addressed",
    ];
    let expected: [(&str, Vec<Vec<&str>>); 10] = [
        (
            "",
            vec![vec![
                "schema",
                "record",
                "observation_revision",
                "session",
                "checkout",
                "started",
                "branch",
                "observed",
                "reconciled",
                "repository",
                "current",
                "corrections",
                "transcripts",
                "delegations",
                "restores",
                "discontinuities",
            ]],
        ),
        ("/observed", vec![vec!["count", "last"]]),
        ("/current", vec![vec!["revision", "at", "document"]]),
        (
            "/corrections/*",
            vec![
                [&correction[..], &["anchors_to"]].concat(),
                [&correction[..], &["addressed_unavailable", "anchors_to"]].concat(),
            ],
        ),
        (
            "/corrections/*/decision",
            vec![vec!["record", "session", "revision", "id"]],
        ),
        (
            "/corrections/*/addressed",
            vec![vec!["title", "rationale", "cites"]],
        ),
        (
            "/transcripts/*",
            vec![vec![
                "transcript",
                "agent_id",
                "sidechain",
                "turns",
                "capture_limitations",
            ]],
        ),
        (
            "/transcripts/*/turns/*",
            vec![vec![
                "id", "sighting", "parent", "role", "time", "ordinal", "text", "edits", "commands",
            ]],
        ),
        (
            "/transcripts/*/turns/*/edits/*",
            vec![vec![
                "id",
                "path",
                "operation",
                "patch",
                "new_text",
                "truncated",
            ]],
        ),
        (
            "/transcripts/*/turns/*/commands/*",
            vec![vec![
                "id",
                "cmd",
                "category",
                "exit",
                "outcome",
                "stdout_tail",
                "stderr_tail",
            ]],
        ),
    ];
    for (path, keys) in expected {
        assert_eq!(shapes[path], keys, "{path}");
    }
}

#[test]
fn a_record_another_writer_holds_is_read_as_pinned() {
    let f = synced();
    let record = record_in(&f.records);
    // A sync, say, holding the record's writer lock.
    let held = record.begin().unwrap();

    let started = Instant::now();
    let brief = brief(&f, &f.repo, &[SESSION]);

    assert!(started.elapsed() < Duration::from_secs(30));
    drop(held);
    assert_eq!(brief["reconciled"], false);
    assert_eq!(brief["reconcile_note"], "locked by another writer");
    assert_eq!(turns(&brief["transcripts"][0]).len(), 17);
}

/// Appends turn `u10` to the slice transcript of `f`, synced before, so the
/// next reconcile has an observation to publish.
fn with_pending_turn(f: &Fixture) {
    let later = json!({
        "type": "user", "uuid": "u10", "parentUuid": "a8", "sessionId": SESSION,
        "cwd": "/work/repo", "timestamp": "2026-09-28T10:02:00.000Z", "isSidechain": false,
        "message": {"role": "user", "content": "Thanks."},
    });
    f.transcript(
        &f.repo,
        "slice.jsonl",
        &format!("{}{later}\n", slice_text()),
    );
}

#[test]
fn prepare_reconciles_the_record_before_reading_it() {
    let f = synced();
    let before = record_in(&f.records).manifest().unwrap().revision;
    with_pending_turn(&f);

    let brief = brief(&f, &f.repo, &[SESSION]);

    let listed = turns(&brief["transcripts"][0]);
    assert_eq!(listed.last().unwrap()[0], "u10");
    assert!(brief["observation_revision"].as_u64().unwrap() > before.get());
    assert_eq!(brief["observed"]["last"], "2026-09-28T10:02:00Z");
    assert_eq!(brief["reconciled"], true);
}

#[test]
fn out_replaces_the_file_it_names_and_a_directory_is_refused() {
    let f = synced();
    let file = f.root.join("brief.json");
    std::fs::write(&file, "an older brief").unwrap();

    let printed = ran(&mut annotate(
        &f,
        &f.repo,
        &["prepare", SESSION, "--out", file.to_str().unwrap()],
    ));
    let directory = refused(
        &f,
        &mut annotate(
            &f,
            &f.repo,
            &["prepare", SESSION, "--out", f.root.to_str().unwrap()],
        ),
    );

    assert_eq!(printed, "");
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&file).unwrap()).unwrap(),
        brief(&f, &f.repo, &[SESSION])
    );
    assert!(
        directory.starts_with(&format!("error: cannot write {}: ", f.root.display())),
        "{directory}"
    );
    assert_eq!(directory.lines().count(), 1, "{directory}");
}

#[test]
fn a_destination_that_cannot_be_written_is_refused_before_anything_is_reconciled() {
    let f = synced();
    with_pending_turn(&f);
    let parentless = f.root.join("missing").join("brief.json");

    // `refused` checks that no record changed: the pending turn stays
    // unpublished.
    let directory = refused(
        &f,
        &mut annotate(
            &f,
            &f.repo,
            &["prepare", SESSION, "--out", f.root.to_str().unwrap()],
        ),
    );
    let without_directory = refused(
        &f,
        &mut annotate(
            &f,
            &f.repo,
            &["prepare", SESSION, "--out", parentless.to_str().unwrap()],
        ),
    );

    assert_eq!(
        directory,
        format!(
            "error: cannot write {}: it is a directory\n",
            f.root.display()
        )
    );
    assert_eq!(
        without_directory,
        format!(
            "error: cannot write {}: its directory does not exist\n",
            parentless.display()
        )
    );
}

#[test]
fn a_refused_prefix_may_follow_a_sync() {
    let f = synced();
    with_pending_turn(&f);
    let before = record_in(&f.records).manifest().unwrap().revision;

    let output = annotate(&f, &f.repo, &["prepare", "s-none"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "error: no session matching 's-none'\n"
    );
    // The author's ruling: the record is reconciled before the session is
    // resolved, since reconciling can add the session asked for. Syncing is
    // idempotent and is what `review` does before it can fail, so the sync
    // a refused prefix follows is not a side effect of the refusal.
    assert!(record_in(&f.records).manifest().unwrap().revision > before);
}

#[test]
fn a_prefix_matching_several_sessions_or_none_is_refused() {
    let f = Fixture::new();
    for id in ["s-one", "s-two"] {
        let text = Session::new(id, &f.repo).prompt("Hello.").text();
        f.transcript(&f.repo, &format!("{id}.jsonl"), &text);
    }
    sync_into(&f, &f.repo, &[]);

    let several = refused(&f, &mut annotate(&f, &f.repo, &["prepare", "s-"]));
    let none = refused(&f, &mut annotate(&f, &f.repo, &["prepare", "s-three"]));

    assert_eq!(
        several,
        "error: 's-' matches several sessions: s-one, s-two\n"
    );
    assert_eq!(none, "error: no session matching 's-three'\n");
}

/// Session `s-two` of the repository's directory `sub` in two records: the
/// directory's own, synced first so that it stays apart, and the
/// repository's, which also holds session `s-one`. Returns the fixture,
/// `sub`, and the ids of the directory's record and the repository's.
fn one_session_in_two_records() -> (Fixture, PathBuf, [RecordId; 2]) {
    let f = Fixture::new();
    let sub = f.repo.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let two = Session::new("s-two", &sub).prompt("Hello.").text();
    let transcript = f.transcript(&sub, "two.jsonl", &two);
    sync_into(&f, &sub, &[]);
    let one = Session::new("s-one", &f.repo).prompt("Hello.").text();
    f.transcript(&f.repo, "one.jsonl", &one);
    sync_into(&f, &f.repo, &[]);
    let transcript = transcript.to_str().unwrap();
    ran(&mut Command::from_std(f.plain(
        &f.repo,
        &["session", "sync", "--transcript", transcript],
    )));
    let ids = [&sub, &f.repo].map(|checkout| {
        record_of(&f.records, checkout)
            .manifest()
            .unwrap()
            .record_id
    });
    (f, sub, ids)
}

#[test]
fn a_session_is_found_in_the_checkouts_record_or_an_ancestors() {
    let (f, sub, [own, outer]) = one_session_in_two_records();

    let two = brief(&f, &sub, &["s-two"]);
    let one = brief(&f, &sub, &["s-one"]);
    let across = refused(&f, &mut annotate(&f, &sub, &["prepare", "s-"]));
    // The directory's record lies below the repository, so the repository
    // reads its own record alone.
    let within = refused(&f, &mut annotate(&f, &f.repo, &["prepare", "s-"]));

    // A session two of them hold is read from the record a review reads it
    // from: neither annotates it and both hold the same log, so the one
    // with the smaller id, whichever is nearer.
    assert_eq!(two["record"], json!(std::cmp::min(&own, &outer)));
    assert_eq!(one["record"], json!(outer));
    assert_eq!(one["checkout"], json!(sub));
    assert_eq!(
        across,
        format!("error: 's-' matches sessions in several records: {own}, {outer}\n")
    );
    assert_eq!(
        within,
        "error: 's-' matches several sessions: s-one, s-two\n"
    );
}

#[test]
fn a_session_only_an_enclosing_repositorys_record_holds_is_prepared_from_the_nested_one() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    // An independent repository inside the outer one, whose own session
    // gives it a record.
    let nested = f.repo.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    f.git_in(&nested, &["init", "-q", "-b", "main"]);
    write(&nested, "README.md", "nested\n");
    f.commit(&nested, "base");
    let inner = Session::new("s-inner", &nested)
        .prompt("Look around.")
        .text();
    f.transcript(&nested, "inner.jsonl", &inner);
    sync_into(&f, &nested, &[]);
    // A session run in the outer repository writes a file of the nested
    // one; only the outer repository's record holds it.
    let content = "pub fn a() {}\n";
    let outer = Session::new("s-outer", &f.repo)
        .prompt("Add a to the nested repository.")
        .create("toolu_a", &nested.join("a.rs"), content)
        .text();
    f.transcript(&f.repo, "outer.jsonl", &outer);
    write(&nested, "a.rs", content);
    sync_into(&f, &f.repo, &[]);
    // A review from the nested repository saves its review store.
    f.json(
        &nested,
        &["review", "--format", "json", "--include-untracked", "a.rs"],
    );

    let brief = brief(
        &f,
        &nested,
        &["s-outer", "--checkout", nested.to_str().unwrap()],
    );

    assert_eq!(brief["session"], "s-outer");
    assert_eq!(
        brief["record"],
        json!(record_of(&f.records, &f.repo).manifest().unwrap().record_id)
    );
    assert_eq!(brief["checkout"], json!(nested));
    // The review store stays the nested repository's.
    assert_eq!(brief["repository"], json!(nested.join(".git")));
    let [main] = brief["transcripts"].as_array().unwrap().as_slice() else {
        panic!("{:#}", brief["transcripts"]);
    };
    assert_eq!(
        turns(main)[1],
        json!([
            "s-outer-1",
            1,
            "agent",
            1,
            [event("edit", "s-outer", "toolu_a")],
            []
        ])
    );
}

#[test]
fn prepare_reconciles_the_records_a_review_from_the_checkout_reconciles() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    f.git(&["worktree", "add", "-q", "-b", "x", ".claude/worktrees/x"]);
    // Joined by component, as the brief prints the canonical path.
    let worktree = f.repo.join(".claude").join("worktrees").join("x");
    let own = Session::new("s-own", &worktree).prompt("Hello.").text();
    f.transcript(&worktree, "own.jsonl", &own);
    sync_into(&f, &worktree, &[]);
    // A session in the main checkout writes a file in the worktree, and
    // nothing has synced it: a review from the worktree reads the main
    // checkout's project directory too.
    let parent = Session::new("s-parent", &f.repo)
        .prompt("Add n to the worktree.")
        .create("toolu_parent", &worktree.join("n.rs"), "pub fn n() {}\n")
        .text();
    f.transcript(&f.repo, "parent.jsonl", &parent);

    let brief = brief(
        &f,
        &worktree,
        &["s-parent", "--claude-home", f.claude.to_str().unwrap()],
    );

    // Reconciling gave the main checkout a record, above the worktree's.
    assert_eq!(
        brief["record"],
        json!(record_of(&f.records, &f.repo).manifest().unwrap().record_id)
    );
    assert_eq!(brief["checkout"], json!(worktree));
}

#[test]
fn prepare_reads_the_record_the_store_options_name() {
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
    let repo = f.repo.to_str().unwrap();

    // From outside the checkout, naming it.
    let brief = brief(
        &f,
        &f.root,
        &[SESSION, "--checkout", repo, "--record-dir", records_flag],
    );
    // The fixture's variable names a records directory without the record.
    let without = refused(&f, &mut annotate(&f, &f.repo, &["prepare", SESSION]));

    assert_eq!(
        brief["record"],
        json!(record_in(&records).manifest().unwrap().record_id)
    );
    assert_eq!(brief["checkout"], repo);
    assert_eq!(
        without,
        format!(
            "error: no record for {}; run `supersigil session sync` first\n",
            f.repo.display()
        )
    );
}
