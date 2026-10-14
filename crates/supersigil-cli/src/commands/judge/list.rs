//! `supersigil judge list`: the saved reviews of a repository that have a
//! judgment or are a worktree's selection, most recently judged first.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::io::{self, Write as _};
use std::path::PathBuf;

use serde::Serialize;
use supersigil_record::reviews::{BasisBase, BasisId, BasisTarget, ReviewSnapshot};
use supersigil_review::judging::{GateIndex, GateStates, Ledger, related_bases};
use supersigil_review::model::JudgmentInfo;
use supersigil_review::summary::{aligned, compared};

use super::{Checkout, load_bases};
use crate::commands::JudgeArgs;
use crate::error::CliError;
use crate::format::{OutputFormat, escape_control, write_json};

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

/// Every review in `snapshot` with a judgment or a worktree's selection,
/// the one judged last first, then those never judged.
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
    let ledger = Ledger::new(&judgments);
    let indexes: HashMap<_, _> = bases
        .iter()
        .map(|(id, basis)| (id, GateIndex::new(&basis.gate)))
        .collect();
    let mut listed: Vec<(Option<usize>, Listed)> = bases
        .iter()
        .map(|(id, basis)| {
            let related = related_bases(id, basis, &bases);
            let states = GateStates::with_indexes(id, basis, &related, &indexes, &ledger);
            let last = judgments.iter().rposition(|judgment| judgment.basis == *id);
            let listed = Listed {
                review: id.clone(),
                selected: selections.values().any(|selected| selected == id),
                worktree: basis.worktree.clone(),
                base: basis.base.clone(),
                target: basis.target.clone(),
                complete: states.own_completion().is_some(),
                open: states
                    .items()
                    .iter()
                    .filter(|judged| judged.state.closing().is_none())
                    .count(),
                last_judgment: last.map(|position| JudgmentInfo::new(&judgments[position])),
            };
            (last, listed)
        })
        .collect();
    listed.sort_by_key(|&(last, _)| Reverse(last));
    Ok(listed.into_iter().map(|(_, listed)| listed).collect())
}

/// The lines a terminal shows for `reviews`: a header, then one row per
/// review, a selected one marked `*`; or that there is none.
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
            escape_control(&review.worktree.display().to_string()),
            last.map_or_else(|| "not judged".to_owned(), |judgment| judgment.at.date()),
            if review.complete {
                "complete".to_owned()
            } else {
                format!("{} open", review.open)
            },
            last.map_or_else(String::new, |judgment| escape_control(&judgment.who.name)),
        ]
    }));
    aligned(rows)
}
