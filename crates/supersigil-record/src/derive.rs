//! Computes derivations from a session's observations.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::derivations::{ALGORITHM_VERSION, DerivationSet, Discontinuity, Restore};
use crate::ids::{ContentId, EventId, Revision, SessionId};
use crate::observations::{Edit, Observation};

/// Derives restores and discontinuities from `observations`.
///
/// Edits are grouped by checkout and path and ordered by `source_ordinal`.
/// A claim is made only where both content ids involved are known.
///
/// Each event counts once: a transcript read again from the start appends
/// its edits again under the same ids, and only the first record of an
/// [`EventId`] in log order is used. Without this, a replayed edit would
/// look like a gap between an edit and itself.
#[must_use]
pub fn derive(
    session: &SessionId,
    observations: &[Observation],
    observation_revision: Revision,
) -> DerivationSet {
    let mut seen: BTreeSet<&EventId> = BTreeSet::new();
    let mut groups: BTreeMap<(PathBuf, PathBuf), Vec<&Edit>> = BTreeMap::new();
    for observation in observations {
        if let Observation::Edit(edit) = observation
            && edit.session == *session
            && seen.insert(&edit.id)
        {
            groups
                .entry((edit.checkout.clone(), edit.path.clone()))
                .or_default()
                .push(edit);
        }
    }

    let mut restores = Vec::new();
    let mut discontinuities = Vec::new();
    for ((checkout, path), mut edits) in groups {
        edits.sort_by_key(|e| e.source_ordinal);
        // Known before-contents of the edits seen so far in this group, each
        // with its edits oldest first.
        let mut befores: BTreeMap<&ContentId, Vec<EventId>> = BTreeMap::new();
        for edit in &edits {
            let earlier = edit
                .after_content()
                .filter(|after| edit.before_content() != Some(after))
                .and_then(|after| befores.get(after));
            if let Some(earlier) = earlier {
                restores.push(Restore {
                    edit: edit.id.clone(),
                    restores: earlier.clone(),
                });
            }
            if let Some(before) = edit.before_content() {
                befores.entry(before).or_default().push(edit.id.clone());
            }
        }
        for pair in edits.windows(2) {
            let (prev, next) = (pair[0], pair[1]);
            if let (Some(a), Some(b)) = (prev.after_content(), next.before_content())
                && a != b
            {
                discontinuities.push(Discontinuity {
                    path: path.clone(),
                    checkout: checkout.clone(),
                    prev: prev.id.clone(),
                    next: next.id.clone(),
                });
            }
        }
    }
    restores.sort_by(|a, b| a.edit.cmp(&b.edit));

    DerivationSet {
        session: session.clone(),
        observation_revision,
        algorithm_version: ALGORITHM_VERSION,
        restores,
        discontinuities,
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::ids::{Timestamp, TurnId};
    use crate::observations::{FileState, Material};

    fn session() -> SessionId {
        SessionId::new("s1")
    }

    fn edit(
        tool: &str,
        path: &str,
        ordinal: u64,
        before: FileState,
        after: FileState,
    ) -> Observation {
        Observation::Edit(Edit {
            id: EventId::derive("edit", &session(), tool),
            turn: TurnId::new(format!("a{ordinal}")),
            session: session(),
            path: PathBuf::from(path),
            before,
            after,
            patch: Material::unavailable("test"),
            old_text: Material::unavailable("test"),
            new_text: Material::unavailable("test"),
            replace_all: false,
            checkout: PathBuf::from("/work/repo"),
            time: Timestamp::new(format!("2026-09-28T10:00:{ordinal:02}.000Z")),
            source_ordinal: ordinal,
        })
    }

    fn known(text: &str) -> FileState {
        FileState::known(ContentId::of(text.as_bytes()))
    }

    fn id(tool: &str) -> EventId {
        EventId::derive("edit", &session(), tool)
    }

    #[test]
    fn edit_back_to_earlier_content_is_a_restore() {
        let obs = vec![
            edit("t1", "notes.txt", 1, known("draft\n"), known("final\n")),
            edit("t2", "notes.txt", 2, known("final\n"), known("draft\n")),
        ];
        let set = derive(&session(), &obs, Revision::ZERO.next());
        assert_eq!(
            set.restores,
            vec![Restore {
                edit: id("t2"),
                restores: vec![id("t1")]
            }]
        );
        assert!(set.discontinuities.is_empty());
        assert_eq!(set.observation_revision, Revision::ZERO.next());
        assert_eq!(set.algorithm_version, ALGORITHM_VERSION);
    }

    #[test]
    fn mismatched_known_states_are_a_discontinuity() {
        let obs = vec![
            edit("t1", "src/lib.rs", 1, known("a\n"), known("b\n")),
            edit(
                "t2",
                "src/lib.rs",
                2,
                known("// manual\nb\n"),
                known("// manual\nc\n"),
            ),
        ];
        let set = derive(&session(), &obs, Revision::ZERO.next());
        assert_eq!(
            set.discontinuities,
            vec![Discontinuity {
                path: PathBuf::from("src/lib.rs"),
                checkout: PathBuf::from("/work/repo"),
                prev: id("t1"),
                next: id("t2"),
            }]
        );
        assert!(set.restores.is_empty());
    }

    #[test]
    fn unknown_content_makes_no_claim() {
        let obs = vec![
            edit("t1", "src/lib.rs", 1, known("a\n"), FileState::unknown()),
            edit("t2", "src/lib.rs", 2, known("z\n"), known("y\n")),
            edit("t3", "src/lib.rs", 3, FileState::unknown(), known("a\n")),
        ];
        let set = derive(&session(), &obs, Revision::ZERO.next());
        assert!(set.discontinuities.is_empty());
        // t3 lands on "a\n", which was t1's before-state: that is a restore
        // because both of those hashes are known.
        assert_eq!(
            set.restores,
            vec![Restore {
                edit: id("t3"),
                restores: vec![id("t1")]
            }]
        );
    }

    #[test]
    fn different_paths_and_checkouts_do_not_interact() {
        let mut other = edit("t2", "b.txt", 2, known("x\n"), known("y\n"));
        if let Observation::Edit(e) = &mut other {
            e.checkout = PathBuf::from("/work/other");
        }
        let obs = vec![
            edit("t1", "a.txt", 1, known("x\n"), known("y\n")),
            other,
            edit("t3", "a.txt", 3, known("q\n"), known("x\n")),
        ];
        let set = derive(&session(), &obs, Revision::ZERO.next());
        assert_eq!(
            set.restores,
            vec![Restore {
                edit: id("t3"),
                restores: vec![id("t1")]
            }]
        );
        assert_eq!(set.discontinuities.len(), 1);
        assert_eq!(set.discontinuities[0].next, id("t3"));
    }

    #[test]
    fn no_op_edit_is_not_a_restore_and_creation_after_absence_is_continuous() {
        let obs = vec![
            edit("t1", "a.txt", 1, known("x\n"), known("x\n")),
            edit("t2", "new.rs", 2, FileState::Absent, known("v\n")),
        ];
        let set = derive(&session(), &obs, Revision::ZERO.next());
        assert!(set.restores.is_empty());
        assert!(set.discontinuities.is_empty());
    }

    #[test]
    fn edits_from_other_sessions_are_ignored() {
        let mut obs = vec![
            edit("t1", "notes.txt", 1, known("draft\n"), known("final\n")),
            edit("t2", "notes.txt", 2, known("final\n"), known("draft\n")),
        ];
        if let Observation::Edit(e) = &mut obs[1] {
            e.session = SessionId::new("other");
        }
        let set = derive(&session(), &obs, Revision::ZERO.next());
        assert!(set.restores.is_empty());
        assert!(set.discontinuities.is_empty());
        assert_eq!(set.session, session());
    }

    #[test]
    fn replayed_edits_are_counted_once() {
        let obs = vec![
            edit("t1", "notes.txt", 1, known("draft\n"), known("final\n")),
            edit("t2", "notes.txt", 2, known("final\n"), known("draft\n")),
            // A re-read transcript appends the first edit again.
            edit("t1", "notes.txt", 1, known("draft\n"), known("final\n")),
        ];
        let set = derive(&session(), &obs, Revision::ZERO.next());
        assert_eq!(
            set.restores,
            vec![Restore {
                edit: id("t2"),
                restores: vec![id("t1")]
            }]
        );
        assert!(set.discontinuities.is_empty());

        // A replay whose payload differs is still the same event; the first
        // record of it wins.
        let mut replay = edit("t1", "notes.txt", 1, known("x\n"), known("y\n"));
        if let Observation::Edit(e) = &mut replay {
            e.source_ordinal = 3;
        }
        let obs = vec![
            edit("t1", "notes.txt", 1, known("draft\n"), known("final\n")),
            replay,
        ];
        let set = derive(&session(), &obs, Revision::ZERO.next());
        assert!(set.restores.is_empty());
        assert!(set.discontinuities.is_empty());
    }
}
