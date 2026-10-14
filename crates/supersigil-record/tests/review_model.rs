//! Tests the review store's persisted types: basis identity, path bytes,
//! gate item identity, fingerprints, and judgments.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::json;
use supersigil_record::observations::CaptureCounts;
use supersigil_record::reviews::{
    Algorithms, BASIS_SCHEMA, Basis, BasisBase, BasisCheckout, BasisRecord, BasisScope,
    BasisTarget, CaptureEntry, Explanation, Fingerprint, FingerprintPart, GateDetail, GateItem,
    ItemId, Judgment, JudgmentKind, Origin, PathBytes, PinnedLog, Subject, TargetKind, Who,
    Withdrawal,
};
use supersigil_record::time::UtcTime;
use supersigil_record::{JudgmentId, RecordId, Revision};

fn known(material: &str) -> FingerprintPart {
    FingerprintPart::of(material.as_bytes())
}

fn fingerprint(parts: &[(&str, FingerprintPart)]) -> Fingerprint {
    Fingerprint(
        parts
            .iter()
            .map(|(name, part)| ((*name).to_owned(), part.clone()))
            .collect(),
    )
}

fn lines_item(path: &str) -> GateItem {
    GateItem::new(
        Subject::Path(PathBytes::new(path)),
        GateDetail::UnexplainedLines {
            unattributed: 8,
            content_match_incomplete: 0,
            unresolved: 0,
            ambiguous: 4,
            line_ending: 0,
            content_match_only: 0,
        },
        fingerprint(&[
            ("content", known("blobs")),
            ("evidence", known("edits")),
            ("outcomes", known("spans")),
        ]),
    )
}

/// A basis with every list non-empty and every optional field set except
/// the salt.
fn basis() -> Basis {
    Basis {
        schema: BASIS_SCHEMA.to_owned(),
        worktree: PathBuf::from("/work/repo"),
        base: BasisBase {
            commit: Some("1".repeat(40)),
            tree: "2".repeat(40),
        },
        target: BasisTarget {
            kind: TargetKind::WorkingTree,
            commit: None,
            tree: "3".repeat(40),
        },
        scope: BasisScope {
            include_untracked: vec![PathBytes::new("notes.md")],
            paths: vec![PathBytes::new("src")],
        },
        records: vec![BasisRecord {
            id: RecordId::new("r1"),
            revision: Revision::ZERO.next(),
            logs: BTreeMap::from([(
                "observations/0.jsonl".to_owned(),
                PinnedLog {
                    length: 120,
                    sha256: "4".repeat(64),
                },
            )]),
            not_reconciled: Some("locked by another writer".to_owned()),
            associations: vec![PathBuf::from("/work/repo")],
        }],
        unreconciled_checkouts: vec![BasisCheckout {
            checkout: PathBuf::from("/work/other"),
            reason: "records directory locked by another writer".to_owned(),
        }],
        algorithms: Algorithms { derivation: 2 },
        explanations: vec![Explanation {
            path: PathBytes::new("src/a.rs"),
            digest: "5".repeat(64),
        }],
        capture: vec![CaptureEntry {
            transcript: "/home/u/.claude/projects/-work-repo/s1.jsonl".to_owned(),
            counts: CaptureCounts {
                malformed_lines: 1,
                ..CaptureCounts::default()
            },
        }],
        gate: vec![lines_item("src/a.rs")],
        salt: None,
    }
}

#[test]
fn a_basis_id_is_stable_across_serializations() {
    let original = basis();
    let id = original.basis_id().unwrap();
    assert_eq!(id.as_str().len(), 64);
    assert_eq!(id.short(), &id.as_str()[..8]);
    assert_eq!(id.to_string(), id.as_str());

    let compact: Basis = serde_json::from_slice(&serde_json::to_vec(&original).unwrap()).unwrap();
    let pretty: Basis =
        serde_json::from_str(&serde_json::to_string_pretty(&original).unwrap()).unwrap();
    assert_eq!(compact, original);
    assert_eq!(compact.basis_id().unwrap(), id);
    assert_eq!(pretty.basis_id().unwrap(), id);
}

#[test]
fn a_basis_id_changes_with_every_field() {
    let id = basis().basis_id().unwrap();
    let mutations: [fn(&mut Basis); 22] = [
        |b| b.schema = "supersigil.basis/2".to_owned(),
        |b| b.worktree.push("sub"),
        |b| b.base.commit = None,
        |b| b.base.tree = "6".repeat(40),
        |b| b.target.kind = TargetKind::Commit,
        |b| b.target.commit = Some("7".repeat(40)),
        |b| b.target.tree = "8".repeat(40),
        |b| b.scope.include_untracked.clear(),
        |b| b.scope.paths.push(PathBytes::new("tests")),
        |b| b.records[0].id = RecordId::new("r2"),
        |b| b.records[0].revision = Revision::ZERO,
        |b| {
            b.records[0]
                .logs
                .get_mut("observations/0.jsonl")
                .unwrap()
                .length += 1;
        },
        |b| b.records[0].not_reconciled = None,
        |b| b.records[0].associations.push(PathBuf::from("/work/other")),
        |b| b.unreconciled_checkouts[0].reason.push('!'),
        |b| b.algorithms.derivation = 1,
        |b| b.explanations[0].digest = "9".repeat(64),
        |b| b.capture[0].counts.malformed_lines = 2,
        |b| b.gate.push(lines_item("src/b.rs")),
        |b| {
            b.gate[0]
                .fingerprint
                .0
                .insert("content".to_owned(), known("other"));
        },
        |b| *b.gate[0].fingerprint.0.get_mut("content").unwrap() = FingerprintPart::Unknown,
        |b| b.salt = Some("0f".repeat(16)),
    ];
    for (index, mutate) in mutations.iter().enumerate() {
        let mut changed = basis();
        mutate(&mut changed);
        assert_ne!(changed.basis_id().unwrap(), id, "mutation {index}");
    }
}

#[test]
fn a_basis_lists_its_fields_in_declaration_order() {
    let value = serde_json::to_value(basis()).unwrap();
    let text = serde_json::to_string(&basis()).unwrap();
    let keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let mut positions: Vec<(usize, &str)> = keys
        .iter()
        .map(|key| (text.find(&format!("\"{key}\":")).unwrap(), *key))
        .collect();
    positions.sort_unstable();
    let order: Vec<&str> = positions.into_iter().map(|(_, key)| key).collect();
    assert_eq!(
        order,
        [
            "schema",
            "worktree",
            "base",
            "target",
            "scope",
            "records",
            "unreconciled_checkouts",
            "algorithms",
            "explanations",
            "capture",
            "gate"
        ]
    );
    assert_eq!(value["target"]["kind"], "working_tree");
    assert_eq!(value["schema"], "supersigil.basis/1");
    // The salt is written only when present.
    let mut salted = basis();
    salted.salt = Some("ab".repeat(16));
    let value = serde_json::to_value(&salted).unwrap();
    assert_eq!(value["salt"], "ab".repeat(16));
}

#[test]
fn path_bytes_round_trip_as_text_or_hex() {
    let text = PathBytes::new("src/main.rs");
    assert_eq!(text.as_bytes(), b"src/main.rs");
    assert_eq!(text.to_str(), Some("src/main.rs"));
    let json = serde_json::to_value(&text).unwrap();
    assert_eq!(json, json!("src/main.rs"));
    assert_eq!(serde_json::from_value::<PathBytes>(json).unwrap(), text);

    let raw = PathBytes::new(vec![b'a', 0xff, b'/', 0x80]);
    assert_eq!(raw.to_str(), None);
    let json = serde_json::to_value(&raw).unwrap();
    assert_eq!(json, json!({"hex": "61ff2f80"}));
    assert_eq!(serde_json::from_value::<PathBytes>(json).unwrap(), raw);

    serde_json::from_value::<PathBytes>(json!({"hex": "6"})).unwrap_err();
    serde_json::from_value::<PathBytes>(json!({"hex": "zz"})).unwrap_err();
    serde_json::from_value::<PathBytes>(json!(7)).unwrap_err();
}

#[test]
fn path_bytes_order_by_bytes_not_components() {
    // As a path, `a/b` sorts before `a-b` (component `a` before `a-b`); as
    // bytes, `-` (0x2d) sorts before `/` (0x2f).
    assert!(PathBytes::new("a-b") < PathBytes::new("a/b"));
    assert!(PathBytes::new("a/b") < PathBytes::new(vec![b'a', 0xff]));
}

#[test]
fn an_item_id_hashes_origin_nul_and_subject_bytes() {
    let path = Subject::Path(PathBytes::new("src/a.rs"));
    let id = ItemId::new(Origin::UnexplainedLines, &path);
    assert_eq!(
        id.as_str(),
        "f888be1d9907e18b81646100999e0d520505b00e8811b29bccfc29337cac6408"
    );
    assert_eq!(id.short(), "f888be1d");
    assert_eq!(ItemId::new(Origin::UnexplainedLines, &path), id);
    assert_eq!(
        ItemId::new(Origin::Unreconciled, &Subject::Record(RecordId::new("r1"))).as_str(),
        "d0f3a514c273eb7ef74756534428d74165a7ac1f16c98bfc55a27cdbf32eda62"
    );
    assert_eq!(
        ItemId::new(
            Origin::Unreconciled,
            &Subject::Checkout(PathBuf::from("/work/other"))
        )
        .as_str(),
        "177b757701990a0c516f4b1278af572543cfa72a6b48968b7f71b5316c3c1521"
    );
    // The item's own id agrees with its origin and subject.
    assert_eq!(lines_item("src/a.rs").id, id);
    assert_eq!(lines_item("src/a.rs").origin(), Origin::UnexplainedLines);
}

#[test]
fn item_ids_differ_per_origin_and_subject() {
    let path = Subject::Path(PathBytes::new("src/a.rs"));
    let origins = [
        Origin::UnexplainedLines,
        Origin::NotDiffed,
        Origin::UntrackedRecorded,
        Origin::NotCaptured,
        Origin::CommitWithoutOrigin,
        Origin::Discontinuity,
        Origin::Unreconciled,
    ];
    let mut ids: Vec<ItemId> = origins.iter().map(|o| ItemId::new(*o, &path)).collect();
    ids.push(ItemId::new(
        Origin::UnexplainedLines,
        &Subject::Path(PathBytes::new("src/b.rs")),
    ));
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), origins.len() + 1);
}

#[test]
fn origins_sort_in_gate_order_with_their_qualifiers() {
    let origins = [
        (Origin::UnexplainedLines, "unexplained_lines", "lines"),
        (Origin::NotDiffed, "not_diffed", "notdiffed"),
        (Origin::UntrackedRecorded, "untracked_recorded", "untracked"),
        (Origin::NotCaptured, "not_captured", "uncaptured"),
        (
            Origin::CommitWithoutOrigin,
            "commit_without_origin",
            "commit",
        ),
        (Origin::Discontinuity, "discontinuity", "gap"),
        (Origin::Unreconciled, "unreconciled", "unreconciled"),
    ];
    for pair in origins.windows(2) {
        assert!(pair[0].0 < pair[1].0);
    }
    for (origin, name, qualifier) in origins {
        assert_eq!(origin.as_str(), name);
        assert_eq!(origin.qualifier(), qualifier);
        assert_eq!(serde_json::to_value(origin).unwrap(), json!(name));
    }
}

#[test]
fn a_gate_item_serializes_its_detail_tagged_by_origin() {
    let item = lines_item("src/a.rs");
    let value = serde_json::to_value(&item).unwrap();
    assert_eq!(
        value,
        json!({
            "id": "f888be1d9907e18b81646100999e0d520505b00e8811b29bccfc29337cac6408",
            "subject": {"path": "src/a.rs"},
            "detail": {
                "origin": "unexplained_lines",
                "unattributed": 8,
                "content_match_incomplete": 0,
                "unresolved": 0,
                "ambiguous": 4,
                "line_ending": 0,
                "content_match_only": 0
            },
            "fingerprint": {
                "content": known("blobs"),
                "evidence": known("edits"),
                "outcomes": known("spans")
            }
        })
    );
    assert_eq!(
        value["fingerprint"]["content"].as_str().unwrap().len(),
        64,
        "{value}"
    );
}

#[test]
fn every_origin_has_its_own_detail_and_subject() {
    let details = [
        (
            Subject::Path(PathBytes::new("logo.png")),
            GateDetail::NotDiffed {
                kind: "binary".to_owned(),
            },
            json!({"origin": "not_diffed", "kind": "binary"}),
        ),
        (
            Subject::Path(PathBytes::new("notes.md")),
            GateDetail::UntrackedRecorded { edits: 2 },
            json!({"origin": "untracked_recorded", "edits": 2}),
        ),
        (
            Subject::Path(PathBytes::new(vec![0xff])),
            GateDetail::NotCaptured {
                cause: "skip_worktree".to_owned(),
                on_disk: "missing".to_owned(),
            },
            json!({"origin": "not_captured", "cause": "skip_worktree", "on_disk": "missing"}),
        ),
        (
            Subject::Commit("a".repeat(40)),
            GateDetail::CommitWithoutOrigin,
            json!({"origin": "commit_without_origin"}),
        ),
        (
            Subject::Path(PathBytes::new("src/a.rs")),
            GateDetail::Discontinuity { gaps: 1 },
            json!({"origin": "discontinuity", "gaps": 1}),
        ),
        (
            Subject::Record(RecordId::new("r1")),
            GateDetail::Unreconciled {
                reason: "locked by another writer".to_owned(),
            },
            json!({"origin": "unreconciled", "reason": "locked by another writer"}),
        ),
        (
            Subject::Checkout(PathBuf::from("/work/other")),
            GateDetail::Unreconciled {
                reason: "records directory locked by another writer".to_owned(),
            },
            json!({
                "origin": "unreconciled",
                "reason": "records directory locked by another writer"
            }),
        ),
    ];
    for (subject, detail, expected) in details {
        let origin = detail.origin();
        let item = GateItem::new(
            subject.clone(),
            detail,
            fingerprint(&[("evidence", FingerprintPart::Unknown)]),
        );
        assert_eq!(item.origin(), origin);
        assert_eq!(item.id, ItemId::new(origin, &subject));
        let value = serde_json::to_value(&item).unwrap();
        assert_eq!(value["detail"], expected);
        assert_eq!(value["fingerprint"], json!({"evidence": "unknown"}));
        assert_eq!(serde_json::from_value::<GateItem>(value).unwrap(), item);
    }
    let subjects = json!([
        Subject::Commit("a".repeat(40)),
        Subject::Record(RecordId::new("r1")),
        Subject::Checkout(PathBuf::from("/work/other")),
        Subject::Path(PathBytes::new(vec![0xff])),
    ]);
    assert_eq!(
        subjects,
        json!([
            {"commit": "a".repeat(40)},
            {"record": "r1"},
            {"checkout": "/work/other"},
            {"path": {"hex": "ff"}}
        ])
    );
}

#[test]
fn ids_must_be_64_lowercase_hex_digits() {
    let id = lines_item("src/a.rs").id;
    let json = serde_json::to_value(&id).unwrap();
    assert_eq!(serde_json::from_value::<ItemId>(json).unwrap(), id);
    for bad in [
        "f888be1d".to_owned(),
        id.as_str().to_uppercase(),
        format!("{}0", id.as_str()),
        format!("../{}", &id.as_str()[3..]),
    ] {
        serde_json::from_value::<ItemId>(json!(bad)).unwrap_err();
    }
}

#[test]
fn fingerprints_match_when_every_part_is_known_and_equal() {
    let a = fingerprint(&[("content", known("x")), ("kind", known("binary"))]);
    assert!(a.matches(&a.clone()));
    assert!(a.differing(&a.clone()).is_empty());
    assert!(!a.has_unknown());

    let content_changed = fingerprint(&[("content", known("y")), ("kind", known("binary"))]);
    assert!(!a.matches(&content_changed));
    assert_eq!(a.differing(&content_changed), ["content"]);

    // A part on one side only differs, whichever side has it.
    let fewer = fingerprint(&[("content", known("x"))]);
    assert!(!a.matches(&fewer));
    assert!(!fewer.matches(&a));
    assert_eq!(a.differing(&fewer), ["kind"]);
    assert_eq!(fewer.differing(&a), ["kind"]);
}

#[test]
fn unknown_matches_nothing_not_even_unknown() {
    let unknown = fingerprint(&[
        ("content", known("x")),
        ("evidence", FingerprintPart::Unknown),
    ]);
    assert!(unknown.has_unknown());
    assert!(!unknown.matches(&unknown.clone()));
    assert_eq!(unknown.differing(&unknown.clone()), ["evidence"]);
    let known_evidence = fingerprint(&[("content", known("x")), ("evidence", known("e"))]);
    assert!(!unknown.matches(&known_evidence));
    assert!(!known_evidence.matches(&unknown));
    assert_eq!(known_evidence.differing(&unknown), ["evidence"]);
}

#[test]
fn a_known_part_is_the_hex_sha256_of_its_material() {
    assert_eq!(
        FingerprintPart::of(b"abc"),
        FingerprintPart::Known(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned()
        )
    );
    assert_eq!(
        serde_json::to_value(FingerprintPart::Unknown).unwrap(),
        json!("unknown")
    );
    assert_eq!(
        serde_json::from_value::<FingerprintPart>(json!("unknown")).unwrap(),
        FingerprintPart::Unknown
    );
}

/// A judgment of `kind` on the example basis with nothing else set.
fn judgment(kind: JudgmentKind) -> Judgment {
    Judgment {
        id: JudgmentId::new("4f1c9a2e-0000-4000-8000-000000000001"),
        basis: basis().basis_id().unwrap(),
        kind,
        items: Vec::new(),
        reason: None,
        text: None,
        closes: BTreeMap::new(),
        withdraws: Vec::new(),
        who: Who {
            name: "Ada Lovelace".to_owned(),
            email: "ada@example.com".to_owned(),
        },
        at: UtcTime::from_unix(1_791_894_896),
    }
}

#[test]
fn a_judgment_line_omits_what_its_kind_leaves_empty() {
    let comment = judgment(JudgmentKind::Comment);
    let value = serde_json::to_value(&comment).unwrap();
    assert_eq!(
        value,
        json!({
            "id": "4f1c9a2e-0000-4000-8000-000000000001",
            "basis": basis().basis_id().unwrap(),
            "kind": "comment",
            "who": {"name": "Ada Lovelace", "email": "ada@example.com"},
            "at": "2026-10-13T12:34:56Z"
        })
    );
}

#[test]
fn a_judgment_line_round_trips_with_each_kind() {
    let item = lines_item("src/a.rs").id;
    let earlier = JudgmentId::new("77d0e5b1-0000-4000-8000-000000000002");

    let mut acknowledge = judgment(JudgmentKind::Acknowledge);
    acknowledge.items = vec![item.clone()];
    let mut dismiss = judgment(JudgmentKind::Dismiss);
    dismiss.items = vec![item.clone()];
    dismiss.reason = Some("generated; regenerated by the build".to_owned());
    let mut comment = judgment(JudgmentKind::Comment);
    comment.items = vec![item.clone()];
    comment.text = Some("check the parser's error path".to_owned());
    let mut complete = judgment(JudgmentKind::Complete);
    complete.text = Some("done".to_owned());
    complete.closes = BTreeMap::from([(item.clone(), earlier.clone())]);
    let mut undo = judgment(JudgmentKind::Undo);
    undo.items = vec![item.clone()];
    undo.withdraws = vec![
        Withdrawal {
            judgment: earlier.clone(),
            item: Some(item.clone()),
        },
        Withdrawal {
            judgment: JudgmentId::new("9a8b7c6d-0000-4000-8000-000000000003"),
            item: None,
        },
    ];

    let expected = [
        ("acknowledge", json!({"items": [item]})),
        (
            "dismiss",
            json!({"items": [item], "reason": "generated; regenerated by the build"}),
        ),
        (
            "comment",
            json!({"items": [item], "text": "check the parser's error path"}),
        ),
        (
            "complete",
            json!({"text": "done", "closes": {item.as_str(): earlier}}),
        ),
        (
            "undo",
            json!({
                "items": [item],
                "withdraws": [
                    {"judgment": earlier, "item": item},
                    {"judgment": "9a8b7c6d-0000-4000-8000-000000000003"}
                ]
            }),
        ),
    ];
    for (judgment, (kind, fields)) in [acknowledge, dismiss, comment, complete, undo]
        .into_iter()
        .zip(expected)
    {
        let line = serde_json::to_string(&judgment).unwrap();
        assert!(!line.contains('\n'), "{line}");
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["kind"], kind);
        for (field, expected) in fields.as_object().unwrap() {
            assert_eq!(&value[field], expected, "{kind} {field}");
        }
        let common = ["id", "basis", "kind", "who", "at"];
        assert_eq!(
            value.as_object().unwrap().len(),
            common.len() + fields.as_object().unwrap().len(),
            "{line}"
        );
        assert_eq!(serde_json::from_str::<Judgment>(&line).unwrap(), judgment);
    }
}

#[test]
fn judgment_ids_are_fresh_uuids_printed_by_8_characters() {
    let a = JudgmentId::generate();
    let b = JudgmentId::generate();
    assert_ne!(a, b);
    assert_eq!(a.as_str().len(), 36);
    assert_eq!(a.short(), &a.as_str()[..8]);
    assert_eq!(JudgmentId::new("abc").short(), "abc");
}
