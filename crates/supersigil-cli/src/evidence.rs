//! The recorded evidence around attribution: which transcripts a review
//! draws on and what their capture missed, commands the harness reported as
//! changing a file, commands whose text mentions a file, and the message
//! that preceded each edit.
//!
//! Everything here is an observation, never attribution: a command change
//! says the file differed once the command had finished, a mention is text
//! that contains a path, and a prompt is the nearest earlier Human or
//! Delegation message in the conversation, not a cause.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use supersigil_anchor::walk::AcceptedEdit;
use supersigil_record::observations::{
    CaptureLimitation, Command, Edit, EditOrigin, Observation, Outcome, Role, Turn,
};
use supersigil_record::{EventId, RecordId, SessionId, TurnId};
use supersigil_review::model::{
    CommandChange, EditInfo, EditOriginInfo, Mention, MentionResult, PromptInfo, TranscriptInfo,
};
use supersigil_session::checkout::{canonical, within};

use crate::mapping::map_file;

/// Turns, commands, capture limitations, and transcript locations of every
/// pinned record, indexed for the lookups a review makes.
#[derive(Debug, Default)]
pub struct Evidence {
    /// Every turn by session and id; the first occurrence wins.
    turns: BTreeMap<(SessionId, TurnId), Turn>,
    /// Every command by id; the first occurrence wins.
    commands: BTreeMap<EventId, Command>,
    /// Capture limitations by transcript, then by the record holding them.
    limitations: BTreeMap<String, BTreeMap<RecordId, Vec<CaptureLimitation>>>,
    /// The session each transcript belongs to.
    sessions: BTreeMap<String, SessionId>,
    /// Working directories each transcript's observations name.
    checkouts: BTreeMap<String, Vec<PathBuf>>,
}

impl Evidence {
    /// Indexes the observations of each record, keeping the first
    /// occurrence of each turn and command, and which record each capture
    /// limitation came from.
    #[must_use]
    pub fn index<'a>(records: impl IntoIterator<Item = (&'a RecordId, &'a [Observation])>) -> Self {
        let mut evidence = Self::default();
        for (record, observations) in records {
            for observation in observations {
                evidence.add(record, observation);
            }
        }
        evidence
    }

    /// Indexes one observation of `record`.
    fn add(&mut self, record: &RecordId, observation: &Observation) {
        match observation {
            Observation::SessionStart(start) => {
                if let Some(path) = start.source_ids.get("path") {
                    self.locate(path, &start.session, &start.checkout);
                }
            }
            Observation::Turn(turn) => {
                self.turns
                    .entry((turn.session.clone(), turn.id.clone()))
                    .or_insert_with(|| turn.clone());
            }
            Observation::Edit(edit) => {
                if let Some(transcript) = &edit.transcript {
                    self.locate(transcript, &edit.session, &edit.checkout);
                }
            }
            Observation::Command(command) => {
                if let Some(transcript) = &command.transcript {
                    self.locate(transcript, &command.session, &command.checkout);
                }
                self.commands
                    .entry(command.id.clone())
                    .or_insert_with(|| command.clone());
            }
            Observation::CaptureLimitation(limitation) => {
                self.sessions
                    .entry(limitation.transcript.clone())
                    .or_insert_with(|| limitation.session.clone());
                self.limitations
                    .entry(limitation.transcript.clone())
                    .or_default()
                    .entry(record.clone())
                    .or_default()
                    .push(limitation.clone());
            }
            Observation::SessionEnd(_) => {}
        }
    }

    /// Records that `transcript` belongs to `session` and names `checkout`.
    fn locate(&mut self, transcript: &str, session: &SessionId, checkout: &Path) {
        self.sessions
            .entry(transcript.to_owned())
            .or_insert_with(|| session.clone());
        let checkouts = self.checkouts.entry(transcript.to_owned()).or_default();
        if !checkouts.iter().any(|known| known == checkout) {
            checkouts.push(checkout.to_path_buf());
        }
    }

    /// The candidate transcripts: the union of
    /// the transcripts containing one of `edits` and the transcripts with an
    /// observation whose working directory lies in one of `worktrees`,
    /// whether or not any of their edits end up attributed.
    #[must_use]
    pub fn candidate_transcripts<'a>(
        &self,
        edits: impl IntoIterator<Item = &'a Edit>,
        worktrees: &[PathBuf],
    ) -> BTreeSet<String> {
        let mut transcripts: BTreeSet<String> = edits
            .into_iter()
            .filter_map(|e| e.transcript.clone())
            .collect();
        for (transcript, checkouts) in &self.checkouts {
            let inside = checkouts
                .iter()
                .any(|c| worktrees.iter().any(|w| within(c, w)));
            if inside {
                transcripts.insert(transcript.clone());
            }
        }
        transcripts
    }

    /// One entry per transcript in `transcripts`, with its session and the
    /// capture limitations of the record whose reports reach furthest into
    /// it, never localized to a path.
    ///
    /// A record reports a transcript's capture limitations once per sync
    /// that had any, over the lines that sync read, so one record's reports
    /// tile what it read and add up exactly. Another record holding the same
    /// transcript reports the same lines again, as identical copies or in
    /// other chunks; adding its reports would count one problem twice. On a
    /// tie, the first record by id gives them.
    #[must_use]
    pub fn transcripts(&self, transcripts: &BTreeSet<String>) -> Vec<TranscriptInfo> {
        transcripts
            .iter()
            .map(|transcript| TranscriptInfo {
                transcript: transcript.clone(),
                session: self
                    .sessions
                    .get(transcript)
                    .map(|s| s.as_str().to_owned())
                    .unwrap_or_default(),
                capture_limitations: self
                    .limitations
                    .get(transcript)
                    .and_then(|by_record| {
                        by_record.values().min_by_key(|reports| {
                            Reverse(reports.iter().map(|report| report.to_ordinal).max())
                        })
                    })
                    .cloned()
                    .unwrap_or_default(),
                localized: false,
            })
            .collect()
    }

    /// Commands from `transcripts` run in a checkout inside one of
    /// `worktrees`, in id order, to search for mentions of paths
    /// ([`CandidateCommands::mentions`]). Which worktrees contain each
    /// checkout, and the checkout's resolved spelling, are looked up once
    /// here rather than once per path.
    #[must_use]
    pub fn candidate_commands<'a>(
        &'a self,
        transcripts: &BTreeSet<String>,
        worktrees: &'a [PathBuf],
    ) -> CandidateCommands<'a> {
        let mut commands = Vec::new();
        for command in self.commands.values() {
            if !command
                .transcript
                .as_ref()
                .is_some_and(|t| transcripts.contains(t))
            {
                continue;
            }
            // Every worktree containing the checkout, not just the first:
            // with nested candidate worktrees the outer one contains the
            // inner one's checkouts too.
            let containing: Vec<&Path> = worktrees
                .iter()
                .filter(|w| within(&command.checkout, w))
                .map(PathBuf::as_path)
                .collect();
            if containing.is_empty() {
                continue;
            }
            commands.push(CandidateCommand {
                command,
                checkout: canonical(&command.checkout).unwrap_or_else(|_| command.checkout.clone()),
                worktrees: containing,
            });
        }
        CandidateCommands { commands }
    }

    /// The files that commands of `transcripts` changed in `worktrees`,
    /// as the harness reported them: each change with the file's path
    /// relative to its worktree, newest command first and by command id
    /// among equal times. Time orders the listing only.
    ///
    /// A reported file is placed as an edit's file is ([`map_file`]): in the
    /// innermost of `roots`, the registered worktrees, containing it. A
    /// command without a retained report contributes nothing, which says
    /// nothing about what it changed.
    #[must_use]
    pub fn command_changes(
        &self,
        transcripts: &BTreeSet<String>,
        roots: &[PathBuf],
        worktrees: &[PathBuf],
        ignore_case: bool,
    ) -> Vec<(CommandChange, String)> {
        let mut changes = Vec::new();
        for command in self.commands.values() {
            let from_candidate = command
                .transcript
                .as_ref()
                .is_some_and(|t| transcripts.contains(t));
            let Some(report) = command.changes.retained().filter(|_| from_candidate) else {
                continue;
            };
            for file in &report.files {
                let Some(mapped) = map_file(&command.checkout, &file.path, roots, ignore_case)
                else {
                    continue;
                };
                if !worktrees.contains(&mapped.worktree) {
                    continue;
                }
                changes.push((
                    CommandChange {
                        command: command.id.as_str().to_owned(),
                        session: command.session.as_str().to_owned(),
                        turn: command.turn.as_str().to_owned(),
                        transcript: command.transcript.clone(),
                        time: command.started.as_str().to_owned(),
                        checkout: command.checkout.display().to_string(),
                        worktree: mapped.worktree.display().to_string(),
                        text: command.cmd.clone(),
                        kind: file.kind,
                        patch: file.patch.clone(),
                        unlisted: report.unlisted,
                        outside: report.outside,
                        flags: report.flags.clone(),
                    },
                    mapped.path,
                ));
            }
        }
        changes.sort_by(|(a, _), (b, _)| {
            (Reverse(&a.time), &a.command).cmp(&(Reverse(&b.time), &b.command))
        });
        changes
    }

    /// The nearest recorded ancestor of `edit`'s turn, following parent
    /// pointers within its session, whose role is Human or Delegation;
    /// `None` when the chain breaks first. It is the message that preceded
    /// the edit in the conversation, not its cause or its rationale.
    #[must_use]
    pub fn prompt(&self, edit: &Edit) -> Option<PromptInfo> {
        let mut parent = self
            .turns
            .get(&(edit.session.clone(), edit.turn.clone()))?
            .parent
            .as_ref();
        let mut steps = 0;
        while let Some(id) = parent {
            steps += 1;
            if steps > self.turns.len() {
                return None;
            }
            let turn = self.turns.get(&(edit.session.clone(), id.clone()))?;
            if matches!(turn.role, Role::Human | Role::Delegation) {
                return Some(PromptInfo {
                    turn: turn.id.as_str().to_owned(),
                    role: turn.role,
                    excerpt: turn.excerpt.clone(),
                });
            }
            parent = turn.parent.as_ref();
        }
        None
    }
}

/// The commands a review searches for mentions of its paths
/// ([`Evidence::candidate_commands`]).
#[derive(Debug)]
pub struct CandidateCommands<'a> {
    /// Each command whose checkout lies in a searched worktree, in id order.
    commands: Vec<CandidateCommand<'a>>,
}

/// A command whose checkout lies in a searched worktree.
#[derive(Debug)]
struct CandidateCommand<'a> {
    /// The command.
    command: &'a Command,
    /// Its checkout, canonical when that can be resolved, as written
    /// otherwise.
    checkout: PathBuf,
    /// The searched worktrees containing its checkout, in their order.
    worktrees: Vec<&'a Path>,
}

impl CandidateCommands<'_> {
    /// The commands whose text contains `path` (relative to a worktree that
    /// contains the checkout) as seen from the command's checkout, through
    /// `..` when the file is not below it, or as an absolute path. Textual
    /// evidence only: a mention is never attribution, and commands are
    /// never replayed.
    #[must_use]
    pub fn mentions(&self, path: &str) -> Vec<Mention> {
        self.commands
            .iter()
            .filter(|candidate| {
                let text = &candidate.command.cmd;
                candidate.worktrees.iter().any(|worktree| {
                    let absolute = worktree.join(path).to_string_lossy().into_owned();
                    let relative = relative_to_checkout(&candidate.checkout, worktree, path);
                    text.contains(&absolute) || relative.is_some_and(|r| text.contains(r.as_str()))
                })
            })
            .map(|candidate| mention(candidate.command))
            .collect()
    }
}

/// The mention entry for `command`.
fn mention(command: &Command) -> Mention {
    Mention {
        command: command.id.as_str().to_owned(),
        session: command.session.as_str().to_owned(),
        turn: command.turn.as_str().to_owned(),
        checkout: command.checkout.display().to_string(),
        text: command.cmd.clone(),
        result: command.ended.as_ref().map(|_| MentionResult {
            exit: command.exit,
            outcome: command.outcome.map(outcome_word),
        }),
    }
}

/// Adds to `edits`, keyed by edit id, the entry of each of `accepted` that
/// `referenced` names: an edit offered to anchor that no chain, stop reason,
/// or outcome names stays out. An id already in `edits` keeps its entry.
pub fn add_referenced_edits(
    edits: &mut BTreeMap<String, EditInfo>,
    accepted: &[AcceptedEdit],
    referenced: &BTreeSet<EventId>,
    evidence: &Evidence,
) {
    for accepted in accepted {
        if referenced.contains(&accepted.edit.id) {
            edits
                .entry(accepted.edit.id.as_str().to_owned())
                .or_insert_with(|| edit_info(accepted, evidence));
        }
    }
}

/// The review's `edits` map entry for `accepted`.
fn edit_info(accepted: &AcceptedEdit, evidence: &Evidence) -> EditInfo {
    let edit = &accepted.edit;
    EditInfo {
        session: edit.session.as_str().to_owned(),
        transcript: edit.transcript.clone(),
        turn: edit.turn.as_str().to_owned(),
        time: edit.time.as_str().to_owned(),
        worktree: accepted.worktree.display().to_string(),
        operation: edit.operation,
        origin: match &edit.origin {
            EditOrigin::Tool => EditOriginInfo::Tool,
            EditOrigin::Shell { command } => EditOriginInfo::Shell {
                command: command.as_str().to_owned(),
                text: evidence.commands.get(command).map(|c| c.cmd.clone()),
            },
        },
        prompt: evidence.prompt(edit),
    }
}

/// `path` (relative to `worktree`, `/`-separated) as seen from `checkout`,
/// a directory inside it, through their common ancestor: one `..` for each
/// directory climbed, so the root file `old.txt` is `../old.txt` from `src`.
/// `checkout` is compared as given, so callers resolve it first. `None` when
/// `checkout` is not inside `worktree`, a component of it is not a plain
/// UTF-8 name, or `path` names `checkout` or one of its ancestors.
fn relative_to_checkout(checkout: &Path, worktree: &Path, path: &str) -> Option<String> {
    let below = checkout.strip_prefix(worktree).ok()?;
    let from = below
        .components()
        .map(|c| match c {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let to: Vec<&str> = path.split('/').collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    if common == to.len() {
        return None;
    }
    let mut parts = vec![".."; from.len() - common];
    parts.extend_from_slice(&to[common..]);
    Some(parts.join("/"))
}

/// The JSON word for a command outcome.
fn outcome_word(outcome: Outcome) -> String {
    match outcome {
        Outcome::Passed => "passed",
        Outcome::Failed => "failed",
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use supersigil_anchor::walk::AcceptedEdit;
    use supersigil_record::observations::{
        CaptureCounts, CaptureLimitation, ChangeKind, ChangeReport, Command, CommandCategory, Edit,
        EditOperation, EditOrigin, FileChange, FileState, Hunk, Material, Observation, Role, Turn,
    };
    use supersigil_record::{EventId, RecordId, SessionId, Timestamp, TurnId};
    use supersigil_review::model::{CommandChange, EditOriginInfo};

    use super::{Evidence, edit_info};

    /// Indexes the observations of each record, named by its id.
    fn index_records(records: &[(&str, Vec<Observation>)]) -> Evidence {
        let ids: Vec<RecordId> = records.iter().map(|(id, _)| RecordId::new(*id)).collect();
        Evidence::index(
            ids.iter()
                .zip(records)
                .map(|(id, (_, observations))| (id, observations.as_slice())),
        )
    }

    /// Indexes `observations` as the one record `r1` holds.
    fn index(observations: &[Observation]) -> Evidence {
        index_records(&[("r1", observations.to_vec())])
    }

    /// A capture limitation of `t.jsonl` over ordinals `from..to`.
    fn limitation(from: u64, to: u64, counts: CaptureCounts) -> CaptureLimitation {
        CaptureLimitation {
            session: SessionId::new("s1"),
            transcript: "t.jsonl".to_owned(),
            from_ordinal: from,
            to_ordinal: to,
            counts,
        }
    }

    /// Counts of `malformed` malformed lines and `unsupported` unsupported
    /// tool uses.
    fn counts(malformed: u64, unsupported: u64) -> CaptureCounts {
        CaptureCounts {
            malformed_lines: malformed,
            unsupported_tool_uses: unsupported,
            ..CaptureCounts::default()
        }
    }

    /// The capture limitations `evidence` lists for `t.jsonl`, and their
    /// total as the terminal header sums it.
    fn listed(evidence: &Evidence) -> (Vec<CaptureLimitation>, CaptureCounts) {
        let info = evidence.transcripts(&BTreeSet::from(["t.jsonl".to_owned()]));
        let reports = info[0].capture_limitations.clone();
        let total = reports.iter().map(|report| &report.counts).sum();
        (reports, total)
    }

    #[test]
    fn a_capture_limitation_two_records_hold_counts_once() {
        let report = limitation(0, 10, counts(1, 0));
        let copy = || vec![Observation::CaptureLimitation(report.clone())];
        let evidence = index_records(&[("r1", copy()), ("r2", copy())]);
        assert_eq!(listed(&evidence), (vec![report.clone()], counts(1, 0)));
    }

    #[test]
    fn one_records_disjoint_capture_limitations_are_all_kept() {
        let reports = [
            limitation(0, 10, counts(1, 0)),
            limitation(10, 20, counts(0, 1)),
        ];
        let evidence = index_records(&[(
            "r1",
            reports
                .iter()
                .cloned()
                .map(Observation::CaptureLimitation)
                .collect(),
        )]);
        assert_eq!(listed(&evidence), (reports.to_vec(), counts(1, 1)));
    }

    #[test]
    fn the_record_reaching_furthest_into_a_transcript_gives_its_limitations() {
        // Malformed lines at ordinals 5 and 12, an unsupported tool use at
        // 25. Record r1 read the transcript to ordinal 15 in one sync; r2
        // read it to 30 in two. Each record's reports tile what it read, so
        // r2's add up to the exact totals of the longer prefix, while adding
        // both records' reports would count the malformed lines twice.
        let shorter = vec![Observation::CaptureLimitation(limitation(
            0,
            15,
            counts(2, 0),
        ))];
        let longer = [
            limitation(0, 10, counts(1, 0)),
            limitation(10, 30, counts(1, 1)),
        ];
        let evidence = index_records(&[
            ("r1", shorter),
            (
                "r2",
                longer
                    .iter()
                    .cloned()
                    .map(Observation::CaptureLimitation)
                    .collect(),
            ),
        ]);
        assert_eq!(listed(&evidence), (longer.to_vec(), counts(2, 1)));
    }

    fn turn(id: &str, parent: Option<&str>, role: Role) -> Observation {
        Observation::Turn(Turn {
            id: TurnId::new(id),
            session: SessionId::new("s1"),
            parent: parent.map(TurnId::new),
            role,
            time: Timestamp::new("2026-09-29T10:00:00.000Z"),
            sidechain: false,
            agent_id: None,
            excerpt: Material::Retained(format!("text of {id}")),
            source_ordinal: 0,
            transcript: Some("t.jsonl".to_owned()),
        })
    }

    fn edit_in_turn(turn: &str) -> Edit {
        Edit {
            id: EventId::new("e1"),
            turn: TurnId::new(turn),
            session: SessionId::new("s1"),
            path: PathBuf::from("a.rs"),
            before: FileState::unknown(),
            after: FileState::unknown(),
            patch: Material::unavailable("test"),
            old_text: Material::unavailable("test"),
            new_text: Material::unavailable("test"),
            replace_all: false,
            operation: EditOperation::Replace,
            origin: EditOrigin::Tool,
            checkout: PathBuf::from("/work/repo"),
            time: Timestamp::new("2026-09-29T10:00:00.000Z"),
            source_ordinal: 0,
            agent_id: None,
            transcript: Some("t.jsonl".to_owned()),
        }
    }

    fn command(id: &str, cmd: &str, checkout: &Path, transcript: &str) -> Observation {
        Observation::Command(Command {
            id: EventId::new(id),
            turn: TurnId::new("a1"),
            session: SessionId::new("s1"),
            cmd: cmd.to_owned(),
            exit: Some(0),
            stdout_tail: Material::unavailable("test"),
            stderr_tail: Material::unavailable("test"),
            category: CommandCategory::Other,
            reported_error: false,
            outcome: None,
            started: Timestamp::new("2026-09-29T10:00:00.000Z"),
            ended: Some(Timestamp::new("2026-09-29T10:00:01.000Z")),
            checkout: checkout.to_path_buf(),
            changes: Material::unavailable("test"),
            source_ordinal: 0,
            agent_id: None,
            transcript: Some(transcript.to_owned()),
        })
    }

    #[test]
    fn prompt_is_the_nearest_human_ancestor() {
        let observations = [
            turn("u1", None, Role::Human),
            turn("a1", Some("u1"), Role::Agent),
            turn("u2", Some("a1"), Role::Tool),
            turn("a2", Some("u2"), Role::Agent),
        ];
        let evidence = index(&observations);
        let prompt = evidence.prompt(&edit_in_turn("a2")).unwrap();
        assert_eq!(prompt.turn, "u1");
        assert_eq!(prompt.role, Role::Human);
        assert_eq!(prompt.excerpt, Material::Retained("text of u1".to_owned()));
    }

    #[test]
    fn a_broken_parent_chain_has_no_prompt() {
        let observations = [turn("a2", Some("missing"), Role::Agent)];
        let evidence = index(&observations);
        assert_eq!(evidence.prompt(&edit_in_turn("a2")), None);
        assert_eq!(evidence.prompt(&edit_in_turn("unknown")), None);
    }

    #[test]
    fn mentions_match_absolute_and_checkout_relative_paths() {
        let repo = PathBuf::from("/work/repo");
        let observations = [
            command("c1", "rm old.txt", &repo, "t.jsonl"),
            command("c2", "cat lib.rs", &repo.join("src"), "t.jsonl"),
            command(
                "c3",
                "cat /work/repo/src/lib.rs",
                Path::new("/elsewhere"),
                "t.jsonl",
            ),
            command("c4", "rm old.txt", &repo, "other.jsonl"),
        ];
        let evidence = index(&observations);
        let transcripts = BTreeSet::from(["t.jsonl".to_owned()]);
        let worktrees = [repo];
        let ids = |path: &str| -> Vec<String> {
            evidence
                .candidate_commands(&transcripts, &worktrees)
                .mentions(path)
                .into_iter()
                .map(|m| m.command)
                .collect()
        };
        assert_eq!(ids("old.txt"), vec!["c1"]);
        // c3 ran outside the file's worktree; c4 is not a candidate transcript.
        assert_eq!(ids("src/lib.rs"), vec!["c2"]);
    }

    #[test]
    fn mentions_match_paths_relative_through_a_parent_directory() {
        let repo = PathBuf::from("/work/repo");
        let src = repo.join("src");
        let observations = [
            // A root file deleted from a subdirectory.
            command("c1", "rm ../old.txt", &src, "t.jsonl"),
            // A file in a sibling directory, from one and two levels down.
            command("c2", "rm ../docs/old.md", &src, "t.jsonl"),
            command("c3", "rm ../../docs/old.md", &src.join("deep"), "t.jsonl"),
            // From the subdirectory, `old.txt` is src/old.txt, not the root
            // file.
            command("c4", "rm old.txt", &src, "t.jsonl"),
            // A file beside the checkout, through their common parent.
            command("c5", "cat ../lib.rs", &src.join("deep"), "t.jsonl"),
        ];
        let evidence = index(&observations);
        let transcripts = BTreeSet::from(["t.jsonl".to_owned()]);
        let worktrees = [repo];
        let ids = |path: &str| -> Vec<String> {
            evidence
                .candidate_commands(&transcripts, &worktrees)
                .mentions(path)
                .into_iter()
                .map(|m| m.command)
                .collect()
        };
        assert_eq!(ids("old.txt"), vec!["c1"]);
        assert_eq!(ids("docs/old.md"), vec!["c2", "c3"]);
        assert_eq!(ids("src/lib.rs"), vec!["c5"]);
    }

    #[test]
    fn mentions_are_found_in_a_nested_candidate_worktree() {
        let repo = PathBuf::from("/work/repo");
        let nested = repo.join(".claude/worktrees/x");
        let observations = [command("c1", "rm old.txt", &nested, "t.jsonl")];
        let evidence = index(&observations);
        let transcripts = BTreeSet::from(["t.jsonl".to_owned()]);
        // Both worktrees originated reviewed commits; sorted, the outer one
        // comes first and also contains the command's checkout.
        let worktrees = [repo, nested];
        let ids: Vec<String> = evidence
            .candidate_commands(&transcripts, &worktrees)
            .mentions("old.txt")
            .into_iter()
            .map(|m| m.command)
            .collect();
        assert_eq!(ids, vec!["c1"]);
    }

    /// A command issued at second `second` whose change report names
    /// `files`, each as modified with no hunks retained.
    fn changing(
        id: &str,
        second: u32,
        checkout: &Path,
        transcript: &str,
        files: &[&str],
    ) -> Observation {
        let Observation::Command(mut command) = command(id, "make", checkout, transcript) else {
            unreachable!("`command` builds a command");
        };
        command.started = Timestamp::new(format!("2026-09-29T10:00:{second:02}.000Z"));
        command.changes = Material::Retained(ChangeReport {
            files: files
                .iter()
                .map(|path| FileChange {
                    path: PathBuf::from(path),
                    kind: ChangeKind::Modified,
                    patch: Material::unavailable("test"),
                })
                .collect(),
            ..ChangeReport::default()
        });
        Observation::Command(command)
    }

    /// `(command, path)` of every change `evidence` lists for `t.jsonl` in
    /// `worktrees`, with `roots` as the registered worktrees.
    fn changes(
        evidence: &Evidence,
        roots: &[PathBuf],
        worktrees: &[PathBuf],
    ) -> Vec<(String, String)> {
        evidence
            .command_changes(
                &BTreeSet::from(["t.jsonl".to_owned()]),
                roots,
                worktrees,
                false,
            )
            .into_iter()
            .map(|(change, path)| (change.command, path))
            .collect()
    }

    fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
        expected
            .iter()
            .map(|(command, path)| ((*command).to_owned(), (*path).to_owned()))
            .collect()
    }

    #[test]
    fn command_changes_are_listed_newest_first_by_worktree_path() {
        let repo = PathBuf::from("/work/repo");
        let observations = [
            changing("c1", 1, &repo, "t.jsonl", &["src/lib.rs", "notes.txt"]),
            // From a subdirectory, the path is relative to it.
            changing("c2", 3, &repo.join("src"), "t.jsonl", &["lib.rs"]),
            // Equal times fall back to the command id.
            changing("c4", 2, &repo, "t.jsonl", &["src/lib.rs"]),
            changing("c3", 2, &repo, "t.jsonl", &["src/lib.rs"]),
            // Not a candidate transcript, and a command without a report.
            changing("c5", 9, &repo, "other.jsonl", &["src/lib.rs"]),
            command("c6", "rm src/lib.rs", &repo, "t.jsonl"),
        ];
        let evidence = index(&observations);
        let roots = [repo];
        assert_eq!(
            changes(&evidence, &roots, &roots),
            pairs(&[
                ("c2", "src/lib.rs"),
                ("c3", "src/lib.rs"),
                ("c4", "src/lib.rs"),
                ("c1", "src/lib.rs"),
                ("c1", "notes.txt"),
            ])
        );
    }

    #[test]
    fn a_command_change_belongs_to_the_innermost_worktree_holding_the_file() {
        let repo = PathBuf::from("/work/repo");
        let nested = repo.join(".claude/worktrees/x");
        let observations = [
            // Run from the main checkout, changing a file of the nested one.
            changing(
                "c1",
                1,
                &repo,
                "t.jsonl",
                &[".claude/worktrees/x/a.rs", "a.rs"],
            ),
            changing("c2", 2, &nested, "t.jsonl", &["a.rs"]),
            // Outside every registered worktree.
            changing("c3", 3, Path::new("/elsewhere"), "t.jsonl", &["a.rs"]),
        ];
        let evidence = index(&observations);
        let roots = [repo.clone(), nested.clone()];
        assert_eq!(
            changes(&evidence, &roots, std::slice::from_ref(&repo)),
            pairs(&[("c1", "a.rs")])
        );
        assert_eq!(
            changes(&evidence, &roots, std::slice::from_ref(&nested)),
            pairs(&[("c2", "a.rs"), ("c1", "a.rs")])
        );
        // With both observed, one command is listed once per worktree it
        // changed the path in, each entry naming its worktree.
        let both = evidence.command_changes(
            &BTreeSet::from(["t.jsonl".to_owned()]),
            &roots,
            &roots,
            false,
        );
        let listed: Vec<(&str, PathBuf)> = both
            .iter()
            .map(|(change, _)| (change.command.as_str(), PathBuf::from(&change.worktree)))
            .collect();
        assert_eq!(
            listed,
            vec![("c2", nested.clone()), ("c1", nested), ("c1", repo)]
        );
    }

    #[test]
    fn a_command_two_records_hold_changes_its_files_once() {
        let repo = PathBuf::from("/work/repo");
        let copy = || vec![changing("c1", 1, &repo, "t.jsonl", &["a.rs"])];
        let evidence = index_records(&[("r1", copy()), ("r2", copy())]);
        let roots = [repo];
        assert_eq!(changes(&evidence, &roots, &roots), pairs(&[("c1", "a.rs")]));
    }

    #[test]
    fn a_command_change_carries_the_command_and_what_the_harness_reported() {
        let repo = PathBuf::from("/work/repo");
        let Observation::Command(mut recorded) = changing("c1", 1, &repo, "t.jsonl", &[]) else {
            unreachable!("`changing` builds a command");
        };
        let hunk = Hunk {
            old_start: 1,
            old_lines: 0,
            new_start: 1,
            new_lines: 1,
            lines: vec!["+x".to_owned()],
        };
        recorded.changes = Material::Retained(ChangeReport {
            files: vec![FileChange {
                path: PathBuf::from("a.rs"),
                kind: ChangeKind::Created,
                patch: Material::Retained(vec![hunk.clone()]),
            }],
            outside: 1,
            unlisted: 3,
            flags: BTreeSet::from(["shared".to_owned()]),
        });
        let evidence = index(&[Observation::Command(recorded)]);
        let roots = [repo];
        let found = evidence.command_changes(
            &BTreeSet::from(["t.jsonl".to_owned()]),
            &roots,
            &roots,
            false,
        );
        assert_eq!(
            found,
            vec![(
                CommandChange {
                    command: "c1".to_owned(),
                    session: "s1".to_owned(),
                    turn: "a1".to_owned(),
                    transcript: Some("t.jsonl".to_owned()),
                    time: "2026-09-29T10:00:01.000Z".to_owned(),
                    checkout: "/work/repo".to_owned(),
                    worktree: "/work/repo".to_owned(),
                    text: "make".to_owned(),
                    kind: ChangeKind::Created,
                    patch: Material::Retained(vec![hunk]),
                    unlisted: 3,
                    outside: 1,
                    flags: BTreeSet::from(["shared".to_owned()]),
                },
                "a.rs".to_owned()
            )]
        );
    }

    #[test]
    fn a_shell_edit_names_its_command_and_the_text_when_recorded() {
        let repo = PathBuf::from("/work/repo");
        let evidence = index(&[command("c1", "cat > a.rs <<'EOF'", &repo, "t.jsonl")]);
        let info = |command: &str| {
            let mut edit = edit_in_turn("a1");
            edit.origin = EditOrigin::Shell {
                command: EventId::new(command),
            };
            let accepted = AcceptedEdit {
                edit,
                worktree: repo.clone(),
                records: vec![RecordId::new("r1")],
            };
            edit_info(&accepted, &evidence).origin
        };
        assert_eq!(
            info("c1"),
            EditOriginInfo::Shell {
                command: "c1".to_owned(),
                text: Some("cat > a.rs <<'EOF'".to_owned()),
            }
        );
        // The command may be in a record the review did not read.
        assert_eq!(
            info("c9"),
            EditOriginInfo::Shell {
                command: "c9".to_owned(),
                text: None,
            }
        );
    }
}
