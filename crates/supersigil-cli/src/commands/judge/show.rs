//! `supersigil judge show` displays a saved review's gate items and closing
//! judgments, comments, and undo history.
//!
//! `judge` reads no records and cannot detect evidence changes. It marks a review
//! complete only with its own standing completion, and lists completions of the
//! same content as related.

use std::io::{self, Write as _};
use std::path::Path;

use serde::Serialize;
use supersigil_record::reviews::{
    BasisBase, BasisId, BasisScope, BasisTarget, ItemId, Judgment, JudgmentKind,
};
use supersigil_review::judging::{JudgedStore, Ledger};
use supersigil_review::model::{GateInfo, JudgmentInfo, StatusInfo};
use supersigil_review::names::ItemNames;
use supersigil_review::printed::{Addressing, Printed};
use supersigil_review::summary::{
    NOTHING_TO_REVIEW, base_line, compared, gate_lines, gate_row, next_lines, noun, review_line,
    shown_path, worktree_line,
};

use super::{Checkout, addressing, item_name, select};
use crate::commands::{JudgeArgs, ShowArgs};
use crate::error::CliError;
use crate::format::{OutputFormat, write_json};
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
    let snapshot = checkout.store(prefix, &addressing)?.snapshot()?;
    let id = select(&snapshot, prefix, &checkout.worktree, &mut addressing)?;
    let basis = snapshot.basis(&id)?;
    let judgments = snapshot.judgments()?;
    let others = judged_bases(&snapshot, &judgments, Some(&id), &basis)?;
    let store = JudgedStore::new(&judgments, &others);
    let ledger = store.ledger();
    let names = checkout.names(&basis)?;
    let review = store.review(&id, &basis);
    let report = review.report(&names);
    let shown = Shown {
        review: &id,
        worktree: &basis.worktree,
        base: &basis.base,
        target: &basis.target,
        scope: &basis.scope,
        status: report.status,
        same_content_completions: review.same_content(),
        gate: report.gate,
        comments: comments(&id, &judgments, ledger, &names),
        history: history(&id, &judgments, ledger, &names),
        nothing_to_review: basis.explanations.is_empty() && basis.gate.is_empty(),
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
    /// Withdrawals affecting this review's judgments or recorded on this review,
    /// in withdrawal order.
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

/// Lists standing comments from `judgments` on review `id`, naming items with `names`.
fn comments(
    id: &BasisId,
    judgments: &[Judgment],
    ledger: &Ledger<'_>,
    names: &ItemNames<'_>,
) -> Vec<Comment> {
    judgments
        .iter()
        .filter(|judgment| {
            judgment.basis == *id
                && judgment.kind == JudgmentKind::Comment
                && ledger.stands(judgment)
        })
        .map(|judgment| Comment {
            judgment: JudgmentInfo::new(judgment),
            items: named(names, &judgment.items),
        })
        .collect()
}

/// Lists withdrawals by each undo in `judgments` affecting review `id`'s judgments
/// or recorded on that review. Looks up withdrawals in `ledger` and names items
/// with `names`.
fn history(
    id: &BasisId,
    judgments: &[Judgment],
    ledger: &Ledger<'_>,
    names: &ItemNames<'_>,
) -> Vec<Withdrawn> {
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
                    items: named(names, items),
                    undo: JudgmentInfo::new(undo),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The names of `items` in `names`, as [`item_name`] gives them.
fn named<'i>(names: &ItemNames<'_>, items: impl IntoIterator<Item = &'i ItemId>) -> Vec<String> {
    items
        .into_iter()
        .map(|item| item_name(names, item))
        .collect()
}

impl Shown<'_> {
    /// Renders the header, status, gate rows, comments, history, and next commands,
    /// escaping untrusted text. Uses `addressing` for commands and appends the note
    /// when they show a value as given. Replaces status with an empty-review message
    /// and suggests no command when there is nothing to review.
    fn lines(&self, addressing: &Addressing) -> Vec<String> {
        let mut out = Printed::new();
        out.lines(self.header());
        if self.nothing_to_review {
            out.line(NOTHING_TO_REVIEW);
        } else {
            out.lines(self.status());
        }
        out.lines(gate_lines(&self.gate, |item| {
            let [name, state, text] = gate_row(item, 0);
            let text = match &item.judgment {
                Some(judgment) => format!(
                    "{text} (judgment {}){}",
                    judgment.id.short(),
                    words(judgment.reason.as_deref())
                ),
                None => text,
            };
            [name, state, text]
        }));
        if !self.comments.is_empty() {
            out.lines(["", "Comments:"]);
            out.lines(self.comments.iter().map(|comment| {
                let on = if comment.items.is_empty() {
                    "the review".to_owned()
                } else {
                    comment.items.join(", ")
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
            out.lines(["", "History:"]);
            out.lines(self.history.iter().map(|withdrawn| {
                format!(
                    "  {} {}, withdrawn {}{}",
                    self.what(withdrawn),
                    by(&withdrawn.judgment),
                    by(&withdrawn.undo),
                    words(withdrawn.undo.reason.as_deref())
                )
            }));
        }
        if !self.nothing_to_review {
            let open = self.gate.open_names();
            let next = next_lines(&open, self.status.complete, addressing, &mut out);
            if !next.is_empty() {
                out.line("");
                out.lines(next);
            }
        }
        out.finish()
    }

    /// The header: the review, what it compared, its worktree, its base
    /// and target, and its scope, paths as repository paths.
    fn header(&self) -> Vec<String> {
        let (base, target) = (self.base, self.target);
        let compared = compared(target.commit.as_deref(), base.commit.as_deref());
        let mut lines = vec![
            review_line(self.review, &compared),
            worktree_line(&self.worktree.display().to_string()),
            base_line(base.commit.as_deref(), &base.tree),
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
            let path = shown_path(path);
            lines.push(format!("  scope: untracked {path} included"));
        }
        lines
    }

    /// Renders completion from this review's own standing completion.
    /// Otherwise reports it incomplete and lists same-content completions as related.
    fn status(&self) -> Vec<String> {
        if let Some(completion) = &self.status.completion {
            return vec![format!(
                "Review complete: completed on {} by {} (judgment {}){}.",
                completion.at.date(),
                completion.who.name,
                completion.id.short(),
                words(completion.text.as_deref())
            )];
        }
        let mut lines = vec!["Review not complete.".to_owned()];
        lines.extend(self.same_content_completions.iter().map(same_content));
        lines
    }

    /// What `withdrawn` withdrew: the kind of judgment, the items it named,
    /// and its review when it is not this one.
    fn what(&self, withdrawn: &Withdrawn) -> String {
        let judgment = &withdrawn.judgment;
        let items = withdrawn.items.join(", ");
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
        judgment.who.name,
        judgment.at.date(),
        judgment.id.short()
    )
}

/// `": <text>"` when there is text.
fn words(text: Option<&str>) -> String {
    text.map_or_else(String::new, |text| format!(": {text}"))
}

/// Names `completion` of a review of the same content, which `judge` cannot
/// tell carries over without reading records.
pub(super) fn same_content(completion: &JudgmentInfo) -> String {
    format!(
        "Completed {} for the same content as review {}; supersigil review shows whether it \
         carries over.",
        completion.at.date(),
        completion.review.short()
    )
}
