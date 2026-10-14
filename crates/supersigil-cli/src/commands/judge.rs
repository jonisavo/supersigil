//! `supersigil judge` records a person's judgments on a saved review.
//! Acknowledgments and dismissals close gate items. Comments close nothing.
//! Completions record that a review is done. Undos withdraw earlier judgments.
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

use std::collections::BTreeMap;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use serde::Serialize;
use supersigil_git::retention::{self, Identity};
use supersigil_git::{Git, ObjectId, Repo};
use supersigil_record::reviews::{
    self, Basis, BasisId, BasisTarget, ItemId, Judgment, JudgmentKind, Origin, ReviewSnapshot,
    ReviewStore, Who, Withdrawal,
};
use supersigil_record::time::UtcTime;
use supersigil_record::{JudgmentId, StoreError};
use supersigil_review::judging::{
    AlreadyClosed, CompleteRefusal, FindJudgmentError, GateIndex, GateStates, ItemState, Ledger,
    NotClosed, WithdrawRefusal, related_bases,
};
use supersigil_review::model::{GateInfo, StatusInfo};
use supersigil_review::names::{ItemNames, Name, Parsed, parse};
use supersigil_review::summary::{
    Addressing, Command, closed_word, compared, next_lines, noun, shell_word,
};
use supersigil_session::checkout::canonical;

use crate::agent::in_agent_session;
use crate::commands::{CloseArgs, JudgeArgs, JudgeCommand};
use crate::error::CliError;
use crate::format::{OutputFormat, escape_control, write_json};
use crate::item_names::{item_names, name};
use crate::reconcile::LOCK_WAIT;
use crate::record_dir::canonical_checkout;
use crate::review_dir::{begin_within, canonical_common_dir, judged_bases, resolve_review_dir};

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
    let call = match &args.command {
        JudgeCommand::Acknowledge(close) => Call::closing(JudgmentKind::Acknowledge, close),
        JudgeCommand::Dismiss(close) => Call::closing(JudgmentKind::Dismiss, close),
        JudgeCommand::Comment(comment) => Call {
            kind: JudgmentKind::Comment,
            names: comment.item.as_deref().into_iter().collect(),
            reason: None,
            text: Some(&comment.text),
            judgment: None,
        },
        JudgeCommand::Complete(complete) => Call {
            kind: JudgmentKind::Complete,
            names: Vec::new(),
            reason: None,
            text: complete.text.as_deref(),
            judgment: None,
        },
        JudgeCommand::Undo(undo) => Call {
            kind: JudgmentKind::Undo,
            names: undo.items.iter().map(String::as_str).collect(),
            reason: undo.reason.as_deref(),
            text: None,
            judgment: undo.judgment.as_deref(),
        },
        JudgeCommand::List => return list::run(args),
        JudgeCommand::Show(show) => return show::run(args, show),
    };
    write(args, &call).map_err(|error| match error {
        CliError::Refused(message) => CliError::Refused(format!("No judgment recorded. {message}")),
        error => error,
    })
}

/// A write command's judgment kind, typed item names, reason or text,
/// and judgment id for an undo.
struct Call<'a> {
    kind: JudgmentKind,
    names: Vec<&'a str>,
    reason: Option<&'a str>,
    text: Option<&'a str>,
    judgment: Option<&'a str>,
}

impl<'a> Call<'a> {
    /// Builds an acknowledgment or dismissal of `kind` from `args`.
    fn closing(kind: JudgmentKind, args: &'a CloseArgs) -> Self {
        Self {
            kind,
            names: args.items.iter().map(String::as_str).collect(),
            reason: args.reason.as_deref(),
            text: None,
            judgment: None,
        }
    }
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
        ..Addressing::default()
    }
}

/// Records `call` on the review `args` selects, then prints what it
/// recorded, the gate it leaves, and what to run next.
///
/// # Errors
///
/// As [`run`]; a refusal's message here lacks its "No judgment recorded."
fn write(args: &JudgeArgs, call: &Call<'_>) -> Result<(), CliError> {
    if in_agent_session(|name| std::env::var_os(name)) {
        return Err(refusal([AGENT_SESSION]));
    }
    if call.kind == JudgmentKind::Dismiss
        && call.reason.is_none_or(|reason| reason.trim().is_empty())
    {
        return Err(refusal(["A dismissal needs --reason."]));
    }
    let mut addressing = addressing(args);
    let checkout = Checkout::open(args)?;
    let store = checkout.store(args.review.as_deref(), &addressing)?;
    let Some(mut tx) = begin_within(store, LOCK_WAIT)? else {
        return Err(refusal(["The review store is busy; try again."]));
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
    let others = judged_bases(&snapshot, &judgments, &id, &basis)?;
    let related = related_bases(&id, &basis, &others);
    let names = checkout.names(&basis)?;
    let items = resolve(&names, &call.names, |text| {
        name(&checkout.worktree, &checkout.cwd, text)
    })?;
    let indexes = GateIndex::of(&related);
    let ledger = Ledger::new(&judgments);
    let states = GateStates::new(&id, &basis, &related, &indexes, &ledger);
    let closes = check(call.kind, &id, &states, &items, &names, &addressing)?;
    let withdraws = match call.judgment {
        Some(prefix) => withdraw_judgment(&ledger, prefix, &addressing)?,
        None if call.kind == JudgmentKind::Undo => withdraw_items(&states, &items, &names)?,
        None => Vec::new(),
    };
    let who = retention::identity(&checkout.repo)?.ok_or_else(|| refusal(NO_IDENTITY.lines()))?;
    let first = ledger.on(&id).next().is_none();
    if first {
        keep(&checkout.repo, &id, &basis, &who, &addressing)?;
    }
    let now = UtcTime::now();
    let judgment = Judgment {
        id: JudgmentId::generate(),
        basis: id.clone(),
        kind: call.kind,
        items,
        reason: call.reason.map(str::to_owned),
        text: call.text.map(str::to_owned),
        closes,
        withdraws,
        who: Who {
            name: who.name,
            email: who.email,
        },
        at: now,
    };
    tx.append_judgment(&judgment)?;
    tx.cleanup(now)?;
    tx.commit()?;

    judgments.push(judgment.clone());
    let ledger = Ledger::new(&judgments);
    let after = GateStates::new(&id, &basis, &related, &indexes, &ledger);
    let recorded = Recorded {
        judgment: &judgment,
        review: &id,
        target: &basis.target,
        status: StatusInfo::new(&after),
        gate: GateInfo::new(&after, &names),
    };
    match args.format.resolve() {
        OutputFormat::Json => write_json(&recorded)?,
        OutputFormat::Terminal => {
            let compared = compared(basis.target.commit.as_deref(), basis.base.commit.as_deref());
            let mut out = io::stdout().lock();
            let lines = recorded.lines(&compared, first, &addressing, &ledger, &names);
            for line in lines {
                writeln!(out, "{}", escape_control(&line))?;
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
    /// Whether the review is complete now.
    status: StatusInfo,
    /// The review's gate now, as `review` shows it.
    gate: GateInfo,
}

impl Recorded<'_> {
    /// Renders the recorded judgment, items, review, and `compared` description.
    /// Reports snapshot retention on the `first` judgment, commands to review
    /// dismissed untracked files, withdrawals from `ledger`, the judgment id,
    /// gate state, and next commands. Uses the gate's `names` for items and
    /// `addressing` for commands. Appends its note once when commands show a
    /// value as given.
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
        let mut lines = vec![match kind {
            JudgmentKind::Acknowledge => format!("Acknowledged {items} on {review}."),
            JudgmentKind::Dismiss => format!("Dismissed {items} on {review}."),
            JudgmentKind::Comment if subjects.is_empty() => format!("Commented on {review}."),
            JudgmentKind::Comment => format!("Commented on {items} on {review}."),
            JudgmentKind::Complete => format!("Completed {review}."),
            // An undo by id names no items; it withdraws that judgment
            // first.
            JudgmentKind::Undo => match self.judgment.withdraws.first() {
                Some(undone) if subjects.is_empty() => {
                    format!("Undid judgment {} on {review}.", undone.judgment.short())
                }
                _ => format!("Undid {items} on {review}."),
            },
        }];
        if first {
            lines.push(format!(
                "Kept the reviewed snapshot as refs/supersigil/reviews/{id}…"
            ));
        }
        // Every command printed, for the note that follows them.
        let mut commands = Vec::new();
        if kind == JudgmentKind::Dismiss {
            for (name, _) in subjects
                .iter()
                .filter(|(_, origin)| *origin == Origin::UntrackedRecorded)
            {
                // An item named by its id has no path to type. Joined to
                // its flag, a path starting with `-` is never read as one.
                if let Parsed::Path { path, .. } = parse(name) {
                    let include = format!("--include-untracked={}", shell_word(path));
                    let command = addressing.review(&include);
                    lines.push(format!(
                        "The file stays outside the review; to review its content, run {command}."
                    ));
                    commands.push(command);
                }
            }
        }
        lines.extend(self.withdrawn(ledger, names));
        let open = self.gate.open_names();
        let complete = self.status.complete;
        let gate = if !open.is_empty() {
            format!("Gate: {} open: {}.", open.len(), open.join(", "))
        } else if complete {
            "Gate: 0 open. Review complete.".to_owned()
        } else {
            "Gate: 0 open. Review not yet complete.".to_owned()
        };
        lines.push(format!("Judgment {}. {gate}", self.judgment.id.short()));
        let (next, next_commands) = next_lines(&open, complete, addressing, str::to_owned);
        lines.extend(next);
        commands.extend(next_commands);
        lines.extend(addressing.note(&commands));
        lines
    }

    /// Renders one line per judgment withdrawn from `ledger`, in withdrawal order.
    /// Reports what was withdrawn and names items with [`item_name`] in `names`.
    /// Also reports completions withdrawn because they counted a withdrawn
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
            let mut lines = vec![format!(
                "Several saved reviews start with {prefix}; give more digits:"
            )];
            lines.extend(candidates.iter().map(|id| format!("  {id}")));
            refusal(lines)
        }
        StoreError::InvalidBasisPrefix(_) => {
            let list = addressing.judge("list", &[], "");
            addressed_refusal(
                addressing,
                [format!(
                    "Name a review by at least 8 hex digits of its id, not {prefix} ({list} shows \
                     reviews)."
                )],
                [&list],
            )
        }
        other => other.into(),
    })
}

/// The refusal when no saved review starts with `prefix`.
fn unknown_review(prefix: &str, addressing: &Addressing) -> CliError {
    let list = addressing.judge("list", &[], "");
    addressed_refusal(
        addressing,
        [format!(
            "No saved review starts with {prefix} ({list} shows reviews)."
        )],
        [&list],
    )
}

/// The refusal when no `--review` names a review and `worktree` selects
/// none. A worktree whose path is not UTF-8 never has a selection: the
/// review store cannot hold its path, so running `review` cannot help.
fn no_selection(worktree: &Path, addressing: &Addressing) -> CliError {
    if worktree.to_str().is_none() {
        return refusal([
            "The review store cannot hold this worktree's path (it is not valid UTF-8), so \
             this worktree has no saved reviews.",
        ]);
    }
    let (review, list) = (addressing.review(""), addressing.judge("list", &[], ""));
    addressed_refusal(
        addressing,
        [format!(
            "No review selected in this worktree. Run {review} first, or pass --review <id> \
             ({list} shows reviews)."
        )],
        [&review, &list],
    )
}

/// Resolves `typed` to unique gate item ids in the order named.
/// `name` parses each name, and `names` finds its item.
///
/// # Errors
///
/// Returns a refusal explaining each name that fails to resolve to exactly one item.
fn resolve(
    names: &ItemNames<'_>,
    typed: &[&str],
    name: impl Fn(&str) -> Result<Name, CliError>,
) -> Result<Vec<ItemId>, CliError> {
    let mut errors = Vec::new();
    let mut meanings = Vec::new();
    for text in typed {
        match name(text) {
            Ok(name) => meanings.push((*text, name)),
            Err(error) => errors.push(error.to_string()),
        }
    }
    match names.resolve(&meanings) {
        Ok(ids) if errors.is_empty() => return Ok(ids),
        Ok(_) => {}
        Err(unresolved) => errors.extend(unresolved.iter().map(ToString::to_string)),
    }
    let mut lines = vec!["Each name must name one gate item:".to_owned()];
    lines.extend(errors.iter().map(|error| format!("  {error}")));
    Err(refusal(lines))
}

/// Checks `kind` and `items` against review `id`'s gate `states` after judgments.
/// Returns the items and judgments a completion closes, or an empty map for
/// other calls. [`withdraw_items`] and [`withdraw_judgment`] check undos
/// while planning them.
///
/// # Errors
///
/// Returns a refusal when an item to acknowledge or dismiss is already closed,
/// or when a completion finds an open item or an already completed review.
fn check(
    kind: JudgmentKind,
    id: &BasisId,
    states: &GateStates<'_>,
    items: &[ItemId],
    names: &ItemNames<'_>,
    addressing: &Addressing,
) -> Result<BTreeMap<ItemId, JudgmentId>, CliError> {
    match kind {
        JudgmentKind::Acknowledge | JudgmentKind::Dismiss => states
            .check_close(items)
            .map(|()| BTreeMap::new())
            .map_err(|closed| already_closed(&closed, names, addressing)),
        JudgmentKind::Comment | JudgmentKind::Undo => Ok(BTreeMap::new()),
        JudgmentKind::Complete => states.check_complete().map_err(|refused| match refused {
            CompleteRefusal::AlreadyComplete(completion) => refusal([format!(
                "Review {} is already complete: completed on {} by {} (judgment {}).",
                id.short(),
                completion.at.date(),
                completion.who.name,
                completion.id.short()
            )]),
            CompleteRefusal::Unclosed(open) => {
                let open: Vec<&str> = open
                    .iter()
                    .map(|item| names.name(&item.id).unwrap_or_default())
                    .collect();
                let count = match open.len() {
                    1 => "1 open item".to_owned(),
                    count => format!("{count} open items"),
                };
                let mut lines = vec![format!(
                    "Review {} has {count}: {}. Comments do not close items.",
                    id.short(),
                    open.join(", ")
                )];
                let (next, commands) = next_lines(&open, false, addressing, str::to_owned);
                lines.extend(next);
                addressed_refusal(addressing, lines, &commands)
            }
        }),
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
    let mut lines = vec!["Already closed; undo it first:".to_owned()];
    for (closed, name) in closed.iter().zip(&typed) {
        let judgment = closed.judgment;
        let state = closed_word(judgment.kind);
        let carried = if closed.carried {
            format!(" on review {}, carried", judgment.basis.short())
        } else {
            String::new()
        };
        lines.push(format!(
            "  {name}: {state} by {} on {}{carried} (judgment {})",
            judgment.who.name,
            judgment.at.date(),
            judgment.id.short()
        ));
    }
    let undo = addressing.judge("undo", &typed, "");
    lines.push(format!("Next: {undo}"));
    addressed_refusal(addressing, lines, [&undo])
}

/// Plans `undo <items>` against gate `states`. Withdraws every own or carried
/// judgment closing an item in `items`, and standing completions counting
/// those judgments.
///
/// # Errors
///
/// Refuses items without closing judgments. Lists their states as open or
/// reopened, with the judgment each reopened item no longer matches.
fn withdraw_items(
    states: &GateStates<'_>,
    items: &[ItemId],
    names: &ItemNames<'_>,
) -> Result<Vec<Withdrawal>, CliError> {
    states.plan_undo(items).map_err(|unclosed| {
        let mut lines = vec!["Nothing closes these items, so there is nothing to undo:".to_owned()];
        for NotClosed { item, state } in &unclosed {
            let name = names.name(&item.id).unwrap_or_default();
            lines.push(match state {
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
        refusal(lines)
    })
}

/// Plans withdrawal of the whole judgment named by `prefix` in `ledger`,
/// and every standing completion counting it.
///
/// # Errors
///
/// Refuses prefixes shorter than 8 characters, unknown or ambiguous prefixes,
/// and judgments that are undos or already withdrawn.
fn withdraw_judgment(
    ledger: &Ledger<'_>,
    prefix: &str,
    addressing: &Addressing,
) -> Result<Vec<Withdrawal>, CliError> {
    // Print this command and its note when the prefix is too short or names no
    // judgment.
    let show = addressing.judge("show", &[], "");
    let listed = format!("{show} lists each judgment's id");
    let judgment = ledger.find(prefix).map_err(|error| match error {
        FindJudgmentError::TooShort => addressed_refusal(
            addressing,
            [format!(
                "Name a judgment by at least 8 characters of its id, not {prefix} ({listed})."
            )],
            [&show],
        ),
        FindJudgmentError::NoMatch => addressed_refusal(
            addressing,
            [format!("No judgment starts with {prefix} ({listed}).")],
            [&show],
        ),
        FindJudgmentError::Ambiguous(judgments) => {
            let mut lines = vec![format!(
                "Several judgments start with {prefix}; give more characters:"
            )];
            lines.extend(
                judgments
                    .iter()
                    .map(|judgment| format!("  {}", judgment.id)),
            );
            refusal(lines)
        }
    })?;
    ledger
        .plan_withdrawal(judgment)
        .map_err(|refused| match refused {
            WithdrawRefusal::Undo => refusal([format!(
                "Judgment {} is an undo; an undo cannot be withdrawn.",
                judgment.id.short()
            )]),
            WithdrawRefusal::Withdrawn => refusal([format!(
                "Judgment {} is already withdrawn.",
                judgment.id.short()
            )]),
        })
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
        let review = addressing.review("");
        return Err(addressed_refusal(
            addressing,
            [format!(
                "The reviewed snapshot is no longer in the repository; run {review} again."
            )],
            [&review],
        ));
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

/// Escapes each line of `rest` and joins them into a refusal.
/// [`run`] prefixes write command refusals with "No judgment recorded."
fn refusal<S: AsRef<str>>(rest: impl IntoIterator<Item = S>) -> CliError {
    let lines: Vec<String> = rest
        .into_iter()
        .map(|line| escape_control(line.as_ref()))
        .collect();
    CliError::Refused(lines.join("\n"))
}

/// Builds a [`refusal`] from `rest`, which prints `commands` using `addressing`.
/// Appends the addressing note when a command shows a value as given.
fn addressed_refusal<'c, S: AsRef<str>>(
    addressing: &Addressing,
    rest: impl IntoIterator<Item = S>,
    commands: impl IntoIterator<Item = &'c Command>,
) -> CliError {
    let mut lines: Vec<String> = rest
        .into_iter()
        .map(|line| line.as_ref().to_owned())
        .collect();
    lines.extend(addressing.note(commands));
    refusal(lines)
}
