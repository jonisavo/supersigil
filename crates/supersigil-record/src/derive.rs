//! Compares a session's edits to find restores and discontinuities.

use std::collections::{BTreeMap, BTreeSet};

use crate::derivations::{ALGORITHM_VERSION, DerivationSet, Discontinuity, Restore};
use crate::ids::{ContentId, EventId, Revision, SessionId};
use crate::observations::{Content, Edit, FileState, Observation};

/// Finds restores and discontinuities among the edits belonging to `session`.
///
/// Keeps the first occurrence of each edit ID in `observations`, then groups
/// edits by transcript and file `key`, regardless of checkout and path
/// spelling. Excludes edits whose key is `None`. Edits without a transcript
/// path form a separate group for each key. Sorts each group by
/// [`Edit::source_ordinal`], ignoring timestamps. Equal ordinals identify edits
/// issued by one record, such as parallel tool calls, with no recorded order.
///
/// Within each group:
///
/// - A restore matches an edit's after-content hash to the before-content
///   hash of an edit with a lower ordinal. Both hashes must be known. An edit
///   with identical before and after hashes is not reported as a restore.
/// - A discontinuity compares one edit's after-state with the next edit's
///   before-state. Different known hashes, or a known hash paired with an
///   absent file, count as a mismatch. Unknown content prevents comparison,
///   and so does an edit that shares its ordinal, since which edit came
///   next is unknown. The discontinuity names the next edit's checkout and
///   path.
///
/// Copies `observation_revision` into the result to identify the input revision.
/// Comparisons use only the supplied observations and cannot establish that
/// every file change was recorded.
#[must_use]
pub fn derive<K: Ord>(
    session: &SessionId,
    observations: &[Observation],
    observation_revision: Revision,
    key: impl Fn(&Edit) -> Option<K>,
) -> DerivationSet {
    let mut restores = Vec::new();
    let mut discontinuities = Vec::new();
    for group in groups(session, observations, key) {
        let issued = issued(&group);
        find_restores(&issued, &mut restores);
        find_discontinuities(&issued, &mut discontinuities);
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

/// Finds the same discontinuities as [`derive()`] for `session`, without
/// computing restores.
///
/// Repeated restores can name quadratically many earlier edits. This function
/// skips that work when callers need only gaps. Accepts observations in any
/// order, including observations from several records, without copying them.
#[must_use]
pub fn discontinuities<'a, K: Ord>(
    session: &SessionId,
    observations: impl IntoIterator<Item = &'a Observation>,
    key: impl Fn(&Edit) -> Option<K>,
) -> Vec<Discontinuity> {
    let mut found = Vec::new();
    for group in groups(session, observations, key) {
        find_discontinuities(&issued(&group), &mut found);
    }
    found
}

/// Keeps the first occurrence of each edit ID for `session`, groups by `key`
/// and transcript, and sorts each group by ordinal, as [`derive()`] requires.
fn groups<'a, K: Ord>(
    session: &SessionId,
    observations: impl IntoIterator<Item = &'a Observation>,
    key: impl Fn(&Edit) -> Option<K>,
) -> Vec<Vec<&'a Edit>> {
    let mut seen: BTreeSet<&EventId> = BTreeSet::new();
    let mut groups: BTreeMap<(K, Option<&str>), Vec<&Edit>> = BTreeMap::new();
    for observation in observations {
        if let Observation::Edit(edit) = observation
            && edit.session == *session
            && seen.insert(&edit.id)
            && let Some(file) = key(edit)
        {
            groups
                .entry((file, edit.transcript.as_deref()))
                .or_default()
                .push(edit);
        }
    }
    groups
        .into_values()
        .map(|mut edits| {
            edits.sort_by_key(|e| e.source_ordinal);
            edits
        })
        .collect()
}

/// Splits the ordinal-sorted `group` into slices of edits with equal ordinals.
fn issued<'g, 'a>(group: &'g [&'a Edit]) -> Vec<&'g [&'a Edit]> {
    group
        .chunk_by(|a, b| a.source_ordinal == b.source_ordinal)
        .collect()
}

/// Appends restores from `issued`, a group's edits split by ordinal in
/// ascending order (see [`derive()`]).
fn find_restores(issued: &[&[&Edit]], restores: &mut Vec<Restore>) {
    // Maps known before-content hashes to edits with lower ordinals, oldest first.
    let mut befores: BTreeMap<&ContentId, Vec<EventId>> = BTreeMap::new();
    for &together in issued {
        for edit in together {
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
        }
        for edit in together {
            if let Some(before) = edit.before_content() {
                befores.entry(before).or_default().push(edit.id.clone());
            }
        }
    }
}

/// Appends discontinuities from `issued`, a group's edits split by ordinal
/// in ascending order (see [`derive()`]).
fn find_discontinuities(issued: &[&[&Edit]], discontinuities: &mut Vec<Discontinuity>) {
    for pair in issued.windows(2) {
        let (&[prev], &[next]) = (pair[0], pair[1]) else {
            continue;
        };
        if let (Some(a), Some(b)) = (known_state(&prev.after), known_state(&next.before))
            && a != b
        {
            discontinuities.push(Discontinuity {
                path: next.path.clone(),
                checkout: next.checkout.clone(),
                prev: prev.id.clone(),
                next: next.id.clone(),
            });
        }
    }
}

/// Returns `state` if the file is absent or has a known content hash.
fn known_state(state: &FileState) -> Option<&FileState> {
    match state {
        FileState::Present {
            content: Content::Unknown,
        } => None,
        FileState::Absent
        | FileState::Present {
            content: Content::Known(_),
        } => Some(state),
    }
}

#[cfg(test)]
mod tests {

    use std::path::PathBuf;

    use super::*;
    use crate::ids::{Timestamp, TurnId};
    use crate::observations::{EditOperation, EditOrigin, Material};

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
            operation: EditOperation::Replace,
            origin: EditOrigin::Tool,
            checkout: PathBuf::from("/work/repo"),
            time: Timestamp::new(format!("2026-09-28T10:00:{ordinal:02}.000Z")),
            source_ordinal: ordinal,
            agent_id: None,
            transcript: None,
        })
    }

    fn known(text: &str) -> FileState {
        FileState::known(ContentId::of(text.as_bytes()))
    }

    fn id(tool: &str) -> EventId {
        EventId::derive("edit", &session(), tool)
    }

    /// Moves an edit observation to `checkout`.
    fn in_checkout(mut observation: Observation, checkout: &str) -> Observation {
        if let Observation::Edit(e) = &mut observation {
            e.checkout = PathBuf::from(checkout);
        }
        observation
    }

    /// Keys each edit by its checkout and path as recorded.
    fn as_recorded() -> impl Fn(&Edit) -> Option<(PathBuf, PathBuf)> {
        |edit| Some((edit.checkout.clone(), edit.path.clone()))
    }

    /// Keys each edit by the file it names: its checkout joined with its path.
    fn joined() -> impl Fn(&Edit) -> Option<PathBuf> {
        |edit| Some(edit.checkout.join(&edit.path))
    }

    /// Edits `sub/a.rs` from `/r`, then `a.rs` from `/r/sub` at `ordinal`.
    /// The second edit starts from different content and restores the first's
    /// before-content.
    fn after_a_directory_change(ordinal: u64) -> Vec<Observation> {
        vec![
            in_checkout(edit("t1", "sub/a.rs", 1, known("a\n"), known("b\n")), "/r"),
            in_checkout(
                edit("t2", "a.rs", ordinal, known("z\n"), known("a\n")),
                "/r/sub",
            ),
        ]
    }

    #[test]
    fn edits_of_one_file_under_two_spellings_are_compared() {
        let set = derive(
            &session(),
            &after_a_directory_change(2),
            Revision::ZERO.next(),
            joined(),
        );
        assert_eq!(
            set.discontinuities,
            vec![Discontinuity {
                path: PathBuf::from("a.rs"),
                checkout: PathBuf::from("/r/sub"),
                prev: id("t1"),
                next: id("t2"),
            }]
        );
        assert_eq!(
            set.restores,
            vec![Restore {
                edit: id("t2"),
                restores: vec![id("t1")]
            }]
        );

        // Keyed as recorded, the two spellings are two files.
        let set = derive(
            &session(),
            &after_a_directory_change(2),
            Revision::ZERO.next(),
            as_recorded(),
        );
        assert_eq!(set.discontinuities, []);
        assert_eq!(set.restores, []);
    }

    #[test]
    fn edits_without_a_key_are_not_compared() {
        let no_key = |_: &Edit| None::<PathBuf>;
        let set = derive(
            &session(),
            &after_a_directory_change(2),
            Revision::ZERO.next(),
            no_key,
        );
        assert_eq!(set.discontinuities, []);
        assert_eq!(set.restores, []);

        // An edit without a key is not compared with keyed edits either.
        let only_first = |e: &Edit| (e.id == id("t1")).then(|| e.checkout.join(&e.path));
        let set = derive(
            &session(),
            &after_a_directory_change(2),
            Revision::ZERO.next(),
            only_first,
        );
        assert_eq!(set.discontinuities, []);
        assert_eq!(set.restores, []);
    }

    #[test]
    fn edits_sharing_an_ordinal_under_two_spellings_are_not_compared() {
        let set = derive(
            &session(),
            &after_a_directory_change(1),
            Revision::ZERO.next(),
            joined(),
        );
        assert!(set.discontinuities.is_empty(), "{:?}", set.discontinuities);
        assert!(set.restores.is_empty(), "{:?}", set.restores);
    }

    #[test]
    fn edit_back_to_earlier_content_is_a_restore() {
        let obs = vec![
            edit("t1", "notes.txt", 1, known("draft\n"), known("final\n")),
            edit("t2", "notes.txt", 2, known("final\n"), known("draft\n")),
        ];
        let set = derive(&session(), &obs, Revision::ZERO.next(), as_recorded());
        assert_eq!(
            set.restores,
            vec![Restore {
                edit: id("t2"),
                restores: vec![id("t1")]
            }]
        );
        assert_eq!(set.discontinuities, []);
        assert_eq!(set.observation_revision, Revision::ZERO.next());
        assert_eq!(set.algorithm_version, ALGORITHM_VERSION);
    }

    #[test]
    fn edits_issued_by_one_record_make_no_claim_between_them() {
        // Two parallel edits of one file, issued by the record at ordinal 3:
        // either could have run first.
        let obs = vec![
            edit("t1", "notes.txt", 3, known("draft\n"), known("final\n")),
            edit("t2", "notes.txt", 3, known("final\n"), known("draft\n")),
            edit("t3", "src/lib.rs", 3, known("a\n"), known("b\n")),
            edit("t4", "src/lib.rs", 3, known("c\n"), known("d\n")),
        ];
        let set = derive(&session(), &obs, Revision::ZERO.next(), as_recorded());
        assert!(set.restores.is_empty(), "{:?}", set.restores);
        assert!(set.discontinuities.is_empty(), "{:?}", set.discontinuities);

        // A later edit still restores content from before both.
        let mut obs = obs;
        obs.push(edit(
            "t5",
            "notes.txt",
            4,
            known("draft\n"),
            known("final\n"),
        ));
        let set = derive(&session(), &obs, Revision::ZERO.next(), as_recorded());
        assert_eq!(
            set.restores,
            vec![Restore {
                edit: id("t5"),
                restores: vec![id("t2")]
            }]
        );
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
        let set = derive(&session(), &obs, Revision::ZERO.next(), as_recorded());
        assert_eq!(
            set.discontinuities,
            vec![Discontinuity {
                path: PathBuf::from("src/lib.rs"),
                checkout: PathBuf::from("/work/repo"),
                prev: id("t1"),
                next: id("t2"),
            }]
        );
        assert_eq!(set.restores, []);
    }

    #[test]
    fn unknown_content_makes_no_claim() {
        let obs = vec![
            edit("t1", "src/lib.rs", 1, known("a\n"), FileState::unknown()),
            edit("t2", "src/lib.rs", 2, known("z\n"), known("y\n")),
            edit("t3", "src/lib.rs", 3, FileState::unknown(), known("a\n")),
        ];
        let set = derive(&session(), &obs, Revision::ZERO.next(), as_recorded());
        assert_eq!(set.discontinuities, []);
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
        let set = derive(&session(), &obs, Revision::ZERO.next(), as_recorded());
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
    fn edits_from_different_transcripts_make_no_claim_between_them() {
        let from = |mut observation: Observation, transcript: &str| {
            if let Observation::Edit(e) = &mut observation {
                e.agent_id = Some("agent1".to_owned());
                e.transcript = Some(transcript.to_owned());
            }
            observation
        };
        let mismatched = || {
            [
                edit("t1", "src/lib.rs", 1, known("a\n"), known("b\n")),
                edit("t2", "src/lib.rs", 2, known("z\n"), known("a\n")),
            ]
        };

        // Ordinals from different transcripts share no order, even for the
        // same agent.
        let [first, second] = mismatched();
        let obs = vec![from(first, "one.jsonl"), from(second, "two.jsonl")];
        let set = derive(&session(), &obs, Revision::ZERO.next(), as_recorded());
        assert_eq!(set.discontinuities, []);
        assert_eq!(set.restores, []);

        // Within one transcript the same edits are a discontinuity and a
        // restore.
        let [first, second] = mismatched();
        let obs = vec![from(first, "one.jsonl"), from(second, "one.jsonl")];
        let set = derive(&session(), &obs, Revision::ZERO.next(), as_recorded());
        assert_eq!(set.discontinuities.len(), 1);
        assert_eq!(set.restores.len(), 1);
    }

    #[test]
    fn no_op_edit_is_not_a_restore_and_creation_after_absence_is_continuous() {
        let obs = vec![
            edit("t1", "a.txt", 1, known("x\n"), known("x\n")),
            edit("t2", "new.rs", 2, FileState::Absent, known("v\n")),
        ];
        let set = derive(&session(), &obs, Revision::ZERO.next(), as_recorded());
        assert_eq!(set.restores, []);
        assert_eq!(set.discontinuities, []);
    }

    #[test]
    fn a_file_deleted_between_edits_is_a_discontinuity() {
        let obs = vec![
            edit("t1", "a.txt", 1, known("x\n"), known("y\n")),
            edit("t2", "a.txt", 2, FileState::Absent, known("z\n")),
        ];
        let set = derive(&session(), &obs, Revision::ZERO.next(), as_recorded());
        assert_eq!(
            set.discontinuities,
            vec![Discontinuity {
                path: PathBuf::from("a.txt"),
                checkout: PathBuf::from("/work/repo"),
                prev: id("t1"),
                next: id("t2"),
            }]
        );
    }

    #[test]
    fn a_file_created_between_edits_is_a_discontinuity() {
        let obs = vec![
            edit("t1", "a.txt", 1, known("x\n"), FileState::Absent),
            edit("t2", "a.txt", 2, known("y\n"), known("z\n")),
        ];
        let set = derive(&session(), &obs, Revision::ZERO.next(), as_recorded());
        assert_eq!(set.discontinuities.len(), 1);
        assert_eq!(set.discontinuities[0].prev, id("t1"));
        assert_eq!(set.discontinuities[0].next, id("t2"));

        // Absent to absent meets, and unknown content on either side makes
        // no claim.
        let obs = vec![
            edit("t1", "a.txt", 1, known("x\n"), FileState::Absent),
            edit("t2", "a.txt", 2, FileState::Absent, known("z\n")),
            edit("t3", "a.txt", 3, FileState::unknown(), FileState::Absent),
            edit("t4", "a.txt", 4, FileState::Absent, FileState::unknown()),
            edit("t5", "a.txt", 5, FileState::Absent, known("w\n")),
        ];
        let set = derive(&session(), &obs, Revision::ZERO.next(), as_recorded());
        assert!(set.discontinuities.is_empty(), "{:?}", set.discontinuities);
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
        let set = derive(&session(), &obs, Revision::ZERO.next(), as_recorded());
        assert_eq!(set.restores, []);
        assert_eq!(set.discontinuities, []);
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
        let set = derive(&session(), &obs, Revision::ZERO.next(), as_recorded());
        assert_eq!(
            set.restores,
            vec![Restore {
                edit: id("t2"),
                restores: vec![id("t1")]
            }]
        );
        assert_eq!(set.discontinuities, []);

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
        let set = derive(&session(), &obs, Revision::ZERO.next(), as_recorded());
        assert_eq!(set.restores, []);
        assert_eq!(set.discontinuities, []);
    }

    /// Alternates one file between two contents for `count` edits, then adds
    /// an edit that starts from different content.
    fn back_and_forth(count: u64) -> Vec<Observation> {
        let mut obs: Vec<Observation> = (1..=count)
            .map(|ordinal| {
                let (before, after) = if ordinal % 2 == 1 {
                    ("a\n", "b\n")
                } else {
                    ("b\n", "a\n")
                };
                let tool = format!("t{ordinal}");
                edit(&tool, "a.txt", ordinal, known(before), known(after))
            })
            .collect();
        obs.push(edit("t0", "a.txt", count + 1, known("z\n"), known("a\n")));
        obs
    }

    #[test]
    fn discontinuities_alone_are_those_derive_finds() {
        let cases = [
            after_a_directory_change(2),
            after_a_directory_change(1),
            back_and_forth(6),
            vec![
                edit("t1", "notes.txt", 3, known("draft\n"), known("final\n")),
                edit("t2", "notes.txt", 3, known("final\n"), known("draft\n")),
                edit("t3", "notes.txt", 4, known("x\n"), known("y\n")),
            ],
            vec![
                edit("t1", "a.txt", 1, known("x\n"), FileState::Absent),
                edit("t2", "a.txt", 2, known("y\n"), known("z\n")),
                edit("t3", "b.txt", 1, known("x\n"), known("y\n")),
                edit("t4", "b.txt", 2, FileState::unknown(), known("z\n")),
                edit("t1", "a.txt", 1, known("q\n"), known("r\n")),
            ],
        ];
        for (k, obs) in cases.iter().enumerate() {
            let derived = derive(&session(), obs, Revision::ZERO.next(), joined());
            let alone = discontinuities(&session(), obs, joined());
            assert_eq!(alone, derived.discontinuities, "{k}");
            let derived = derive(&session(), obs, Revision::ZERO.next(), as_recorded());
            let alone = discontinuities(&session(), obs, as_recorded());
            assert_eq!(alone, derived.discontinuities, "{k}");
        }
    }

    #[test]
    fn the_discontinuities_of_many_edits_back_and_forth_are_found_promptly() {
        // Each edit restores every earlier one that started where it ends:
        // about 25 million restored IDs for 10,000 edits, which derive
        // collected in about 1.1 s and 1.3 GB here in a debug build.
        let obs = back_and_forth(10_000);

        let started = std::time::Instant::now();
        let found = discontinuities(&session(), &obs, as_recorded());
        let elapsed = started.elapsed();

        eprintln!("10,000 edits back and forth: {elapsed:?}");
        assert!(elapsed < std::time::Duration::from_secs(20), "{elapsed:?}");
        assert_eq!(
            found,
            [Discontinuity {
                path: PathBuf::from("a.txt"),
                checkout: PathBuf::from("/work/repo"),
                prev: id("t10000"),
                next: id("t0"),
            }]
        );
    }
}
