//! The manifest pins exactly one revision; readers never mix two.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;

use supersigil_record::store::{
    Association, SourceCursor, Store, StoreError, decode_storage_key, observations_log, storage_key,
};
use supersigil_record::{RecordId, Revision, SessionId};

fn assoc() -> Association {
    Association {
        checkout: PathBuf::from("/work/repo"),
    }
}

fn session() -> SessionId {
    SessionId::new("s1")
}

fn dir_names(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
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
    let log = observations_log(&session());

    let mut tx = store.begin().unwrap();
    tx.append_log(&log, &[b"one", b"two"]).unwrap();
    let rev = tx.commit().unwrap();
    assert_eq!(rev, Revision::ZERO.next());

    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.revision(), rev);
    assert_eq!(snapshot.manifest().logs[&log], 8);
    assert_eq!(
        snapshot.read_log(&log).unwrap(),
        vec![b"one".to_vec(), b"two".to_vec()]
    );
    assert_eq!(snapshot.sessions(), vec![session()]);
}

#[test]
fn reader_stays_pinned_to_its_manifest_while_writer_appends() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let log = observations_log(&session());
    let mut tx = store.begin().unwrap();
    tx.append_log(&log, &[b"one", b"two"]).unwrap();
    tx.commit().unwrap();

    let pinned = store.snapshot().unwrap();
    let mut tx = store.begin().unwrap();
    tx.append_log(&log, &[b"three"]).unwrap();
    // Not yet committed: the pinned reader sees two lines, a fresh reader too.
    assert_eq!(pinned.read_log(&log).unwrap().len(), 2);
    assert_eq!(store.snapshot().unwrap().read_log(&log).unwrap().len(), 2);
    tx.commit().unwrap();
    // Committed: the pinned reader still sees two, a fresh reader sees three.
    assert_eq!(pinned.read_log(&log).unwrap().len(), 2);
    assert_eq!(store.snapshot().unwrap().read_log(&log).unwrap().len(), 3);
}

#[test]
fn tail_beyond_pinned_length_is_ignored_and_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let log = observations_log(&session());
    let mut tx = store.begin().unwrap();
    tx.append_log(&log, &[b"one"]).unwrap();
    tx.commit().unwrap();

    // Simulate a crash after appending but before publishing the manifest.
    let path = dir.path().join(&log);
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(b"{\"partial").unwrap();
    drop(file);
    assert_eq!(
        store.snapshot().unwrap().read_log(&log).unwrap(),
        vec![b"one".to_vec()]
    );

    let mut tx = store.begin().unwrap();
    tx.append_log(&log, &[b"two"]).unwrap();
    tx.commit().unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"one\ntwo\n");
    assert_eq!(
        store.snapshot().unwrap().read_log(&log).unwrap(),
        vec![b"one".to_vec(), b"two".to_vec()]
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
    tx.append_log(&observations_log(&session()), &[b"one"])
        .unwrap();

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
    let mut tx = store.begin().unwrap();
    tx.put_document("derivations/s1", b"{\"v\":1}").unwrap();
    tx.commit().unwrap();
    let first = store.snapshot().unwrap();

    let mut tx = store.begin().unwrap();
    tx.put_document("derivations/s1", b"{\"v\":2}").unwrap();
    tx.commit().unwrap();
    let second = store.snapshot().unwrap();

    assert_eq!(
        first.read_document("derivations/s1").unwrap().unwrap(),
        b"{\"v\":1}"
    );
    assert_eq!(
        second.read_document("derivations/s1").unwrap().unwrap(),
        b"{\"v\":2}"
    );
    assert_eq!(
        first.manifest().documents["derivations/s1"],
        "derivations/s1.r1.json"
    );
    assert_eq!(
        second.manifest().documents["derivations/s1"],
        "derivations/s1.r2.json"
    );
    assert!(dir.path().join("derivations/s1.r1.json").exists());
    assert_eq!(first.read_document("missing").unwrap(), None);
}

#[test]
fn dropped_transaction_leaves_manifest_unchanged_and_releases_lock() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let log = observations_log(&session());
    {
        let mut tx = store.begin().unwrap();
        tx.append_log(&log, &[b"one"]).unwrap();
        tx.set_cursor(
            "t",
            SourceCursor {
                offset: 4,
                next_ordinal: 1,
                session: Some(session()),
                prefix_hash: None,
            },
        );
    };
    let manifest = store.manifest().unwrap();
    assert_eq!(manifest.revision, Revision::ZERO);
    assert!(manifest.logs.is_empty());
    assert!(manifest.cursors.is_empty());
    assert!(store.snapshot().unwrap().read_log(&log).unwrap().is_empty());
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
            prefix_hash: None,
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
fn log_lines_may_not_contain_newlines() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let mut tx = store.begin().unwrap();
    assert!(matches!(
        tx.append_log("observations/s1/events.jsonl", &[b"a\nb"]),
        Err(StoreError::Corrupt(_))
    ));
}

#[test]
fn stale_lock_file_from_a_dead_writer_does_not_block() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    // A writer that was killed leaves the file behind. Only the OS lock
    // matters, and the kernel released that with the process.
    fs::write(dir.path().join("write.lock"), b"").unwrap();
    let mut tx = store.begin().unwrap();
    tx.append_log(&observations_log(&session()), &[b"one"])
        .unwrap();
    assert_eq!(tx.commit().unwrap(), Revision::ZERO.next());
}

#[test]
fn transaction_snapshot_is_pinned_to_its_starting_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let log = observations_log(&session());
    let mut tx = store.begin().unwrap();
    tx.append_log(&log, &[b"one"]).unwrap();
    tx.commit().unwrap();

    let mut tx = store.begin().unwrap();
    let pinned = tx.snapshot();
    assert_eq!(pinned.revision(), Revision::ZERO.next());
    tx.append_log(&log, &[b"two"]).unwrap();
    assert_eq!(pinned.read_log(&log).unwrap(), vec![b"one".to_vec()]);
}

#[test]
fn log_names_cannot_escape_the_record() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let victim = observations_log(&SessionId::new("victim"));
    let mut tx = store.begin().unwrap();
    tx.append_log(&victim, &[b"keep me"]).unwrap();
    tx.commit().unwrap();

    let mut tx = store.begin().unwrap();
    for bad in [
        "../outside.jsonl",
        "observations/../../x",
        "/abs/path",
        "observations//events.jsonl",
        "./events.jsonl",
        "",
    ] {
        assert!(
            matches!(tx.append_log(bad, &[b"x"]), Err(StoreError::InvalidName(_))),
            "append_log accepted {bad:?}"
        );
        assert!(
            matches!(tx.put_document(bad, b"{}"), Err(StoreError::InvalidName(_))),
            "put_document accepted {bad:?}"
        );
    }
    drop(tx);
    let snapshot = store.snapshot().unwrap();
    assert!(matches!(
        snapshot.read_log("../x"),
        Err(StoreError::InvalidName(_))
    ));
    assert_eq!(
        snapshot.read_log(&victim).unwrap(),
        vec![b"keep me".to_vec()]
    );
    assert!(!dir.path().parent().unwrap().join("outside.jsonl").exists());
}

#[test]
fn odd_session_ids_get_safe_storage_keys() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let victim = SessionId::new("victim");
    let attacker = SessionId::new("../observations/victim");
    let mut tx = store.begin().unwrap();
    tx.append_log(&observations_log(&victim), &[b"keep me"])
        .unwrap();
    tx.commit().unwrap();

    let mut tx = store.begin().unwrap();
    tx.append_log(&observations_log(&attacker), &[b"other"])
        .unwrap();
    tx.commit().unwrap();

    let snapshot = store.snapshot().unwrap();
    assert_eq!(
        snapshot.read_log(&observations_log(&victim)).unwrap(),
        vec![b"keep me".to_vec()]
    );
    assert_eq!(
        snapshot.read_log(&observations_log(&attacker)).unwrap(),
        vec![b"other".to_vec()]
    );
    assert_eq!(storage_key(&attacker), "%2E%2E%2Fobservations%2Fvictim");
    let mut sessions = snapshot.sessions();
    sessions.sort();
    assert_eq!(sessions, vec![attacker, victim]);
}

#[test]
fn storage_keys_stay_distinct_when_case_is_folded() {
    // Case-insensitive file systems fold `S` and `s` into one file name, so
    // uppercase letters are escaped like every other byte outside the plain
    // set.
    assert_eq!(storage_key(&SessionId::new("S")), "%53");
    assert_eq!(storage_key(&SessionId::new("s")), "s");
    assert_ne!(
        storage_key(&SessionId::new("S")).to_lowercase(),
        storage_key(&SessionId::new("s")).to_lowercase()
    );
    assert_eq!(decode_storage_key("%53"), SessionId::new("S"));
    let uuid = "11111111-1111-4111-8111-111111111111";
    assert_eq!(storage_key(&SessionId::new(uuid)), uuid);
}

#[test]
fn names_outside_the_log_namespace_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let mut tx = store.begin().unwrap();
    for reserved in ["manifest.json", "manifest.json.tmp", "write.lock"] {
        assert!(
            matches!(
                tx.append_log(reserved, &[b"x"]),
                Err(StoreError::InvalidName(_))
            ),
            "append_log accepted {reserved:?}"
        );
    }
    tx.put_document("derivations/s1", b"{}").unwrap();
    tx.commit().unwrap();

    let pinned = store.manifest().unwrap().documents["derivations/s1"].clone();
    let mut tx = store.begin().unwrap();
    assert!(matches!(
        tx.append_log(&pinned, &[b"x"]),
        Err(StoreError::InvalidName(_))
    ));
    // A log may not take a document's file-name form, so no document can
    // ever alias a log either.
    assert!(matches!(
        tx.append_log("derivations/s2.r2.json", &[b"x"]),
        Err(StoreError::InvalidName(_))
    ));
    assert!(
        fs::read_to_string(dir.path().join("manifest.json"))
            .unwrap()
            .contains("record_id")
    );
}

#[test]
fn pinned_document_names_are_validated_at_read() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let mut tx = store.begin().unwrap();
    tx.put_document("d", b"{}").unwrap();
    tx.commit().unwrap();

    let manifest_path = dir.path().join("manifest.json");
    let text = fs::read_to_string(&manifest_path).unwrap();
    fs::write(&manifest_path, text.replace("d.r1.json", "../outside.json")).unwrap();
    let snapshot = store.snapshot().unwrap();
    assert!(matches!(
        snapshot.read_document("d"),
        Err(StoreError::InvalidName(_))
    ));
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
fn empty_log_lines_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let log = observations_log(&session());
    let mut tx = store.begin().unwrap();
    tx.append_log(&log, &[b"", b"x", b""]).unwrap();
    tx.commit().unwrap();
    assert_eq!(
        store.snapshot().unwrap().read_log(&log).unwrap(),
        vec![Vec::new(), b"x".to_vec(), Vec::new()]
    );
}

#[test]
fn derivations_round_trip_through_the_store() {
    use supersigil_record::derivations::{ALGORITHM_VERSION, DerivationSet};
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let set = DerivationSet {
        session: session(),
        observation_revision: Revision::ZERO.next(),
        algorithm_version: ALGORITHM_VERSION,
        restores: Vec::new(),
        discontinuities: Vec::new(),
    };
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
fn noncanonical_names_cannot_alias_a_committed_log() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let log = "observations/s1/events.jsonl";
    let mut tx = store.begin().unwrap();
    tx.append_log(log, &[b"keep me"]).unwrap();
    tx.commit().unwrap();

    let mut tx = store.begin().unwrap();
    for alias in [
        "observations/s1/./events.jsonl",
        "observations/./s1/events.jsonl",
        "a\\b.jsonl",
        "a/../b.jsonl",
        "a/b\0.jsonl",
        "observations/s1/events.jsonl/",
    ] {
        assert!(
            matches!(
                tx.append_log(alias, &[b"x"]),
                Err(StoreError::InvalidName(_))
            ),
            "append_log accepted {alias:?}"
        );
    }
    assert!(matches!(
        tx.put_document("a\\b", b"{}"),
        Err(StoreError::InvalidName(_))
    ));
    assert!(matches!(
        tx.put_document("a/../b", b"{}"),
        Err(StoreError::InvalidName(_))
    ));
    drop(tx);
    assert_eq!(fs::read(dir.path().join(log)).unwrap(), b"keep me\n");
    assert_eq!(
        store.snapshot().unwrap().read_log(log).unwrap(),
        vec![b"keep me".to_vec()]
    );
}

#[test]
fn logs_cannot_alias_documents_of_any_revision() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let mut tx = store.begin().unwrap();
    tx.put_document("d", b"{\"v\":1}").unwrap();
    tx.commit().unwrap();
    let first = store.snapshot().unwrap();

    let mut tx = store.begin().unwrap();
    tx.put_document("d", b"{\"v\":2}").unwrap();
    // `d.r1.json` is no longer the current pin once r2 is staged, but the
    // first snapshot still reads it.
    assert!(matches!(
        tx.append_log("d.r1.json", &[b"x"]),
        Err(StoreError::InvalidName(_))
    ));
    assert!(matches!(
        tx.append_log("notes.txt", &[b"x"]),
        Err(StoreError::InvalidName(_))
    ));
    drop(tx);
    assert_eq!(first.read_document("d").unwrap().unwrap(), b"{\"v\":1}");
    assert_eq!(
        fs::read(dir.path().join("d.r1.json")).unwrap(),
        b"{\"v\":1}"
    );
}

#[test]
fn commit_into_a_new_nested_directory_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let log = observations_log(&SessionId::new("fresh"));
    assert!(!dir.path().join("observations").exists());
    let mut tx = store.begin().unwrap();
    tx.append_log(&log, &[b"one"]).unwrap();
    tx.put_document("derivations/fresh", b"{}").unwrap();
    assert_eq!(tx.commit().unwrap(), Revision::ZERO.next());
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.read_log(&log).unwrap(), vec![b"one".to_vec()]);
    assert_eq!(
        snapshot
            .read_document("derivations/fresh")
            .unwrap()
            .unwrap(),
        b"{}"
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
fn drive_prefixed_and_rooted_names_are_rejected_on_every_platform() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let mut tx = store.begin().unwrap();
    for bad in [
        "C:/outside.jsonl",
        "C:outside.jsonl",
        "//server/share/x.jsonl",
    ] {
        assert!(
            matches!(tx.append_log(bad, &[b"x"]), Err(StoreError::InvalidName(_))),
            "append_log accepted {bad:?}"
        );
        assert!(
            matches!(tx.put_document(bad, b"{}"), Err(StoreError::InvalidName(_))),
            "put_document accepted {bad:?}"
        );
    }
    drop(tx);
    let snapshot = store.snapshot().unwrap();
    assert!(matches!(
        snapshot.read_log("C:/outside.jsonl"),
        Err(StoreError::InvalidName(_))
    ));
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
