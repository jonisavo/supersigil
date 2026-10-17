//! `supersigil judge list`: the saved reviews of a repository that have a
//! judgment or are a worktree's selection, most recently judged first.

use std::cmp::Reverse;
use std::io::{self, Write as _};
use std::path::PathBuf;

use serde::Serialize;
use supersigil_record::reviews::{BasisBase, BasisId, BasisTarget, ReviewSnapshot};
use supersigil_review::judging::JudgedStore;
use supersigil_review::model::JudgmentInfo;
use supersigil_review::printed::aligned;
use supersigil_review::summary::compared;

use super::Checkout;
use crate::commands::JudgeArgs;
use crate::error::CliError;
use crate::format::{OutputFormat, write_json};
use crate::review_dir::load_bases;

/// Runs `supersigil judge list`.
///
/// # Errors
///
/// As [`super::run`].
pub(super) fn run(args: &JudgeArgs) -> Result<(), CliError> {
    let checkout = Checkout::open(args)?;
    let reviews = match &checkout.store {
        Some(store) => listed(&store.snapshot()?)?,
        None => Vec::new(),
    };
    match args.format.resolve() {
        OutputFormat::Json => write_json(&List { reviews })?,
        OutputFormat::Terminal => {
            let mut out = io::stdout().lock();
            for line in lines(&reviews) {
                writeln!(out, "{line}")?;
            }
        }
    }
    Ok(())
}

/// What `judge list` prints as JSON.
#[derive(Debug, Serialize)]
struct List {
    /// Each review, most recently judged first, then those never judged.
    reviews: Vec<Listed>,
}

/// A saved review as `judge list` shows it.
#[derive(Debug, Serialize)]
struct Listed {
    /// Its full id.
    review: BasisId,
    /// Whether a worktree selects it.
    selected: bool,
    /// The worktree it reviewed.
    worktree: PathBuf,
    /// What it compared against.
    base: BasisBase,
    /// What it reviewed.
    target: BasisTarget,
    /// Whether its own standing completion completes it.
    complete: bool,
    /// How many of its gate items are open or reopened.
    open: usize,
    /// The judgment last recorded on it, if any.
    last_judgment: Option<JudgmentInfo>,
}

/// Lists reviews in `snapshot` with a judgment or worktree selection.
/// Orders them by most recent judgment, then lists reviews never judged.
///
/// # Errors
///
/// Returns the store errors of reading the judgments and the bases.
fn listed(snapshot: &ReviewSnapshot) -> Result<Vec<Listed>, CliError> {
    let judgments = snapshot.judgments()?;
    let selections = &snapshot.manifest().selections;
    let bases = load_bases(
        snapshot,
        judgments
            .iter()
            .map(|judgment| &judgment.basis)
            .chain(selections.values()),
    )?;
    let store = JudgedStore::new(&judgments, &bases);
    let mut listed: Vec<(Option<usize>, Listed)> = bases
        .iter()
        .map(|(id, basis)| {
            let review = store.review(id, basis);
            let last = store.ledger().on(id).last();
            let listed = Listed {
                review: id.clone(),
                selected: selections.values().any(|selected| selected == id),
                worktree: basis.worktree.clone(),
                base: basis.base.clone(),
                target: basis.target.clone(),
                complete: review.completion().is_some(),
                open: review.open(),
                last_judgment: last.map(|(_, judgment)| JudgmentInfo::new(judgment)),
            };
            (last.map(|(position, _)| position), listed)
        })
        .collect();
    listed.sort_by_key(|&(last, _)| Reverse(last));
    Ok(listed.into_iter().map(|(_, listed)| listed).collect())
}

/// Renders a header and one row per review, marking selected reviews with `*`.
/// Renders an empty-list message when `reviews` is empty.
fn lines(reviews: &[Listed]) -> Vec<String> {
    if reviews.is_empty() {
        return vec!["No judged or selected reviews in this repository.".to_owned()];
    }
    let header = [
        "  review",
        "target",
        "worktree",
        "last judged",
        "status",
        "by",
    ];
    let mut rows = vec![header.map(str::to_owned)];
    rows.extend(reviews.iter().map(|review| {
        let marker = if review.selected { '*' } else { ' ' };
        let last = review.last_judgment.as_ref();
        [
            format!("{marker} {}", review.review.short()),
            compared(
                review.target.commit.as_deref(),
                review.base.commit.as_deref(),
            ),
            review.worktree.display().to_string(),
            last.map_or_else(|| "not judged".to_owned(), |judgment| judgment.at.date()),
            if review.complete {
                "complete".to_owned()
            } else {
                format!("{} open", review.open)
            },
            last.map_or_else(String::new, |judgment| judgment.who.name.clone()),
        ]
    }));
    aligned(rows)
}
