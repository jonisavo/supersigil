//! Tests the review store: finding it by repository, saving bases,
//! selections, and the two-phase cleanup.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use supersigil_record::Revision;
use supersigil_record::reviews::{
    Algorithms, BASIS_SCHEMA, Basis, BasisBase, BasisId, BasisScope, BasisTarget, BasisTimes,
    PathBytes, ReviewStore, TargetKind, create_in, find,
};
use supersigil_record::store::StoreError;
use supersigil_record::time::{GRACE_SECONDS, REMOVAL_DELAY_SECONDS, UtcTime};

const REPOSITORY: &str = "/work/repo/.git";

/// 2026-10-13T00:00:00Z plus `seconds`.
fn at(seconds: i64) -> UtcTime {
    UtcTime::from_unix(1_791_849_600 + seconds)
}

/// A minimal basis of `worktree` whose target tree is `tree` repeated.
fn basis(worktree: &str, tree: char) -> Basis {
    Basis {
        schema: BASIS_SCHEMA.to_owned(),
        worktree: PathBuf::from(worktree),
        base: BasisBase {
            commit: Some("1".repeat(40)),
            tree: "2".repeat(40),
        },
        target: BasisTarget {
            kind: TargetKind::WorkingTree,
            commit: None,
            tree: tree.to_string().repeat(40),
        },
        scope: BasisScope {
            include_untracked: Vec::new(),
            paths: vec![PathBytes::new("src")],
        },
        records: Vec::new(),
        unreconciled_checkouts: Vec::new(),
        algorithms: Algorithms { derivation: 2 },
        explanations: Vec::new(),
        capture: Vec::new(),
        gate: Vec::new(),
        salt: None,
    }
}

fn new_store(dir: &Path) -> ReviewStore {
    ReviewStore::create(&dir.join("store"), Path::new(REPOSITORY)).unwrap()
}

/// Saves `basis` at `now` in its own transaction and commits it.
fn save(store: &ReviewStore, basis: &Basis, now: UtcTime) -> BasisId {
    let mut tx = store.begin().unwrap();
    let id = tx.save_basis(basis, now).unwrap();
    tx.commit().unwrap();
    id
}

/// Runs cleanup at `now` in its own transaction and commits it.
fn clean(store: &ReviewStore, now: UtcTime) {
    let mut tx = store.begin().unwrap();
    tx.cleanup(now).unwrap();
    tx.commit().unwrap();
}

fn basis_file(store: &ReviewStore, id: &BasisId) -> PathBuf {
    store.root().join("bases").join(format!("{id}.json"))
}

/// Names of the files in the store's `bases` directory, sorted.
fn bases_files(store: &ReviewStore) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(store.root().join("bases"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn file_name(id: &BasisId) -> String {
    format!("{id}.json")
}

/// The extension of a temporary file the store writes a basis file
/// through: `json`, a fresh UUID, and `tmp`.
const TEMPORARY: &str = "json.0f6e3a52-8d1c-4b7e-9a35-2c4d6e8f1a0b.tmp";

/// A name the store writes in `bases/` that no basis here has: a basis
/// file's with `extension` `json`, a temporary file's with [`TEMPORARY`].
fn stray(extension: &str) -> String {
    format!("{}.{extension}", "f".repeat(64))
}

/// Rewrites the store's manifest through `edit`, as a crash or another
/// build might have left it.
fn edit_manifest(store: &ReviewStore, edit: impl FnOnce(&mut Value)) {
    let path = store.root().join("manifest.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    edit(&mut value);
    fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

#[test]
fn a_store_is_created_opened_and_found_by_its_repository() {
    let dir = tempfile::tempdir().unwrap();
    let reviews = dir.path().join("reviews");
    assert!(find(&reviews, Path::new(REPOSITORY)).unwrap().is_none());

    let store = create_in(&reviews, Path::new(REPOSITORY)).unwrap();
    let manifest = store.snapshot().unwrap().manifest().clone();
    assert_eq!(
        store.root().file_name().unwrap().to_str(),
        Some(manifest.store_id.as_str())
    );
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.revision, Revision::ZERO);
    assert_eq!(manifest.repository, Path::new(REPOSITORY));
    assert_eq!(manifest.judgments_length, 0);
    assert!(manifest.bases.is_empty());
    assert!(manifest.removed.is_empty());
    assert!(manifest.selections.is_empty());

    // The directory lock and anything else without a manifest are skipped.
    fs::write(reviews.join(".lock"), b"").unwrap();
    fs::create_dir(reviews.join("empty")).unwrap();
    let found = find(&reviews, Path::new(REPOSITORY)).unwrap().unwrap();
    assert_eq!(found.root(), store.root());
    assert!(
        find(&reviews, Path::new("/work/other/.git"))
            .unwrap()
            .is_none()
    );

    let opened = ReviewStore::open(store.root()).unwrap();
    assert_eq!(opened.snapshot().unwrap().manifest(), &manifest);
    assert!(matches!(
        ReviewStore::open(&reviews.join("empty")),
        Err(StoreError::NotARecord(_))
    ));
    assert!(matches!(
        ReviewStore::create(store.root(), Path::new("/work/other/.git")),
        Err(StoreError::AlreadyExists(_))
    ));
    assert_eq!(
        find(&reviews, Path::new(REPOSITORY))
            .unwrap()
            .unwrap()
            .snapshot()
            .unwrap()
            .manifest()
            .repository,
        Path::new(REPOSITORY)
    );
}

#[test]
fn each_repository_gets_its_own_store() {
    let dir = tempfile::tempdir().unwrap();
    let reviews = dir.path().join("reviews");
    let first = create_in(&reviews, Path::new(REPOSITORY)).unwrap();
    let second = create_in(&reviews, Path::new("/work/other/.git")).unwrap();
    assert_ne!(first.root(), second.root());
    assert_eq!(
        find(&reviews, Path::new(REPOSITORY))
            .unwrap()
            .unwrap()
            .root(),
        first.root()
    );
    assert_eq!(
        find(&reviews, Path::new("/work/other/.git"))
            .unwrap()
            .unwrap()
            .root(),
        second.root()
    );
}

#[test]
fn two_stores_naming_one_repository_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let reviews = dir.path().join("reviews");
    let first = create_in(&reviews, Path::new(REPOSITORY)).unwrap();
    let second = create_in(&reviews, Path::new(REPOSITORY)).unwrap();
    let error = find(&reviews, Path::new(REPOSITORY)).unwrap_err();
    let StoreError::SeveralReviewStores { repository, stores } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(repository, Path::new(REPOSITORY));
    let mut expected = vec![first.root().to_path_buf(), second.root().to_path_buf()];
    expected.sort();
    assert_eq!(stores, &expected);
    let message = error.to_string();
    assert!(message.contains(REPOSITORY), "{message}");
}

#[test]
fn the_writer_lock_admits_one_writer() {
    let dir = tempfile::tempdir().unwrap();
    let store = new_store(dir.path());
    let first = store.begin().unwrap();
    assert!(matches!(store.begin(), Err(StoreError::Locked(_))));
    drop(first);
    store.begin().unwrap();
}

#[test]
fn another_review_store_format_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = new_store(dir.path());
    edit_manifest(&store, |m| m["schema_version"] = json!(2));
    let error = store.snapshot().unwrap_err();
    assert!(
        matches!(
            error,
            StoreError::UnsupportedReviewStore {
                found: 2,
                supported: 1,
                ..
            }
        ),
        "{error:?}"
    );
    assert!(matches!(
        store.begin(),
        Err(StoreError::UnsupportedReviewStore { .. })
    ));
}

#[test]
fn a_basis_saved_twice_keeps_one_file_and_updates_last_reviewed() {
    let dir = tempfile::tempdir().unwrap();
    let store = new_store(dir.path());
    let original = basis("/work/repo", 'a');
    let before = store.snapshot().unwrap();

    let id = save(&store, &original, at(0));
    assert_eq!(id, original.basis_id().unwrap());
    // The file holds exactly the bytes its id hashes.
    assert_eq!(
        fs::read(basis_file(&store, &id)).unwrap(),
        serde_json::to_vec(&original).unwrap()
    );
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.manifest().revision, Revision::ZERO.next());
    assert_eq!(snapshot.basis(&id).unwrap(), original);
    assert_eq!(
        snapshot.manifest().bases[&id],
        BasisTimes {
            first_written: at(0),
            last_reviewed: at(0)
        }
    );
    // A snapshot from before the save does not see it.
    assert!(matches!(
        before.basis(&id),
        Err(StoreError::NoSuchBasis(prefix)) if prefix == id.as_str()
    ));

    assert_eq!(save(&store, &original, at(60)), id);
    assert_eq!(bases_files(&store), [file_name(&id)]);
    assert_eq!(
        store.snapshot().unwrap().manifest().bases[&id],
        BasisTimes {
            first_written: at(0),
            last_reviewed: at(60)
        }
    );
}

#[test]
fn a_damaged_basis_file_is_refused_on_read_and_replaced_on_save() {
    let dir = tempfile::tempdir().unwrap();
    let store = new_store(dir.path());
    let original = basis("/work/repo", 'a');
    let other = basis("/work/repo", 'b');
    let id = save(&store, &original, at(0));
    let path = basis_file(&store, &id);
    let bytes = fs::read(&path).unwrap();

    let damaged = [
        bytes[..bytes.len() / 2].to_vec(),
        serde_json::to_vec(&other).unwrap(),
    ];
    for content in damaged {
        fs::write(&path, &content).unwrap();
        let error = store.snapshot().unwrap().basis(&id).unwrap_err();
        assert!(matches!(error, StoreError::Corrupt(_)), "{error:?}");
        assert!(error.to_string().contains(&id.to_string()), "{error}");

        assert_eq!(save(&store, &original, at(1)), id);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(store.snapshot().unwrap().basis(&id).unwrap(), original);
    }

    // A file a crashed writer left before the basis entered any manifest
    // is checked too.
    let other_id = other.basis_id().unwrap();
    fs::write(basis_file(&store, &other_id), b"{\"partial").unwrap();
    assert_eq!(save(&store, &other, at(2)), other_id);
    assert_eq!(store.snapshot().unwrap().basis(&other_id).unwrap(), other);
    assert_eq!(bases_files(&store), {
        let mut names = [file_name(&id), file_name(&other_id)];
        names.sort();
        names
    });
}

#[test]
fn leftover_files_in_bases_are_deleted_by_the_next_write() {
    let dir = tempfile::tempdir().unwrap();
    let store = new_store(dir.path());
    let kept = save(&store, &basis("/work/repo", 'a'), at(0));
    // A transaction dropped before its commit leaves its basis file.
    let mut tx = store.begin().unwrap();
    let dropped = tx.save_basis(&basis("/work/repo", 'b'), at(1)).unwrap();
    drop(tx);
    // So does a save interrupted before its rename.
    fs::write(store.root().join("bases").join(stray(TEMPORARY)), b"{").unwrap();
    assert_eq!(bases_files(&store).len(), 3);
    assert!(
        !store
            .snapshot()
            .unwrap()
            .manifest()
            .bases
            .contains_key(&dropped)
    );

    clean(&store, at(2));
    assert_eq!(bases_files(&store), [file_name(&kept)]);
}

#[test]
fn cleanup_deletes_only_names_the_store_writes_in_bases() {
    let dir = tempfile::tempdir().unwrap();
    let store = new_store(dir.path());
    let kept = save(&store, &basis("/work/repo", 'a'), at(0));
    let bases = store.root().join("bases");
    // Neither a basis file's name nor a temporary file's, such as the
    // fixed name temporary files once had.
    let foreign = [
        "notes.json".to_owned(),
        "stray.json.tmp".to_owned(),
        format!("{}.json", "g".repeat(64)),
        stray("json.tmp"),
        stray("json.unique.tmp"),
    ];
    for name in foreign
        .iter()
        .cloned()
        .chain([stray("json"), stray(TEMPORARY)])
    {
        fs::write(bases.join(name), b"{").unwrap();
    }

    clean(&store, at(1));
    let mut expected = vec![file_name(&kept)];
    expected.extend(foreign);
    expected.sort();
    assert_eq!(bases_files(&store), expected);
}

#[test]
fn a_write_never_opens_a_file_at_a_former_temporary_name() {
    let dir = tempfile::tempdir().unwrap();
    let store = new_store(dir.path());
    let original = basis("/work/repo", 'a');
    let id = original.basis_id().unwrap();
    let held_name = format!("{}.tmp", file_name(&id));
    // Files at the fixed names temporary files were once written under:
    // the manifest's and that of the basis about to be saved.
    let held = [
        store.root().join("manifest.json.tmp"),
        store.root().join("bases").join(&held_name),
    ];
    fs::create_dir(store.root().join("bases")).unwrap();
    for path in &held {
        fs::write(path, b"held").unwrap();
    }

    let mut tx = store.begin().unwrap();
    tx.save_basis(&original, at(0)).unwrap();
    tx.cleanup(at(0)).unwrap();
    tx.commit().unwrap();

    assert_eq!(store.snapshot().unwrap().basis(&id).unwrap(), original);
    for path in &held {
        assert_eq!(fs::read(path).unwrap(), b"held", "{}", path.display());
    }
    // The write's own temporary file is renamed into place.
    assert_eq!(bases_files(&store), [file_name(&id), held_name]);
}

/// Replaces the file of basis `id` with a symbolic link to `target`, as
/// someone with access to the store might.
#[cfg(unix)]
fn link_basis(store: &ReviewStore, id: &BasisId, target: &str) {
    let path = basis_file(store, id);
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(target, &path).unwrap();
}

#[cfg(unix)]
#[test]
fn a_live_basis_file_that_is_a_symlink_is_made_regular_before_anything_is_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let store = new_store(dir.path());
    let original = basis("/work/repo", 'a');
    let mut tx = store.begin().unwrap();
    let id = tx.save_basis(&original, at(0)).unwrap();
    tx.select(Path::new("/work/repo"), &id);
    tx.commit().unwrap();
    let path = basis_file(&store, &id);
    let current = store.root().join("bases").join(stray("json"));

    // A review of the same content (saving and cleaning), a write that
    // only saves, and a judgment's write (only cleaning).
    for (k, saves, cleans) in [(1, true, true), (2, true, false), (3, false, true)] {
        // The live basis's file moved to a leftover's name, with a link to
        // it in its place.
        fs::copy(&path, &current).unwrap();
        link_basis(&store, &id, &stray("json"));

        let mut tx = store.begin().unwrap();
        if saves {
            tx.save_basis(&original, at(k)).unwrap();
        }
        if cleans {
            tx.cleanup(at(k)).unwrap();
        }
        tx.commit().unwrap();

        assert!(path.symlink_metadata().unwrap().is_file(), "{k}");
        assert_eq!(store.snapshot().unwrap().basis(&id).unwrap(), original);
        // What the link led to is deleted only once the basis no longer
        // needs it.
        assert_eq!(current.exists(), !cleans, "{k}");
    }
}

#[cfg(unix)]
#[test]
fn making_a_basis_file_regular_never_overwrites_what_another_one_leads_to() {
    let dir = tempfile::tempdir().unwrap();
    let store = new_store(dir.path());
    let mut tx = store.begin().unwrap();
    let mut ids = [
        tx.save_basis(&basis("/work/repo", 'a'), at(0)).unwrap(),
        tx.save_basis(&basis("/work/repo", 'c'), at(0)).unwrap(),
    ];
    tx.commit().unwrap();
    // Cleanup makes the smaller ID's file regular first.
    ids.sort();
    let [first, second] = ids;
    let bytes: Vec<Vec<u8>> = [&first, &second]
        .iter()
        .map(|id| fs::read(basis_file(&store, id)).unwrap())
        .collect();
    // The first's file links to a copy of its bytes; the second's links to
    // its only copy, a regular file at the first's former temporary name.
    let bases = store.root().join("bases");
    let held = format!("{}.tmp", file_name(&first));
    fs::write(bases.join("current.json"), &bytes[0]).unwrap();
    link_basis(&store, &first, "current.json");
    fs::write(bases.join(&held), &bytes[1]).unwrap();
    link_basis(&store, &second, &held);

    // A review of a third basis: its write saves, cleans, and commits.
    let mut tx = store.begin().unwrap();
    tx.save_basis(&basis("/work/repo", 'b'), at(1)).unwrap();
    tx.cleanup(at(1)).unwrap();
    tx.commit().unwrap();

    let snapshot = store.snapshot().unwrap();
    for (id, bytes) in [&first, &second].into_iter().zip(&bytes) {
        snapshot.basis(id).unwrap();
        let path = basis_file(&store, id);
        assert!(path.symlink_metadata().unwrap().is_file(), "{id}");
        assert_eq!(&fs::read(path).unwrap(), bytes, "{id}");
    }
    assert_eq!(fs::read(bases.join(&held)).unwrap(), bytes[1]);
}

#[cfg(unix)]
#[test]
fn nothing_is_deleted_while_a_live_basis_file_links_to_bytes_of_another_id() {
    for (target, content) in [("other.json", Some(b"{}")), ("missing.json", None)] {
        let dir = tempfile::tempdir().unwrap();
        let store = new_store(dir.path());
        let mut tx = store.begin().unwrap();
        let live = tx.save_basis(&basis("/work/repo", 'a'), at(0)).unwrap();
        let unused = tx.save_basis(&basis("/work/repo", 'b'), at(0)).unwrap();
        tx.select(Path::new("/work/repo"), &live);
        tx.commit().unwrap();
        clean(&store, at(GRACE_SECONDS + 1));
        let bases = store.root().join("bases");
        if let Some(content) = content {
            fs::write(bases.join(target), content).unwrap();
        }
        link_basis(&store, &live, target);
        fs::write(bases.join(stray("json")), b"{").unwrap();

        clean(&store, at(GRACE_SECONDS + 2 + REMOVAL_DELAY_SECONDS));

        // Neither the leftover nor the due removed basis's file is
        // deleted, and the link stays.
        assert!(bases.join(stray("json")).exists(), "{target}");
        assert!(basis_file(&store, &unused).exists(), "{target}");
        assert!(
            store
                .snapshot()
                .unwrap()
                .manifest()
                .removed
                .contains_key(&unused),
            "{target}"
        );
        assert!(
            basis_file(&store, &live)
                .symlink_metadata()
                .unwrap()
                .is_symlink(),
            "{target}"
        );
    }
}

#[cfg(unix)]
#[test]
fn cleanup_never_deletes_a_symlink_or_through_one() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let store = new_store(dir.path());
    let original = basis("/work/repo", 'a');
    let mut tx = store.begin().unwrap();
    let live = tx.save_basis(&original, at(0)).unwrap();
    let unused = tx.save_basis(&basis("/work/repo", 'b'), at(0)).unwrap();
    tx.select(Path::new("/work/repo"), &live);
    tx.commit().unwrap();
    clean(&store, at(GRACE_SECONDS + 1));
    // A removed basis whose file is a link to a file outside the store,
    // and a leftover link.
    let bases = store.root().join("bases");
    let outside = dir.path().join("outside.json");
    fs::copy(basis_file(&store, &unused), &outside).unwrap();
    link_basis(&store, &unused, "../../outside.json");
    symlink(file_name(&live), bases.join(stray("json"))).unwrap();

    clean(&store, at(GRACE_SECONDS + 2 + REMOVAL_DELAY_SECONDS));
    // The links stay, and so does what they lead to; the removal is done.
    for link in [basis_file(&store, &unused), bases.join(stray("json"))] {
        assert!(link.symlink_metadata().unwrap().is_symlink());
    }
    assert!(outside.exists());
    assert!(store.snapshot().unwrap().manifest().removed.is_empty());

    // `bases` itself a link: no leftover in what it leads to is deleted.
    let elsewhere = dir.path().join("elsewhere");
    fs::rename(&bases, &elsewhere).unwrap();
    symlink(&elsewhere, &bases).unwrap();
    fs::write(elsewhere.join(stray(TEMPORARY)), b"{").unwrap();
    clean(&store, at(GRACE_SECONDS + 3 + REMOVAL_DELAY_SECONDS));
    assert!(elsewhere.join(stray(TEMPORARY)).exists());
    assert_eq!(store.snapshot().unwrap().basis(&live).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn nothing_is_deleted_while_the_manifest_or_the_lock_is_a_symlink() {
    for entry in ["manifest.json", ".lock"] {
        let dir = tempfile::tempdir().unwrap();
        let store = new_store(dir.path());
        save(&store, &basis("/work/repo", 'a'), at(0));
        // The entry moved into `bases/` under a name the store writes
        // there, with a link to it in its place.
        let moved = store.root().join("bases").join(stray("json"));
        fs::rename(store.root().join(entry), &moved).unwrap();
        std::os::unix::fs::symlink(
            Path::new("bases").join(stray("json")),
            store.root().join(entry),
        )
        .unwrap();

        // Another review: its write saves, cleans, and commits.
        let mut tx = store.begin().unwrap();
        let other = tx.save_basis(&basis("/work/repo", 'b'), at(1)).unwrap();
        tx.cleanup(at(1)).unwrap();
        tx.commit().unwrap();

        assert!(moved.exists(), "{entry}");
        assert!(
            store
                .snapshot()
                .unwrap()
                .manifest()
                .bases
                .contains_key(&other),
            "{entry}"
        );
    }
}

#[test]
fn selections_are_persisted_per_worktree() {
    let dir = tempfile::tempdir().unwrap();
    let store = new_store(dir.path());
    let mut tx = store.begin().unwrap();
    let first = tx.save_basis(&basis("/work/repo", 'a'), at(0)).unwrap();
    let second = tx.save_basis(&basis("/work/wt", 'b'), at(0)).unwrap();
    tx.select(Path::new("/work/repo"), &first);
    tx.select(Path::new("/work/wt"), &first);
    tx.select(Path::new("/work/wt"), &second);
    assert_eq!(tx.manifest().selections.len(), 2);
    tx.commit().unwrap();

    let manifest = store.snapshot().unwrap().manifest().clone();
    assert_eq!(manifest.selections[Path::new("/work/repo")], first);
    assert_eq!(manifest.selections[Path::new("/work/wt")], second);
}

#[test]
fn an_unused_basis_is_removed_after_14_days_and_deleted_a_day_later() {
    let dir = tempfile::tempdir().unwrap();
    let store = new_store(dir.path());
    let id = save(&store, &basis("/work/repo", 'a'), at(0));
    let path = basis_file(&store, &id);
    let reader = store.snapshot().unwrap();

    clean(&store, at(GRACE_SECONDS));
    assert!(store.snapshot().unwrap().manifest().bases.contains_key(&id));

    let removal = at(GRACE_SECONDS + 1);
    clean(&store, removal);
    let manifest = store.snapshot().unwrap().manifest().clone();
    assert!(!manifest.bases.contains_key(&id));
    assert_eq!(manifest.removed[&id], removal);
    // The published removal leaves the file for readers of older manifests.
    assert!(path.exists());
    reader.basis(&id).unwrap();
    assert!(matches!(
        store.snapshot().unwrap().basis(&id),
        Err(StoreError::NoSuchBasis(_))
    ));

    clean(&store, at(GRACE_SECONDS + 1 + REMOVAL_DELAY_SECONDS));
    assert!(path.exists());
    assert_eq!(store.snapshot().unwrap().manifest().removed.len(), 1);

    clean(&store, at(GRACE_SECONDS + 2 + REMOVAL_DELAY_SECONDS));
    assert!(!path.exists());
    assert!(store.snapshot().unwrap().manifest().removed.is_empty());
}

#[test]
fn a_removal_is_never_deleted_by_the_write_that_publishes_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = new_store(dir.path());
    let id = save(&store, &basis("/work/repo", 'a'), at(0));

    let mut tx = store.begin().unwrap();
    tx.cleanup(at(GRACE_SECONDS + 1)).unwrap();
    tx.cleanup(at(GRACE_SECONDS + 10 * REMOVAL_DELAY_SECONDS))
        .unwrap();
    assert!(tx.manifest().removed.contains_key(&id));
    tx.commit().unwrap();
    assert!(basis_file(&store, &id).exists());
}

#[test]
fn a_selected_basis_is_kept_until_another_review_is_selected() {
    let dir = tempfile::tempdir().unwrap();
    let store = new_store(dir.path());
    let mut tx = store.begin().unwrap();
    let old = tx.save_basis(&basis("/work/repo", 'a'), at(0)).unwrap();
    tx.select(Path::new("/work/repo"), &old);
    tx.commit().unwrap();

    let later = at(30 * 86_400);
    clean(&store, later);
    assert!(
        store
            .snapshot()
            .unwrap()
            .manifest()
            .bases
            .contains_key(&old)
    );

    let mut tx = store.begin().unwrap();
    let new = tx.save_basis(&basis("/work/repo", 'b'), later).unwrap();
    tx.select(Path::new("/work/repo"), &new);
    tx.cleanup(later).unwrap();
    tx.commit().unwrap();
    let manifest = store.snapshot().unwrap().manifest().clone();
    assert_eq!(manifest.removed[&old], later);
    assert!(manifest.bases.contains_key(&new));
}

#[test]
fn a_basis_listed_live_is_never_deleted_whatever_removed_says() {
    let dir = tempfile::tempdir().unwrap();
    let store = new_store(dir.path());
    let original = basis("/work/repo", 'a');
    let reviewed = at(10 * 86_400);
    let mut tx = store.begin().unwrap();
    let live = tx.save_basis(&original, reviewed).unwrap();
    tx.select(Path::new("/work/repo"), &live);
    tx.commit().unwrap();
    // Also listed as removed ten days earlier, as no writer of this build
    // leaves a manifest.
    edit_manifest(&store, |m| {
        m["removed"][live.as_str()] = json!("2026-10-13T00:00:00Z");
    });

    // Another review is selected in its worktree, so no selection names it.
    let now = at(10 * 86_400 + 60);
    let mut tx = store.begin().unwrap();
    let other = tx.save_basis(&basis("/work/repo", 'b'), now).unwrap();
    tx.select(Path::new("/work/repo"), &other);
    tx.cleanup(now).unwrap();
    tx.commit().unwrap();

    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.manifest().bases[&live].last_reviewed, reviewed);
    assert!(!snapshot.manifest().removed.contains_key(&live));
    assert_eq!(snapshot.basis(&live).unwrap(), original);
}

#[test]
fn saving_a_removed_basis_revives_it_before_and_after_its_file_is_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let store = new_store(dir.path());
    let original = basis("/work/repo", 'a');
    let id = save(&store, &original, at(0));
    let path = basis_file(&store, &id);
    let removal = at(GRACE_SECONDS + 1);

    // Before deletion: the file is still there.
    clean(&store, removal);
    let revived = at(GRACE_SECONDS + 3_600);
    let mut tx = store.begin().unwrap();
    tx.save_basis(&original, revived).unwrap();
    tx.cleanup(revived).unwrap();
    tx.commit().unwrap();
    let manifest = store.snapshot().unwrap().manifest().clone();
    assert!(manifest.removed.is_empty());
    assert_eq!(
        manifest.bases[&id],
        BasisTimes {
            first_written: revived,
            last_reviewed: revived
        }
    );
    assert_eq!(store.snapshot().unwrap().basis(&id).unwrap(), original);

    // Still listed as removed, but its file is gone (a deletion whose
    // manifest was never published): saving writes the file again.
    clean(&store, at(3 * GRACE_SECONDS));
    assert!(
        store
            .snapshot()
            .unwrap()
            .manifest()
            .removed
            .contains_key(&id)
    );
    fs::remove_file(&path).unwrap();
    save(&store, &original, at(3 * GRACE_SECONDS + 60));
    assert!(store.snapshot().unwrap().manifest().removed.is_empty());
    assert_eq!(store.snapshot().unwrap().basis(&id).unwrap(), original);

    // After deletion: the basis is saved as new.
    clean(&store, at(5 * GRACE_SECONDS));
    clean(&store, at(6 * GRACE_SECONDS));
    assert!(!path.exists());
    let manifest = store.snapshot().unwrap().manifest().clone();
    assert!(manifest.removed.is_empty() && manifest.bases.is_empty());
    let saved = at(7 * GRACE_SECONDS);
    save(&store, &original, saved);
    assert_eq!(
        store.snapshot().unwrap().manifest().bases[&id],
        BasisTimes {
            first_written: saved,
            last_reviewed: saved
        }
    );
    assert_eq!(store.snapshot().unwrap().basis(&id).unwrap(), original);
}

#[test]
fn a_review_is_found_by_a_unique_prefix_of_at_least_8_digits() {
    let dir = tempfile::tempdir().unwrap();
    let store = new_store(dir.path());
    let id = save(&store, &basis("/work/repo", 'a'), at(0));
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.find_basis(id.as_str()).unwrap(), id);
    assert_eq!(snapshot.find_basis(id.short()).unwrap(), id);
    assert_eq!(
        snapshot
            .find_basis(&id.as_str()[..12].to_uppercase())
            .unwrap(),
        id
    );

    for invalid in [&id.as_str()[..7], "zzzzzzzz", ""] {
        assert!(
            matches!(
                snapshot.find_basis(invalid),
                Err(StoreError::InvalidBasisPrefix(prefix)) if prefix == invalid
            ),
            "{invalid}"
        );
    }
    let unknown = if id.as_str().starts_with('0') {
        "10000000"
    } else {
        "00000000"
    };
    assert!(matches!(
        snapshot.find_basis(unknown),
        Err(StoreError::NoSuchBasis(prefix)) if prefix == unknown
    ));

    // Two reviews sharing the first 8 digits need a longer prefix.
    let one = format!("abcdef01{}", "1".repeat(56));
    let two = format!("abcdef01{}", "2".repeat(56));
    edit_manifest(&store, |m| {
        for fake in [&one, &two] {
            m["bases"][fake] = json!({
                "first_written": "2026-10-13T00:00:00Z",
                "last_reviewed": "2026-10-13T00:00:00Z"
            });
        }
    });
    let snapshot = store.snapshot().unwrap();
    let error = snapshot.find_basis("abcdef01").unwrap_err();
    let StoreError::AmbiguousBasis { prefix, candidates } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(prefix, "abcdef01");
    let candidates: Vec<&str> = candidates.iter().map(BasisId::as_str).collect();
    assert_eq!(candidates, [one.as_str(), two.as_str()]);
    assert!(error.to_string().contains(&one), "{error}");
    assert_eq!(snapshot.find_basis("abcdef012").unwrap().as_str(), two);

    // A removed review is not found.
    clean(&store, at(GRACE_SECONDS + 1));
    assert!(matches!(
        store.snapshot().unwrap().find_basis(id.as_str()),
        Err(StoreError::NoSuchBasis(_))
    ));
}
