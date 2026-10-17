use std::collections::BTreeMap;

use super::*;
use crate::JudgmentId;
use crate::files::Step;
use crate::reviews::{
    Algorithms, BASIS_SCHEMA, BasisBase, BasisScope, BasisTarget, JudgmentKind, PathBytes,
    TargetKind, Who,
};

fn at(seconds: i64) -> UtcTime {
    UtcTime::from_unix(1_791_849_600 + seconds)
}

fn store(dir: &Path) -> ReviewStore {
    ReviewStore::create(&dir.join("store"), Path::new("/work/repo/.git")).unwrap()
}

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

/// The syncs a saved basis file needs: the file, the store and `bases/`.
fn basis_syncs<'a>(root: &'a Path, file: &'a Path) -> [Step<'a>; 3] {
    [
        Step::File(file),
        Step::Dir(root),
        Step::Dir(file.parent().unwrap()),
    ]
}

#[test]
fn a_new_basis_file_is_synced_with_its_directories() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let mut tx = store.begin().unwrap();
    let id = tx.save_basis(&basis(), at(0)).unwrap();
    let file = store.root().join(BASES_DIR).join(basis_file_name(&id));
    assert_eq!(tx.publication.syncs(), basis_syncs(store.root(), &file));
}

#[test]
fn a_reused_basis_file_is_synced_with_its_directories() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let mut tx = store.begin().unwrap();
    tx.save_basis(&basis(), at(0)).unwrap();
    drop(tx);

    // A crash may have left the file renamed into place but not durable.
    let mut tx = store.begin().unwrap();
    let id = tx.save_basis(&basis(), at(1)).unwrap();
    let file = store.root().join(BASES_DIR).join(basis_file_name(&id));
    assert_eq!(tx.publication.syncs(), basis_syncs(store.root(), &file));
}

#[test]
fn an_appended_judgment_syncs_the_log_and_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let mut tx = store.begin().unwrap();
    let id = tx.save_basis(&basis(), at(0)).unwrap();
    tx.commit(at(0)).unwrap();

    let mut tx = store.begin().unwrap();
    tx.append_judgment(&Judgment {
        id: JudgmentId::generate(),
        basis: id,
        kind: JudgmentKind::Acknowledge,
        items: Vec::new(),
        reason: None,
        text: None,
        closes: BTreeMap::new(),
        withdraws: Vec::new(),
        who: Who {
            name: "Ada Lovelace".to_owned(),
            email: "ada@example.com".to_owned(),
        },
        at: at(1),
    })
    .unwrap();
    let root = store.root();
    assert_eq!(
        tx.publication.syncs(),
        [Step::File(&root.join(JUDGMENTS_FILE)), Step::Dir(root)]
    );
}

#[test]
fn a_cleanup_deletion_syncs_the_bases_directory() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let mut tx = store.begin().unwrap();
    tx.save_basis(&basis(), at(0)).unwrap();
    // Dropped before its commit, the transaction leaves its basis file.
    drop(tx);

    let mut tx = store.begin().unwrap();
    tx.cleanup(at(1)).unwrap();
    let root = store.root();
    assert_eq!(fs::read_dir(root.join(BASES_DIR)).unwrap().count(), 0);
    assert_eq!(
        tx.publication.syncs(),
        [Step::Dir(root), Step::Dir(&root.join(BASES_DIR))]
    );
}

#[test]
fn a_write_with_nothing_on_disk_to_change_syncs_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let mut tx = store.begin().unwrap();
    tx.select(Path::new("/work/repo"), &BasisId::of(b"{}"));
    tx.cleanup(at(0)).unwrap();
    assert_eq!(tx.publication.syncs(), []);
}
