//! Tests snapshot isolation, atomic publication, writer locking, crash
//! recovery, and the removal of replaced documents.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use supersigil_record::derivations::{ALGORITHM_VERSION, DerivationSet};
use supersigil_record::observations::{EndReason, Observation, SessionEnd};
use supersigil_record::reviews::PinnedLog;
use supersigil_record::store::{
    Association, RecordSnapshot, SessionLog, SourceCursor, Store, StoreError,
};
use supersigil_record::time::{GRACE_SECONDS, UtcTime};
use supersigil_record::{ContentId, RecordId, Revision, SessionId, Timestamp};

fn assoc() -> Association {
    Association {
        checkout: PathBuf::from("/work/repo"),
    }
}

fn session() -> SessionId {
    SessionId::new("s1")
}

/// Creates a session-end event with `second` as the seconds field of its timestamp.
fn observation(session: &str, second: u32) -> Observation {
    Observation::SessionEnd(SessionEnd {
        session: SessionId::new(session),
        time: Timestamp::new(format!("2026-09-28T10:00:{second:02}.000Z")),
        reason: EndReason::Unknown,
    })
}

/// Creates an empty derivation set for `session` at `revision`.
fn derivation_set(session: &str, revision: Revision) -> DerivationSet {
    DerivationSet {
        session: SessionId::new(session),
        observation_revision: revision,
        algorithm_version: ALGORITHM_VERSION,
        restores: Vec::new(),
        discontinuities: Vec::new(),
    }
}

fn dir_names(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Lists all files under `dir`, sorted by relative path with `/` separators.
fn files_under(dir: &std::path::Path) -> Vec<String> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type().unwrap().is_dir() {
            files.extend(
                files_under(&entry.path())
                    .into_iter()
                    .map(|file| format!("{name}/{file}")),
            );
        } else {
            files.push(name);
        }
    }
    files.sort();
    files
}

#[test]
fn session_ids_never_reach_file_names() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("record");
    let store = Store::create(&root, assoc()).unwrap();
    let long = "セッション".repeat(60);
    let ids = ["../escape", "CON", "Abc", "abc", "a/b", long.as_str()];
    let observations: Vec<Observation> = ids
        .iter()
        .zip(0..)
        .map(|(id, second)| observation(id, second))
        .collect();
    let mut tx = store.begin().unwrap();
    tx.append_observations(&observations).unwrap();
    for id in ids {
        tx.put_derivations(&derivation_set(id, Revision::ZERO))
            .unwrap();
    }
    tx.commit().unwrap();

    let snapshot = store.snapshot().unwrap();
    let mut expected: Vec<SessionId> = ids.iter().map(|id| SessionId::new(*id)).collect();
    expected.sort();
    assert_eq!(snapshot.sessions(), expected);
    for (id, observation) in ids.iter().zip(&observations) {
        let session = SessionId::new(*id);
        assert_eq!(
            snapshot.observations(&session).unwrap(),
            vec![observation.clone()],
            "{id}"
        );
        assert_eq!(
            snapshot.derivations(&session).unwrap(),
            Some(derivation_set(id, Revision::ZERO)),
            "{id}"
        );
    }
    // Nothing was written beside the record, and inside it every name is
    // the store's own: fixed names, or session numbers and the revision.
    assert_eq!(dir_names(dir.path()), vec!["record".to_owned()]);
    let mut files = vec!["manifest.json".to_owned(), "write.lock".to_owned()];
    for number in 0..ids.len() {
        files.push(format!("derivations/{number}.r1.json"));
        files.push(format!("observations/{number}.jsonl"));
    }
    files.sort();
    assert_eq!(files_under(&root), files);
}

#[test]
fn create_then_open_keeps_identity_at_revision_zero() {
    let dir = tempfile::tempdir().unwrap();
    let created = Store::create(dir.path(), assoc()).unwrap();
    let id = created.manifest().unwrap().record_id;
    let opened = Store::open(dir.path()).unwrap();
    let manifest = opened.manifest().unwrap();
    assert_eq!(manifest.record_id, id);
    assert_eq!(manifest.revision, Revision::ZERO);
    assert_eq!(manifest.associations, vec![assoc()]);
    assert!(matches!(
        Store::create(dir.path(), assoc()),
        Err(StoreError::AlreadyExists(_))
    ));
    assert!(matches!(
        Store::open(&dir.path().join("nope")),
        Err(StoreError::NotARecord(_))
    ));
}

#[test]
fn commit_publishes_new_revision_and_pins_log_length() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let observations = [observation("s1", 1), observation("s1", 2)];

    let mut tx = store.begin().unwrap();
    tx.append_observations(&observations).unwrap();
    let rev = tx.commit().unwrap();
    assert_eq!(rev, Revision::ZERO.next());

    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.revision(), rev);
    let written = fs::read(dir.path().join("observations/0.jsonl")).unwrap();
    assert_eq!(snapshot.manifest().sessions[&session()], 0);
    assert_eq!(
        snapshot.manifest().logs["observations/0.jsonl"],
        written.len() as u64
    );
    assert_eq!(snapshot.observations(&session()).unwrap(), observations);
    assert_eq!(snapshot.sessions(), vec![session()]);
}

#[test]
fn reader_stays_pinned_to_its_manifest_while_writer_appends() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let observations = [
        observation("s1", 1),
        observation("s1", 2),
        observation("s1", 3),
    ];
    let mut tx = store.begin().unwrap();
    tx.append_observations(&observations[..2]).unwrap();
    tx.commit().unwrap();

    let pinned = store.snapshot().unwrap();
    let mut tx = store.begin().unwrap();
    tx.append_observations(&observations[2..]).unwrap();
    // Not yet committed: the pinned reader sees two, a fresh reader too.
    assert_eq!(pinned.observations(&session()).unwrap().len(), 2);
    let fresh = store.snapshot().unwrap();
    assert_eq!(fresh.observations(&session()).unwrap().len(), 2);
    tx.commit().unwrap();
    // Committed: the pinned reader still sees two, a fresh reader sees three.
    assert_eq!(pinned.observations(&session()).unwrap().len(), 2);
    let fresh = store.snapshot().unwrap();
    assert_eq!(fresh.observations(&session()).unwrap().len(), 3);
}

#[test]
fn a_session_log_is_read_with_its_pinned_length_and_hash() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let mut tx = store.begin().unwrap();
    tx.append_observations(&[observation("s1", 1), observation("s2", 2)])
        .unwrap();
    tx.commit().unwrap();
    let pinned = store.snapshot().unwrap();
    let mut tx = store.begin().unwrap();
    tx.append_observations(&[observation("s1", 3)]).unwrap();
    tx.commit().unwrap();

    // The first log grew after the pin; the pinned snapshot still reads and
    // hashes only the bytes it pinned.
    for (session, log) in [
        ("s1", "observations/0.jsonl"),
        ("s2", "observations/1.jsonl"),
    ] {
        let session = SessionId::new(session);
        let length = pinned.manifest().logs[log];
        let bytes = fs::read(dir.path().join(log)).unwrap();
        let sha256 = hex::encode(Sha256::digest(&bytes[..usize::try_from(length).unwrap()]));
        assert_eq!(
            pinned.session_log(&session).unwrap(),
            Some(SessionLog {
                path: log.to_owned(),
                pinned: PinnedLog { length, sha256 },
                observations: pinned.observations(&session).unwrap(),
            })
        );
    }
    let read = |snapshot: &RecordSnapshot, session: &str| {
        snapshot
            .session_log(&SessionId::new(session))
            .unwrap()
            .unwrap()
    };
    let grown = store.snapshot().unwrap();
    assert!(read(&grown, "s1").pinned.length > read(&pinned, "s1").pinned.length);
    assert_ne!(
        read(&grown, "s1").pinned.sha256,
        read(&pinned, "s1").pinned.sha256
    );
    assert_eq!(read(&grown, "s1").observations.len(), 2);
    assert_eq!(read(&grown, "s2"), read(&pinned, "s2"));
    assert_eq!(pinned.session_log(&SessionId::new("s3")).unwrap(), None);
}

/// The log of session `s1`, the only one in its record.
const LOG: &str = "observations/0.jsonl";

/// A record whose session `s1` logged one observation, then two, then one
/// more: a snapshot pinning the first, one pinning the first three, and the
/// record's directory. The fourth was committed after both.
fn grown_log() -> (tempfile::TempDir, RecordSnapshot, RecordSnapshot) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let append = |seconds: &[u32]| {
        let observations: Vec<Observation> =
            seconds.iter().map(|&s| observation("s1", s)).collect();
        let mut tx = store.begin().unwrap();
        tx.append_observations(&observations).unwrap();
        tx.commit().unwrap();
        store.snapshot().unwrap()
    };
    let earlier = append(&[1]);
    let pinned = append(&[2, 3]);
    append(&[4]);
    (dir, earlier, pinned)
}

/// The hex SHA-256 of the log `snapshot` pins for session `s1`.
fn pinned_digest(snapshot: &RecordSnapshot) -> String {
    snapshot
        .session_log(&session())
        .unwrap()
        .unwrap()
        .pinned
        .sha256
}

#[test]
fn a_log_is_read_once_for_every_length_earlier_revisions_pinned() {
    let (_dir, earlier, pinned) = grown_log();
    let (start, end) = (earlier.manifest().logs[LOG], pinned.manifest().logs[LOG]);

    // Read from the start, after the first observation, after all three,
    // and beyond what the snapshot pins.
    let lengths = BTreeSet::from([0, start, end, end + 1]);
    let reading = pinned.read_log(LOG, &lengths).unwrap().unwrap();

    let nothing = hex::encode(Sha256::digest(b""));
    assert_eq!(reading.digest(0), Some(nothing.as_str()));
    assert_eq!(
        reading.digest(start),
        Some(pinned_digest(&earlier).as_str())
    );
    assert_eq!(reading.digest(end), Some(pinned_digest(&pinned).as_str()));
    assert_eq!(reading.digest(end + 1), None);
    assert_eq!(
        reading.since(0),
        Some(pinned.observations(&session()).unwrap().as_slice())
    );
    assert_eq!(
        reading.since(start),
        Some([observation("s1", 2), observation("s1", 3)].as_slice())
    );
    assert_eq!(reading.since(end), Some([].as_slice()));
    assert_eq!(reading.since(end + 1), None);
    // A log the snapshot does not pin.
    assert_eq!(
        pinned
            .read_log("observations/1.jsonl", &BTreeSet::from([0]))
            .unwrap(),
        None
    );
}

#[test]
fn a_log_is_parsed_from_the_first_length_where_an_observation_begins() {
    let (dir, earlier, pinned) = grown_log();
    let start = earlier.manifest().logs[LOG];
    let first = pinned_digest(&earlier);
    // The first observation rewritten at the same length, so it no longer
    // parses; no observation begins 3 bytes in.
    let path = dir.path().join(LOG);
    let mut bytes = fs::read(&path).unwrap();
    bytes[0] = b' ';
    fs::write(&path, &bytes).unwrap();

    let reading = pinned
        .read_log(LOG, &BTreeSet::from([3, start]))
        .unwrap()
        .unwrap();

    let three = hex::encode(Sha256::digest(&bytes[..3]));
    assert_eq!(reading.digest(3), Some(three.as_str()));
    assert_ne!(reading.digest(start), Some(first.as_str()));
    assert_eq!(reading.since(3), None);
    assert_eq!(
        reading.since(start),
        Some([observation("s1", 2), observation("s1", 3)].as_slice())
    );
}

#[test]
fn tail_beyond_pinned_length_is_ignored_and_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let observations = [observation("s1", 1), observation("s1", 2)];
    let mut tx = store.begin().unwrap();
    tx.append_observations(&observations[..1]).unwrap();
    tx.commit().unwrap();

    // Simulate a crash after appending but before publishing the manifest.
    let path = dir.path().join("observations/0.jsonl");
    let mut expected = fs::read(&path).unwrap();
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(b"{\"partial").unwrap();
    drop(file);
    assert_eq!(
        store.snapshot().unwrap().observations(&session()).unwrap(),
        observations[..1]
    );

    let mut tx = store.begin().unwrap();
    tx.append_observations(&observations[1..]).unwrap();
    tx.commit().unwrap();
    expected.extend(serde_json::to_vec(&observations[1]).unwrap());
    expected.push(b'\n');
    assert_eq!(fs::read(&path).unwrap(), expected);
    assert_eq!(
        store.snapshot().unwrap().observations(&session()).unwrap(),
        observations
    );
}

#[test]
fn second_writer_is_blocked_while_lock_is_held() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let first = store.begin().unwrap();
    assert!(matches!(store.begin(), Err(StoreError::Locked(_))));
    drop(first);
    store.begin().unwrap();
}

#[test]
fn commit_with_stale_expected_revision_fails() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let mut tx = store.begin().unwrap();
    tx.append_observations(&[observation("s1", 1)]).unwrap();

    // Someone broke the lock and published revision 1 behind our back.
    let manifest_path = dir.path().join("manifest.json");
    let text = fs::read_to_string(&manifest_path).unwrap();
    fs::write(
        &manifest_path,
        text.replace("\"revision\": 0", "\"revision\": 1"),
    )
    .unwrap();

    assert!(matches!(
        tx.commit(),
        Err(StoreError::Conflict {
            expected: 0,
            found: 1
        })
    ));
    assert_eq!(store.manifest().unwrap().revision.get(), 1);
}

#[test]
fn documents_are_immutable_per_revision() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let first_set = derivation_set("s1", Revision::ZERO);
    let mut tx = store.begin().unwrap();
    tx.put_derivations(&first_set).unwrap();
    tx.commit().unwrap();
    let first = store.snapshot().unwrap();

    let second_set = derivation_set("s1", Revision::ZERO.next());
    let mut tx = store.begin().unwrap();
    tx.put_derivations(&second_set).unwrap();
    tx.commit().unwrap();
    let second = store.snapshot().unwrap();

    assert_eq!(first.derivations(&session()).unwrap(), Some(first_set));
    assert_eq!(second.derivations(&session()).unwrap(), Some(second_set));
    assert_eq!(
        first.manifest().documents["derivations/0"],
        "derivations/0.r1.json"
    );
    assert_eq!(
        second.manifest().documents["derivations/0"],
        "derivations/0.r2.json"
    );
    assert!(dir.path().join("derivations/0.r1.json").exists());
    assert_eq!(first.derivations(&SessionId::new("missing")).unwrap(), None);
}

/// 2026-10-13T00:00:00Z plus `seconds`.
fn at(seconds: i64) -> UtcTime {
    UtcTime::from_unix(1_791_849_600 + seconds)
}

/// Writes session `s1`'s derivations in a transaction timed `now` and
/// commits it.
fn put_derivations_at(store: &Store, now: UtcTime) {
    let mut tx = store.begin_at(now).unwrap();
    tx.put_derivations(&derivation_set("s1", Revision::ZERO))
        .unwrap();
    tx.commit().unwrap();
}

/// Commits a transaction timed `now` that changes nothing.
fn commit_at(store: &Store, now: UtcTime) {
    store.begin_at(now).unwrap().commit().unwrap();
}

fn read_manifest_json(dir: &std::path::Path) -> Value {
    serde_json::from_slice(&fs::read(dir.join("manifest.json")).unwrap()).unwrap()
}

#[test]
fn a_replaced_document_is_listed_with_the_time_of_the_commit_that_replaced_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    put_derivations_at(&store, at(0));
    let first = store.snapshot().unwrap();
    assert!(first.manifest().superseded.is_empty());

    // Writing the session twice in one transaction replaces the committed
    // document once; the pending one is not listed.
    let mut tx = store.begin_at(at(60)).unwrap();
    for revision in [Revision::ZERO, Revision::ZERO.next()] {
        tx.put_derivations(&derivation_set("s1", revision)).unwrap();
    }
    tx.commit().unwrap();
    let manifest = store.manifest().unwrap();
    assert_eq!(manifest.documents["derivations/0"], "derivations/0.r2.json");
    assert_eq!(
        manifest.superseded,
        BTreeMap::from([("derivations/0.r1.json".to_owned(), at(60))])
    );
    assert_eq!(
        read_manifest_json(dir.path())["superseded"],
        json!({"derivations/0.r1.json": "2026-10-13T00:01:00Z"})
    );
    // Readers of the earlier revision still find its document.
    assert_eq!(
        first.derivations(&session()).unwrap(),
        Some(derivation_set("s1", Revision::ZERO))
    );
}

#[test]
fn a_replaced_document_is_deleted_by_the_first_commit_more_than_14_days_later() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    put_derivations_at(&store, at(0));
    put_derivations_at(&store, at(60));
    let replaced = dir.path().join("derivations/0.r1.json");

    // Exactly 14 days after the replacement: kept.
    commit_at(&store, at(60 + GRACE_SECONDS));
    assert!(replaced.exists());
    assert_eq!(
        store.manifest().unwrap().superseded,
        BTreeMap::from([("derivations/0.r1.json".to_owned(), at(60))])
    );

    // A second later: deleted, while the current document stays.
    commit_at(&store, at(61 + GRACE_SECONDS));
    assert!(!replaced.exists());
    assert!(store.manifest().unwrap().superseded.is_empty());
    assert_eq!(
        files_under(&dir.path().join("derivations")),
        vec!["0.r2.json".to_owned()]
    );
    assert_eq!(
        store.snapshot().unwrap().derivations(&session()).unwrap(),
        Some(derivation_set("s1", Revision::ZERO))
    );
}

#[test]
fn a_current_document_is_kept_however_long_ago_it_was_written() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    put_derivations_at(&store, at(0));

    // Nothing replaced it, so its age does not matter.
    commit_at(&store, at(10 * GRACE_SECONDS));
    assert_eq!(
        files_under(&dir.path().join("derivations")),
        vec!["0.r1.json".to_owned()]
    );
    assert_eq!(
        store.snapshot().unwrap().derivations(&session()).unwrap(),
        Some(derivation_set("s1", Revision::ZERO))
    );
}

#[test]
fn a_replaced_document_that_cannot_be_deleted_is_tried_again_by_the_next_commit() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    put_derivations_at(&store, at(0));
    put_derivations_at(&store, at(60));
    // A non-empty directory in its place cannot be removed as a file, as a
    // file another process has open cannot be on Windows.
    let replaced = dir.path().join("derivations/0.r1.json");
    fs::remove_file(&replaced).unwrap();
    fs::create_dir(&replaced).unwrap();
    fs::write(replaced.join("held"), b"").unwrap();

    commit_at(&store, at(61 + GRACE_SECONDS));
    assert!(replaced.is_dir());
    assert_eq!(
        store.manifest().unwrap().superseded,
        BTreeMap::from([("derivations/0.r1.json".to_owned(), at(60))])
    );

    // Once nothing is in its place, the next commit drops the entry.
    fs::remove_dir_all(&replaced).unwrap();
    commit_at(&store, at(62 + GRACE_SECONDS));
    assert!(store.manifest().unwrap().superseded.is_empty());
}

#[test]
fn a_manifest_without_superseded_documents_omits_the_field_and_reads() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    put_derivations_at(&store, at(0));
    let json = read_manifest_json(dir.path());
    assert_eq!(json["documents"]["derivations/0"], "derivations/0.r1.json");
    assert!(json.get("superseded").is_none(), "{json}");
    assert!(store.manifest().unwrap().superseded.is_empty());
}

/// Lists each of `files` as superseded at `replaced` in the manifest of the
/// record at `root`, as a malformed manifest might.
fn list_as_superseded(root: &std::path::Path, files: &[&str], replaced: UtcTime) {
    let mut json = read_manifest_json(root);
    for file in files {
        json["superseded"][*file] = serde_json::to_value(replaced).unwrap();
    }
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&json).unwrap(),
    )
    .unwrap();
}

#[test]
fn a_superseded_entry_naming_a_file_the_manifest_references_is_dropped_never_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let mut tx = store.begin_at(at(0)).unwrap();
    tx.append_observations(&[observation("s1", 1)]).unwrap();
    tx.put_derivations(&derivation_set("s1", Revision::ZERO))
        .unwrap();
    tx.commit().unwrap();
    // The live log, the current document (also as a file system that
    // ignores case names it), the manifest, and the lock, listed as
    // replaced long ago.
    list_as_superseded(
        dir.path(),
        &[
            "observations/0.jsonl",
            "derivations/0.r1.json",
            "derivations/0.R1.JSON",
            "manifest.json",
            "write.lock",
        ],
        at(-2 * GRACE_SECONDS),
    );

    // The next sync appends to the log.
    let mut tx = store.begin_at(at(60)).unwrap();
    tx.append_observations(&[observation("s1", 2)]).unwrap();
    tx.commit().unwrap();

    assert_eq!(
        files_under(dir.path()),
        vec![
            "derivations/0.r1.json".to_owned(),
            "manifest.json".to_owned(),
            "observations/0.jsonl".to_owned(),
            "write.lock".to_owned(),
        ]
    );
    let snapshot = store.snapshot().unwrap();
    assert!(snapshot.manifest().superseded.is_empty());
    assert_eq!(
        snapshot.observations(&session()).unwrap(),
        vec![observation("s1", 1), observation("s1", 2)]
    );
    assert_eq!(
        snapshot.derivations(&session()).unwrap(),
        Some(derivation_set("s1", Revision::ZERO))
    );
}

#[test]
fn a_superseded_entry_outside_the_derivations_directory_is_dropped_never_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("record");
    let store = Store::create(&root, assoc()).unwrap();
    put_derivations_at(&store, at(0));
    let outside = dir.path().join("outside.txt");
    fs::write(&outside, b"kept").unwrap();
    fs::write(root.join("notes.json"), b"kept").unwrap();
    fs::create_dir(root.join("derivations/nested")).unwrap();
    fs::write(root.join("derivations/nested/0.r1.json"), b"kept").unwrap();
    list_as_superseded(
        &root,
        &[
            outside.to_str().unwrap(),
            "../outside.txt",
            "derivations/../../outside.txt",
            "derivations/nested/0.r1.json",
        ],
        at(-2 * GRACE_SECONDS),
    );
    // Dropped however recently it was listed.
    list_as_superseded(&root, &["notes.json"], at(0));

    commit_at(&store, at(60));
    assert_eq!(fs::read(&outside).unwrap(), b"kept");
    assert_eq!(
        files_under(&root),
        vec![
            "derivations/0.r1.json".to_owned(),
            "derivations/nested/0.r1.json".to_owned(),
            "manifest.json".to_owned(),
            "notes.json".to_owned(),
            "write.lock".to_owned(),
        ]
    );
    assert!(store.manifest().unwrap().superseded.is_empty());
}

/// Makes the manifest of the record at `root` reference session `s1`'s
/// current document as `file`, as a hand-edited manifest might.
fn reference_as_current(root: &std::path::Path, file: &str) {
    let mut json = read_manifest_json(root);
    json["documents"]["derivations/0"] = json!(file);
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&json).unwrap(),
    )
    .unwrap();
}

#[test]
fn a_document_the_manifest_references_under_another_spelling_is_never_deleted() {
    for spelling in [
        "./derivations/0.r1.json",
        "derivations/./0.r1.json",
        "derivations//0.r1.json",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(dir.path(), assoc()).unwrap();
        put_derivations_at(&store, at(0));
        // The current document, referenced under a spelling that reads it
        // all the same, and listed as replaced long ago under its own.
        reference_as_current(dir.path(), spelling);
        list_as_superseded(
            dir.path(),
            &["derivations/0.r1.json"],
            at(-2 * GRACE_SECONDS),
        );

        // The next sync writes another session's derivations.
        let mut tx = store.begin_at(at(60)).unwrap();
        tx.put_derivations(&derivation_set("s2", Revision::ZERO))
            .unwrap();
        tx.commit().unwrap();

        assert!(
            dir.path().join("derivations/0.r1.json").exists(),
            "{spelling}"
        );
        assert_eq!(
            store.snapshot().unwrap().derivations(&session()).unwrap(),
            Some(derivation_set("s1", Revision::ZERO)),
            "{spelling}"
        );
    }
}

#[test]
fn a_document_the_published_manifest_references_survives_its_own_replacement() {
    for spelling in [
        "./derivations/0.r1.json",
        "derivations/./0.r1.json",
        "derivations//0.r1.json",
        "derivations/../derivations/0.r1.json",
        "Derivations/0.R1.json",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(dir.path(), assoc()).unwrap();
        put_derivations_at(&store, at(0));
        // The current document, referenced under another spelling, and
        // listed as replaced long ago under its own. A file system that
        // tells case apart reads nothing at `Derivations/0.R1.json`.
        reference_as_current(dir.path(), spelling);
        list_as_superseded(
            dir.path(),
            &["derivations/0.r1.json"],
            at(-2 * GRACE_SECONDS),
        );
        let readable = dir.path().join(spelling).is_file();
        let pinned = store.snapshot().unwrap();

        // The same session's derivations are written again: the document is
        // replaced in this commit, so a reader of the published manifest
        // still needs it, and its replacement starts a full grace period,
        // which the next commit keeps.
        let replaced = at(60);
        put_derivations_at(&store, replaced);
        commit_at(&store, at(61));

        assert!(
            dir.path().join("derivations/0.r1.json").exists(),
            "{spelling}"
        );
        if readable {
            assert_eq!(
                pinned.derivations(&session()).unwrap(),
                Some(derivation_set("s1", Revision::ZERO)),
                "{spelling}"
            );
        }
        assert_eq!(
            store.manifest().unwrap().superseded,
            BTreeMap::from([("derivations/0.r1.json".to_owned(), replaced)]),
            "{spelling}"
        );
    }
}

/// Replaces `s1`'s current document, referenced as `reference` spells it
/// for the record at its root, at T, then writes again at T + 1 s, and
/// checks that the document keeps its grace period: it outlives both writes
/// for a reader pinned before them.
fn replaced_through(reference: impl Fn(&std::path::Path) -> String) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    put_derivations_at(&store, at(0));
    let spelling = reference(dir.path());
    reference_as_current(dir.path(), &spelling);
    list_as_superseded(
        dir.path(),
        &["derivations/0.r1.json"],
        at(-2 * GRACE_SECONDS),
    );
    let pinned = store.snapshot().unwrap();

    put_derivations_at(&store, at(60));
    commit_at(&store, at(61));

    assert!(
        dir.path().join("derivations/0.r1.json").exists(),
        "{spelling}"
    );
    assert_eq!(
        pinned.derivations(&session()).unwrap(),
        Some(derivation_set("s1", Revision::ZERO)),
        "{spelling}"
    );
    assert_eq!(
        store
            .manifest()
            .unwrap()
            .superseded
            .get("derivations/0.r1.json"),
        Some(&at(60)),
        "{spelling}"
    );
}

#[test]
fn a_document_replaced_through_an_absolute_reference_keeps_its_grace_period() {
    replaced_through(|root| {
        root.join("derivations/0.r1.json")
            .to_str()
            .unwrap()
            .to_owned()
    });
}

#[cfg(unix)]
#[test]
fn a_document_replaced_through_a_linked_reference_keeps_its_grace_period() {
    replaced_through(|root| {
        std::os::unix::fs::symlink("0.r1.json", root.join("derivations/current")).unwrap();
        "derivations/current".to_owned()
    });
}

#[test]
fn a_reference_that_is_not_a_plain_path_in_the_record_keeps_every_replaced_document() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    put_derivations_at(&store, at(0));
    put_derivations_at(&store, at(60));
    // The replaced document referenced again through `..`, which reads it
    // all the same: what such a path names is not compared.
    reference_as_current(dir.path(), "derivations/../derivations/0.r1.json");

    commit_at(&store, at(61 + GRACE_SECONDS));
    assert!(dir.path().join("derivations/0.r1.json").exists());
    assert_eq!(
        store.manifest().unwrap().superseded,
        BTreeMap::from([("derivations/0.r1.json".to_owned(), at(60))])
    );
    assert_eq!(
        store.snapshot().unwrap().derivations(&session()).unwrap(),
        Some(derivation_set("s1", Revision::ZERO))
    );
}

#[cfg(unix)]
#[test]
fn nothing_is_deleted_while_the_manifest_references_a_document_through_a_symlink() {
    // The current document, listed as replaced, referenced through a link
    // to it, through a link to its directory, and through a link to a
    // directory holding a link to it.
    let replaced = "derivations/0.r1.json";
    for (links, reference) in [
        (&[("current", "0.r1.json")][..], "derivations/current"),
        (&[("here", ".")], "derivations/here/0.r1.json"),
        (
            &[("live/current", "../0.r1.json"), ("route", "live")],
            "derivations/route/current",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(dir.path(), assoc()).unwrap();
        put_derivations_at(&store, at(0));
        for (link, target) in links {
            let link = dir.path().join("derivations").join(link);
            fs::create_dir_all(link.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(target, link).unwrap();
        }
        reference_as_current(dir.path(), reference);
        list_as_superseded(dir.path(), &[replaced], at(-2 * GRACE_SECONDS));

        // The next sync appends another session's observations.
        let mut tx = store.begin_at(at(60)).unwrap();
        tx.append_observations(&[observation("s2", 1)]).unwrap();
        tx.commit().unwrap();

        assert!(
            dir.path().join(replaced).symlink_metadata().is_ok(),
            "{reference}"
        );
        assert_eq!(
            store.manifest().unwrap().superseded,
            BTreeMap::from([(replaced.to_owned(), at(-2 * GRACE_SECONDS))]),
            "{reference}"
        );
        assert_eq!(
            store.snapshot().unwrap().derivations(&session()).unwrap(),
            Some(derivation_set("s1", Revision::ZERO)),
            "{reference}"
        );
    }
}

#[cfg(unix)]
#[test]
fn nothing_is_deleted_through_a_derivations_directory_that_is_a_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("record");
    let store = Store::create(&root, assoc()).unwrap();
    put_derivations_at(&store, at(0));
    put_derivations_at(&store, at(60));
    // The directory moved out of the record, with a link to it in its
    // place, and no document the manifest references inside it.
    let elsewhere = dir.path().join("elsewhere");
    fs::rename(root.join("derivations"), &elsewhere).unwrap();
    std::os::unix::fs::symlink("../elsewhere", root.join("derivations")).unwrap();
    let mut json = read_manifest_json(&root);
    json["documents"] = json!({});
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&json).unwrap(),
    )
    .unwrap();

    commit_at(&store, at(61 + GRACE_SECONDS));
    assert!(elsewhere.join("0.r1.json").exists());
    assert_eq!(
        store.manifest().unwrap().superseded,
        BTreeMap::from([("derivations/0.r1.json".to_owned(), at(60))])
    );
}

#[test]
fn a_reference_that_cannot_be_resolved_on_disk_keeps_every_replaced_document() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    put_derivations_at(&store, at(0));
    put_derivations_at(&store, at(60));
    // The current document referenced as a file that is not there. A
    // reference that cannot be resolved might lead to a replaced document
    // in a way that cannot be seen (through a directory that cannot be
    // searched, say), so none is deleted.
    reference_as_current(dir.path(), "derivations/0.r9.json");

    commit_at(&store, at(61 + GRACE_SECONDS));
    assert!(dir.path().join("derivations/0.r1.json").exists());
    assert_eq!(
        store.manifest().unwrap().superseded,
        BTreeMap::from([("derivations/0.r1.json".to_owned(), at(60))])
    );
}

#[cfg(unix)]
#[test]
fn a_replaced_document_that_is_a_symlink_is_dropped_never_deleted() {
    // A link in its place to itself, where it leads cannot be told, and to
    // a file outside the record.
    for target in ["0.r1.json", "../../outside.txt"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("record");
        let store = Store::create(&root, assoc()).unwrap();
        put_derivations_at(&store, at(0));
        put_derivations_at(&store, at(60));
        let outside = dir.path().join("outside.txt");
        fs::write(&outside, b"kept").unwrap();
        let replaced = root.join("derivations/0.r1.json");
        fs::remove_file(&replaced).unwrap();
        std::os::unix::fs::symlink(target, &replaced).unwrap();

        commit_at(&store, at(61 + GRACE_SECONDS));
        assert!(
            replaced.symlink_metadata().unwrap().is_symlink(),
            "{target}"
        );
        assert_eq!(fs::read(&outside).unwrap(), b"kept", "{target}");
        assert!(store.manifest().unwrap().superseded.is_empty(), "{target}");
    }
}

#[cfg(unix)]
#[test]
fn nothing_is_deleted_while_the_manifest_the_lock_or_the_observations_directory_is_a_symlink() {
    for entry in ["manifest.json", "write.lock", "observations"] {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(dir.path(), assoc()).unwrap();
        put_derivations_at(&store, at(0));
        put_derivations_at(&store, at(60));
        let replaced = dir.path().join("derivations/0.r1.json");
        if entry == "observations" {
            // No log yet, and logs would go to the derivations directory.
            std::os::unix::fs::symlink("derivations", dir.path().join(entry)).unwrap();
        } else {
            // The entry moved over the replaced document, due for deletion,
            // with a link to it in its place.
            fs::rename(dir.path().join(entry), &replaced).unwrap();
            std::os::unix::fs::symlink("derivations/0.r1.json", dir.path().join(entry)).unwrap();
        }

        commit_at(&store, at(61 + GRACE_SECONDS));
        assert!(replaced.is_file(), "{entry}");
        assert_eq!(
            store.manifest().unwrap().superseded,
            BTreeMap::from([("derivations/0.r1.json".to_owned(), at(60))]),
            "{entry}"
        );
    }
}

#[test]
fn a_superseded_entry_under_a_name_the_record_never_writes_is_dropped_never_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    put_derivations_at(&store, at(0));
    // Files in the derivations directory under names no derivation
    // document has, listed as replaced long ago.
    let names = [
        "notes.json",
        "0.json",
        "x.r1.json",
        "0.r.json",
        "0.r1.json.tmp",
        "0.r1.JSON",
    ];
    for name in names {
        fs::write(dir.path().join("derivations").join(name), b"kept").unwrap();
    }
    let listed: Vec<String> = names
        .iter()
        .map(|name| format!("derivations/{name}"))
        .collect();
    let listed: Vec<&str> = listed.iter().map(String::as_str).collect();
    list_as_superseded(dir.path(), &listed, at(-2 * GRACE_SECONDS));

    commit_at(&store, at(60));
    for name in names {
        assert!(dir.path().join("derivations").join(name).exists(), "{name}");
    }
    assert!(store.manifest().unwrap().superseded.is_empty());
}

#[test]
fn dropped_transaction_leaves_manifest_unchanged_and_releases_lock() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    {
        let mut tx = store.begin().unwrap();
        tx.append_observations(&[observation("s1", 1)]).unwrap();
        tx.set_cursor(
            "t",
            SourceCursor {
                offset: 4,
                next_ordinal: 1,
                session: Some(session()),
                prefix_hash: ContentId::of(b"one\n"),
            },
        );
    };
    let manifest = store.manifest().unwrap();
    assert_eq!(manifest.revision, Revision::ZERO);
    assert!(manifest.sessions.is_empty());
    assert!(manifest.logs.is_empty());
    assert!(manifest.cursors.is_empty());
    assert!(
        store
            .snapshot()
            .unwrap()
            .observations(&session())
            .unwrap()
            .is_empty()
    );
    // The lock was released with the transaction.
    store.begin().unwrap();
}

#[test]
fn cursors_and_associations_are_part_of_the_revision() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let mut tx = store.begin().unwrap();
    tx.set_cursor(
        "/home/x/.claude/projects/-work-repo/s1.jsonl",
        SourceCursor {
            offset: 120,
            next_ordinal: 7,
            session: Some(session()),
            prefix_hash: ContentId::of(b"prefix"),
        },
    );
    tx.add_association(Association {
        checkout: PathBuf::from("/work/repo-wt"),
    });
    tx.add_association(assoc());
    tx.commit().unwrap();
    let manifest = store.manifest().unwrap();
    assert_eq!(
        manifest.cursors["/home/x/.claude/projects/-work-repo/s1.jsonl"].offset,
        120
    );
    assert_eq!(manifest.associations.len(), 2);
}

#[test]
fn stale_lock_file_from_a_dead_writer_does_not_block() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    // A writer that was killed leaves the file behind. Only the OS lock
    // matters, and the kernel released that with the process.
    fs::write(dir.path().join("write.lock"), b"").unwrap();
    let mut tx = store.begin().unwrap();
    tx.append_observations(&[observation("s1", 1)]).unwrap();
    assert_eq!(tx.commit().unwrap(), Revision::ZERO.next());
}

#[test]
fn transaction_snapshot_is_pinned_to_its_starting_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let observations = [observation("s1", 1), observation("s1", 2)];
    let mut tx = store.begin().unwrap();
    tx.append_observations(&observations[..1]).unwrap();
    tx.commit().unwrap();

    let mut tx = store.begin().unwrap();
    let pinned = tx.snapshot();
    assert_eq!(pinned.revision(), Revision::ZERO.next());
    tx.append_observations(&observations[1..]).unwrap();
    assert_eq!(pinned.observations(&session()).unwrap(), observations[..1]);
}

#[test]
fn create_is_atomic_against_an_existing_manifest() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("manifest.json"), b"{}").unwrap();
    assert!(matches!(
        Store::create(dir.path(), assoc()),
        Err(StoreError::AlreadyExists(_))
    ));
    assert_eq!(fs::read(dir.path().join("manifest.json")).unwrap(), b"{}");

    // An empty manifest left by anything else is not overwritten either.
    let empty = tempfile::tempdir().unwrap();
    fs::write(empty.path().join("manifest.json"), b"").unwrap();
    assert!(matches!(
        Store::create_with_id(empty.path(), RecordId::generate(), assoc()),
        Err(StoreError::AlreadyExists(_))
    ));
    assert_eq!(fs::read(empty.path().join("manifest.json")).unwrap(), b"");
    assert_eq!(dir_names(empty.path()), vec!["manifest.json".to_owned()]);
}

#[test]
fn create_publishes_a_complete_manifest_and_leaves_no_temporary_file() {
    let dir = tempfile::tempdir().unwrap();
    // A creator killed before linking leaves only its temporary file, which
    // neither makes the directory a record nor blocks the next creator.
    let stale = RecordId::generate();
    let stale_tmp = format!("manifest.json.{}.tmp", stale.as_str());
    fs::write(dir.path().join(&stale_tmp), b"{\"partial").unwrap();
    assert!(matches!(
        Store::open(dir.path()),
        Err(StoreError::NotARecord(_))
    ));

    let id = RecordId::generate();
    let store = Store::create_with_id(dir.path(), id.clone(), assoc()).unwrap();
    assert_eq!(store.manifest().unwrap().record_id, id);
    assert_eq!(
        dir_names(dir.path()),
        vec!["manifest.json".to_owned(), stale_tmp]
    );
}

#[test]
fn derivations_round_trip_through_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let set = derivation_set("s1", Revision::ZERO.next());
    let mut tx = store.begin().unwrap();
    tx.put_derivations(&set).unwrap();
    tx.commit().unwrap();
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.derivations(&session()).unwrap(), Some(set));
    assert_eq!(
        snapshot.derivations(&SessionId::new("other")).unwrap(),
        None
    );
}

/// Writes `s1`'s derivations in the store's first write, at the path
/// `derivations/0.r1.json`, and checks that they read back.
fn put_first_derivations(store: &Store) {
    let set = derivation_set("s1", Revision::ZERO.next());
    let mut tx = store.begin().unwrap();
    tx.put_derivations(&set).unwrap();
    tx.commit().unwrap();
    assert_eq!(
        store.snapshot().unwrap().derivations(&session()).unwrap(),
        Some(set)
    );
}

#[test]
fn a_derivations_document_never_writes_through_a_file_at_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    // A file at the next document's path, with a second name that shows
    // whether its content changes.
    fs::create_dir(dir.path().join("derivations")).unwrap();
    let document = dir.path().join("derivations/0.r1.json");
    fs::write(&document, b"held").unwrap();
    let held = dir.path().join("held");
    fs::hard_link(&document, &held).unwrap();

    put_first_derivations(&store);
    assert_eq!(fs::read(&held).unwrap(), b"held");
}

#[cfg(unix)]
#[test]
fn a_derivations_document_never_writes_through_a_link_at_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(&dir.path().join("record"), assoc()).unwrap();
    let target = dir.path().join("target");
    fs::write(&target, b"held").unwrap();
    fs::create_dir(store.root().join("derivations")).unwrap();
    let document = store.root().join("derivations/0.r1.json");
    std::os::unix::fs::symlink(&target, &document).unwrap();

    put_first_derivations(&store);
    // The link is replaced, never followed.
    assert!(document.symlink_metadata().unwrap().is_file());
    assert_eq!(fs::read(&target).unwrap(), b"held");
}

#[test]
fn open_rejects_a_manifest_that_is_not_a_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("manifest.json")).unwrap();
    assert!(matches!(
        Store::open(dir.path()),
        Err(StoreError::NotARecord(_))
    ));
    let file = dir.path().join("plain-file");
    fs::write(&file, b"").unwrap();
    assert!(matches!(Store::open(&file), Err(StoreError::NotARecord(_))));
}

#[cfg(unix)]
#[test]
fn open_of_an_unreadable_record_is_an_io_error() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("record");
    Store::create(&root, assoc()).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::metadata(root.join("manifest.json")).is_ok() {
        // Permissions are ignored (running as root); nothing to test.
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    let opened = Store::open(&root);
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        matches!(opened, Err(StoreError::Io { .. })),
        "expected an i/o error, got {opened:?}"
    );
}

fn tmp_names(dir: &std::path::Path) -> Vec<String> {
    dir_names(dir)
        .into_iter()
        .filter(|n| std::path::Path::new(n).extension() == Some("tmp".as_ref()))
        .collect()
}

#[test]
fn initial_publish_never_truncates_an_existing_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let id = RecordId::generate();
    Store::create_with_id(dir.path(), id.clone(), assoc()).unwrap();
    let manifest = dir.path().join("manifest.json");
    let before = fs::read(&manifest).unwrap();

    assert!(matches!(
        Store::create_with_id(dir.path(), id, assoc()),
        Err(StoreError::AlreadyExists(_))
    ));
    assert_eq!(fs::read(&manifest).unwrap(), before);
    assert!(tmp_names(dir.path()).is_empty());
}

#[test]
fn a_leftover_temporary_link_cannot_truncate_the_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let id = RecordId::generate();
    Store::create_with_id(dir.path(), id.clone(), assoc()).unwrap();
    let manifest = dir.path().join("manifest.json");
    let before = fs::read(&manifest).unwrap();
    // A creator whose temporary-file removal failed leaves a second link to
    // the live manifest under the old per-record temporary name.
    let stale = dir
        .path()
        .join(format!("manifest.json.{}.tmp", id.as_str()));
    fs::hard_link(&manifest, &stale).unwrap();

    // A different association makes a rewrite through that link visible.
    let other = Association {
        checkout: PathBuf::from("/work/other"),
    };
    assert!(matches!(
        Store::create_with_id(dir.path(), id, other),
        Err(StoreError::AlreadyExists(_))
    ));
    assert_eq!(fs::read(&manifest).unwrap(), before);
    assert_eq!(
        tmp_names(dir.path()),
        vec![stale.file_name().unwrap().to_string_lossy().into_owned()]
    );
}

#[test]
fn publishing_never_opens_a_file_at_the_former_temporary_name() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    // A file at the fixed name the manifest was once written through.
    let held = dir.path().join("manifest.json.tmp");
    fs::write(&held, b"held").unwrap();

    let rev = store.begin().unwrap().commit().unwrap();
    assert_eq!(store.snapshot().unwrap().revision(), rev);
    assert_eq!(fs::read(&held).unwrap(), b"held");
    // The write's own temporary file is renamed into place.
    assert_eq!(tmp_names(dir.path()), ["manifest.json.tmp"]);
}

#[test]
fn a_manifest_with_an_unsupported_schema_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    Store::create(dir.path(), assoc()).unwrap();
    let manifest_path = dir.path().join("manifest.json");
    let text = fs::read_to_string(&manifest_path).unwrap();
    let newer = text.replace("\"schema_version\": 2", "\"schema_version\": 3");
    assert_ne!(newer, text);
    fs::write(&manifest_path, &newer).unwrap();

    let store = Store::open(dir.path()).unwrap();
    assert!(matches!(
        store.manifest(),
        Err(StoreError::UnsupportedSchema {
            found: 3,
            supported: 2,
            ..
        })
    ));
    assert!(matches!(
        store.snapshot(),
        Err(StoreError::UnsupportedSchema {
            found: 3,
            supported: 2,
            ..
        })
    ));
    assert!(matches!(
        store.begin(),
        Err(StoreError::UnsupportedSchema { .. })
    ));
    // A newer record needs a newer reader: nothing suggests removing it.
    let message = store.manifest().unwrap_err().to_string();
    assert!(
        message.contains("uses record format 3; this build reads format 2"),
        "{message}"
    );
    assert!(
        message.contains("A newer supersigil wrote it; use that version to read it."),
        "{message}"
    );
    assert!(!message.contains("delete"), "{message}");
    // Nothing rewrote the newer record.
    assert_eq!(fs::read_to_string(&manifest_path).unwrap(), newer);
}

/// What the message for an older record advises: keeping it comes first.
const OLDER_RECORD_ADVICE: &str = "To keep its observations, move that directory out of the \
    records directory; to discard them, delete it. The next sync then reads the transcripts \
    that still exist.";

/// A manifest in the layout plan 1 wrote: version 1, logs keyed by session
/// id, and no `sessions` map.
const PLAN_1_MANIFEST: &str = r#"{
  "record_id": "d435b3f4-5a03-4c70-a4c8-009547bee6a1",
  "schema_version": 1,
  "revision": 1,
  "associations": [{"checkout": "/work/repo"}],
  "logs": {"observations/s1/events.jsonl": 10},
  "documents": {},
  "cursors": {}
}"#;

#[test]
fn an_older_layout_is_refused_by_its_version_not_by_a_missing_field() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("manifest.json"), PLAN_1_MANIFEST).unwrap();

    let store = Store::open(dir.path()).unwrap();
    let error = store.manifest().unwrap_err();
    assert!(
        matches!(
            &error,
            StoreError::UnsupportedSchema { root, found: 1, supported: 2 } if root == dir.path()
        ),
        "{error:?}"
    );
    let message = error.to_string();
    assert!(
        message.contains(&dir.path().display().to_string()),
        "{message}"
    );
    assert!(
        message.contains("uses record format 1; this build reads format 2"),
        "{message}"
    );
    assert!(message.contains(OLDER_RECORD_ADVICE), "{message}");
    // The manifest is left as it was.
    assert_eq!(
        fs::read_to_string(dir.path().join("manifest.json")).unwrap(),
        PLAN_1_MANIFEST
    );
}

#[test]
fn the_current_layout_at_version_1_is_refused_by_its_version() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let mut tx = store.begin().unwrap();
    tx.append_observations(&[observation("s1", 0)]).unwrap();
    tx.commit().unwrap();
    let manifest_path = dir.path().join("manifest.json");
    let text = fs::read_to_string(&manifest_path).unwrap();
    let parsed: Value = serde_json::from_str(&text).unwrap();
    assert!(parsed["sessions"]["s1"].is_u64(), "{text}");
    let older = text.replace("\"schema_version\": 2", "\"schema_version\": 1");
    assert_ne!(older, text);
    fs::write(&manifest_path, &older).unwrap();

    let store = Store::open(dir.path()).unwrap();
    let error = store.manifest().unwrap_err();
    assert!(
        matches!(
            &error,
            StoreError::UnsupportedSchema { root, found: 1, supported: 2 } if root == dir.path()
        ),
        "{error:?}"
    );
    let message = error.to_string();
    assert!(
        message.contains(&dir.path().display().to_string()),
        "{message}"
    );
    assert!(
        message.contains("uses record format 1; this build reads format 2"),
        "{message}"
    );
    assert!(message.contains(OLDER_RECORD_ADVICE), "{message}");
    // The manifest is left as it was.
    assert_eq!(fs::read_to_string(&manifest_path).unwrap(), older);
}

#[test]
fn a_manifest_without_a_version_is_invalid_json() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("manifest.json"), "{}").unwrap();
    let store = Store::open(dir.path()).unwrap();
    assert!(matches!(store.manifest(), Err(StoreError::Json { .. })));
}
