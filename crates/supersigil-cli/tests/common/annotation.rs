//! Annotations and corrections written through the store APIs, as
//! `annotate import` and `judge correct` store them, without the checks
//! those commands make.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};
use supersigil_record::interpretations::{AnnotationDocument, AnnotationRevision, Observed};
use supersigil_record::reviews::{
    BasisId, CorrectionKind, DecisionRef, Judgment, JudgmentKind, Who,
};
use supersigil_record::store::Store;
use supersigil_record::time::UtcTime;
use supersigil_record::{EventId, JudgmentId, RecordId, ReviewStore, SessionId};

use super::fixture::Fixture;

/// The id of `session`'s edit made by tool use `tool`.
pub fn edit_of(session: &str, tool: &str) -> String {
    EventId::derive("edit", &SessionId::new(session), tool)
        .as_str()
        .to_owned()
}

/// The record owning `checkout` in `records`.
pub fn record_of(records: &Path, checkout: &Path) -> Store {
    supersigil_cli::record_dir::find_record(records, checkout)
        .unwrap()
        .unwrap_or_else(|| panic!("no record for {}", checkout.display()))
}

/// Syncs the transcripts `f`'s Claude home holds for `dir`, and any of
/// `transcripts` named, into the record of `dir`, and returns that record.
pub fn synced(f: &Fixture, dir: &Path, transcripts: &[&Path]) -> Store {
    let mut command = f.plain(dir, &["session", "sync"]);
    if transcripts.is_empty() {
        command.arg("--claude-home").arg(&f.claude);
    }
    for transcript in transcripts {
        command.arg("--transcript").arg(transcript);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    record_of(&f.records, dir)
}

/// The id of `record`.
pub fn record_id(record: &Store) -> RecordId {
    record.manifest().unwrap().record_id
}

/// Stores the next annotation revision of `session` in `record`, built on
/// the current one and prepared from the record as it is now, and returns
/// its revision. `fields` holds the document's other fields as it writes
/// them (`decisions`, `open_questions`, `corrections_seen`), and may
/// replace `prepared`.
pub fn annotate(record: &Store, session: &str, fields: &Value) -> AnnotationRevision {
    let id = SessionId::new(session);
    let snapshot = record.snapshot().unwrap();
    let current = snapshot.annotation_revisions(&id).pop();
    let mut document = json!({
        "schema": "supersigil.annotation/1",
        "session": session,
        "based_on": current,
        "prepared": {
            "record": snapshot.manifest().record_id,
            "observation_revision": snapshot.revision(),
            "observed": Observed::of(&snapshot.observations(&id).unwrap()),
        },
        "produced_by": "claude-code/supersigil-annotate",
    });
    for (key, value) in fields.as_object().unwrap() {
        document[key] = value.clone();
    }
    let document: AnnotationDocument = serde_json::from_value(document).unwrap();
    let mut tx = record.begin().unwrap();
    let revision = tx.put_annotation(document).unwrap();
    tx.commit().unwrap();
    revision
}

/// A decision `id` titled `title` citing `cites`, with one inferred line of
/// rationale.
pub fn decision(id: &str, title: &str, cites: &[&str]) -> Value {
    json!({
        "id": id,
        "title": title,
        "cites": cites,
        "rationale": [{"text": format!("why {id}"), "speaker": "annotator", "explicit": false}],
    })
}

/// A reference to decision `id` of `session`'s `revision` in `record`.
pub fn reference(record: &RecordId, session: &str, revision: u32, id: &str) -> DecisionRef {
    DecisionRef {
        record: record.clone(),
        session: SessionId::new(session),
        revision: AnnotationRevision::new(revision),
        id: id.to_owned(),
    }
}

/// A correction of `kind` of the decision `decision` names, recorded on
/// review `basis` by Test at `at`, with `words` as its reason or, for a
/// constraint, its text.
pub fn correction(
    basis: &BasisId,
    decision: DecisionRef,
    kind: CorrectionKind,
    words: &str,
    at: &str,
) -> Judgment {
    let constraint = kind == CorrectionKind::Constraint;
    Judgment {
        id: JudgmentId::generate(),
        basis: basis.clone(),
        kind: JudgmentKind::Correct,
        items: Vec::new(),
        reason: (!constraint).then(|| words.to_owned()),
        text: constraint.then(|| words.to_owned()),
        decision: Some(decision),
        correction: Some(kind),
        closes: BTreeMap::new(),
        withdraws: Vec::new(),
        who: Who {
            name: "Test".to_owned(),
            email: "test@example.com".to_owned(),
        },
        at: at.parse::<UtcTime>().unwrap(),
    }
}

/// Appends `judgments` to `store` in one commit.
pub fn record_judgments<'j>(
    store: &ReviewStore,
    judgments: impl IntoIterator<Item = &'j Judgment>,
) {
    let mut tx = store.begin().unwrap();
    for judgment in judgments {
        tx.append_judgment(judgment).unwrap();
    }
    tx.commit(UtcTime::now()).unwrap();
}

/// The id of the saved basis of `review`, as the store names it.
pub fn saved_basis(review: &Value) -> BasisId {
    serde_json::from_value(review["basis"]["id"].clone()).unwrap()
}
