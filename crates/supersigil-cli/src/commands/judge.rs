//! `supersigil judge`: a person's judgments on a saved review.
//! Acknowledging or dismissing gate items closes them, a comment notes
//! something and closes nothing, a completion records that the review
//! is done, and an undo withdraws earlier judgments.
//!
//! A writing call refuses inside an agent session: judging is for people.
//! It takes the review store's writer lock before it reads anything, then
//! resolves the review, checks the call, keeps the reviewed snapshot under a
//! ref on the review's first judgment, and appends its judgment while
//! holding the lock, so cleanup cannot remove the review in between. It
//! reads the saved review, never the working tree. A refused call records
//! nothing and says why.
//!
//! A reading call, `list` or `show`, takes no lock and creates no store,
//! so an agent may run it.

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
use supersigil_record::{JudgmentId, RecordId, StoreError};
use supersigil_review::judging::{
    AlreadyClosed, CompleteRefusal, FindJudgmentError, GateStates, ItemState, Ledger, NotClosed,
    WithdrawRefusal, related_bases,
};
use supersigil_review::model::{GateInfo, GateItemInfo, StatusInfo};
use supersigil_review::names::{ItemNames, Name, Parsed, parse};
use supersigil_review::summary::{Addressing, compared, next_lines, shell_word};
use supersigil_session::checkout::canonical;

use crate::agent::in_agent_session;
use crate::commands::{CloseArgs, JudgeArgs, JudgeCommand};
use crate::error::CliError;
use crate::format::{OutputFormat, escape_control, write_json};
use crate::item_names::{associations, name, typed};
use crate::reconcile::LOCK_WAIT;
use crate::record_dir::canonical_checkout;
use crate::review_dir::{begin_within, judged_bases, resolve_review_dir};

/// What a refusal inside an agent session says. It never suggests unsetting
/// the marker.
const AGENT_SESSION: &str = "Judgments must be made by a person; this command is running inside \
                             a Claude Code session. The agent can list the open items \
                             (supersigil judge show) and prepare the exact command for you to \
                             inspect and run outside the agent.";

/// What a refusal without a git identity says, in `git commit`'s words.
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
        JudgeCommand::Acknowledge(close) => Call::closing(Verb::Acknowledge, close),
        JudgeCommand::Dismiss(close) => Call::closing(Verb::Dismiss, close),
        JudgeCommand::Comment(comment) => Call {
            verb: Verb::Comment,
            names: comment.item.as_deref().into_iter().collect(),
            reason: None,
            text: Some(&comment.text),
            judgment: None,
        },
        JudgeCommand::Complete(complete) => Call {
            verb: Verb::Complete,
            names: Vec::new(),
            reason: None,
            text: complete.text.as_deref(),
            judgment: None,
        },
        JudgeCommand::Undo(undo) => Call {
            verb: Verb::Undo,
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

/// What a writing call records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verb {
    Acknowledge,
    Dismiss,
    Comment,
    Complete,
    Undo,
}

impl Verb {
    /// The kind of judgment it records.
    const fn kind(self) -> JudgmentKind {
        match self {
            Self::Acknowledge => JudgmentKind::Acknowledge,
            Self::Dismiss => JudgmentKind::Dismiss,
            Self::Comment => JudgmentKind::Comment,
            Self::Complete => JudgmentKind::Complete,
            Self::Undo => JudgmentKind::Undo,
        }
    }
}

/// A writing call as given: what it records, the items it names as typed,
/// its reason or text, and the judgment an undo names by id.
struct Call<'a> {
    verb: Verb,
    names: Vec<&'a str>,
    reason: Option<&'a str>,
    text: Option<&'a str>,
    judgment: Option<&'a str>,
}

impl<'a> Call<'a> {
    /// The call of `verb`, an acknowledgment or a dismissal, with `args`.
    fn closing(verb: Verb, args: &'a CloseArgs) -> Self {
        Self {
            verb,
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
    /// The canonical path of the current directory, which typed names are
    /// relative to.
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
        let store = reviews::find(&reviews_dir, &retention::common_dir(&repo)?)?;
        Ok(Self {
            repo,
            worktree,
            cwd,
            store,
        })
    }

    /// The names of `basis`'s gate items as typed from the current
    /// directory, with `associations` naming its records.
    ///
    /// # Errors
    ///
    /// Returns [`CliError::Git`] when git cannot read `core.ignorecase`.
    fn names<'g>(
        &self,
        basis: &'g Basis,
        associations: &'g BTreeMap<RecordId, Vec<PathBuf>>,
    ) -> Result<ItemNames<'g>, CliError> {
        let ignore_case = self.repo.config_bool("core.ignorecase")?.unwrap_or(false);
        Ok(ItemNames::new(
            &basis.gate,
            associations,
            ignore_case,
            |path| typed(&self.worktree, &self.cwd, path),
        ))
    }
}

/// How the commands a call prints address its review: with the
/// `--checkout` and `--review-dir` `args` gives. The review is added once
/// resolved (see [`select`]).
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
    if call.verb == Verb::Dismiss && call.reason.is_none_or(|reason| reason.trim().is_empty()) {
        return Err(refusal(["A dismissal needs --reason."]));
    }
    let mut addressing = addressing(args);
    let checkout = Checkout::open(args)?;
    let Some(store) = &checkout.store else {
        return Err(match &args.review {
            Some(prefix) => unknown_review(prefix, &addressing),
            None => no_selection(&checkout.worktree, &addressing),
        });
    };
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
    let others = judged_bases(&snapshot, &judgments, &id)?;
    let related = related_bases(&id, &basis, &others);
    let associations = associations(&basis);
    let names = checkout.names(&basis, &associations)?;
    let items = resolve(&names, &call.names, |text| {
        name(&checkout.worktree, &checkout.cwd, text)
    })?;
    let ledger = Ledger::new(&judgments);
    let states = GateStates::new(&id, &basis, &related, &ledger);
    let closes = check(call.verb, &id, &states, &items, &names, &addressing)?;
    let withdraws = match call.judgment {
        Some(prefix) => withdraw_judgment(&ledger, prefix, &addressing)?,
        None if call.verb == Verb::Undo => withdraw_items(&states, &items, &names)?,
        None => Vec::new(),
    };
    let who = retention::identity(&checkout.repo)?.ok_or_else(|| refusal(NO_IDENTITY.lines()))?;
    let first = !judgments.iter().any(|judgment| judgment.basis == id);
    if first {
        keep(&checkout.repo, &id, &basis, &who, &addressing)?;
    }
    let now = UtcTime::now();
    let judgment = Judgment {
        id: JudgmentId::generate(),
        basis: id.clone(),
        kind: call.verb.kind(),
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
    let after = GateStates::new(&id, &basis, &related, &ledger);
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
            for line in recorded.lines(call.verb, &compared, first, &addressing, &judgments) {
                writeln!(out, "{}", escape_control(&line))?;
            }
        }
    }
    Ok(())
}

/// A judgment `judge` recorded and the review it leaves, as JSON shows
/// them.
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
    /// The lines a terminal shows: what the call `verb` recorded, naming the
    /// items and the review, which `compared` what; the snapshot kept when
    /// the call was the review's `first` judgment; how to review the
    /// content of an untracked file dismissed; what an undo withdrew of
    /// `judgments`; the judgment's id and the gate; and what to run next.
    /// Commands are addressed as `addressing` says, followed by its note
    /// once when they show a value as given.
    fn lines(
        &self,
        verb: Verb,
        compared: &str,
        first: bool,
        addressing: &Addressing,
        judgments: &[Judgment],
    ) -> Vec<String> {
        let id = self.review.short();
        let gate = gate_index(&self.gate);
        let named: Vec<&GateItemInfo> = self
            .judgment
            .items
            .iter()
            .filter_map(|item| gate.get(item).copied())
            .collect();
        let items = sentence_list(named.iter().map(|item| item.name.as_str()));
        let review = format!("review {id} ({compared})");
        let mut lines = vec![match verb {
            Verb::Acknowledge => format!("Acknowledged {items} on {review}."),
            Verb::Dismiss => format!("Dismissed {items} on {review}."),
            Verb::Comment if named.is_empty() => format!("Commented on {review}."),
            Verb::Comment => format!("Commented on {items} on {review}."),
            Verb::Complete => format!("Completed {review}."),
            // An undo by id names no items; it withdraws that judgment
            // first.
            Verb::Undo => match self.judgment.withdraws.first() {
                Some(undone) if named.is_empty() => {
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
        let mut commands = false;
        if verb == Verb::Dismiss {
            for item in named
                .iter()
                .filter(|item| item.origin == Origin::UntrackedRecorded)
            {
                // An item named by its id has no path to type. Joined to
                // its flag, a path starting with `-` is never read as one.
                if let Parsed::Path { path, .. } = parse(&item.name) {
                    let include = format!("--include-untracked={}", shell_word(path));
                    lines.push(format!(
                        "The file stays outside the review; to review its content, run {}.",
                        addressing.review(&include)
                    ));
                    commands = true;
                }
            }
        }
        lines.extend(self.withdrawn(judgments, &gate));
        let open: Vec<&str> = self
            .gate
            .items
            .iter()
            .filter(|item| item.state.is_open())
            .map(|item| item.name.as_str())
            .collect();
        let complete = self.status.complete;
        let gate = if !open.is_empty() {
            format!("Gate: {} open: {}.", open.len(), open.join(", "))
        } else if complete {
            "Gate: 0 open. Review complete.".to_owned()
        } else {
            "Gate: 0 open. Review not yet complete.".to_owned()
        };
        lines.push(format!("Judgment {}. {gate}", self.judgment.id.short()));
        let next = next_lines(&open, complete, addressing, str::to_owned);
        commands |= !next.is_empty();
        lines.extend(next);
        if commands {
            lines.extend(addressing.note());
        }
        lines
    }

    /// One line per judgment of `judgments` the recorded undo withdraws, in
    /// the order withdrawn, naming what of it was withdrawn. A completion
    /// withdrawn because it counted an acknowledgment or dismissal the undo
    /// withdraws is named as also withdrawn.
    fn withdrawn(
        &self,
        judgments: &[Judgment],
        gate: &BTreeMap<&ItemId, &GateItemInfo>,
    ) -> Vec<String> {
        let withdraws = &self.judgment.withdraws;
        let counted = withdraws.iter().any(|withdrawal| withdrawal.item.is_some());
        let by_id: BTreeMap<&JudgmentId, &Judgment> = judgments
            .iter()
            .rev()
            .map(|judgment| (&judgment.id, judgment))
            .collect();
        by_judgment(withdraws)
            .into_iter()
            .filter_map(|(id, items)| {
                let judgment = *by_id.get(id)?;
                let (date, review) = (judgment.at.date(), judgment.basis.short());
                let names = |items: &[&ItemId]| {
                    sentence_list(items.iter().map(|item| item_name(gate, item)))
                };
                Some(match judgment.kind {
                    JudgmentKind::Complete if counted => {
                        format!("Also withdrew the completion of review {review}.")
                    }
                    JudgmentKind::Complete => {
                        format!("Withdrew your completion of review {review} ({date}).")
                    }
                    JudgmentKind::Comment if judgment.items.is_empty() => {
                        format!("Withdrew your comment on review {review} ({date}).")
                    }
                    JudgmentKind::Comment => {
                        let on: Vec<&ItemId> = judgment.items.iter().collect();
                        format!(
                            "Withdrew your comment on {} ({date}, review {review}).",
                            names(&on)
                        )
                    }
                    kind => format!(
                        "Withdrew your {} of {} ({date}, review {review}).",
                        noun(kind),
                        names(&items)
                    ),
                })
            })
            .collect()
    }
}

/// Each judgment `withdraws` withdraws something of, once, in the order
/// withdrawn, with the items whose judgment it withdraws (none for a whole
/// comment or completion).
fn by_judgment(withdraws: &[Withdrawal]) -> Vec<(&JudgmentId, Vec<&ItemId>)> {
    let mut grouped: Vec<(&JudgmentId, Vec<&ItemId>)> = Vec::new();
    let mut position: BTreeMap<&JudgmentId, usize> = BTreeMap::new();
    for withdrawal in withdraws {
        if let Some(&at) = position.get(&withdrawal.judgment) {
            grouped[at].1.extend(&withdrawal.item);
        } else {
            position.insert(&withdrawal.judgment, grouped.len());
            grouped.push((&withdrawal.judgment, withdrawal.item.iter().collect()));
        }
    }
    grouped
}

/// The items of `gate` by id, so each lookup is one search of the index
/// instead of a scan of the gate. The first item with an id wins.
fn gate_index(gate: &GateInfo) -> BTreeMap<&ItemId, &GateItemInfo> {
    gate.items
        .iter()
        .rev()
        .map(|info| (&info.id, info))
        .collect()
}

/// The name of `item` in `gate`, an index made by [`gate_index`], or `id:`
/// and its first 8 digits when the gate does not hold it.
fn item_name(gate: &BTreeMap<&ItemId, &GateItemInfo>, item: &ItemId) -> String {
    gate.get(item)
        .map_or_else(|| format!("id:{}", item.short()), |info| info.name.clone())
}

/// What a judgment of `kind` is called in a sentence.
const fn noun(kind: JudgmentKind) -> &'static str {
    match kind {
        JudgmentKind::Acknowledge => "acknowledgment",
        JudgmentKind::Dismiss => "dismissal",
        JudgmentKind::Comment => "comment",
        JudgmentKind::Complete => "completion",
        JudgmentKind::Undo => "undo",
    }
}

/// `names` as a sentence lists them: `a`, `a and b`, `a, b, and c`.
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

/// The review `prefix` names in `snapshot`, else the one `worktree`
/// selects. Sets `addressing` to name the review when it is not the
/// selection, so printed commands apply to it.
///
/// # Errors
///
/// Returns a refusal when `prefix` names no saved review or several, or is
/// too short, or when there is no prefix and no selection, and the other
/// store errors of [`ReviewSnapshot::find_basis`].
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
        StoreError::InvalidBasisPrefix(_) => addressed_refusal(
            addressing,
            [format!(
                "Name a review by at least 8 hex digits of its id, not {prefix} ({} shows \
                 reviews).",
                addressing.judge("list", &[], "")
            )],
        ),
        other => other.into(),
    })
}

/// The refusal when no saved review starts with `prefix`.
fn unknown_review(prefix: &str, addressing: &Addressing) -> CliError {
    addressed_refusal(
        addressing,
        [format!(
            "No saved review starts with {prefix} ({} shows reviews).",
            addressing.judge("list", &[], "")
        )],
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
    addressed_refusal(
        addressing,
        [format!(
            "No review selected in this worktree. Run {} first, or pass --review <id> ({} shows \
             reviews).",
            addressing.review(""),
            addressing.judge("list", &[], "")
        )],
    )
}

/// The ids of the gate items `typed` names, in the order named, each once:
/// `name` says what a typed name means, and `names` finds its item.
///
/// # Errors
///
/// Returns a refusal listing why each name that does not name exactly one
/// item does not.
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

/// Checks a call of `verb` naming `items` against `states`, the gate of the
/// review `id` as the store's judgments leave it, and returns what a
/// completion closes (empty for other calls). An undo is checked as it is
/// planned ([`withdraw_items`], [`withdraw_judgment`]).
///
/// # Errors
///
/// Returns a refusal when an item to acknowledge or dismiss is already
/// closed, or when a completion finds an item open or the review complete.
fn check(
    verb: Verb,
    id: &BasisId,
    states: &GateStates<'_>,
    items: &[ItemId],
    names: &ItemNames<'_>,
    addressing: &Addressing,
) -> Result<BTreeMap<ItemId, JudgmentId>, CliError> {
    match verb {
        Verb::Acknowledge | Verb::Dismiss => states
            .check_close(items)
            .map(|()| BTreeMap::new())
            .map_err(|closed| already_closed(&closed, names, addressing)),
        Verb::Comment | Verb::Undo => Ok(BTreeMap::new()),
        Verb::Complete => states.check_complete().map_err(|refused| match refused {
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
                lines.extend(next_lines(&open, false, addressing, str::to_owned));
                addressed_refusal(addressing, lines)
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
        let state = match judgment.kind {
            JudgmentKind::Dismiss => "dismissed",
            _ => "acknowledged",
        };
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
    lines.push(format!("Next: {}", addressing.judge("undo", &typed, "")));
    addressed_refusal(addressing, lines)
}

/// What `undo <items>` withdraws on the review whose gate is `states`:
/// every judgment closing each of `items` there, own or carried, and every
/// standing completion counting one of them.
///
/// # Errors
///
/// Returns a refusal naming each item nothing closes with its state: open,
/// or reopened with the judgment it no longer matches.
fn withdraw_items(
    states: &GateStates<'_>,
    items: &[ItemId],
    names: &ItemNames<'_>,
) -> Result<Vec<Withdrawal>, CliError> {
    let planned = states.plan_undo(items).map_err(|unclosed| {
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
    })?;
    Ok(planned.into_iter().map(|plan| plan.withdrawal).collect())
}

/// What `undo --judgment <prefix>` withdraws: the judgment `prefix` names
/// in `ledger`, whole, and every standing completion counting it.
///
/// # Errors
///
/// Returns a refusal when `prefix` is shorter than 8 characters or names
/// no judgment or several, or when the judgment is an undo or already
/// withdrawn.
fn withdraw_judgment(
    ledger: &Ledger<'_>,
    prefix: &str,
    addressing: &Addressing,
) -> Result<Vec<Withdrawal>, CliError> {
    let listed = format!(
        "{} lists each judgment's id",
        addressing.judge("show", &[], "")
    );
    let judgment = ledger.find(prefix).map_err(|error| match error {
        FindJudgmentError::TooShort => addressed_refusal(
            addressing,
            [format!(
                "Name a judgment by at least 8 characters of its id, not {prefix} ({listed})."
            )],
        ),
        FindJudgmentError::NoMatch => addressed_refusal(
            addressing,
            [format!("No judgment starts with {prefix} ({listed}).")],
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
    let planned = ledger
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
        })?;
    Ok(planned.into_iter().map(|plan| plan.withdrawal).collect())
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
        return Err(addressed_refusal(
            addressing,
            [format!(
                "The reviewed snapshot is no longer in the repository; run {} again.",
                addressing.review("")
            )],
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

/// A refused call's error: the lines of `rest`, each escaped. A writing
/// call's refusal gains "No judgment recorded." in front when [`run`]
/// returns it.
fn refusal<S: AsRef<str>>(rest: impl IntoIterator<Item = S>) -> CliError {
    let lines: Vec<String> = rest
        .into_iter()
        .map(|line| escape_control(line.as_ref()))
        .collect();
    CliError::Refused(lines.join("\n"))
}

/// A refused call's error, as [`refusal`] makes it, whose `rest` prints
/// commands addressed as `addressing` says, followed by its note when they
/// show a value as given.
fn addressed_refusal<S: AsRef<str>>(
    addressing: &Addressing,
    rest: impl IntoIterator<Item = S>,
) -> CliError {
    let mut lines: Vec<String> = rest
        .into_iter()
        .map(|line| line.as_ref().to_owned())
        .collect();
    lines.extend(addressing.note());
    refusal(lines)
}
