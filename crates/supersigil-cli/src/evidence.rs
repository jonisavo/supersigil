//! The recorded evidence around attribution: which transcripts a review
//! draws on and what their capture missed, commands whose text mentions a
//! file, and the message that preceded each edit.
//!
//! Everything here is an observation, never attribution: a mention is text
//! that contains a path, and a prompt is the nearest earlier Human or
//! Delegation message in the conversation, not a cause.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use supersigil_anchor::walk::AcceptedEdit;
use supersigil_record::observations::{
    CaptureLimitation, Command, Edit, Observation, Outcome, Role, Turn,
};
use supersigil_record::{EventId, SessionId, TurnId};
use supersigil_review::model::{EditInfo, Mention, MentionResult, PromptInfo, TranscriptInfo};
use supersigil_session::checkout::{canonical, within};

/// Turns, commands, capture limitations, and transcript locations of every
/// pinned record, indexed for the lookups a review makes.
#[derive(Debug, Default)]
pub struct Evidence {
    /// Every turn by session and id; the first occurrence wins.
    turns: BTreeMap<(SessionId, TurnId), Turn>,
    /// Every command by id; the first occurrence wins.
    commands: BTreeMap<EventId, Command>,
    /// Capture limitations by transcript.
    limitations: BTreeMap<String, Vec<CaptureLimitation>>,
    /// The session each transcript belongs to.
    sessions: BTreeMap<String, SessionId>,
    /// Working directories each transcript's observations name.
    checkouts: BTreeMap<String, Vec<PathBuf>>,
}

impl Evidence {
    /// Indexes `observations`, keeping the first occurrence of each turn and
    /// command.
    #[must_use]
    pub fn index<'a>(observations: impl IntoIterator<Item = &'a Observation>) -> Self {
        let mut evidence = Self::default();
        for observation in observations {
            match observation {
                Observation::SessionStart(start) => {
                    if let Some(path) = start.source_ids.get("path") {
                        evidence.locate(path, &start.session, &start.checkout);
                    }
                }
                Observation::Turn(turn) => {
                    evidence
                        .turns
                        .entry((turn.session.clone(), turn.id.clone()))
                        .or_insert_with(|| turn.clone());
                }
                Observation::Edit(edit) => {
                    if let Some(transcript) = &edit.transcript {
                        evidence.locate(transcript, &edit.session, &edit.checkout);
                    }
                }
                Observation::Command(command) => {
                    if let Some(transcript) = &command.transcript {
                        evidence.locate(transcript, &command.session, &command.checkout);
                    }
                    evidence
                        .commands
                        .entry(command.id.clone())
                        .or_insert_with(|| command.clone());
                }
                Observation::CaptureLimitation(limitation) => {
                    evidence
                        .sessions
                        .entry(limitation.transcript.clone())
                        .or_insert_with(|| limitation.session.clone());
                    evidence
                        .limitations
                        .entry(limitation.transcript.clone())
                        .or_default()
                        .push(limitation.clone());
                }
                Observation::SessionEnd(_) => {}
            }
        }
        evidence
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

    /// The candidate transcripts (design section 1, step 7): the union of
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

    /// One entry per transcript in `transcripts`, with its session and every
    /// capture limitation recorded for it, never localized to a path.
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

    use supersigil_record::observations::{
        Command, CommandCategory, Edit, EditOperation, FileState, Material, Observation, Role, Turn,
    };
    use supersigil_record::{EventId, SessionId, Timestamp, TurnId};

    use super::Evidence;

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
        let evidence = Evidence::index(&observations);
        let prompt = evidence.prompt(&edit_in_turn("a2")).unwrap();
        assert_eq!(prompt.turn, "u1");
        assert_eq!(prompt.role, Role::Human);
        assert_eq!(prompt.excerpt, Material::Retained("text of u1".to_owned()));
    }

    #[test]
    fn a_broken_parent_chain_has_no_prompt() {
        let observations = [turn("a2", Some("missing"), Role::Agent)];
        let evidence = Evidence::index(&observations);
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
        let evidence = Evidence::index(&observations);
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
        let evidence = Evidence::index(&observations);
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
        let evidence = Evidence::index(&observations);
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
}
