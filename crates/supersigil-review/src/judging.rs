//! Rules for related reviews, standing judgments, gate states, completion,
//! and undo withdrawals.
//!
//! [`JudgedStore`] takes the judgments and bases the CLI loads from the
//! review store, and [`JudgedReview`] is one review's judged view: its item
//! states, its completion, and its status and gate for output.

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use serde::Serialize;
use supersigil_record::JudgmentId;
use supersigil_record::reviews::{
    Basis, BasisHeader, BasisId, BasisScope, Fingerprint, GateItem, ItemId, Judgment, JudgmentKind,
    Origin, PathBytes, Withdrawal,
};
use supersigil_record::time::UtcTime;

use crate::model::{
    CompletionSource, GateInfo, JudgmentInfo, RelatedInfo, RelatedReading, StatusInfo,
};
use crate::names::ItemNames;

/// How an earlier saved review relates to the current one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    /// Same base tree, target tree, and scope, from any worktree or
    /// command.
    SameContent,
    /// Same worktree, base tree, and scope; another target tree.
    CodeChanged,
}

/// A saved review related to the current one, as [`JudgedReview`] hands it
/// out for reading the changes since.
#[derive(Debug, Clone, Copy)]
pub struct Earlier<'a> {
    /// The related review's basis ID.
    id: &'a BasisId,
    /// Its basis.
    basis: &'a Basis,
    /// How it relates to the current review.
    relation: Relation,
}

impl<'a> Earlier<'a> {
    /// The related review's basis ID.
    #[must_use]
    pub const fn id(self) -> &'a BasisId {
        self.id
    }

    /// Its basis.
    #[must_use]
    pub const fn basis(self) -> &'a Basis {
        self.basis
    }

    /// How it relates to the current review.
    #[must_use]
    pub const fn relation(self) -> Relation {
        self.relation
    }
}

/// Finds reviews in `others` related to review `id` with `basis`, preserving
/// input order and excluding the current review itself.
fn related_bases<'a>(
    id: &BasisId,
    basis: &Basis,
    others: impl IntoIterator<Item = (&'a BasisId, &'a Basis)>,
) -> Vec<Earlier<'a>> {
    others
        .into_iter()
        .filter(|(other_id, _)| *other_id != id)
        .filter_map(|(other_id, other)| {
            relation(basis.into(), other.into()).map(|relation| Earlier {
                id: other_id,
                basis: other,
                relation,
            })
        })
        .collect()
}

/// Checks whether `other` relates to `current` by the rules
/// [`JudgedStore::review`] relates reviews by. If not, the caller can skip
/// reading the rest of `other`'s basis.
#[must_use]
pub fn relates(current: &Basis, other: &BasisHeader) -> bool {
    relation(current.into(), other.into()).is_some()
}

/// Basis fields used to relate two saved reviews.
#[derive(Clone, Copy)]
struct Compared<'a> {
    /// The reviewed worktree.
    worktree: &'a Path,
    /// The base tree.
    base: &'a str,
    /// The target tree.
    target: &'a str,
    /// The path options.
    scope: &'a BasisScope,
}

impl<'a> From<&'a Basis> for Compared<'a> {
    fn from(basis: &'a Basis) -> Self {
        Self {
            worktree: &basis.worktree,
            base: &basis.base.tree,
            target: &basis.target.tree,
            scope: &basis.scope,
        }
    }
}

impl<'a> From<&'a BasisHeader> for Compared<'a> {
    fn from(header: &'a BasisHeader) -> Self {
        Self {
            worktree: &header.worktree,
            base: &header.base.tree,
            target: &header.target.tree,
            scope: &header.scope,
        }
    }
}

fn relation(current: Compared<'_>, other: Compared<'_>) -> Option<Relation> {
    if current.base != other.base || current.scope != other.scope {
        None
    } else if current.target == other.target {
        Some(Relation::SameContent)
    } else if current.worktree == other.worktree {
        Some(Relation::CodeChanged)
    } else {
        None
    }
}

/// Tracks judgments in a review store and which remain standing.
///
/// An acknowledgment or dismissal stands for an item until an undo withdraws
/// that judgment-item pair. A comment or completion stands until an undo
/// withdraws the whole judgment. A standing completion counts the standing
/// judgments its `closes` names as closing those items on its review, so they
/// carry from it as from that review's own judgments.
#[derive(Debug)]
pub struct Ledger<'a> {
    /// Every judgment, in the order recorded.
    judgments: &'a [Judgment],
    /// Every withdrawal of every undo: a pair, or a whole judgment with no
    /// item.
    withdrawn: HashSet<(&'a JudgmentId, Option<&'a ItemId>)>,
    /// The position in `judgments` of each judgment, by ID; the first
    /// recorded when several share one.
    by_id: HashMap<&'a JudgmentId, usize>,
    /// Positions in `judgments` of the judgments on each basis.
    by_basis: HashMap<&'a BasisId, Vec<usize>>,
    /// Positions of each item's standing acknowledgments and dismissals,
    /// grouped by basis in recorded order. Resolving a state neither scans
    /// judgment item lists nor visits reviews that never closed the item.
    closing: HashMap<&'a ItemId, HashMap<&'a BasisId, Vec<usize>>>,
    /// Positions of each item's standing acknowledgments and dismissals
    /// that a standing completion counts, grouped by the completion's basis.
    counted: HashMap<&'a ItemId, HashMap<&'a BasisId, Vec<usize>>>,
    /// Each standing acknowledgment or dismissal of an item, by position.
    standing: HashSet<(usize, &'a ItemId)>,
}

impl<'a> Ledger<'a> {
    /// Builds a ledger from all store judgments in recorded order.
    #[must_use]
    pub fn new(judgments: &'a [Judgment]) -> Self {
        let mut withdrawn = HashSet::new();
        let mut by_id = HashMap::new();
        let mut by_basis: HashMap<_, Vec<_>> = HashMap::new();
        for (position, judgment) in judgments.iter().enumerate() {
            by_id.entry(&judgment.id).or_insert(position);
            by_basis.entry(&judgment.basis).or_default().push(position);
            if judgment.kind == JudgmentKind::Undo {
                withdrawn.extend(
                    judgment
                        .withdraws
                        .iter()
                        .map(|withdrawal| (&withdrawal.judgment, withdrawal.item.as_ref())),
                );
            }
        }
        let mut closing: HashMap<_, HashMap<_, Vec<_>>> = HashMap::new();
        let mut standing = HashSet::new();
        for (position, judgment) in judgments.iter().enumerate() {
            let closes = matches!(
                judgment.kind,
                JudgmentKind::Acknowledge | JudgmentKind::Dismiss
            ) && !withdrawn.contains(&(&judgment.id, None));
            if !closes {
                continue;
            }
            for item in &judgment.items {
                if withdrawn.contains(&(&judgment.id, Some(item))) {
                    continue;
                }
                standing.insert((position, item));
                let positions = closing
                    .entry(item)
                    .or_default()
                    .entry(&judgment.basis)
                    .or_default();
                // An item named twice in one judgment is listed once.
                if positions.last() != Some(&position) {
                    positions.push(position);
                }
            }
        }
        let mut ledger = Self {
            judgments,
            withdrawn,
            by_id,
            by_basis,
            closing,
            counted: HashMap::new(),
            standing,
        };
        let mut counted: HashMap<_, HashMap<_, Vec<_>>> = HashMap::new();
        for completion in judgments {
            if completion.kind != JudgmentKind::Complete || !ledger.stands(completion) {
                continue;
            }
            for (item, id) in &completion.closes {
                if let Some(position) = ledger.standing_position(id, item) {
                    counted
                        .entry(item)
                        .or_default()
                        .entry(&completion.basis)
                        .or_default()
                        .push(position);
                }
            }
        }
        ledger.counted = counted;
        ledger
    }

    /// Finds judgment `id`, if present. Returns the first recorded judgment
    /// when several share the ID.
    #[must_use]
    pub fn judgment(&self, id: &JudgmentId) -> Option<&'a Judgment> {
        self.by_id
            .get(id)
            .map(|&position| &self.judgments[position])
    }

    /// The judgments recorded on `basis`, in the order recorded, each with
    /// its position in the log.
    pub fn on(&self, basis: &BasisId) -> impl Iterator<Item = (usize, &'a Judgment)> {
        let judgments = self.judgments;
        self.by_basis
            .get(basis)
            .into_iter()
            .flatten()
            .map(move |&position| (position, &judgments[position]))
    }

    /// Groups `withdraws` by judgment in withdrawal order, omitting judgments
    /// absent from the store. Lists each judgment once with its withdrawn
    /// items, or all its items for a whole-judgment withdrawal.
    #[must_use]
    pub fn withdrawn<'u>(&self, withdraws: &'u [Withdrawal]) -> Vec<(&'a Judgment, Vec<&'u ItemId>)>
    where
        'a: 'u,
    {
        let mut grouped: Vec<(&JudgmentId, Vec<&ItemId>)> = Vec::new();
        let mut at: HashMap<&JudgmentId, usize> = HashMap::new();
        for withdrawal in withdraws {
            if let Some(&index) = at.get(&withdrawal.judgment) {
                grouped[index].1.extend(&withdrawal.item);
            } else {
                at.insert(&withdrawal.judgment, grouped.len());
                grouped.push((&withdrawal.judgment, withdrawal.item.iter().collect()));
            }
        }
        grouped
            .into_iter()
            .filter_map(|(id, items)| {
                let judgment = self.judgment(id)?;
                let items = if items.is_empty() {
                    judgment.items.iter().collect()
                } else {
                    items
                };
                Some((judgment, items))
            })
            .collect()
    }

    /// Checks whether `judgment` still stands for any item if it acknowledges
    /// or dismisses, or as a whole if it comments or completes. Undos always
    /// stand because nothing withdraws them.
    #[must_use]
    pub fn stands(&self, judgment: &Judgment) -> bool {
        match judgment.kind {
            JudgmentKind::Acknowledge | JudgmentKind::Dismiss => judgment
                .items
                .iter()
                .any(|item| self.not_withdrawn(judgment, item)),
            JudgmentKind::Comment | JudgmentKind::Complete => {
                !self.withdrawn.contains(&(&judgment.id, None))
            }
            JudgmentKind::Undo => true,
        }
    }

    fn not_withdrawn(&self, judgment: &Judgment, item: &ItemId) -> bool {
        !self.withdrawn.contains(&(&judgment.id, Some(item)))
            && !self.withdrawn.contains(&(&judgment.id, None))
    }

    /// The most recent standing completion recorded on `basis`.
    #[must_use]
    pub fn completion(&self, basis: &BasisId) -> Option<&'a Judgment> {
        self.latest_completion([basis])
    }

    /// The most recent standing completion recorded on any of `bases`.
    #[must_use]
    pub fn latest_completion<'b>(
        &self,
        bases: impl IntoIterator<Item = &'b BasisId>,
    ) -> Option<&'a Judgment> {
        self.latest_completion_where(bases, |_| true)
    }

    /// The most recent standing completion recorded on any of `bases` that
    /// `counts` accepts.
    fn latest_completion_where<'b>(
        &self,
        bases: impl IntoIterator<Item = &'b BasisId>,
        counts: impl Fn(&Judgment) -> bool,
    ) -> Option<&'a Judgment> {
        self.standing_completions(bases)
            .filter(|&(_, judgment)| counts(judgment))
            .max_by_key(|&(position, judgment)| recency(position, judgment))
            .map(|(_, judgment)| judgment)
    }

    /// The standing completions recorded on any of `bases`, with their
    /// positions.
    fn standing_completions<'b>(
        &self,
        bases: impl IntoIterator<Item = &'b BasisId>,
    ) -> impl Iterator<Item = (usize, &'a Judgment)> {
        bases
            .into_iter()
            .flat_map(|basis| self.on(basis))
            .filter(|(_, judgment)| {
                judgment.kind == JudgmentKind::Complete && self.stands(judgment)
            })
    }

    /// The position of judgment `id` if it is an acknowledgment or
    /// dismissal that still stands for `item`.
    fn standing_position(&self, id: &JudgmentId, item: &ItemId) -> Option<usize> {
        self.by_id
            .get(id)
            .copied()
            .filter(|&position| self.standing.contains(&(position, item)))
    }

    /// Resolves `prefix` against all judgments. Accepts a full ID or at least
    /// 8 leading characters, ignoring ASCII case.
    ///
    /// # Errors
    ///
    /// Returns [`FindJudgmentError::TooShort`] for a prefix shorter than 8
    /// characters, [`FindJudgmentError::NoMatch`] when no judgment's ID
    /// starts with it, or [`FindJudgmentError::Ambiguous`] when several
    /// do.
    pub fn find(&self, prefix: &str) -> Result<&'a Judgment, FindJudgmentError<'a>> {
        if prefix.chars().count() < 8 {
            return Err(FindJudgmentError::TooShort);
        }
        let prefix = prefix.to_ascii_lowercase();
        let matching: Vec<_> = self
            .judgments
            .iter()
            .filter(|judgment| judgment.id.as_str().starts_with(&prefix))
            .collect();
        match matching.as_slice() {
            [] => Err(FindJudgmentError::NoMatch),
            [judgment] => Ok(judgment),
            _ => Err(FindJudgmentError::Ambiguous(matching)),
        }
    }

    /// Plans `undo --judgment` for `judgment`. Withdraws a standing comment or
    /// completion as a whole. For an acknowledgment or dismissal, withdraws
    /// each standing item pair, then every standing completion that counts
    /// one of those pairs.
    ///
    /// # Errors
    ///
    /// Returns [`WithdrawRefusal::Undo`] for an undo and
    /// [`WithdrawRefusal::Withdrawn`] for a judgment of which nothing
    /// stands.
    pub fn plan_withdrawal(&self, judgment: &Judgment) -> Result<Vec<Withdrawal>, WithdrawRefusal> {
        match judgment.kind {
            JudgmentKind::Undo => Err(WithdrawRefusal::Undo),
            _ if !self.stands(judgment) => Err(WithdrawRefusal::Withdrawn),
            JudgmentKind::Comment | JudgmentKind::Complete => Ok(vec![Withdrawal {
                judgment: judgment.id.clone(),
                item: None,
            }]),
            JudgmentKind::Acknowledge | JudgmentKind::Dismiss => Ok(self.withdraw_pairs(
                judgment
                    .items
                    .iter()
                    .filter(|item| self.not_withdrawn(judgment, item))
                    .map(|item| (judgment, item)),
            )),
        }
    }

    /// Withdraws each standing acknowledgment or dismissal pair in `pairs`
    /// once, then every standing completion that counts one of those pairs,
    /// in recorded order.
    fn withdraw_pairs<'j, 'i>(
        &self,
        pairs: impl IntoIterator<Item = (&'j Judgment, &'i ItemId)>,
    ) -> Vec<Withdrawal> {
        let mut seen = HashSet::new();
        let pairs: Vec<_> = pairs
            .into_iter()
            .filter(|&(judgment, item)| seen.insert((&judgment.id, item)))
            .collect();
        let completions = self.judgments.iter().filter(|completion| {
            completion.kind == JudgmentKind::Complete
                && self.stands(completion)
                && pairs
                    .iter()
                    .any(|&(judgment, item)| completion.closes.get(item) == Some(&judgment.id))
        });
        pairs
            .iter()
            .map(|&(judgment, item)| (judgment, Some(item.clone())))
            .chain(completions.map(|completion| (completion, None)))
            .map(|(judgment, item)| Withdrawal {
                judgment: judgment.id.clone(),
                item,
            })
            .collect()
    }

    /// The standing acknowledgments and dismissals of `item` on `basis`,
    /// with their positions in the log.
    fn closing<'s>(
        &'s self,
        basis: &'s BasisId,
        item: &'s ItemId,
    ) -> impl Iterator<Item = (usize, &'a Judgment)> + 's {
        let judgments = self.judgments;
        self.closing
            .get(item)
            .and_then(|bases| bases.get(basis))
            .into_iter()
            .flatten()
            .map(move |&position| (position, &judgments[position]))
    }

    /// The standing acknowledgments and dismissals of `item` on any basis,
    /// or counted by a standing completion on it, each with that basis and
    /// its position in the log.
    fn closing_anywhere<'s>(
        &'s self,
        item: &'s ItemId,
    ) -> impl Iterator<Item = (&'a BasisId, usize, &'a Judgment)> + 's {
        let judgments = self.judgments;
        [&self.closing, &self.counted]
            .into_iter()
            .filter_map(move |index| index.get(item))
            .flatten()
            .flat_map(move |(&basis, positions)| {
                positions
                    .iter()
                    .map(move |&position| (basis, position, &judgments[position]))
            })
    }
}

/// Why a judgment ID prefix names no single judgment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindJudgmentError<'a> {
    /// The prefix has fewer than 8 characters.
    TooShort,
    /// No judgment's ID starts with it.
    NoMatch,
    /// Every judgment whose ID starts with it, in the order recorded.
    Ambiguous(Vec<&'a Judgment>),
}

/// Why `undo --judgment` is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WithdrawRefusal {
    /// The judgment is an undo, which nothing withdraws.
    Undo,
    /// No part of the judgment still stands.
    Withdrawn,
}

/// The key that orders judgments from least to most recent: `at`, then
/// `position`, the judgment's position in the log.
fn recency(position: usize, judgment: &Judgment) -> (UtcTime, usize) {
    (judgment.at, position)
}

/// Every judgment of a review store, with the saved reviews judged views
/// relate to. Built once and asked about one review at a time.
#[derive(Debug)]
pub struct JudgedStore<'a> {
    /// Every judgment in the store.
    ledger: Ledger<'a>,
    /// The saved reviews that may relate, in the order given.
    bases: Vec<(&'a BasisId, &'a Basis)>,
    /// Each of their gates, by basis ID.
    indexes: HashMap<&'a BasisId, GateIndex<'a>>,
}

impl<'a> JudgedStore<'a> {
    /// Builds a store view from all store `judgments` in recorded order and
    /// the loaded `bases` of saved reviews that may relate to the reviews
    /// asked about. Only reviews in `bases` relate to a review or carry
    /// judgments and completions to it; a review in `bases` is never related
    /// to itself. Their order is the order of related reviews in status.
    #[must_use]
    pub fn new(
        judgments: &'a [Judgment],
        bases: impl IntoIterator<Item = (&'a BasisId, &'a Basis)>,
    ) -> Self {
        let bases: Vec<_> = bases.into_iter().collect();
        let indexes = bases
            .iter()
            .map(|&(id, basis)| (id, GateIndex::new(&basis.gate)))
            .collect();
        Self {
            ledger: Ledger::new(judgments),
            bases,
            indexes,
        }
    }

    /// The judged view of review `id` with `basis`, which need not be in
    /// the store's bases.
    #[must_use]
    pub fn review<'s>(&'s self, id: &'s BasisId, basis: &'s Basis) -> JudgedReview<'s> {
        let related = related_bases(id, basis, self.bases.iter().copied());
        let states = GateStates::new(id, basis, &related, &self.indexes, &self.ledger);
        JudgedReview {
            basis,
            related,
            states,
        }
    }

    /// Every judgment in the store.
    #[must_use]
    pub const fn ledger(&self) -> &Ledger<'a> {
        &self.ledger
    }
}

/// One review's judged view: its gate items' states, the related reviews,
/// and its completion.
///
/// A standing completion counts only if every gate item is closed and, for
/// each item, the judgment its `closes` names still stands for that item.
/// The same rule decides the review's own completion, a completion carried
/// from a review of the same content, and whether a new completion is
/// refused as already complete.
#[derive(Debug)]
pub struct JudgedReview<'a> {
    /// The review's basis.
    basis: &'a Basis,
    /// The related reviews, in store order.
    related: Vec<Earlier<'a>>,
    /// The gate items' states.
    states: GateStates<'a>,
}

impl<'a> JudgedReview<'a> {
    /// Every gate item with its state, in gate order.
    #[must_use]
    pub fn items(&self) -> &[JudgedItem<'a>] {
        self.states.items()
    }

    /// How many gate items need a judgment: those open or reopened.
    #[must_use]
    pub fn open(&self) -> usize {
        self.items()
            .iter()
            .filter(|judged| judged.state.closing().is_none())
            .count()
    }

    /// The review's own most recent standing completion that counts.
    #[must_use]
    pub fn completion(&self) -> Option<&'a Judgment> {
        self.states
            .ledger
            .latest_completion_where([self.states.id], |completion| self.counts(completion))
    }

    /// Builds the status and gate without reading the changes since related
    /// reviews, naming items with `names`. Only the review's own completion
    /// can complete it, and the status relates no reviews.
    #[must_use]
    pub fn report(&self, names: &ItemNames<'_>) -> Report {
        let completion = self.completion();
        let source = if completion.is_some() {
            CompletionSource::Own
        } else {
            CompletionSource::NotChecked
        };
        self.reported(completion, source, Vec::new(), names)
    }

    /// Builds the status and gate with the changes since related reviews,
    /// naming items with `names`. `read` gets every related review, in store
    /// order, and returns one reading for each, in the same order.
    ///
    /// Uses the review's own completion first, then the most recent
    /// related completion that carries. Carrying requires the same content,
    /// every current gate item in the earlier gate with a matching
    /// fingerprint, and no evidence changes since that review.
    ///
    /// # Errors
    ///
    /// Returns the error of `read`.
    ///
    /// # Panics
    ///
    /// Panics if `read` returns a different number of readings than related
    /// reviews.
    pub fn report_reading<E>(
        &self,
        names: &ItemNames<'_>,
        read: impl FnOnce(&[Earlier<'a>]) -> Result<Vec<RelatedReading>, E>,
    ) -> Result<Report, E> {
        let readings = read(&self.related)?;
        assert_eq!(
            readings.len(),
            self.related.len(),
            "one reading per related review"
        );
        let ledger = self.states.ledger;
        let mut related: Vec<RelatedInfo> = self
            .related
            .iter()
            .zip(readings)
            .map(|(&earlier, reading)| {
                let completion = ledger.completion(earlier.id);
                RelatedInfo::new(self.basis, earlier, reading, completion, names)
            })
            .collect();
        let carried = ledger.latest_completion_where(
            related
                .iter()
                .filter(|info| info.carries())
                .map(|info| &info.review),
            |completion| self.counts(completion),
        );
        let about =
            carried.or_else(|| ledger.latest_completion(related.iter().map(|info| &info.review)));
        // Put the relevant completion first; preserve the others' order.
        related.sort_by_key(|info| about.is_none_or(|about| about.basis != info.review));
        let (completion, source) = match (self.completion(), carried) {
            (Some(own), _) => (Some(own), CompletionSource::Own),
            (None, Some(carried)) => (
                Some(carried),
                CompletionSource::Carried(carried.basis.clone()),
            ),
            (None, None) => (None, CompletionSource::Absent),
        };
        Ok(self.reported(completion, source, related, names))
    }

    /// Builds the `closes` map for a new completion of the current review,
    /// mapping each gate item to its closing judgment.
    ///
    /// # Errors
    ///
    /// Returns [`CompleteRefusal::AlreadyComplete`] with the current
    /// review's own completion, or [`CompleteRefusal::Unclosed`] with the
    /// items that are open or reopened.
    pub fn check_complete(&self) -> Result<BTreeMap<ItemId, JudgmentId>, CompleteRefusal<'a>> {
        if let Some(completion) = self.completion() {
            return Err(CompleteRefusal::AlreadyComplete(completion));
        }
        let items = self.items();
        let unclosed: Vec<_> = items
            .iter()
            .filter(|judged| judged.state.closing().is_none())
            .map(|judged| judged.item)
            .collect();
        if !unclosed.is_empty() {
            return Err(CompleteRefusal::Unclosed(unclosed));
        }
        Ok(items
            .iter()
            .filter_map(|judged| {
                let closing = judged.state.closing()?;
                Some((judged.item.id.clone(), closing.id.clone()))
            })
            .collect())
    }

    /// Checks whether a new acknowledgment or dismissal may name `items`.
    /// Ignores IDs absent from the current review's gate.
    ///
    /// # Errors
    ///
    /// Returns every named item that a judgment already closes, in the
    /// order named.
    pub fn check_close(&self, items: &[ItemId]) -> Result<(), Vec<AlreadyClosed<'a>>> {
        self.states.check_close(items)
    }

    /// Plans `undo <items>` on the current review, ignoring IDs absent from
    /// its gate. Withdraws every standing acknowledgment or dismissal that
    /// closes each item, own or carried, including older closers. Then
    /// withdraws every standing completion on any review that counts one of
    /// those judgment-item pairs.
    ///
    /// # Errors
    ///
    /// Returns every named item that nothing closes, with its state, in
    /// the order named.
    pub fn plan_undo(&self, items: &[ItemId]) -> Result<Vec<Withdrawal>, Vec<NotClosed<'a>>> {
        self.states.plan_undo(items)
    }

    /// Checks whether `completion` counts for this review: every gate item
    /// is closed, and the judgment `completion` names for each item still
    /// stands for it.
    fn counts(&self, completion: &Judgment) -> bool {
        let ledger = self.states.ledger;
        self.open() == 0
            && self.items().iter().all(|judged| {
                completion
                    .closes
                    .get(&judged.item.id)
                    .is_some_and(|closer| {
                        ledger.standing_position(closer, &judged.item.id).is_some()
                    })
            })
    }

    /// Builds the report with `completion` from `source` and `related`
    /// review details.
    fn reported(
        &self,
        completion: Option<&Judgment>,
        source: CompletionSource,
        related: Vec<RelatedInfo>,
        names: &ItemNames<'_>,
    ) -> Report {
        let ledger = self.states.ledger;
        let mut same_content: Vec<&Judgment> = self
            .related
            .iter()
            .filter(|earlier| earlier.relation == Relation::SameContent)
            .filter_map(|earlier| ledger.completion(earlier.id))
            .collect();
        same_content.sort_by_key(|completion| Reverse(completion.at));
        Report {
            status: StatusInfo {
                complete: completion.is_some(),
                completion: completion.map(JudgmentInfo::new),
                related,
                source,
            },
            gate: GateInfo::judged(self.items(), names),
            same_content: same_content.into_iter().map(JudgmentInfo::new).collect(),
        }
    }

    /// The standing completions of this review that do not count, which
    /// every reachable history avoids.
    #[cfg(test)]
    fn uncounted(&self) -> Vec<&'a Judgment> {
        self.states
            .ledger
            .standing_completions([self.states.id])
            .map(|(_, judgment)| judgment)
            .filter(|judgment| !self.counts(judgment))
            .collect()
    }
}

/// A judged review's status and gate, as output shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Whether the review is complete, and what changed since related
    /// reviews when they were read.
    pub status: StatusInfo,
    /// The gate items and their states.
    pub gate: GateInfo,
    /// The most recent standing completion of each related review of the
    /// same content, most recent first.
    pub same_content: Vec<JudgmentInfo>,
}

/// Every gate item of the current review with its state.
#[derive(Debug)]
struct GateStates<'a> {
    /// The current review's basis ID.
    id: &'a BasisId,
    /// Every judgment in the store.
    ledger: &'a Ledger<'a>,
    /// The related reviews' gates, by basis ID.
    related: HashMap<&'a BasisId, &'a GateIndex<'a>>,
    /// The current review's gate items with their states, in gate order.
    items: Vec<JudgedItem<'a>>,
    /// Each item's position in `items`, by ID.
    positions: HashMap<&'a ItemId, usize>,
}

/// Indexes a saved review's gate by ID for reuse across related reviews'
/// [`GateStates`].
#[derive(Debug, Clone)]
struct GateIndex<'a>(HashMap<&'a ItemId, &'a GateItem>);

impl<'a> GateIndex<'a> {
    /// Indexes `gate`, a saved review's gate items.
    fn new(gate: &'a [GateItem]) -> Self {
        Self(gate.iter().map(|item| (&item.id, item)).collect())
    }
}

/// A gate item of the current review and its state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JudgedItem<'a> {
    /// The item.
    pub item: &'a GateItem,
    /// Its state.
    pub state: ItemState<'a>,
}

/// Whether a gate item is closed on the current review, and by what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemState<'a> {
    /// A standing acknowledgment on the current review closes it.
    Acknowledged(&'a Judgment),
    /// A standing dismissal on the current review closes it.
    Dismissed(&'a Judgment),
    /// No standing acknowledgment or dismissal on the current review closes
    /// it. A standing acknowledgment or dismissal carries from a related
    /// review whose gate holds the item with a matching fingerprint.
    Carried(&'a Judgment),
    /// Nothing closes it, but a related review has a standing
    /// acknowledgment or dismissal of it, made when its fingerprint
    /// differed.
    Reopened {
        /// The most recent such judgment.
        judgment: &'a Judgment,
        /// The reopen reason of each fingerprint part that differs, in
        /// part-name order.
        reasons: Vec<&'static str>,
    },
    /// No standing acknowledgment or dismissal closes or reopens it.
    Open,
}

impl<'a> ItemState<'a> {
    /// Returns the item's closing acknowledgment or dismissal, own or
    /// carried, or `None` for an open or reopened item.
    #[must_use]
    pub const fn closing(&self) -> Option<&'a Judgment> {
        match self {
            Self::Acknowledged(judgment) | Self::Dismissed(judgment) | Self::Carried(judgment) => {
                Some(judgment)
            }
            Self::Reopened { .. } | Self::Open => None,
        }
    }
}

impl<'a> GateStates<'a> {
    /// Resolves gate states for review `id` and `basis` using `related`
    /// reviews and `ledger` judgments. `indexes` maps related basis IDs to
    /// gates and may contain other reviews. Related reviews missing from
    /// `indexes` cannot close items here.
    fn new(
        id: &'a BasisId,
        basis: &'a Basis,
        related: &[Earlier<'a>],
        indexes: &'a HashMap<&'a BasisId, GateIndex<'a>>,
        ledger: &'a Ledger<'a>,
    ) -> Self {
        let related = related
            .iter()
            .filter_map(|related| Some((related.id, indexes.get(related.id)?)))
            .collect();
        let mut states = Self {
            id,
            ledger,
            related,
            items: Vec::new(),
            positions: basis
                .gate
                .iter()
                .enumerate()
                .map(|(position, item)| (&item.id, position))
                .collect(),
        };
        states.items = basis
            .gate
            .iter()
            .map(|item| JudgedItem {
                item,
                state: states.state(item),
            })
            .collect();
        states
    }

    /// Every gate item with its state, in gate order.
    fn items(&self) -> &[JudgedItem<'a>] {
        &self.items
    }

    /// As [`JudgedReview::check_close`].
    fn check_close(&self, items: &[ItemId]) -> Result<(), Vec<AlreadyClosed<'a>>> {
        let closed: Vec<_> = items
            .iter()
            .filter_map(|id| {
                let judged = self.judged(id)?;
                Some(AlreadyClosed {
                    item: judged.item,
                    judgment: judged.state.closing()?,
                    carried: matches!(judged.state, ItemState::Carried(_)),
                })
            })
            .collect();
        if closed.is_empty() {
            Ok(())
        } else {
            Err(closed)
        }
    }

    /// As [`JudgedReview::plan_undo`].
    fn plan_undo(&self, items: &[ItemId]) -> Result<Vec<Withdrawal>, Vec<NotClosed<'a>>> {
        let named: Vec<_> = items.iter().filter_map(|id| self.judged(id)).collect();
        let unclosed: Vec<_> = named
            .iter()
            .filter(|judged| judged.state.closing().is_none())
            .map(|judged| NotClosed {
                item: judged.item,
                state: judged.state.clone(),
            })
            .collect();
        if !unclosed.is_empty() {
            return Err(unclosed);
        }
        Ok(self.ledger.withdraw_pairs(named.iter().flat_map(|judged| {
            self.closers(judged.item)
                .into_iter()
                .map(|judgment| (judgment, &judged.item.id))
        })))
    }

    /// Resolves `item` using the most recent standing own closer, then the
    /// most recent related closer with a matching fingerprint, then the most
    /// recent differing closer to reopen it. Otherwise leaves it open.
    fn state(&self, item: &'a GateItem) -> ItemState<'a> {
        let own = self
            .ledger
            .closing(self.id, &item.id)
            .max_by_key(|&(position, judgment)| recency(position, judgment));
        if let Some((_, judgment)) = own {
            return if judgment.kind == JudgmentKind::Dismiss {
                ItemState::Dismissed(judgment)
            } else {
                ItemState::Acknowledged(judgment)
            };
        }
        let (matching, differing): (Vec<_>, Vec<_>) = self
            .related_closing(item)
            .partition(|(_, _, earlier)| earlier.fingerprint.matches(&item.fingerprint));
        let latest = |judgments: Vec<(usize, &'a Judgment, &'a GateItem)>| {
            judgments
                .into_iter()
                .max_by_key(|&(position, judgment, _)| recency(position, judgment))
        };
        if let Some((_, judgment, _)) = latest(matching) {
            ItemState::Carried(judgment)
        } else if let Some((_, judgment, earlier)) = latest(differing) {
            ItemState::Reopened {
                judgment,
                reasons: reopen_reasons(item, &earlier.fingerprint),
            }
        } else {
            ItemState::Open
        }
    }

    /// Every standing acknowledgment or dismissal that closes `item` on the
    /// current review, its own or carried, in the order recorded.
    fn closers(&self, item: &'a GateItem) -> Vec<&'a Judgment> {
        let carried = self
            .related_closing(item)
            .filter(|(_, _, earlier)| earlier.fingerprint.matches(&item.fingerprint))
            .map(|(position, judgment, _)| (position, judgment));
        let mut closers: Vec<_> = self
            .ledger
            .closing(self.id, &item.id)
            .chain(carried)
            .collect();
        closers.sort_by_key(|&(position, _)| position);
        closers.into_iter().map(|(_, judgment)| judgment).collect()
    }

    fn judged(&self, id: &ItemId) -> Option<&JudgedItem<'a>> {
        self.positions
            .get(id)
            .map(|&position| &self.items[position])
    }

    /// Finds standing acknowledgments and dismissals of `item` on related
    /// reviews whose gates hold it, or counted by a standing completion on
    /// one or on the current review, made on another review. Includes each
    /// judgment's log position and the earlier item. Visits only reviews
    /// with such judgments.
    fn related_closing<'s>(
        &'s self,
        item: &'a GateItem,
    ) -> impl Iterator<Item = (usize, &'a Judgment, &'a GateItem)> + 's {
        self.ledger
            .closing_anywhere(&item.id)
            .filter_map(move |(basis, position, judgment)| {
                let earlier = if basis == self.id {
                    (judgment.basis != *self.id).then_some(item)?
                } else {
                    *self.related.get(basis)?.0.get(&item.id)?
                };
                Some((position, judgment, earlier))
            })
    }
}

/// Why a completion of the current review is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompleteRefusal<'a> {
    /// The current review already has this standing completion.
    AlreadyComplete(&'a Judgment),
    /// These gate items are open or reopened, in gate order.
    Unclosed(Vec<&'a GateItem>),
}

/// An item that already has a closing judgment, refusing a new
/// acknowledgment or dismissal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlreadyClosed<'a> {
    /// The item.
    pub item: &'a GateItem,
    /// The judgment that closes it.
    pub judgment: &'a Judgment,
    /// Whether that judgment is carried from another review, the one
    /// `judgment.basis` names.
    pub carried: bool,
}

/// A gate item an undo may not name, since nothing closes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotClosed<'a> {
    /// The item.
    pub item: &'a GateItem,
    /// Its state, open or reopened.
    pub state: ItemState<'a>,
}

/// A current gate item absent from an earlier gate or with a changed fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uncovered<'a> {
    /// The current review's item.
    pub item: &'a GateItem,
    /// How the earlier gate differs on it.
    pub change: ItemChange,
}

/// How an earlier review's gate differs on a current gate item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemChange {
    /// The earlier gate does not hold the item.
    New,
    /// The earlier gate holds it with another fingerprint: the reopen
    /// reason of each part that differs, in part-name order.
    Changed(Vec<&'static str>),
}

/// Finds current `gate` items absent from `completed` or with nonmatching
/// fingerprints, in gate order. An earlier completion covers this gate only
/// if none differ. Ignores items found only in the earlier gate.
#[must_use]
pub fn uncovered_items<'a>(gate: &'a [GateItem], completed: &[GateItem]) -> Vec<Uncovered<'a>> {
    let completed: HashMap<_, _> = completed.iter().map(|item| (&item.id, item)).collect();
    gate.iter()
        .filter_map(|item| {
            let change = match completed.get(&item.id) {
                None => ItemChange::New,
                Some(earlier) => {
                    let reasons = reopen_reasons(item, &earlier.fingerprint);
                    if reasons.is_empty() {
                        return None;
                    }
                    ItemChange::Changed(reasons)
                }
            };
            Some(Uncovered { item, change })
        })
        .collect()
}

/// Finds `current` files whose explanation differs from `earlier`, in path
/// order. Excludes `changed` paths, whose content and explanations changed
/// together. A file absent from `earlier`'s explanations also differs.
#[must_use]
pub fn explanations_differ(
    current: &Basis,
    earlier: &Basis,
    changed: &[PathBytes],
) -> Vec<PathBytes> {
    let explained: HashMap<_, _> = earlier
        .explanations
        .iter()
        .map(|explanation| (&explanation.path, &explanation.digest))
        .collect();
    let changed: HashSet<&PathBytes> = changed.iter().collect();
    current
        .explanations
        .iter()
        .filter(|explanation| {
            !changed.contains(&explanation.path)
                && explained.get(&explanation.path) != Some(&&explanation.digest)
        })
        .map(|explanation| explanation.path.clone())
        .collect()
}

/// The reopen reason of each part of `item`'s fingerprint that differs
/// from `earlier`, in part-name order.
fn reopen_reasons(item: &GateItem, earlier: &Fingerprint) -> Vec<&'static str> {
    item.fingerprint
        .differing(earlier)
        .iter()
        .map(|part| reopen_reason(item.origin(), part))
        .collect()
}

/// Why an item of `origin` reopens when its fingerprint part `part`
/// differs.
fn reopen_reason(origin: Origin, part: &str) -> &'static str {
    match (origin, part) {
        (Origin::UnexplainedLines | Origin::NotDiffed, "content") => "file content or mode changed",
        (Origin::UnexplainedLines, "outcomes") => "line explanations changed",
        (Origin::UnexplainedLines, "evidence") => "evidence behind the lines changed",
        (Origin::NotDiffed, "kind") => "file kind changed",
        (Origin::UntrackedRecorded | Origin::NotCaptured, "on_disk") => "file on disk changed",
        (Origin::UntrackedRecorded, "edits") => "recorded edits changed",
        (Origin::NotCaptured, "cause") => "capture result changed",
        (Origin::CommitWithoutOrigin, "origin") => "origin evidence changed",
        (Origin::Discontinuity, "gaps") => "gaps changed",
        (Origin::Unreconciled, "evidence") => "evidence not read",
        _ => "gate item changed",
    }
}

#[cfg(test)]
mod tests;
