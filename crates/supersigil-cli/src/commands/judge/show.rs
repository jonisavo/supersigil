//! `supersigil judge show`: one saved review as its judgments leave it:
//! each gate item with the judgment that closes it, the comments, and what
//! undos withdrew.
//!
//! `judge` reads no records, so it cannot tell whether evidence changed: a
//! review is complete only with its own standing completion, and a
//! completion of the same content is shown as related.

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::io::{self, Write as _};
use std::path::Path;

use serde::Serialize;
use supersigil_record::reviews::{
    BasisBase, BasisId, BasisScope, BasisTarget, ItemId, Judgment, JudgmentKind, PathBytes,
};
use supersigil_review::judging::{GateIndex, GateStates, Ledger, Relation, related_bases};
use supersigil_review::model::{GateInfo, GateItemInfo, JudgmentInfo, StatusInfo};
use supersigil_review::summary::{
    Addressing, NOTHING_TO_REVIEW, aligned, compared, gate_row, next_lines,
};

use super::{
    Checkout, addressing, gate_index, item_name, no_selection, noun, select, unknown_review,
};
use crate::commands::{JudgeArgs, ShowArgs};
use crate::error::CliError;
use crate::format::{OutputFormat, escape_control, write_json};
use crate::item_names::associations;
use crate::review_dir::judged_bases;

/// Runs `supersigil judge show`.
///
/// # Errors
///
/// As [`super::run`]; a review that cannot be resolved is refused without
/// "No judgment recorded.", since showing records nothing.
pub(super) fn run(args: &JudgeArgs, show: &ShowArgs) -> Result<(), CliError> {
    let mut addressing = addressing(args);
    let checkout = Checkout::open(args)?;
    let prefix = show.review.as_deref().or(args.review.as_deref());
    let Some(store) = &checkout.store else {
        return Err(match prefix {
            Some(prefix) => unknown_review(prefix, &addressing),
            None => no_selection(&checkout.worktree, &addressing),
        });
    };
    let snapshot = store.snapshot()?;
    let id = select(&snapshot, prefix, &checkout.worktree, &mut addressing)?;
    let basis = snapshot.basis(&id)?;
    let judgments = snapshot.judgments()?;
    let others = judged_bases(&snapshot, &judgments, &id)?;
    let related = related_bases(&id, &basis, &others);
    let indexes = GateIndex::of(&related);
    let ledger = Ledger::new(&judgments);
    let states = GateStates::new(&id, &basis, &related, &indexes, &ledger);
    let associations = associations(&basis);
    let gate = GateInfo::new(&states, &checkout.names(&basis, &associations)?);
    let mut same_content: Vec<&Judgment> = related
        .iter()
        .filter(|related| related.relation == Relation::SameContent)
        .filter_map(|related| ledger.completion(related.id))
        .collect();
    same_content.sort_by_key(|completion| Reverse(completion.at));
    let shown = Shown {
        review: &id,
        worktree: &basis.worktree,
        base: &basis.base,
        target: &basis.target,
        scope: &basis.scope,
        status: StatusInfo::new(&states),
        same_content_completions: same_content.into_iter().map(JudgmentInfo::new).collect(),
        comments: comments(&id, &judgments, &ledger, &gate),
        history: history(&id, &judgments, &ledger, &gate),
        nothing_to_review: basis.explanations.is_empty() && basis.gate.is_empty(),
        gate,
    };
    match args.format.resolve() {
        OutputFormat::Json => write_json(&shown)?,
        OutputFormat::Terminal => {
            let mut out = io::stdout().lock();
            for line in shown.lines(&addressing) {
                writeln!(out, "{line}")?;
            }
        }
    }
    Ok(())
}

/// A saved review as `judge show` shows it.
#[derive(Debug, Serialize)]
struct Shown<'a> {
    /// The review's full id.
    review: &'a BasisId,
    /// The worktree it reviewed.
    worktree: &'a Path,
    /// What it compared against.
    base: &'a BasisBase,
    /// What it reviewed.
    target: &'a BasisTarget,
    /// The paths it was limited to and the untracked files it included.
    scope: &'a BasisScope,
    /// Whether its own standing completion completes it.
    status: StatusInfo,
    /// The standing completions of reviews of the same content, most
    /// recent first; `supersigil review` tells whether one carries over.
    same_content_completions: Vec<JudgmentInfo>,
    /// Its gate, each item with its state and closing judgment.
    gate: GateInfo,
    /// Its standing comments, in the order recorded.
    comments: Vec<Comment>,
    /// What undos withdrew of its judgments, or recorded on it withdrew,
    /// in the order withdrawn.
    history: Vec<Withdrawn>,
    /// Whether it reviewed no changed file and has no gate item, so there
    /// is nothing to judge; the terminal says so, and JSON leaves it out.
    #[serde(skip)]
    nothing_to_review: bool,
}

/// A standing comment.
#[derive(Debug, Serialize)]
struct Comment {
    /// The comment, its note in `text`.
    judgment: JudgmentInfo,
    /// The items it is about, by name; none for the review as a whole.
    items: Vec<String>,
}

/// What one undo withdrew of one judgment.
#[derive(Debug, Serialize)]
struct Withdrawn {
    /// The withdrawn judgment.
    judgment: JudgmentInfo,
    /// The items whose acknowledgment or dismissal was withdrawn, by name;
    /// none when the judgment was withdrawn whole.
    items: Vec<String>,
    /// The undo.
    undo: JudgmentInfo,
}

/// The standing comments of `judgments` recorded on the review `id`, their
/// items named as `gate` names them.
fn comments(
    id: &BasisId,
    judgments: &[Judgment],
    ledger: &Ledger<'_>,
    gate: &GateInfo,
) -> Vec<Comment> {
    let gate = gate_index(gate);
    judgments
        .iter()
        .filter(|judgment| {
            judgment.basis == *id
                && judgment.kind == JudgmentKind::Comment
                && ledger.stands(judgment)
        })
        .map(|judgment| Comment {
            judgment: JudgmentInfo::new(judgment),
            items: names(&gate, &judgment.items),
        })
        .collect()
}

/// What every undo in `judgments` withdrew of a judgment of `ledger` on the
/// review `id`, or withdrew while recorded on it, its items named as `gate`
/// names them.
fn history(
    id: &BasisId,
    judgments: &[Judgment],
    ledger: &Ledger<'_>,
    gate: &GateInfo,
) -> Vec<Withdrawn> {
    let gate = gate_index(gate);
    judgments
        .iter()
        .filter(|judgment| judgment.kind == JudgmentKind::Undo)
        .flat_map(|undo| {
            ledger
                .withdrawn(&undo.withdraws)
                .into_iter()
                .filter(|(withdrawn, _)| withdrawn.basis == *id || undo.basis == *id)
                .map(|(withdrawn, items)| Withdrawn {
                    judgment: JudgmentInfo::new(withdrawn),
                    items: names(&gate, items),
                    undo: JudgmentInfo::new(undo),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The names of `items` in `gate`.
fn names<'i>(
    gate: &BTreeMap<&ItemId, &GateItemInfo>,
    items: impl IntoIterator<Item = &'i ItemId>,
) -> Vec<String> {
    items
        .into_iter()
        .map(|item| item_name(gate, item))
        .collect()
}

impl Shown<'_> {
    /// The lines a terminal shows, untrusted text escaped: the header, the
    /// status, the gate rows, the comments, the history, and what to run
    /// next, addressed as `addressing` says, followed by its note when the
    /// commands show a value as given. A review with nothing to review says
    /// so in place of its status and suggests no command.
    fn lines(&self, addressing: &Addressing) -> Vec<String> {
        let mut lines = self.header();
        if self.nothing_to_review {
            lines.push(NOTHING_TO_REVIEW.to_owned());
        } else {
            lines.extend(self.status());
        }
        let items = &self.gate.items;
        if items.is_empty() {
            lines.push("Gate: no items.".to_owned());
        } else {
            let closed = items.len() - self.gate.open;
            lines.push(format!("Gate: {} open, {closed} closed.", self.gate.open));
            lines.push(String::new());
            let rows = items.iter().map(|item| {
                let [name, state, text] = gate_row(item, 0, escape_control);
                let text = match &item.judgment {
                    Some(judgment) => format!(
                        "{text} (judgment {}){}",
                        judgment.id.short(),
                        words(judgment.reason.as_deref())
                    ),
                    None => text,
                };
                [name, state, text]
            });
            lines.extend(
                aligned(rows.collect())
                    .into_iter()
                    .map(|row| format!("  {row}")),
            );
        }
        if !self.comments.is_empty() {
            lines.extend([String::new(), "Comments:".to_owned()]);
            lines.extend(self.comments.iter().map(|comment| {
                let on = if comment.items.is_empty() {
                    "the review".to_owned()
                } else {
                    escape_control(&comment.items.join(", "))
                };
                let judgment = &comment.judgment;
                format!(
                    "  on {on}, {}{}",
                    by(judgment),
                    words(judgment.text.as_deref())
                )
            }));
        }
        if !self.history.is_empty() {
            lines.extend([String::new(), "History:".to_owned()]);
            lines.extend(self.history.iter().map(|withdrawn| {
                format!(
                    "  {} {}, withdrawn {}{}",
                    self.what(withdrawn),
                    by(&withdrawn.judgment),
                    by(&withdrawn.undo),
                    words(withdrawn.undo.reason.as_deref())
                )
            }));
        }
        let open: Vec<&str> = items
            .iter()
            .filter(|item| item.state.is_open())
            .map(|item| item.name.as_str())
            .collect();
        let next = if self.nothing_to_review {
            Vec::new()
        } else {
            next_lines(&open, self.status.complete, addressing, escape_control)
        };
        if !next.is_empty() {
            lines.push(String::new());
            lines.extend(next);
            lines.extend(addressing.note());
        }
        lines
    }

    /// The header: the review, what it compared, its worktree, its base
    /// and target, and its scope, paths as repository paths.
    fn header(&self) -> Vec<String> {
        let (base, target) = (self.base, self.target);
        let mut lines = vec![
            format!(
                "Review {}: {}.",
                self.review.short(),
                compared(target.commit.as_deref(), base.commit.as_deref())
            ),
            format!(
                "  worktree: {}",
                escape_control(&self.worktree.display().to_string())
            ),
            match &base.commit {
                Some(commit) => format!("  base: {commit} (tree {})", base.tree),
                None => format!("  base: empty tree {}", base.tree),
            },
            match &target.commit {
                Some(commit) => format!("  target: commit {commit} (tree {})", target.tree),
                None => format!("  target: working tree (tree {})", target.tree),
            },
        ];
        let scope = self.scope;
        if scope.paths.is_empty() && scope.include_untracked.is_empty() {
            lines.push("  scope: all tracked paths".to_owned());
        }
        if !scope.paths.is_empty() {
            let paths: Vec<String> = scope.paths.iter().map(shown_path).collect();
            lines.push(format!("  scope: limited to {}", paths.join(", ")));
        }
        for path in &scope.include_untracked {
            lines.push(format!("  scope: untracked {} included", shown_path(path)));
        }
        lines
    }

    /// The status: complete with the review's own completion; otherwise
    /// not, with each completion of the same content as related.
    fn status(&self) -> Vec<String> {
        if let Some(completion) = &self.status.completion {
            return vec![format!(
                "Review complete: completed on {} by {} (judgment {}){}.",
                completion.at.date(),
                escape_control(&completion.who.name),
                completion.id.short(),
                words(completion.text.as_deref())
            )];
        }
        let mut lines = vec!["Review not complete.".to_owned()];
        lines.extend(self.same_content_completions.iter().map(|completion| {
            format!(
                "Completed {} for the same content as review {}; supersigil review shows \
                 whether it carries over.",
                completion.at.date(),
                completion.review.short()
            )
        }));
        lines
    }

    /// What `withdrawn` withdrew: the kind of judgment, the items it named,
    /// and its review when it is not this one.
    fn what(&self, withdrawn: &Withdrawn) -> String {
        let judgment = &withdrawn.judgment;
        let items = escape_control(&withdrawn.items.join(", "));
        let what = match judgment.kind {
            JudgmentKind::Acknowledge | JudgmentKind::Dismiss => {
                format!("{} of {items}", noun(judgment.kind))
            }
            kind if items.is_empty() => noun(kind).to_owned(),
            kind => format!("{} on {items}", noun(kind)),
        };
        if judgment.review == *self.review {
            what
        } else {
            format!("{what} on review {}", judgment.review.short())
        }
    }
}

/// Who recorded `judgment`, when, and its id.
fn by(judgment: &JudgmentInfo) -> String {
    format!(
        "by {} on {} (judgment {})",
        escape_control(&judgment.who.name),
        judgment.at.date(),
        judgment.id.short()
    )
}

/// `": <text>"`, escaped, when there is text.
fn words(text: Option<&str>) -> String {
    text.map_or_else(String::new, |text| format!(": {}", escape_control(text)))
}

/// `path`, a repository path, as a terminal shows it.
fn shown_path(path: &PathBytes) -> String {
    escape_control(&String::from_utf8_lossy(path.as_bytes()))
}
