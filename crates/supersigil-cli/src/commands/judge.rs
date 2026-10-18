//! `supersigil judge` records a person's judgments on a saved review.
//! Acknowledgments and dismissals close gate items. Comments close nothing.
//! Corrections dispute or add to an annotated decision the review shows,
//! named from the saved review alone, and close nothing either. Completions
//! record that a review is done. Undos withdraw earlier judgments.
//!
//! Write commands refuse to run inside agent sessions. They take the review
//! store's writer lock before reading the saved review, then resolve the review,
//! check the call, retain its snapshot under a ref on its first judgment, and
//! append the judgment. The lock prevents cleanup from removing the review
//! between these steps. These commands read the saved review without inspecting
//! the working tree. Refused calls record nothing and explain why.
//!
//! Agents may run `list` and `show`. These read commands take no lock and
//! create no store.

mod list;
mod show;

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use serde::Serialize;
use supersigil_git::retention::{self, Identity};
use supersigil_git::{Git, ObjectId, Repo};
use supersigil_record::reviews::{
    self, Basis, BasisDecision, BasisId, BasisTarget, CorrectionKind, ItemId, Judgment,
    JudgmentKind, Origin, ReviewSnapshot, ReviewStore, Who,
};
use supersigil_record::time::UtcTime;
use supersigil_record::{JudgmentId, StoreError};
use supersigil_review::judging::{
    AlreadyClosed, Call, FindJudgmentError, ItemState, JudgedStore, Ledger, NotClosed, Refusal,
    WithdrawRefusal, Words,
};
use supersigil_review::model::{GateInfo, JudgmentInfo, StatusInfo};
use supersigil_review::names::{ItemNames, Name, Parsed, parse};
use supersigil_review::printed::{Addressing, Printed, Shell};
use supersigil_review::summary::{closed_word, compared, next_lines, noun};
use supersigil_session::checkout::canonical;

use crate::agent::in_agent_session;
use crate::commands::{JudgeArgs, JudgeCommand};
use crate::error::CliError;
use crate::format::{OutputFormat, write_json};
use crate::item_names::{item_names, name};
use crate::reconcile::LOCK_WAIT;
use crate::record_dir::begin_within;
use crate::record_dir::canonical_checkout;
use crate::review_dir::{canonical_common_dir, judged_bases, resolve_review_dir};

/// Refusal message for an agent session. It never suggests unsetting the marker.
const AGENT_SESSION: &str = "Judgments must be made by a person; this command is running inside \
                             a Claude Code session. The agent can list the open items \
                             (supersigil judge show) and prepare the exact command for you to \
                             inspect and run outside the agent.";

/// Refusal message for a missing git identity, using `git commit`'s wording.
const NO_IDENTITY: &str = "Author identity unknown

*** Please tell me who you are.

Run

  git config --global user.email \"you@example.com\"
  git config --global user.name \"Your Name\"

to set your account's default identity.
Omit --global to set the identity only in this repository.";

/// Runs `supersigil judge`.
///
/// # Errors
///
/// Returns [`CliError::Refused`] when the call is refused, recording
/// nothing; [`CliError::Git`] when the repository cannot be opened or git
/// fails; [`CliError::Record`] when the review store cannot be read or
/// written; and an I/O error when writing the output fails.
pub fn run(args: &JudgeArgs) -> Result<(), CliError> {
    match &args.command {
        JudgeCommand::List => list::run(args),
        JudgeCommand::Show(show) => show::run(args, show),
        command => write(args, command).map_err(|error| match error {
            CliError::Refused(message) => {
                CliError::Refused(format!("No judgment recorded. {message}"))
            }
            error => error,
        }),
    }
}

/// The call `command` makes, its items named as typed.
///
/// # Errors
///
/// Refuses a dismissal without a reason, and a blank reason, text, or
/// constraint.
fn call(command: &JudgeCommand) -> Result<Call<'_, String>, CliError> {
    Ok(match command {
        JudgeCommand::Acknowledge(close) => Call::Acknowledge {
            items: &close.items,
            reason: reason(close.reason.as_deref())?,
        },
        JudgeCommand::Dismiss(close) => Call::Dismiss {
            items: &close.items,
            reason: close
                .reason
                .as_deref()
                .and_then(Words::new)
                .ok_or_else(|| CliError::refused(["A dismissal needs --reason."].into()))?,
        },
        JudgeCommand::Comment(comment) => Call::Comment {
            item: comment.item.as_ref(),
            text: Words::new(&comment.text)
                .ok_or_else(|| CliError::refused(["A comment's text can't be blank."].into()))?,
        },
        JudgeCommand::Correct(correct) => {
            let (kind, words) = match &correct.constraint {
                Some(constraint) => (
                    CorrectionKind::Constraint,
                    Words::new(constraint).ok_or_else(|| {
                        CliError::refused(["A constraint can't be blank."].into())
                    })?,
                ),
                None => (
                    if correct.wrong {
                        CorrectionKind::Wrong
                    } else {
                        CorrectionKind::NotTogether
                    },
                    // Clap requires --reason with either dispute.
                    correct
                        .reason
                        .as_deref()
                        .and_then(Words::new)
                        .ok_or_else(|| CliError::refused(["A reason can't be blank."].into()))?,
                ),
            };
            Call::Correct {
                decision: &correct.decision,
                kind,
                words,
            }
        }
        JudgeCommand::Complete(complete) => Call::Complete {
            text: complete
                .text
                .as_deref()
                .map(|text| {
                    Words::new(text).ok_or_else(|| {
                        CliError::refused(
                            ["A completion's text can't be blank; give some or leave out --text."]
                                .into(),
                        )
                    })
                })
                .transpose()?,
        },
        JudgeCommand::Undo(undo) => match &undo.judgment {
            Some(prefix) => Call::UndoJudgment {
                prefix,
                reason: reason(undo.reason.as_deref())?,
            },
            None => Call::UndoItems {
                items: &undo.items,
                reason: reason(undo.reason.as_deref())?,
            },
        },
        JudgeCommand::List | JudgeCommand::Show(_) => unreachable!("not a write command"),
    })
}

/// The reason given, if any.
///
/// # Errors
///
/// Refuses a blank reason.
fn reason(given: Option<&str>) -> Result<Option<Words<'_>>, CliError> {
    given
        .map(|text| {
            Words::new(text).ok_or_else(|| {
                CliError::refused(
                    ["A reason can't be blank; give one or leave out --reason."].into(),
                )
            })
        })
        .transpose()
}

/// The checkout a call names, with its repository's review store.
struct Checkout {
    /// The checkout's repository.
    repo: Repo,
    /// The canonical path of its worktree.
    worktree: PathBuf,
    /// The canonical current directory. Typed names are relative to it.
    cwd: PathBuf,
    /// The repository's review store, found without creating one; `None`
    /// when no review was saved for the repository.
    store: Option<ReviewStore>,
}

impl Checkout {
    /// Opens the checkout `args` names and finds its repository's review
    /// store in the reviews directory `args` names.
    ///
    /// # Errors
    ///
    /// Returns [`CliError::Git`] when the repository cannot be opened,
    /// [`CliError::CommandFailed`] when no reviews directory can be
    /// resolved, an I/O error when a path cannot be made canonical, and the
    /// store errors of [`reviews::find`].
    fn open(args: &JudgeArgs) -> Result<Self, CliError> {
        let reviews_dir = resolve_review_dir(args.review_dir.as_deref())?;
        let repo = Repo::open(Git::new(&canonical_checkout(args.checkout.as_deref())?))?;
        let worktree = canonical(repo.root())?;
        let cwd = canonical_checkout(None)?;
        let store = reviews::find(&reviews_dir, &canonical_common_dir(&repo)?)?;
        Ok(Self {
            repo,
            worktree,
            cwd,
            store,
        })
    }

    /// Returns the repository's review store for `prefix` or the worktree's selection.
    ///
    /// # Errors
    ///
    /// Refuses if the repository has no saved review. Reports an unknown review
    /// when `prefix` is present, or no selection otherwise. Uses `addressing`
    /// for commands in the refusal.
    fn store(
        &self,
        prefix: Option<&str>,
        addressing: &Addressing,
    ) -> Result<&ReviewStore, CliError> {
        self.store.as_ref().ok_or_else(|| match prefix {
            Some(prefix) => unknown_review(prefix, addressing),
            None => no_selection(&self.worktree, addressing),
        })
    }

    /// The names of `basis`'s gate items as typed from the current
    /// directory.
    ///
    /// # Errors
    ///
    /// Returns [`CliError::Git`] when git cannot read `core.ignorecase`.
    fn names<'g>(&self, basis: &'g Basis) -> Result<ItemNames<'g>, CliError> {
        let ignore_case = self.repo.config_bool("core.ignorecase")?.unwrap_or(false);
        Ok(item_names(basis, ignore_case, &self.worktree, &self.cwd))
    }
}

/// Addresses printed commands with `args`'s `--checkout` and `--review-dir`.
/// [`select`] adds the review once resolved.
fn addressing(args: &JudgeArgs) -> Addressing {
    Addressing {
        review: None,
        checkout: args.checkout.clone(),
        review_dir: args.review_dir.clone(),
        shell: Shell::current(),
        ..Addressing::default()
    }
}

/// Records the judgment `command` makes on the review `args` selects,
/// then prints what it recorded, the gate it leaves, and what to run next.
///
/// # Errors
///
/// As [`run`]; a refusal's message here lacks its "No judgment recorded."
fn write(args: &JudgeArgs, command: &JudgeCommand) -> Result<(), CliError> {
    if in_agent_session(|name| std::env::var_os(name)) {
        return Err(CliError::refused([AGENT_SESSION].into()));
    }
    let call = call(command)?;
    let mut addressing = addressing(args);
    let checkout = Checkout::open(args)?;
    let store = checkout.store(args.review.as_deref(), &addressing)?;
    let Some(mut tx) = begin_within(LOCK_WAIT, || store.begin())? else {
        return Err(CliError::refused(
            ["The review store is busy; try again."].into(),
        ));
    };
    // Under the writer lock, the published manifest is the one the
    // transaction read.
    let snapshot = store.snapshot()?;
    let id = select(
        &snapshot,
        args.review.as_deref(),
        &checkout.worktree,
        &mut addressing,
    )?;
    let basis = snapshot.basis(&id)?;
    let mut judgments = tx.judgments().to_vec();
    let others = judged_bases(&snapshot, &judgments, Some(&id), &basis)?;
    let names = checkout.names(&basis)?;
    let items = resolve(&names, call.items(), |text| {
        name(&checkout.worktree, &checkout.cwd, text)
    })?;
    let judged = JudgedStore::new(&judgments, &others);
    let planned = judged
        .review(&id, &basis)
        .plan(call.naming(&items))
        .map_err(|refused| refused_call(&refused, call, &id, &names, &addressing))?;
    let who = retention::identity(&checkout.repo)?
        .ok_or_else(|| CliError::refused(NO_IDENTITY.lines().collect()))?;
    let first = planned.first();
    if first {
        keep(&checkout.repo, &id, &basis, &who, &addressing)?;
    }
    let now = UtcTime::now();
    let who = Who {
        name: who.name,
        email: who.email,
    };
    let judgment = planned.record(JudgmentId::generate(), who, now);
    tx.append_judgment(&judgment)?;
    tx.commit(now)?;

    judgments.push(judgment.clone());
    let judged = JudgedStore::new(&judgments, &others);
    let review = judged.review(&id, &basis);
    let after = review.report(&names);
    let recorded = Recorded {
        judgment: &judgment,
        review: &id,
        target: &basis.target,
        decisions: &basis.decisions,
        status: after.status,
        same_content_completions: review.same_content(),
        gate: after.gate,
    };
    match args.format.resolve() {
        OutputFormat::Json => write_json(&recorded)?,
        OutputFormat::Terminal => {
            let compared = compared(basis.target.commit.as_deref(), basis.base.commit.as_deref());
            let mut out = io::stdout().lock();
            let lines = recorded.lines(&compared, first, &addressing, judged.ledger(), &names);
            for line in lines {
                writeln!(out, "{line}")?;
            }
        }
    }
    Ok(())
}

/// JSON output for a recorded judgment and the resulting review state.
#[derive(Debug, Serialize)]
struct Recorded<'a> {
    /// The judgment, as the review store holds it.
    judgment: &'a Judgment,
    /// The full id of the review it was recorded on.
    review: &'a BasisId,
    /// What that review reviewed.
    target: &'a BasisTarget,
    /// The decisions that review showed, which name what a correction
    /// addresses; the judgment holds its reference, so JSON leaves them out.
    #[serde(skip)]
    decisions: &'a [BasisDecision],
    /// Whether the review's own completion completes it now.
    status: StatusInfo,
    /// The standing completions of reviews of the same content, most
    /// recent first; `supersigil review` tells whether one carries over.
    same_content_completions: Vec<JudgmentInfo>,
    /// The review's gate now, as `review` shows it.
    gate: GateInfo,
}

impl Recorded<'_> {
    /// Renders the recorded judgment, items, review, and `compared` description.
    /// Reports snapshot retention on the `first` judgment, commands to review
    /// dismissed untracked files, withdrawals from `ledger`, the judgment id,
    /// gate state, and next commands. Uses the gate's `names` for items and
    /// `addressing` for commands. Escapes untrusted text and appends the note
    /// once when commands show a value as given.
    fn lines(
        &self,
        compared: &str,
        first: bool,
        addressing: &Addressing,
        ledger: &Ledger<'_>,
        names: &ItemNames<'_>,
    ) -> Vec<String> {
        let id = self.review.short();
        // The judgment's items the gate holds, by name, with their origins.
        let subjects: Vec<(&str, Origin)> = self
            .judgment
            .items
            .iter()
            .filter_map(|item| Some((names.name(item)?, names.item(item)?.origin())))
            .collect();
        let items = sentence_list(subjects.iter().map(|(name, _)| name));
        let review = format!("review {id} ({compared})");
        let kind = self.judgment.kind;
        let mut out = Printed::new();
        out.line(match kind {
            JudgmentKind::Acknowledge => format!("Acknowledged {items} on {review}."),
            JudgmentKind::Dismiss => format!("Dismissed {items} on {review}."),
            JudgmentKind::Comment if subjects.is_empty() => format!("Commented on {review}."),
            JudgmentKind::Comment => format!("Commented on {items} on {review}."),
            JudgmentKind::Correct => format!(
                "Corrected {} on {review}: {}.",
                corrected(self.decisions, self.judgment),
                correction_words(self.judgment.correction)
            ),
            JudgmentKind::Complete => format!("Completed {review}."),
            // An undo by id names no items; it withdraws that judgment
            // first.
            JudgmentKind::Undo => match self.judgment.withdraws.first() {
                Some(undone) if subjects.is_empty() => {
                    format!("Undid judgment {} on {review}.", undone.judgment.short())
                }
                _ => format!("Undid {items} on {review}."),
            },
        });
        if first {
            out.line(format!(
                "Kept the reviewed snapshot as refs/supersigil/reviews/{id}…"
            ));
        }
        if kind == JudgmentKind::Dismiss {
            for (name, _) in subjects
                .iter()
                .filter(|(_, origin)| *origin == Origin::UntrackedRecorded)
            {
                // An item named by its id has no path to type. Joined to
                // its flag, a path starting with `-` is never read as one.
                if let Parsed::Path { path, .. } = parse(name) {
                    let command = out.command(addressing.review_flag("--include-untracked", path));
                    out.line(format!(
                        "The file stays outside the review; to review its content, run {command}."
                    ));
                }
            }
        }
        out.lines(self.withdrawn(ledger, names));
        let open = self.gate.open_names();
        let complete = self.status.complete;
        // Without reading records, `judge` cannot tell whether a completion
        // of the same content carries over, so it names them instead.
        let same_content = &self.same_content_completions;
        let gate = if !open.is_empty() {
            format!("Gate: {} open: {}.", open.len(), open.join(", "))
        } else if complete {
            "Gate: 0 open. Review complete.".to_owned()
        } else if same_content.is_empty() {
            "Gate: 0 open. Review not yet complete.".to_owned()
        } else {
            "Gate: 0 open.".to_owned()
        };
        out.line(format!("Judgment {}. {gate}", self.judgment.id.short()));
        if open.is_empty() && !complete {
            out.lines(same_content.iter().map(show::same_content));
        }
        let next = next_lines(&open, complete, addressing, &mut out);
        out.lines(next);
        out.finish()
    }

    /// Renders one line per judgment withdrawn from `ledger`, in withdrawal order.
    /// Reports what was withdrawn, names items with [`item_name`] in `names`
    /// and a correction's decision with [`corrected`]. Also reports
    /// completions withdrawn because they counted a withdrawn
    /// acknowledgment or dismissal.
    fn withdrawn(&self, ledger: &Ledger<'_>, names: &ItemNames<'_>) -> Vec<String> {
        let withdraws = &self.judgment.withdraws;
        let counted = withdraws.iter().any(|withdrawal| withdrawal.item.is_some());
        ledger
            .withdrawn(withdraws)
            .into_iter()
            .map(|(judgment, items)| {
                let (date, review) = (judgment.at.date(), judgment.basis.short());
                let listed = sentence_list(items.iter().map(|item| item_name(names, item)));
                match judgment.kind {
                    JudgmentKind::Complete if counted => {
                        format!("Also withdrew the completion of review {review}.")
                    }
                    JudgmentKind::Complete => {
                        format!("Withdrew your completion of review {review} ({date}).")
                    }
                    JudgmentKind::Comment if items.is_empty() => {
                        format!("Withdrew your comment on review {review} ({date}).")
                    }
                    JudgmentKind::Comment => {
                        format!("Withdrew your comment on {listed} ({date}, review {review}).")
                    }
                    JudgmentKind::Correct => format!(
                        "Withdrew your correction of {} ({date}, review {review}).",
                        corrected(self.decisions, judgment)
                    ),
                    kind => format!(
                        "Withdrew your {} of {listed} ({date}, review {review}).",
                        noun(kind)
                    ),
                }
            })
            .collect()
    }
}

/// Returns `item`'s name in `names`, or `id:` followed by its first 8 digits
/// when the gate does not contain it.
fn item_name(names: &ItemNames<'_>, item: &ItemId) -> String {
    names
        .name(item)
        .map_or_else(|| format!("id:{}", item.short()), str::to_owned)
}

/// Joins `names` as `a`, `a and b`, or `a, b, and c`.
fn sentence_list<S: AsRef<str>>(names: impl IntoIterator<Item = S>) -> String {
    let names: Vec<S> = names.into_iter().collect();
    let names: Vec<&str> = names.iter().map(AsRef::as_ref).collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [first, second] => format!("{first} and {second}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

/// The name of the decision correction `judgment` addresses, as a review's
/// `decisions` give it, or `<session short>:<id>` from its reference when
/// they do not list it; `a decision` when it holds no reference.
fn corrected(decisions: &[BasisDecision], judgment: &Judgment) -> String {
    let Some(reference) = &judgment.decision else {
        return "a decision".to_owned();
    };
    decisions
        .iter()
        .find(|decision| decision.decision == *reference)
        .map_or_else(
            || format!("{}:{}", reference.session.short(), reference.id),
            |decision| decision.name.clone(),
        )
}

/// What a correction of `kind` says, as `judge` prints it; `correction`
/// for a judgment that records no kind.
const fn correction_words(kind: Option<CorrectionKind>) -> &'static str {
    match kind {
        Some(CorrectionKind::Wrong) => "explanation wrong",
        Some(CorrectionKind::NotTogether) => "edits do not belong together",
        Some(CorrectionKind::Constraint) => "constraint",
        None => "correction",
    }
}

/// The most decision names a refused correction lists.
const LISTED_DECISIONS: usize = 20;

/// The first [`LISTED_DECISIONS`] of `names`, joined with commas, then how
/// many more there are.
fn decision_list(names: &[&str]) -> String {
    let listed = names
        .iter()
        .take(LISTED_DECISIONS)
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
    match names.len().saturating_sub(LISTED_DECISIONS) {
        0 => listed,
        more => format!("{listed} … and {more} more"),
    }
}

/// Finds the review named by `prefix` in `snapshot`, or `worktree`'s selection.
/// Adds the review to `addressing` when it differs from the selection, so printed
/// commands apply to it.
///
/// # Errors
///
/// Refuses an invalid, unknown, or ambiguous `prefix`, or no prefix and no selection.
/// Also returns other store errors from [`ReviewSnapshot::find_basis`].
fn select(
    snapshot: &ReviewSnapshot,
    prefix: Option<&str>,
    worktree: &Path,
    addressing: &mut Addressing,
) -> Result<BasisId, CliError> {
    let selected = snapshot.manifest().selections.get(worktree);
    let id = match prefix {
        Some(prefix) => find_review(snapshot, prefix, addressing)?,
        None => selected
            .cloned()
            .ok_or_else(|| no_selection(worktree, addressing))?,
    };
    addressing.review = (selected != Some(&id)).then(|| id.short().to_owned());
    Ok(id)
}

/// The saved review `prefix` names in `snapshot`.
///
/// # Errors
///
/// Returns a refusal when `prefix` names no saved review or several, or is
/// not 8 or more hex digits, and the other store errors of
/// [`ReviewSnapshot::find_basis`].
fn find_review(
    snapshot: &ReviewSnapshot,
    prefix: &str,
    addressing: &Addressing,
) -> Result<BasisId, CliError> {
    snapshot.find_basis(prefix).map_err(|error| match error {
        StoreError::NoSuchBasis(_) => unknown_review(prefix, addressing),
        StoreError::AmbiguousBasis { candidates, .. } => {
            let mut out = Printed::new();
            out.line(format!(
                "Several saved reviews start with {prefix}; give more digits:"
            ));
            out.lines(candidates.iter().map(|id| format!("  {id}")));
            CliError::refused(out)
        }
        StoreError::InvalidBasisPrefix(_) => {
            let mut out = Printed::new();
            let list = out.command(addressing.judge("list", &[], ""));
            out.line(format!(
                "Name a review by at least 8 hex digits of its id, not {prefix} ({list} shows \
                 reviews)."
            ));
            CliError::refused(out)
        }
        other => other.into(),
    })
}

/// The refusal when no saved review starts with `prefix`.
fn unknown_review(prefix: &str, addressing: &Addressing) -> CliError {
    let mut out = Printed::new();
    let list = out.command(addressing.judge("list", &[], ""));
    out.line(format!(
        "No saved review starts with {prefix} ({list} shows reviews)."
    ));
    CliError::refused(out)
}

/// The refusal when no `--review` names a review and `worktree` selects
/// none. A worktree whose path is not UTF-8 never has a selection: the
/// review store cannot hold its path, so running `review` cannot help.
fn no_selection(worktree: &Path, addressing: &Addressing) -> CliError {
    if worktree.to_str().is_none() {
        return CliError::refused(
            [
                "The review store cannot hold this worktree's path (it is not valid UTF-8), so \
                 this worktree has no saved reviews.",
            ]
            .into(),
        );
    }
    let mut out = Printed::new();
    let review = out.command(addressing.review(""));
    let list = out.command(addressing.judge("list", &[], ""));
    out.line(format!(
        "No review selected in this worktree. Run {review} first, or pass --review <id> \
         ({list} shows reviews)."
    ));
    CliError::refused(out)
}

/// Resolves `typed` to unique gate item ids in the order named.
/// `name` parses each name, and `names` finds its item.
///
/// # Errors
///
/// Returns a refusal explaining each name that fails to resolve to exactly one item.
fn resolve(
    names: &ItemNames<'_>,
    typed: &[String],
    name: impl Fn(&str) -> Result<Name, CliError>,
) -> Result<Vec<ItemId>, CliError> {
    let mut errors = Vec::new();
    let mut meanings = Vec::new();
    for text in typed {
        match name(text) {
            Ok(name) => meanings.push((text.as_str(), name)),
            Err(error) => errors.push(error.to_string()),
        }
    }
    match names.resolve(&meanings) {
        Ok(ids) if errors.is_empty() => return Ok(ids),
        Ok(_) => {}
        Err(unresolved) => errors.extend(unresolved.iter().map(ToString::to_string)),
    }
    let mut out = Printed::new();
    out.line("Each name must name one gate item:");
    out.lines(errors.iter().map(|error| format!("  {error}")));
    Err(CliError::refused(out))
}

/// The refusal of `call` on review `id` for `refused`, naming items with
/// `names` and addressing commands with `addressing`.
fn refused_call(
    refused: &Refusal<'_>,
    call: Call<'_, String>,
    id: &BasisId,
    names: &ItemNames<'_>,
    addressing: &Addressing,
) -> CliError {
    match refused {
        Refusal::AlreadyClosed(closed) => already_closed(closed, names, addressing),
        Refusal::AlreadyComplete(completion) => CliError::refused(
            [format!(
                "Review {} is already complete: completed on {} by {} (judgment {}).",
                id.short(),
                completion.at.date(),
                completion.who.name,
                completion.id.short()
            )]
            .into(),
        ),
        Refusal::Unclosed(open) => {
            let open: Vec<&str> = open
                .iter()
                .map(|item| names.name(&item.id).unwrap_or_default())
                .collect();
            let count = match open.len() {
                1 => "1 open item".to_owned(),
                count => format!("{count} open items"),
            };
            let mut out = Printed::new();
            out.line(format!(
                "Review {} has {count}: {}. Comments do not close items.",
                id.short(),
                open.join(", ")
            ));
            let next = next_lines(&open, false, addressing, &mut out);
            out.lines(next);
            CliError::refused(out)
        }
        Refusal::NotClosed(unclosed) => {
            let mut out = Printed::new();
            out.line("Nothing closes these items, so there is nothing to undo:");
            for NotClosed { item, state } in unclosed {
                let name = names.name(&item.id).unwrap_or_default();
                out.line(match state {
                    ItemState::Reopened { judgment, reasons } => format!(
                        "  {name}: reopened: {} since your {} (judgment {}, review {})",
                        reasons.join("; "),
                        noun(judgment.kind),
                        judgment.id.short(),
                        judgment.basis.short()
                    ),
                    _ => format!("  {name}: open"),
                });
            }
            CliError::refused(out)
        }
        Refusal::NoJudgment(error) => {
            let Call::UndoJudgment { prefix, .. } = call else {
                unreachable!("only an undo by judgment names a judgment");
            };
            no_judgment(error, prefix, addressing)
        }
        Refusal::OtherReview(judgment) => {
            let mut out = Printed::new();
            out.line(format!(
                "Judgment {} was recorded on review {} and closes none of review {}'s items.",
                judgment.id.short(),
                judgment.basis.short(),
                id.short()
            ));
            let there = Addressing {
                review: Some(judgment.basis.short().to_owned()),
                ..addressing.clone()
            };
            // The whole id, since a prefix unique here may not be.
            let options = format!("--judgment {}", judgment.id);
            let undo = out.command(there.judge("undo", &[], &options));
            out.line(format!("To withdraw it there: {undo}"));
            CliError::refused(out)
        }
        Refusal::NotWithdrawable(judgment, WithdrawRefusal::Undo) => CliError::refused(
            [format!(
                "Judgment {} is an undo; an undo cannot be withdrawn.",
                judgment.id.short()
            )]
            .into(),
        ),
        Refusal::NotWithdrawable(judgment, WithdrawRefusal::Withdrawn) => CliError::refused(
            [format!(
                "Judgment {} is already withdrawn.",
                judgment.id.short()
            )]
            .into(),
        ),
        Refusal::NoDecision { typed, names } => no_decision(typed, names),
        Refusal::AmbiguousDecision { typed, names } => CliError::refused(
            [format!(
                "{typed} names several decisions: {}. Give the qualified name.",
                decision_list(names)
            )]
            .into(),
        ),
    }
}

/// The refusal of an acknowledgment or dismissal naming `closed` items:
/// each item, the judgment that closes it, and the undo to run first.
fn already_closed(
    closed: &[AlreadyClosed<'_>],
    names: &ItemNames<'_>,
    addressing: &Addressing,
) -> CliError {
    let typed: Vec<&str> = closed
        .iter()
        .map(|closed| names.name(&closed.item.id).unwrap_or_default())
        .collect();
    let mut out = Printed::new();
    out.line("Already closed; undo it first:");
    for (closed, name) in closed.iter().zip(&typed) {
        let judgment = closed.judgment;
        let state = closed_word(judgment.kind);
        let carried = if closed.carried {
            format!(" on review {}, carried", judgment.basis.short())
        } else {
            String::new()
        };
        out.line(format!(
            "  {name}: {state} by {} on {}{carried} (judgment {})",
            judgment.who.name,
            judgment.at.date(),
            judgment.id.short()
        ));
    }
    let undo = out.command(addressing.judge("undo", &typed, ""));
    out.line(format!("Next: {undo}"));
    CliError::refused(out)
}

/// The refusal of a correction whose `typed` name matches none of the
/// review's decision `names`.
fn no_decision(typed: &str, names: &[&str]) -> CliError {
    let shown = if names.is_empty() {
        "This review shows no decisions.".to_owned()
    } else {
        format!("Decisions: {}.", decision_list(names))
    };
    CliError::refused([format!("No decision {typed} on this review. {shown}")].into())
}

/// The refusal of an undo whose `prefix` names no single judgment, for
/// `error`.
fn no_judgment(error: &FindJudgmentError<'_>, prefix: &str, addressing: &Addressing) -> CliError {
    // A refusal of `prefix` that says where `judge show` lists each
    // judgment's id.
    let listed = |line: fn(&str, &str) -> String| {
        let mut out = Printed::new();
        let show = out.command(addressing.judge("show", &[], ""));
        out.line(line(prefix, &format!("{show} lists each judgment's id")));
        CliError::refused(out)
    };
    match error {
        FindJudgmentError::TooShort => listed(|prefix, listed| {
            format!("Name a judgment by at least 8 characters of its id, not {prefix} ({listed}).")
        }),
        FindJudgmentError::NoMatch => {
            listed(|prefix, listed| format!("No judgment starts with {prefix} ({listed})."))
        }
        FindJudgmentError::Ambiguous(judgments) => {
            let mut out = Printed::new();
            out.line(format!(
                "Several judgments start with {prefix}; give more characters:"
            ));
            out.lines(
                judgments
                    .iter()
                    .map(|judgment| format!("  {}", judgment.id)),
            );
            CliError::refused(out)
        }
    }
}

/// Keeps the snapshot `basis` reviewed reachable under
/// `refs/supersigil/reviews/<id>`, through a commit by `who` whose parents
/// are the base commit and the reviewed commit, each if there is one.
///
/// # Errors
///
/// Returns a refusal when git has pruned the reviewed tree, and the errors
/// of [`retention::tree_exists`] and [`retention::retain`].
fn keep(
    repo: &Repo,
    id: &BasisId,
    basis: &Basis,
    who: &Identity,
    addressing: &Addressing,
) -> Result<(), CliError> {
    let format = repo.format();
    let tree = ObjectId::parse(&basis.target.tree, format)?;
    if !retention::tree_exists(repo, &tree)? {
        let mut out = Printed::new();
        let review = out.command(addressing.review(""));
        out.line(format!(
            "The reviewed snapshot is no longer in the repository; run {review} again."
        ));
        return Err(CliError::refused(out));
    }
    let parents = [&basis.base.commit, &basis.target.commit]
        .into_iter()
        .flatten()
        .map(|commit| ObjectId::parse(commit, format))
        .collect::<Result<Vec<_>, _>>()?;
    retention::retain(
        repo,
        &format!("refs/supersigil/reviews/{id}"),
        &tree,
        &parents,
        &format!("supersigil review {id}"),
        who,
    )?;
    Ok(())
}
