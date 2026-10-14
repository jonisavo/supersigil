//! What `review` reads beside the review to build its gate: the gaps
//! between recorded edits of each reviewed file, derived in memory from the
//! pinned observations, what is on disk at the paths the capture left out,
//! and every recorded sighting of the edits the review shows.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, FileType, Metadata, OpenOptions};
use std::io::{self, Read as _};
use std::path::Path;

use sha2::{Digest as _, Sha256};
use supersigil_git::RepoPath;
use supersigil_record::derive;
use supersigil_record::observations::{Edit, Observation};
use supersigil_record::reviews::PathBytes;
use supersigil_record::{EventId, SessionId};
use supersigil_review::Review;
use supersigil_review::gate::{Gap, GateInputs, OnDiskState, UntrackedEvidence};

use crate::pipeline::{Gathered, MappedCandidate, PinnedRecord, path_key};

/// What `review` supplies beside it to build its gate
/// ([`supersigil_review::gate::gate_items`]): the gaps between recorded
/// edits of its files; for each untracked file with recorded edits, what is
/// on disk there and every sighting of those edits, whatever file a
/// sighting names; what is on disk at each path not captured; and every
/// sighting of each edit the review shows.
#[must_use]
pub fn gate_inputs<'g>(g: &'g Gathered, review: &Review) -> GateInputs<'g> {
    let recorded = Recorded::new(&g.records, &g.candidates, &g.worktree, g.ignore_case);
    let scope = &review.scope;
    GateInputs {
        discontinuities: recorded.discontinuities(review.files.iter().map(|file| &file.path_bytes)),
        untracked: scope
            .recorded_untracked()
            .map(|untracked| {
                let path = &untracked.path_bytes;
                let evidence = UntrackedEvidence {
                    on_disk: on_disk_at(&g.worktree, path),
                    edits: recorded.naming(path),
                };
                (path.clone(), evidence)
            })
            .collect(),
        not_captured: scope
            .not_captured
            .iter()
            .map(|entry| {
                let path = &entry.path_bytes;
                (path.clone(), on_disk_at(&g.worktree, path))
            })
            .collect(),
        edits: recorded.sightings(review.edits.keys().map(String::as_str)),
    }
}

/// Every recorded sighting of an edit in the involved records, indexed once:
/// by edit id, by session, and, for the sightings in the reviewed worktree,
/// by the file they name. Collecting the gate's inputs then never scans the
/// records or the candidate sightings again for each file or session.
struct Recorded<'g> {
    /// Every sighting of each edit, by id, in the order of the records and
    /// then of each record's observations: a record may hold an edit more
    /// than once, and another record may hold it with another payload.
    by_id: BTreeMap<&'g str, Vec<&'g Edit>>,
    /// Every edit observation of each session, in the same order.
    by_session: BTreeMap<&'g SessionId, Vec<&'g Observation>>,
    /// The ids of the edits with a sighting naming each file of the reviewed
    /// worktree, by the file's path key ([`path_key`]).
    naming: BTreeMap<Cow<'g, str>, BTreeSet<&'g str>>,
    /// Every sighting whose file lies in a candidate worktree
    /// ([`Gathered::candidates`]).
    candidates: &'g [MappedCandidate],
    /// Whether paths are compared ignoring ASCII case.
    ignore_case: bool,
}

impl<'g> Recorded<'g> {
    /// Indexes every observation of `records`, and the sightings among
    /// `candidates` that lie in `worktree`, the reviewed worktree, with
    /// paths compared ignoring ASCII case when `ignore_case` is set.
    fn new(
        records: &'g [PinnedRecord],
        candidates: &'g [MappedCandidate],
        worktree: &Path,
        ignore_case: bool,
    ) -> Self {
        let mut by_id: BTreeMap<&str, Vec<&Edit>> = BTreeMap::new();
        let mut by_session: BTreeMap<&SessionId, Vec<&Observation>> = BTreeMap::new();
        for observation in records.iter().flat_map(|record| &record.observations) {
            if let Observation::Edit(edit) = observation {
                by_id.entry(edit.id.as_str()).or_default().push(edit);
                by_session
                    .entry(&edit.session)
                    .or_default()
                    .push(observation);
            }
        }
        let mut naming: BTreeMap<Cow<str>, BTreeSet<&str>> = BTreeMap::new();
        for sighting in candidates {
            if sighting.candidate.worktree == worktree {
                naming
                    .entry(path_key(&sighting.path, ignore_case))
                    .or_default()
                    .insert(sighting.candidate.edit.id.as_str());
            }
        }
        Self {
            by_id,
            by_session,
            naming,
            candidates,
            ignore_case,
        }
    }

    /// Every sighting of the edit `id`, in any record.
    fn every(&self, id: &str) -> Vec<&'g Edit> {
        self.by_id.get(id).cloned().unwrap_or_default()
    }

    /// Every sighting of each edit `ids` names, by id.
    fn sightings<'i>(
        &self,
        ids: impl IntoIterator<Item = &'i str>,
    ) -> BTreeMap<String, Vec<&'g Edit>> {
        ids.into_iter()
            .map(|id| (id.to_owned(), self.every(id)))
            .collect()
    }

    /// Every sighting of each edit with a sighting naming `path`, a file of
    /// the reviewed worktree, as [`crate::pipeline::same_path`] matches
    /// paths, in id order: one recorded again naming another file changed
    /// under its id. None when `path` is not UTF-8.
    fn naming(&self, path: &PathBytes) -> Vec<&'g Edit> {
        let ids = path
            .to_str()
            .and_then(|path| self.naming.get(path_key(path, self.ignore_case).as_ref()));
        ids.into_iter()
            .flatten()
            .flat_map(|id| self.every(id))
            .collect()
    }

    /// The gaps between recorded edits of each of `files`, each edit with
    /// every sighting of it in any record.
    ///
    /// Edits are keyed by where they map, as attribution maps them
    /// ([`Gathered::candidates`]): the candidate worktree, and the reviewed
    /// file whose path matches the path in it as
    /// [`crate::pipeline::same_path`] matches paths. An edit that maps into
    /// no reviewed file is compared with nothing. Every keyed edit is a
    /// candidate edit, so its session is a candidate session. Each of those
    /// sessions is derived once, gaps only, over the observations of every
    /// record together, each edit id once (its first sighting, in record
    /// order, as [`derive::discontinuities`] takes it): an edit that one
    /// record missed, made from a working directory outside its
    /// association, still joins the edits before and after it there. A
    /// later sighting, recorded again with another payload, changes the
    /// gap's fingerprint all the same.
    fn discontinuities<'p>(&self, files: impl IntoIterator<Item = &'p PathBytes>) -> Vec<Gap<'g>> {
        let reviewed: BTreeMap<String, &PathBytes> = files
            .into_iter()
            .filter_map(|file| {
                let path = file.to_str()?;
                Some((path_key(path, self.ignore_case).into_owned(), file))
            })
            .collect();
        // Each edit id once, where its first sighting in a candidate
        // worktree maps.
        let mut keyed: BTreeMap<&EventId, (&Edit, &Path, &PathBytes)> = BTreeMap::new();
        for sighting in self.candidates {
            if let Some(&file) = reviewed.get(path_key(&sighting.path, self.ignore_case).as_ref()) {
                let candidate = &sighting.candidate;
                keyed.entry(&candidate.edit.id).or_insert((
                    &candidate.edit,
                    &candidate.worktree,
                    file,
                ));
            }
        }
        let sessions: BTreeSet<&SessionId> =
            keyed.values().map(|(edit, ..)| &edit.session).collect();
        let mut gaps = Vec::new();
        for session in sessions {
            let observations = self.by_session.get(session).into_iter().flatten();
            let found = derive::discontinuities(session, observations.copied(), |edit| {
                keyed
                    .get(&edit.id)
                    .map(|&(_, worktree, file)| (worktree, file))
            });
            for gap in found {
                if let (Some(&(prev, ..)), Some(&(next, _, file))) =
                    (keyed.get(&gap.prev), keyed.get(&gap.next))
                {
                    gaps.push(Gap {
                        path: file.clone(),
                        prev: self.every(prev.id.as_str()),
                        next: self.every(next.id.as_str()),
                    });
                }
            }
        }
        gaps
    }
}

/// What is on disk at the repository path `path` of `worktree`
/// ([`on_disk_state`]); unknown when this platform cannot name the path.
fn on_disk_at(worktree: &Path, path: &PathBytes) -> OnDiskState {
    RepoPath::new(path.as_bytes().to_vec())
        .to_path()
        .map_or(OnDiskState::Unknown, |path| {
            on_disk_state(&worktree.join(path))
        })
}

/// What is on disk at `path`, never following a symbolic link: nothing; a
/// regular file's executable bit and the SHA-256 of its bytes; or a
/// symbolic link's target, hashed. A directory, any other kind of entry,
/// or a path that cannot be read is unknown, as is a path replaced by
/// another kind of entry or another file, or a file changed, while it is
/// read.
#[must_use]
pub fn on_disk_state(path: &Path) -> OnDiskState {
    as_inspected(path, std::fs::symlink_metadata(path))
}

/// What is on disk at `path`, which `inspected` describes as it was a
/// moment earlier ([`on_disk_state`]). A symbolic link's target is read
/// from the path, so a link replaced by anything else is unknown. A
/// regular file is opened without following a symbolic link or waiting
/// for a writer, shared on Windows with readers only ([`open_unfollowed`]);
/// its kind, executable bit, and bytes are all taken from that one handle
/// ([`opened_state`]).
fn as_inspected(path: &Path, inspected: io::Result<Metadata>) -> OnDiskState {
    let kind = match inspected {
        Ok(metadata) => metadata.file_type(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return OnDiskState::Missing,
        Err(_) => return OnDiskState::Unknown,
    };
    let read = if kind.is_file() {
        open_unfollowed(path).and_then(|file| opened_state(path, kind, file))
    } else if kind.is_symlink() {
        std::fs::read_link(path).map(|target| OnDiskState::Symlink {
            target: hex::encode(Sha256::digest(target.as_os_str().as_encoded_bytes())),
        })
    } else {
        return OnDiskState::Unknown;
    };
    read.unwrap_or_else(|e| read_failed(&e))
}

/// What is on disk at a path inspected a moment earlier whose read failed
/// with `error`: missing when it was removed since, else unknown, as when
/// on Windows another process holds the file for writing (a sharing
/// violation, [`open_unfollowed`]).
fn read_failed(error: &io::Error) -> OnDiskState {
    if error.kind() == io::ErrorKind::NotFound {
        OnDiskState::Missing
    } else {
        OnDiskState::Unknown
    }
}

/// Opens what is at `path` to read it, never following a symbolic link and
/// never waiting. On Unix opening a symbolic link fails, and a FIFO opens
/// at once instead of waiting for a writer; on Windows a symbolic link is
/// opened itself, so the handle's metadata says it is one, and the file is
/// shared with readers only: while it is open no writer can change it and
/// no one can rename or delete it, and opening a file another process
/// holds for writing fails.
///
/// # Errors
///
/// Returns the I/O error of opening it, as for a symbolic link on Unix or,
/// on Windows, a file another process holds for writing.
fn open_unfollowed(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(
        &mut options,
        libc::O_NOFOLLOW | libc::O_NONBLOCK,
    );
    #[cfg(windows)]
    std::os::windows::fs::OpenOptionsExt::custom_flags(&mut options, FILE_FLAG_OPEN_REPARSE_POINT);
    #[cfg(windows)]
    std::os::windows::fs::OpenOptionsExt::share_mode(&mut options, FILE_SHARE_READ);
    options.open(path)
}

/// The Windows flag that opens a symbolic link rather than what it names.
#[cfg(windows)]
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

/// The Windows share mode that lets others open a file only to read it.
#[cfg(windows)]
const FILE_SHARE_READ: u32 = 0x0000_0001;

/// What `file`, opened at `path`, inspected as `inspected` a moment
/// earlier, holds: a regular file's executable bit and the SHA-256 of its
/// bytes, both from the handle. Unknown unless the inspection and the
/// handle both found a regular file, since the path may have been replaced
/// in between, unknown when the file changed while it was hashed
/// ([`hashed`]), and unknown when it is no longer the file at `path` once
/// hashed ([`still_at`]).
///
/// # Errors
///
/// Returns the I/O error of inspecting or reading the opened file.
fn opened_state(path: &Path, inspected: FileType, mut file: File) -> io::Result<OnDiskState> {
    let before = file.metadata()?;
    if !(inspected.is_file() && before.is_file()) {
        return Ok(OnDiskState::Unknown);
    }
    let content = file_digest(&mut file)?;
    let after = file.metadata()?;
    if !still_at(path, &after) {
        return Ok(OnDiskState::Unknown);
    }
    Ok(hashed(&before, &after, content))
}

/// Whether the file whose handle's metadata was `opened` once its bytes were
/// hashed is still the one at `path`, unchanged: on Unix, the path's own
/// entry, never followed, has the handle's device and inode and the same
/// state ([`same_state`]). A file moved away while it was open, with another
/// put at the path or none, or changed since and moved back, may hold bytes
/// the path never had; an entry that cannot be inspected tells nothing
/// either.
#[cfg(unix)]
fn still_at(path: &Path, opened: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::symlink_metadata(path).is_ok_and(|entry| {
        entry.dev() == opened.dev() && entry.ino() == opened.ino() && same_state(&entry, opened)
    })
}

/// Always: on Windows the file was opened sharing it with readers only
/// ([`open_unfollowed`]), which denies renaming or deleting it while it is
/// open, so it stays the file at its path.
#[cfg(not(unix))]
const fn still_at(_path: &Path, _opened: &Metadata) -> bool {
    true
}

/// A regular file's state from `content`, the SHA-256 of the bytes read
/// from a handle whose metadata was `before` as the read began and `after`
/// once it ended. Unknown when the two differ ([`same_state`]): the file
/// changed while it was read, so its bytes and its executable bit may never
/// have been there together.
fn hashed(before: &Metadata, after: &Metadata, content: String) -> OnDiskState {
    if same_state(before, after) {
        OnDiskState::File {
            executable: executable(after),
            content,
        }
    } else {
        OnDiskState::Unknown
    }
}

/// Whether two looks at one file see it in the same state: the same length,
/// executable bit, modification time (where the platform records one), and,
/// on Unix, status change time, which no writer can set back.
fn same_state(a: &Metadata, b: &Metadata) -> bool {
    a.len() == b.len()
        && executable(a) == executable(b)
        && a.modified().ok() == b.modified().ok()
        && status_changed(a) == status_changed(b)
}

/// Bytes [`file_digest`] reads at a time.
const CHUNK: usize = 8 * 1024;

/// Hex SHA-256 of the bytes of `file`, read [`CHUNK`] bytes at a time so a
/// large file is never held whole.
///
/// # Errors
///
/// Returns the I/O error of reading the file.
fn file_digest(file: &mut File) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut chunk = [0; CHUNK];
    loop {
        match file.read(&mut chunk) {
            Ok(0) => return Ok(hex::encode(hasher.finalize())),
            Ok(read) => hasher.update(&chunk[..read]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

/// Whether the owner's executable bit is set, the bit git records as mode
/// `100755`.
#[cfg(unix)]
fn executable(metadata: &Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o100 != 0
}

/// Never: this platform has no executable bit.
#[cfg(not(unix))]
fn executable(_metadata: &Metadata) -> bool {
    false
}

/// When the file's status last changed (seconds and nanoseconds). Any write
/// or permission change moves it, and no writer can set it back.
#[cfg(unix)]
fn status_changed(metadata: &Metadata) -> (i64, i64) {
    use std::os::unix::fs::MetadataExt as _;
    (metadata.ctime(), metadata.ctime_nsec())
}

/// Always the same: this platform's status change time is not compared.
#[cfg(not(unix))]
const fn status_changed(_metadata: &Metadata) -> (i64, i64) {
    (0, 0)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs::File;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use sha2::{Digest as _, Sha256};
    use supersigil_anchor::CandidateEdit;
    use supersigil_record::observations::{
        Edit, EditOperation, EditOrigin, FileState, Material, Observation,
    };
    use supersigil_record::reviews::PathBytes;
    use supersigil_record::store::{Association, Store};
    use supersigil_record::{ContentId, EventId, RecordId, Revision, SessionId, Timestamp, TurnId};
    use supersigil_review::gate::OnDiskState;

    use super::{CHUNK, Recorded, hashed, on_disk_state, opened_state, read_failed};
    #[cfg(unix)]
    use super::{as_inspected, file_digest, still_at};
    use crate::pipeline::{MappedCandidate, PinnedRecord};

    /// Hex SHA-256 of `hello\n`.
    const HELLO: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";

    /// The worktree the edits here lie in.
    const WORKTREE: &str = "/r";

    /// Edit `tool` of session `session`, at `ordinal` in its transcript,
    /// writing `path` in [`WORKTREE`] from content `before` to `after`.
    fn edit(
        session: &str,
        tool: &str,
        path: &str,
        ordinal: u64,
        before: &str,
        after: &str,
    ) -> Edit {
        let content = |text: &str| FileState::known(ContentId::of(text.as_bytes()));
        Edit {
            id: EventId::derive("edit", &SessionId::new(session), tool),
            turn: TurnId::new(format!("a{ordinal}")),
            session: SessionId::new(session),
            path: PathBuf::from(path),
            before: content(before),
            after: content(after),
            patch: Material::unavailable("test"),
            old_text: Material::unavailable("test"),
            new_text: Material::unavailable("test"),
            replace_all: false,
            operation: EditOperation::Replace,
            origin: EditOrigin::Tool,
            checkout: PathBuf::from(WORKTREE),
            time: Timestamp::new("2026-10-14T10:00:00.000Z"),
            source_ordinal: ordinal,
            agent_id: None,
            transcript: Some(format!("{session}.jsonl")),
        }
    }

    /// Record `id`, pinned, holding `edits`.
    fn record(id: &str, edits: &[Edit]) -> PinnedRecord {
        // An empty record's snapshot, which nothing here reads.
        let dir = tempfile::tempdir().unwrap();
        let checkout = PathBuf::from(WORKTREE);
        let store = Store::create(dir.path(), Association { checkout }).unwrap();
        PinnedRecord {
            record_id: RecordId::new(id),
            revision: Revision::ZERO,
            not_reconciled: None,
            associations: vec![PathBuf::from(WORKTREE)],
            logs: BTreeMap::new(),
            observations: edits.iter().cloned().map(Observation::Edit).collect(),
            snapshot: store.snapshot().unwrap(),
        }
    }

    /// Every sighting `records` hold, mapped to its path in [`WORKTREE`],
    /// a candidate worktree, as gathering maps them.
    fn candidates(records: &[PinnedRecord]) -> Vec<MappedCandidate> {
        let mut candidates = Vec::new();
        for record in records {
            for observation in &record.observations {
                if let Observation::Edit(edit) = observation {
                    candidates.push(MappedCandidate {
                        candidate: CandidateEdit {
                            record: record.record_id.clone(),
                            worktree: PathBuf::from(WORKTREE),
                            edit: edit.clone(),
                        },
                        path: edit.path.to_string_lossy().into_owned(),
                    });
                }
            }
        }
        candidates
    }

    #[test]
    fn an_edit_one_record_missed_closes_the_gap_in_another_record() {
        // Three writes of `x/gap.rs`, each starting from what the last one
        // left. The record of `x` missed the second, made from outside it.
        let writes = [
            edit("s", "one", "x/gap.rs", 1, "", "one\n"),
            edit("s", "two", "x/gap.rs", 3, "one\n", "two\n"),
            edit("s", "three", "x/gap.rs", 5, "two\n", "three\n"),
        ];
        let partial = [writes[0].clone(), writes[2].clone()];
        let reviewed = [PathBytes::new("x/gap.rs")];

        let records = [record("x", &partial), record("root", &writes)];
        let mapped = candidates(&records);
        let recorded = Recorded::new(&records, &mapped, Path::new(WORKTREE), false);
        let found = recorded.discontinuities(&reviewed);
        assert!(found.is_empty(), "{found:#?}");

        // When neither record holds the second write, the gap is found once,
        // with both records' sightings of its edits.
        let records = [record("x", &partial), record("root", &partial)];
        let mapped = candidates(&records);
        let recorded = Recorded::new(&records, &mapped, Path::new(WORKTREE), false);
        let found = recorded.discontinuities(&reviewed);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].path, reviewed[0]);
        assert_eq!(found[0].prev, [&writes[0], &writes[0]]);
        assert_eq!(found[0].next, [&writes[2], &writes[2]]);
    }

    #[test]
    fn the_gate_inputs_of_many_recorded_edits_are_collected_promptly() {
        // One record of 20,001 edits: 10,000 of one reviewed file, each
        // returning it to where the one before started, then one from
        // other content; one edit of each of 5,000 more reviewed files, each
        // by a session of its own; and 5,000 untracked files written by one
        // session.
        let mut edits: Vec<Edit> = (1..=10_000)
            .map(|k| {
                let (before, after) = if k % 2 == 1 {
                    ("a\n", "b\n")
                } else {
                    ("b\n", "a\n")
                };
                edit("s-back", &format!("t{k}"), "a.rs", k, before, after)
            })
            .collect();
        edits.push(edit("s-back", "t0", "a.rs", 10_001, "z\n", "a\n"));
        let mut reviewed = vec![PathBytes::new("a.rs")];
        let mut untracked = Vec::new();
        for k in 0..5_000 {
            let path = format!("f{k}.rs");
            edits.push(edit(&format!("s{k}"), "t", &path, 1, "", "f\n"));
            reviewed.push(PathBytes::new(path));
            let path = format!("u{k}.rs");
            edits.push(edit("s-new", &path, &path, k, "", "u\n"));
            untracked.push(PathBytes::new(path));
        }
        let records = [record("r", &edits)];
        let candidates = candidates(&records);

        let started = Instant::now();
        let recorded = Recorded::new(&records, &candidates, Path::new(WORKTREE), false);
        let gaps = recorded.discontinuities(&reviewed);
        let naming: Vec<usize> = untracked
            .iter()
            .map(|path| recorded.naming(path).len())
            .collect();
        let elapsed = started.elapsed();

        eprintln!("gate inputs of 20,001 edits: {elapsed:?}");
        assert!(elapsed < Duration::from_secs(20), "{elapsed:?}");
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].prev, [&edits[9_999]]);
        assert_eq!(gaps[0].next, [&edits[10_000]]);
        assert!(naming.iter().all(|&count| count == 1));
    }

    #[test]
    fn a_file_is_its_bytes_hashed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "hello\n").unwrap();
        assert_eq!(
            on_disk_state(&path),
            OnDiskState::File {
                executable: false,
                content: HELLO.to_owned()
            }
        );
    }

    #[test]
    fn a_file_longer_than_a_chunk_is_hashed_whole() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.txt");
        let bytes: Vec<u8> = (0..3 * CHUNK + 17)
            .map(|i| u8::try_from(i % 251).unwrap())
            .collect();
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(
            on_disk_state(&path),
            OnDiskState::File {
                executable: false,
                content: hex::encode(Sha256::digest(&bytes))
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_executable_file_says_so() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.sh");
        std::fs::write(&path, "hello\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            on_disk_state(&path),
            OnDiskState::File {
                executable: true,
                content: HELLO.to_owned()
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_its_target_hashed_never_followed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("link");
        // A dangling link: what it names is never read.
        std::os::unix::fs::symlink("target.txt", &path).unwrap();
        assert_eq!(
            on_disk_state(&path),
            OnDiskState::Symlink {
                // Hex SHA-256 of `target.txt`.
                target: "199b3badd968634ea14e351d1134ada738894a90a2efa66983101ece99a33572"
                    .to_owned()
            }
        );
    }

    #[test]
    fn a_directory_is_unknown_and_a_missing_path_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(on_disk_state(dir.path()), OnDiskState::Unknown);
        assert_eq!(
            on_disk_state(&dir.path().join("gone")),
            OnDiskState::Missing
        );
    }

    /// Returns what `read` returns, run on another thread, failing unless
    /// it returns within ten seconds.
    #[cfg(unix)]
    fn promptly<T: Send + 'static>(read: impl FnOnce() -> T + Send + 'static) -> T {
        let (done, result) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = done.send(read());
        });
        result
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the read returns promptly")
    }

    /// Makes a FIFO at `path`.
    #[cfg(unix)]
    fn mkfifo(path: &Path) {
        let made = std::process::Command::new("mkfifo")
            .arg(path)
            .status()
            .unwrap();
        assert!(made.success());
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_is_unknown_promptly_even_where_a_file_was_inspected() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "hello\n").unwrap();
        let fifo = dir.path().join("fifo");
        mkfifo(&fifo);
        let at_fifo = fifo.clone();
        assert_eq!(
            promptly(move || on_disk_state(&at_fifo)),
            OnDiskState::Unknown
        );
        // A FIFO that replaced the file after it was inspected: opening it
        // to read would wait for a writer.
        let inspected = std::fs::symlink_metadata(&file);
        assert_eq!(
            promptly(move || as_inspected(&fifo, inspected)),
            OnDiskState::Unknown
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_that_replaced_an_inspected_file_is_unknown_never_followed() {
        let dir = tempfile::tempdir().unwrap();
        // What the judged review saw, now outside the path.
        let old = dir.path().join("old.txt");
        std::fs::write(&old, "hello\n").unwrap();
        let path = dir.path().join("new.rs");
        std::fs::write(&path, "changed\n").unwrap();
        let inspected = std::fs::symlink_metadata(&path);
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&old, &path).unwrap();
        assert_eq!(as_inspected(&path, inspected), OnDiskState::Unknown);
    }

    #[test]
    fn a_handle_unlike_what_was_inspected_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "hello\n").unwrap();
        // A directory was inspected, and a regular file opened since.
        let directory = std::fs::symlink_metadata(dir.path()).unwrap();
        assert_eq!(
            opened_state(&path, directory.file_type(), File::open(&path).unwrap()).unwrap(),
            OnDiskState::Unknown
        );
        let regular = std::fs::symlink_metadata(&path).unwrap();
        assert_eq!(
            opened_state(&path, regular.file_type(), File::open(&path).unwrap()).unwrap(),
            OnDiskState::File {
                executable: false,
                content: HELLO.to_owned()
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_no_longer_at_its_path_once_hashed_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        let moved = dir.path().join("moved.txt");
        // Once opened, the file is moved away, and then another file with
        // the same bytes is put at its path, or none.
        for replaced in [true, false] {
            std::fs::write(&path, "hello\n").unwrap();
            let inspected = std::fs::symlink_metadata(&path).unwrap();
            let file = File::open(&path).unwrap();
            std::fs::rename(&path, &moved).unwrap();
            if replaced {
                std::fs::write(&path, "hello\n").unwrap();
            }
            assert_eq!(
                opened_state(&path, inspected.file_type(), file).unwrap(),
                OnDiskState::Unknown,
                "{replaced}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_file_changed_once_hashed_is_not_the_one_at_its_path() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "howdy\n").unwrap();
        let file = File::open(&path).unwrap();
        let hashed_as = file.metadata().unwrap();
        assert!(still_at(&path, &hashed_as));
        // The kernel's status change time can be coarse: let a tick pass.
        std::thread::sleep(std::time::Duration::from_millis(50));
        // Once hashed, the same file, the same inode still at its path, is
        // given other bytes at the same length and modification time (or
        // moved away, changed, and moved back): it is no longer the file
        // whose bytes were hashed.
        let mut rewrite = File::options().write(true).open(&path).unwrap();
        rewrite.write_all(b"hello\n").unwrap();
        rewrite.set_modified(hashed_as.modified().unwrap()).unwrap();
        drop(rewrite);
        assert!(!still_at(&path, &hashed_as));
    }

    #[test]
    fn a_file_whose_metadata_changed_while_it_was_hashed_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "hello\n").unwrap();
        let before = std::fs::metadata(&path).unwrap();
        let modified = before.modified().unwrap();
        let state_after = |after| hashed(&before, &after, HELLO.to_owned());
        assert_eq!(
            state_after(std::fs::metadata(&path).unwrap()),
            OnDiskState::File {
                executable: false,
                content: HELLO.to_owned()
            }
        );
        // Another modification time, the length kept.
        let set_modified = |time| {
            let file = File::options().write(true).open(&path).unwrap();
            file.set_modified(time).unwrap();
        };
        set_modified(modified + Duration::from_secs(1));
        assert_eq!(
            state_after(std::fs::metadata(&path).unwrap()),
            OnDiskState::Unknown
        );
        // Another length, the modification time put back.
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(3)
            .unwrap();
        set_modified(modified);
        let after = std::fs::metadata(&path).unwrap();
        assert_eq!(after.modified().unwrap(), modified);
        assert_eq!(state_after(after), OnDiskState::Unknown);
    }

    #[test]
    fn a_file_another_process_holds_for_writing_is_unknown() {
        // On Windows a file is opened to be hashed sharing it with readers
        // only, so opening one another process holds for writing fails with
        // a sharing violation (raw OS error 32).
        assert_eq!(
            read_failed(&io::Error::from_raw_os_error(32)),
            OnDiskState::Unknown
        );
        // Removed since it was inspected.
        assert_eq!(
            read_failed(&io::ErrorKind::NotFound.into()),
            OnDiskState::Missing
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_made_executable_and_rewritten_while_it_was_hashed_is_unknown() {
        use std::io::Write as _;
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "howdy\n").unwrap();
        let mut file = File::open(&path).unwrap();
        let before = file.metadata().unwrap();
        // Once the handle's metadata is read, and before its bytes are, the
        // file is made executable and given back the bytes a judged review
        // saw, at the same length and modification time.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut rewrite = File::options().write(true).open(&path).unwrap();
        rewrite.write_all(b"hello\n").unwrap();
        rewrite.set_modified(before.modified().unwrap()).unwrap();
        drop(rewrite);

        let content = file_digest(&mut file).unwrap();
        assert_eq!(content, HELLO);
        assert_eq!(
            hashed(&before, &file.metadata().unwrap(), content),
            OnDiskState::Unknown
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_rewritten_at_the_same_length_and_time_while_it_was_hashed_is_unknown() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "howdy\n").unwrap();
        let mut file = File::open(&path).unwrap();
        let before = file.metadata().unwrap();
        // The kernel's status change time can be coarse: let a tick pass.
        std::thread::sleep(std::time::Duration::from_millis(50));
        // Once the handle's metadata is read, and before its bytes are, the
        // file is given back the bytes a judged review saw, at the same
        // length, executable bit, and modification time. Only its status
        // change time, which no writer can set, tells.
        let mut rewrite = File::options().write(true).open(&path).unwrap();
        rewrite.write_all(b"hello\n").unwrap();
        rewrite.set_modified(before.modified().unwrap()).unwrap();
        drop(rewrite);

        let content = file_digest(&mut file).unwrap();
        assert_eq!(content, HELLO);
        assert_eq!(
            hashed(&before, &file.metadata().unwrap(), content),
            OnDiskState::Unknown
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_executable_bit_is_the_opened_files() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let inspected = dir.path().join("a.txt");
        std::fs::write(&inspected, "hello\n").unwrap();
        let opened = dir.path().join("run.sh");
        std::fs::write(&opened, "hello\n").unwrap();
        std::fs::set_permissions(&opened, std::fs::Permissions::from_mode(0o755)).unwrap();
        let inspected = std::fs::symlink_metadata(&inspected).unwrap();
        assert_eq!(
            opened_state(&opened, inspected.file_type(), File::open(&opened).unwrap()).unwrap(),
            OnDiskState::File {
                executable: true,
                content: HELLO.to_owned()
            }
        );
    }
}
