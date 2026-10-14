//! `supersigil judge`: a person's judgments on a saved review.
//! Acknowledging or dismissing gate items closes them, a comment notes
//! something and closes nothing, and a completion records that the review
//! is done.
//!
//! A writing call refuses inside an agent session: judging is for people.
//! It takes the review store's writer lock before it reads anything, then
//! resolves the review, checks the call, keeps the reviewed snapshot under a
//! ref on the review's first judgment, and appends its judgment while
//! holding the lock, so cleanup cannot remove the review in between. It
//! reads the saved review, never the working tree. A refused call records
//! nothing and says why.

use std::collections::BTreeMap;
use std::io::{self, Write as _};
use std::path::Path;

use serde::Serialize;
use supersigil_git::retention::{self, Identity};
use supersigil_git::{Git, ObjectId, Repo};
use supersigil_record::reviews::{
    self, Basis, BasisId, BasisTarget, ItemId, Judgment, JudgmentKind, Origin, ReviewSnapshot, Who,
};
use supersigil_record::time::UtcTime;
use supersigil_record::{JudgmentId, StoreError};
use supersigil_review::judging::{
    AlreadyClosed, CompleteRefusal, GateStates, Ledger, related_bases,
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
use crate::review_dir::{begin_within, resolve_review_dir};

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
    write(args, &Call::of(&args.command))
}

/// What a writing call records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verb {
    Acknowledge,
    Dismiss,
    Comment,
    Complete,
}

impl Verb {
    /// The kind of judgment it records.
    const fn kind(self) -> JudgmentKind {
        match self {
            Self::Acknowledge => JudgmentKind::Acknowledge,
            Self::Dismiss => JudgmentKind::Dismiss,
            Self::Comment => JudgmentKind::Comment,
            Self::Complete => JudgmentKind::Complete,
        }
    }
}

/// A writing call as given: what it records, the items it names as typed,
/// and its reason or text.
struct Call<'a> {
    verb: Verb,
    names: Vec<&'a str>,
    reason: Option<&'a str>,
    text: Option<&'a str>,
}

impl<'a> Call<'a> {
    /// The call `command` makes.
    fn of(command: &'a JudgeCommand) -> Self {
        let closing = |verb, args: &'a CloseArgs| Self {
            verb,
            names: args.items.iter().map(String::as_str).collect(),
            reason: args.reason.as_deref(),
            text: None,
        };
        match command {
            JudgeCommand::Acknowledge(args) => closing(Verb::Acknowledge, args),
            JudgeCommand::Dismiss(args) => closing(Verb::Dismiss, args),
            JudgeCommand::Comment(args) => Self {
                verb: Verb::Comment,
                names: args.item.as_deref().into_iter().collect(),
                reason: None,
                text: Some(&args.text),
            },
            JudgeCommand::Complete(args) => Self {
                verb: Verb::Complete,
                names: Vec::new(),
                reason: None,
                text: args.text.as_deref(),
            },
        }
    }
}

/// How the commands a call prints address its review: with the
/// `--checkout` and `--review-dir` `args` gives. The review is added once
/// resolved.
fn addressing(args: &JudgeArgs) -> Addressing {
    Addressing {
        review: None,
        checkout: args.checkout.clone(),
        review_dir: args.review_dir.clone(),
    }
}

/// Records `call` on the review `args` selects, then prints what it
/// recorded, the gate it leaves, and what to run next.
///
/// # Errors
///
/// As [`run`].
fn write(args: &JudgeArgs, call: &Call<'_>) -> Result<(), CliError> {
    if in_agent_session(|name| std::env::var_os(name)) {
        return Err(refusal([AGENT_SESSION]));
    }
    if call.verb == Verb::Dismiss && call.reason.is_none_or(|reason| reason.trim().is_empty()) {
        return Err(refusal(["A dismissal needs --reason."]));
    }
    let mut addressing = addressing(args);
    let reviews_dir = resolve_review_dir(args.review_dir.as_deref())?;
    let repo = Repo::open(Git::new(&canonical_checkout(args.checkout.as_deref())?))?;
    let worktree = canonical(repo.root())?;
    let cwd = canonical_checkout(None)?;
    let Some(store) = reviews::find(&reviews_dir, &retention::common_dir(&repo)?)? else {
        return Err(match &args.review {
            Some(prefix) => unknown_review(prefix, &addressing),
            None => no_selection(&worktree, &addressing),
        });
    };
    let Some(mut tx) = begin_within(&store, LOCK_WAIT)? else {
        return Err(refusal(["The review store is busy; try again."]));
    };
    // Under the writer lock, the published manifest is the one the
    // transaction read.
    let snapshot = store.snapshot()?;
    let selected = snapshot.manifest().selections.get(&worktree);
    let id = match &args.review {
        Some(prefix) => find_review(&snapshot, prefix, &addressing)?,
        None => selected
            .cloned()
            .ok_or_else(|| no_selection(&worktree, &addressing))?,
    };
    addressing.review = (selected != Some(&id)).then(|| id.short().to_owned());
    let basis = snapshot.basis(&id)?;
    let mut judgments = tx.judgments().to_vec();
    let others = judged_bases(&snapshot, &judgments, &id)?;
    let related = related_bases(&id, &basis, &others);
    let associations = associations(&basis);
    let ignore_case = repo.config_bool("core.ignorecase")?.unwrap_or(false);
    let names = ItemNames::new(&basis.gate, &associations, ignore_case, |path| {
        typed(&worktree, &cwd, path)
    });
    let items = resolve(&names, &call.names, |text| name(&worktree, &cwd, text))?;
    let closes = check(
        call.verb,
        &id,
        &GateStates::new(&id, &basis, &related, &Ledger::new(&judgments)),
        &items,
        &names,
        &addressing,
    )?;
    let who = retention::identity(&repo)?.ok_or_else(|| refusal(NO_IDENTITY.lines()))?;
    let first = !judgments.iter().any(|judgment| judgment.basis == id);
    if first {
        keep(&repo, &id, &basis, &who, &addressing)?;
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
        withdraws: Vec::new(),
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
            for line in recorded.lines(call.verb, &compared, first, &addressing) {
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
    /// content of an untracked file dismissed; the judgment's id and the
    /// gate; and what to run next. Commands are addressed as `addressing`
    /// says, followed by its note once when they show a value as given.
    fn lines(
        &self,
        verb: Verb,
        compared: &str,
        first: bool,
        addressing: &Addressing,
    ) -> Vec<String> {
        let id = self.review.short();
        let named: Vec<&GateItemInfo> = self
            .judgment
            .items
            .iter()
            .filter_map(|item| self.gate.items.iter().find(|info| info.id == *item))
            .collect();
        let items = sentence_list(named.iter().map(|item| item.name.as_str()));
        let review = format!("review {id} ({compared})");
        let mut lines = vec![match verb {
            Verb::Acknowledge => format!("Acknowledged {items} on {review}."),
            Verb::Dismiss => format!("Dismissed {items} on {review}."),
            Verb::Comment if named.is_empty() => format!("Commented on {review}."),
            Verb::Comment => format!("Commented on {items} on {review}."),
            Verb::Complete => format!("Completed {review}."),
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
}

/// `names` as a sentence lists them: `a`, `a and b`, `a, b, and c`.
fn sentence_list<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
    let names: Vec<&str> = names.into_iter().collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [first, second] => format!("{first} and {second}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
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

/// The basis of every review other than `id` that one of `judgments` is
/// recorded on, so carry-forward can relate it to `id`.
///
/// # Errors
///
/// Returns the store errors of [`ReviewSnapshot::basis`].
fn judged_bases(
    snapshot: &ReviewSnapshot,
    judgments: &[Judgment],
    id: &BasisId,
) -> Result<BTreeMap<BasisId, Basis>, CliError> {
    let mut bases = BTreeMap::new();
    for judgment in judgments {
        if judgment.basis != *id && !bases.contains_key(&judgment.basis) {
            bases.insert(judgment.basis.clone(), snapshot.basis(&judgment.basis)?);
        }
    }
    Ok(bases)
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
/// completion closes (empty for other calls).
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
        Verb::Comment => Ok(BTreeMap::new()),
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

/// A refused call's error: "No judgment recorded." followed by `rest`, its
/// first line on the same line, every line escaped.
fn refusal<S: AsRef<str>>(rest: impl IntoIterator<Item = S>) -> CliError {
    let mut message = "No judgment recorded.".to_owned();
    for (index, line) in rest.into_iter().enumerate() {
        message.push(if index == 0 { ' ' } else { '\n' });
        message.push_str(&escape_control(line.as_ref()));
    }
    CliError::Refused(message)
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
