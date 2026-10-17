//! Reads changes since an earlier related judged review: paths differing between
//! the target trees while the earlier tree still exists, new observations editing
//! or changing reviewed paths, new capture limitations in candidate transcripts,
//! and previously read records now gone or rewritten.
//! The review model compares explanations and gate items from the two bases
//! ([`JudgedReview::report_reading`](supersigil_review::judging::JudgedReview::report_reading)).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use supersigil_git::ObjectId;
use supersigil_git::changes::changed_paths;
use supersigil_git::retention::trees_exist;
use supersigil_record::RecordId;
use supersigil_record::observations::{Edit, Observation};
use supersigil_record::reviews::{Basis, BasisRecord, PathBytes, PinnedLog};
use supersigil_record::store::LogReading;
use supersigil_review::Review;
use supersigil_review::judging::{Earlier, Relation};
use supersigil_review::model::{NewObservations, RelatedReading};
use supersigil_session::checkout::Resolutions;

use crate::error::CliError;
use crate::mapping::map_edit;
use crate::pipeline::{Gathered, PinnedRecord, by_path_key, path_key};

/// Reads code and record changes since each earlier review in `related`, in order,
/// for `review` gathered in `g`. Reads each current log at most once across all
/// comparisons. Reports unknown changed paths if an earlier target tree is gone.
/// Its retention ref keeps it until someone deletes the ref and git prunes the tree.
///
/// # Errors
///
/// Returns [`CliError::Git`] if an earlier target tree cannot be looked up
/// or compared with the current one, and the store errors of reading a pinned log.
pub fn read_changes(
    g: &Gathered,
    review: &Review,
    related: &[Earlier<'_>],
) -> Result<Vec<RelatedReading>, CliError> {
    let earlier: Vec<&[BasisRecord]> = related
        .iter()
        .map(|related| related.basis().records.as_slice())
        .collect();
    let logs = Logs::read(&g.records, &earlier)?;
    let changed = code_changed(g, related)?;
    Ok(related
        .iter()
        .zip(changed)
        .map(|(&related, changed_paths)| {
            let since = records_since(g, related.basis(), &logs);
            RelatedReading {
                changed_paths,
                new: new_observations(g, review, &since.appended),
                capture: capture_changed(g, &since.appended),
                gone_or_rewritten: since.gone_or_rewritten,
            }
        })
        .collect())
}

/// Compares scoped paths between each `related` target tree and `g`'s target tree,
/// in order. Returns no changed paths for matching content, or `None` when the
/// earlier tree is gone. Checks all earlier trees' existence in one git call.
/// Compares each distinct earlier tree once, regardless of how many related
/// reviews share it.
///
/// # Errors
///
/// Returns [`CliError::Git`] if an earlier tree cannot be looked up or
/// compared with the current one.
fn code_changed(
    g: &Gathered,
    related: &[Earlier<'_>],
) -> Result<Vec<Option<Vec<PathBytes>>>, CliError> {
    // Each earlier tree whose code changed since, once, in order.
    let mut seen = BTreeSet::new();
    let trees: Vec<&str> = related
        .iter()
        .filter(|related| related.relation() == Relation::CodeChanged)
        .map(|related| related.basis().target.tree.as_str())
        .filter(|&tree| seen.insert(tree))
        .collect();
    let ids = trees
        .iter()
        .map(|tree| ObjectId::parse(tree, g.repo.format()))
        .collect::<Result<Vec<_>, _>>()?;
    let exist = trees_exist(&g.repo, &ids)?;
    let mut changed = BTreeMap::new();
    for ((tree, id), exists) in trees.into_iter().zip(&ids).zip(exist) {
        let paths = if exists {
            let mut paths: Vec<PathBytes> = changed_paths(&g.repo, id, &g.target_tree, &g.paths)?
                .iter()
                .map(|change| PathBytes::new(change.path.as_bytes()))
                .collect();
            paths.sort();
            Some(paths)
        } else {
            None
        };
        changed.insert(tree, paths);
    }
    Ok(related
        .iter()
        .map(|related| match related.relation() {
            Relation::SameContent => Some(Vec::new()),
            Relation::CodeChanged => changed[related.basis().target.tree.as_str()].clone(),
        })
        .collect())
}

/// The current logs of a review's records, each read at most once for all
/// the earlier reviews related to it.
struct Logs {
    /// Logs by record and path, each read once because an earlier review pinned
    /// fewer bytes or none. Each still matches the current review's pinned bytes.
    readings: BTreeMap<RecordId, BTreeMap<String, LogReading>>,
}

impl Logs {
    /// Reads each log in `records` once if an earlier review pinned fewer bytes or none.
    /// Hashes prefixes at each earlier pinned length and the current pinned length
    /// ([`RecordSnapshot::read_log`]). Each entry in `earlier` holds one review's records.
    /// Discards a reading if its current pinned prefix digest changed, and counts the
    /// log as rewritten rather than supplying observations the current review never saw.
    ///
    /// [`RecordSnapshot::read_log`]: supersigil_record::store::RecordSnapshot::read_log
    ///
    /// # Errors
    ///
    /// Returns the store errors of reading a log.
    fn read(records: &[PinnedRecord], earlier: &[&[BasisRecord]]) -> Result<Self, CliError> {
        let mut readings: BTreeMap<RecordId, BTreeMap<String, LogReading>> = BTreeMap::new();
        for record in records {
            for (log, now) in &record.logs {
                let mut lengths: BTreeSet<u64> = earlier
                    .iter()
                    .map(|pinned| {
                        pinned
                            .iter()
                            .find(|then| then.id == record.record_id)
                            .and_then(|then| then.logs.get(log))
                            .map_or(0, |then| then.length)
                    })
                    .filter(|&length| length < now.length)
                    .collect();
                if lengths.is_empty() {
                    continue;
                }
                lengths.insert(now.length);
                if let Some(reading) = record.snapshot.read_log(log, &lengths)?
                    && reading.digest(now.length) == Some(now.sha256.as_str())
                {
                    readings
                        .entry(record.record_id.clone())
                        .or_default()
                        .insert(log.clone(), reading);
                }
            }
        }
        Ok(Self { readings })
    }

    /// Returns observations appended to `record`'s `log` since pin `then`, or all
    /// observations if the earlier review did not pin it. Returns `None` if the log
    /// shrank, its earlier pinned prefix changed, or it changed since the current
    /// review read it. Checks digests, not length alone. [`Self::read`] read `record`.
    fn since<'s>(
        &'s self,
        record: &PinnedRecord,
        log: &str,
        then: Option<&PinnedLog>,
    ) -> Option<&'s [Observation]> {
        let now = record.logs.get(log)?;
        let start = then.map_or(0, |then| then.length);
        if start == now.length {
            // Equal pinned lengths mean no append; compare the pinned digests.
            return then
                .is_none_or(|then| then.sha256 == now.sha256)
                .then_some(&[]);
        }
        let reading = self.readings.get(&record.record_id)?.get(log)?;
        if then.is_some_and(|then| reading.digest(then.length) != Some(then.sha256.as_str())) {
            return None;
        }
        reading.since(start)
    }
}

/// What the records hold since an earlier review.
struct Since<'l> {
    /// The observations appended since to the records' logs.
    appended: Vec<&'l Observation>,
    /// The records the earlier review read that are gone or rewritten,
    /// sorted.
    gone_or_rewritten: Vec<RecordId>,
}

/// Collects observations from `logs` beyond `earlier`'s pins in `g`'s records,
/// including all observations from records or logs not previously pinned.
/// Lists earlier records now absent, with missing logs, or with changed pinned
/// prefixes as gone or rewritten. Also counts logs changed during the current
/// review's read as rewritten. Excludes later observations from rewritten logs
/// because their starting position is unknown.
fn records_since<'l>(g: &Gathered, earlier: &Basis, logs: &'l Logs) -> Since<'l> {
    let mut gone = BTreeSet::new();
    let mut appended = Vec::new();
    for record in &g.records {
        let then = earlier
            .records
            .iter()
            .find(|then| then.id == record.record_id);
        for log in record.logs.keys() {
            match logs.since(record, log, then.and_then(|then| then.logs.get(log))) {
                Some(observations) => appended.extend(observations),
                None => {
                    gone.insert(record.record_id.clone());
                }
            }
        }
    }
    for then in &earlier.records {
        let now = g.records.iter().find(|now| now.record_id == then.id);
        if now.is_none_or(|now| then.logs.keys().any(|log| !now.logs.contains_key(log))) {
            gone.insert(then.id.clone());
        }
    }
    Since {
        appended,
        gone_or_rewritten: gone.into_iter().collect(),
    }
}

/// Groups `appended` observations editing or changing reviewed paths by session.
/// Maps edits using the checkout resolutions made while gathering the review.
/// Maps each command sighting's reported changes the same way, in any transcript.
/// Reviewed paths include reviewed files, recorded untracked files, and uncaptured
/// paths. Excludes turns, delegation steps, and commands without reported changes
/// to these paths. A textual path mention alone does not count.
fn new_observations(
    g: &Gathered,
    review: &Review,
    appended: &[&Observation],
) -> Vec<NewObservations> {
    let reviewed = by_path_key(reviewed_paths(review), g.ignore_case);
    let mut named: BTreeMap<String, BTreeSet<&PathBytes>> = BTreeMap::new();
    let mut name = |session: &str, path: &str| {
        if let Some(&reviewed) = reviewed.get(path_key(path, g.ignore_case).as_ref()) {
            named
                .entry(session.to_owned())
                .or_default()
                .insert(reviewed);
        }
    };
    let observed = g.observed_worktrees();
    let evidence = &g.evidence;
    for observation in appended {
        match observation {
            Observation::Edit(edit) => {
                let resolutions = evidence.resolutions();
                let worktrees = &g.candidate_worktrees;
                let path = candidate_path(edit, resolutions, &g.roots, worktrees, g.ignore_case);
                if let Some(path) = path {
                    name(edit.session.as_str(), &path);
                }
            }
            // Count every command sighting, even from noncandidate transcripts.
            // Later sightings may add change reports; candidates use first sightings.
            Observation::Command(command) => {
                for path in evidence.reported_paths(command, &g.roots, &observed, g.ignore_case) {
                    name(command.session.as_str(), &path);
                }
            }
            _ => {}
        }
    }
    named
        .into_iter()
        .map(|(session, paths)| NewObservations {
            session,
            paths: paths.into_iter().cloned().collect(),
        })
        .collect()
}

/// Maps `edit`'s file to a candidate `worktree` among `roots`, using the review's
/// checkout `resolutions`. Appended edits therefore map like the review's own
/// sightings even if a checkout now resolves differently. Resolves previously
/// unseen checkouts now.
fn candidate_path(
    edit: &Edit,
    resolutions: &Resolutions,
    roots: &[PathBuf],
    worktrees: &[PathBuf],
    ignore_case: bool,
) -> Option<String> {
    map_edit(resolutions, edit, roots, ignore_case)
        .filter(|mapped| worktrees.contains(&mapped.worktree))
        .map(|mapped| mapped.path)
}

/// Lists candidate transcripts with capture limitations in `appended`, sorted.
/// Counts only limitations recorded since the earlier review, even for newly
/// candidate transcripts.
fn capture_changed(g: &Gathered, appended: &[&Observation]) -> Vec<String> {
    let transcripts: BTreeSet<&String> = appended
        .iter()
        .filter_map(|observation| match observation {
            Observation::CaptureLimitation(limitation)
                if !limitation.counts.is_empty()
                    && g.candidate_transcripts.contains(&limitation.transcript) =>
            {
                Some(&limitation.transcript)
            }
            _ => None,
        })
        .collect();
    transcripts.into_iter().cloned().collect()
}

/// The paths `review` reviewed: its files, its untracked files with
/// recorded edits, and its paths not captured.
fn reviewed_paths(review: &Review) -> impl Iterator<Item = &PathBytes> {
    let scope = &review.scope;
    review
        .files
        .iter()
        .map(|file| &file.path_bytes)
        .chain(
            scope
                .recorded_untracked()
                .map(|untracked| &untracked.path_bytes),
        )
        .chain(scope.not_captured.iter().map(|entry| &entry.path_bytes))
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::slice;

    use supersigil_record::observations::{
        EditOperation, EditOrigin, EndReason, FileState, Material, SessionEnd,
    };
    use supersigil_record::store::{Association, Store};
    use supersigil_record::{EventId, SessionId, Timestamp, TurnId};
    use supersigil_session::checkout::canonical;

    use super::*;

    /// The log of the only session.
    const LOG: &str = "observations/0.jsonl";

    /// Appends the end of session `s1` at `second` to `store`, then pins it
    /// as `review` does.
    fn append_and_pin(store: &Store, second: u32) -> PinnedRecord {
        let end = Observation::SessionEnd(SessionEnd {
            session: SessionId::new("s1"),
            time: Timestamp::new(format!("2026-09-28T10:00:{second:02}.000Z")),
            reason: EndReason::Unknown,
        });
        let mut tx = store.begin().unwrap();
        tx.append_observations(&[end]).unwrap();
        tx.commit().unwrap();
        let snapshot = store.snapshot().unwrap();
        let log = snapshot
            .session_log(&SessionId::new("s1"))
            .unwrap()
            .unwrap();
        PinnedRecord {
            record_id: RecordId::new("r1"),
            revision: snapshot.revision(),
            not_reconciled: None,
            associations: Vec::new(),
            logs: BTreeMap::from([(log.path, log.pinned)]),
            observations: log.observations,
            snapshot,
        }
    }

    /// What `record` pinned, as a basis keeps it.
    fn kept(record: &PinnedRecord) -> Vec<BasisRecord> {
        vec![BasisRecord {
            id: record.record_id.clone(),
            revision: record.revision,
            logs: record.logs.clone(),
            not_reconciled: None,
            associations: Vec::new(),
        }]
    }

    #[test]
    fn each_earlier_pin_reads_what_was_appended_while_the_log_begins_with_its_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let checkout = PathBuf::from("/work/repo");
        let store = Store::create(dir.path(), Association { checkout }).unwrap();
        let (first, second) = (append_and_pin(&store, 1), append_and_pin(&store, 2));
        let now = append_and_pin(&store, 3);
        let (first_pins, second_pins, now_pins) = (kept(&first), kept(&second), kept(&now));
        let other_bytes = |pinned: &PinnedLog| PinnedLog {
            sha256: "0".repeat(64),
            ..pinned.clone()
        };
        let longer = PinnedLog {
            length: now.logs[LOG].length + 1,
            ..now.logs[LOG].clone()
        };

        // Reviews that pinned the log at each length so far, and one that
        // did not pin it.
        let earlier = [first_pins.as_slice(), &second_pins, &now_pins, &[]];
        let logs = Logs::read(slice::from_ref(&now), &earlier).unwrap();
        let since = |then: &PinnedLog| logs.since(&now, LOG, Some(then)).map(<[_]>::len);

        assert_eq!(since(&first.logs[LOG]), Some(2));
        assert_eq!(since(&second.logs[LOG]), Some(1));
        assert_eq!(since(&now.logs[LOG]), Some(0));
        assert_eq!(logs.since(&now, LOG, None).map(<[_]>::len), Some(3));
        // Other bytes at an earlier length or at the length now, or a log
        // shorter now than it was read.
        assert_eq!(since(&other_bytes(&first.logs[LOG])), None);
        assert_eq!(since(&other_bytes(&now.logs[LOG])), None);
        assert_eq!(since(&longer), None);
    }

    #[test]
    fn a_log_changed_since_the_current_review_pinned_it_is_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let checkout = PathBuf::from("/work/repo");
        let store = Store::create(dir.path(), Association { checkout }).unwrap();
        let first = append_and_pin(&store, 1);
        let now = append_and_pin(&store, 2);
        // The observation appended since the first pin edited by hand after
        // the current review read the log, keeping its length and the bytes
        // the first pin read.
        let path = dir.path().join(LOG);
        let read = std::fs::read_to_string(&path).unwrap();
        let edited = read.replace("10:00:02", "10:00:09");
        assert_eq!(edited.len(), read.len());
        std::fs::write(&path, edited).unwrap();

        let first_pins = kept(&first);
        let logs = Logs::read(slice::from_ref(&now), &[first_pins.as_slice(), &[]]).unwrap();

        let since = |then: Option<&PinnedLog>| logs.since(&now, LOG, then).map(<[_]>::len);
        assert_eq!(since(Some(&first.logs[LOG])), None);
        assert_eq!(since(None), None);
    }

    /// A Write of `path` recorded with `checkout` as its working directory.
    fn written(checkout: &Path, path: &str) -> Edit {
        Edit {
            id: EventId::new("e1"),
            turn: TurnId::new("a1"),
            session: SessionId::new("s1"),
            path: PathBuf::from(path),
            before: FileState::unknown(),
            after: FileState::unknown(),
            patch: Material::unavailable("test"),
            old_text: Material::unavailable("test"),
            new_text: Material::unavailable("test"),
            replace_all: false,
            operation: EditOperation::Write,
            origin: EditOrigin::Tool,
            checkout: checkout.to_path_buf(),
            time: Timestamp::new("2026-09-28T10:00:00.000Z"),
            source_ordinal: 0,
            agent_id: None,
            transcript: None,
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_appended_edit_maps_through_the_checkout_resolution_the_review_made() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path()).unwrap();
        let repo = root.join("repo");
        let elsewhere = root.join("elsewhere");
        std::fs::create_dir(&repo).unwrap();
        std::fs::create_dir(&elsewhere).unwrap();
        // The review resolved a link to the repository; then the link was
        // repointed outside every worktree.
        let alias = root.join("alias");
        symlink(&repo, &alias).unwrap();
        let mut resolutions = Resolutions::default();
        resolutions.add(&alias);
        std::fs::remove_file(&alias).unwrap();
        symlink(&elsewhere, &alias).unwrap();
        // A link to the repository the review never saw.
        let unseen = root.join("unseen");
        symlink(&repo, &unseen).unwrap();
        let roots = [repo];

        let path = |checkout: &Path| {
            let edit = written(checkout, "a.sh");
            candidate_path(&edit, &resolutions, &roots, &roots, false)
        };
        assert_eq!(path(&alias), Some("a.sh".to_owned()));
        assert_eq!(path(&unseen), Some("a.sh".to_owned()));
    }
}
