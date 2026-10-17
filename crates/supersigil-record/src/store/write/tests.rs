use std::path::PathBuf;

use super::*;
use crate::Timestamp;
use crate::derivations::ALGORITHM_VERSION;
use crate::files::Step;
use crate::observations::{EndReason, SessionEnd};

fn store(dir: &Path) -> Store {
    Store::create(
        dir,
        manifest::Association {
            checkout: PathBuf::from("/work/repo"),
        },
    )
    .unwrap()
}

fn at(seconds: i64) -> UtcTime {
    UtcTime::from_unix(1_791_849_600 + seconds)
}

fn derivations() -> DerivationSet {
    DerivationSet {
        session: SessionId::new("s1"),
        observation_revision: Revision::ZERO,
        algorithm_version: ALGORITHM_VERSION,
        restores: Vec::new(),
        discontinuities: Vec::new(),
    }
}

#[test]
fn appended_logs_and_written_documents_are_synced_with_their_directories() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let root = store.root();
    let mut tx = store.begin_at(at(0)).unwrap();
    tx.append_observations(&[Observation::SessionEnd(SessionEnd {
        session: SessionId::new("s1"),
        time: Timestamp::new("2026-09-28T10:00:00.000Z"),
        reason: EndReason::Unknown,
    })])
    .unwrap();
    tx.put_derivations(&derivations()).unwrap();

    let log = root.join(log_path(0));
    let document = root.join(DERIVATIONS_DIR).join("0.r1.json");
    assert_eq!(
        tx.publication.syncs(),
        [
            Step::File(&document),
            Step::File(&log),
            Step::Dir(root),
            Step::Dir(&root.join(DERIVATIONS_DIR)),
            Step::Dir(log.parent().unwrap()),
        ]
    );
}

#[test]
fn deleting_a_superseded_document_syncs_its_directory() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    for now in [at(0), at(60)] {
        let mut tx = store.begin_at(now).unwrap();
        tx.put_derivations(&derivations()).unwrap();
        tx.commit().unwrap();
    }
    let root = store.root();
    let replaced = root.join(DERIVATIONS_DIR).join("0.r1.json");

    let mut tx = store.begin_at(at(61 + GRACE_SECONDS)).unwrap();
    tx.delete_superseded();
    assert!(!replaced.exists());
    assert_eq!(
        tx.publication.syncs(),
        [Step::Dir(root), Step::Dir(&root.join(DERIVATIONS_DIR))]
    );
}
