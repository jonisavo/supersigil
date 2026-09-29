//! Tests snapshot isolation, atomic publication, writer locking, and crash recovery.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;

use supersigil_record::derivations::{ALGORITHM_VERSION, DerivationSet};
use supersigil_record::observations::{EndReason, Observation, SessionEnd};
use supersigil_record::store::{Association, SourceCursor, Store, StoreError};
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
fn a_manifest_with_an_unsupported_schema_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    Store::create(dir.path(), assoc()).unwrap();
    let manifest_path = dir.path().join("manifest.json");
    let text = fs::read_to_string(&manifest_path).unwrap();
    let newer = text.replace("\"schema_version\": 1", "\"schema_version\": 2");
    assert_ne!(newer, text);
    fs::write(&manifest_path, &newer).unwrap();

    let store = Store::open(dir.path()).unwrap();
    assert!(matches!(
        store.manifest(),
        Err(StoreError::UnsupportedSchema {
            found: 2,
            supported: 1
        })
    ));
    assert!(matches!(
        store.snapshot(),
        Err(StoreError::UnsupportedSchema {
            found: 2,
            supported: 1
        })
    ));
    assert!(matches!(
        store.begin(),
        Err(StoreError::UnsupportedSchema { .. })
    ));
    // Nothing rewrote the newer record.
    assert_eq!(fs::read_to_string(&manifest_path).unwrap(), newer);
}
