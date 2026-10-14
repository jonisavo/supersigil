//! Rules for related reviews, standing judgments, gate states, completion,
//! and undo withdrawals.
//!
//! Uses only the current basis, judged bases, and judgments the CLI loads
//! from the review store.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use serde::Serialize;
use supersigil_record::JudgmentId;
use supersigil_record::reviews::{
    Basis, BasisHeader, BasisId, BasisScope, Fingerprint, GateItem, ItemId, Judgment, JudgmentKind,
    Origin, PathBytes, Withdrawal,
};
use supersigil_record::time::UtcTime;

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

/// A saved review related to the current one.
#[derive(Debug, Clone, Copy)]
pub struct RelatedBasis<'a> {
    /// The related review's basis ID.
    pub id: &'a BasisId,
    /// Its basis.
    pub basis: &'a Basis,
    /// How it relates to the current review.
    pub relation: Relation,
}

/// Finds reviews in `others` related to review `id` with `basis`, preserving
/// input order and excluding the current review itself.
#[must_use]
pub fn related_bases<'a>(
    id: &BasisId,
    basis: &Basis,
    others: impl IntoIterator<Item = (&'a BasisId, &'a Basis)>,
) -> Vec<RelatedBasis<'a>> {
    others
        .into_iter()
        .filter(|(other_id, _)| *other_id != id)
        .filter_map(|(other_id, other)| {
            relation(basis.into(), other.into()).map(|relation| RelatedBasis {
                id: other_id,
                basis: other,
                relation,
            })
        })
        .collect()
}

/// Checks whether `other` relates to `current` by the [`related_bases`]
/// rules. If not, the caller can skip reading the rest of `other`'s basis.
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
/// withdraws the whole judgment.
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
        Self {
            judgments,
            withdrawn,
            by_id,
            by_basis,
            closing,
        }
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
        bases
            .into_iter()
            .flat_map(|basis| self.on(basis))
            .filter(|(_, judgment)| {
                judgment.kind == JudgmentKind::Complete && self.stands(judgment)
            })
            .max_by_key(|&(position, judgment)| recency(position, judgment))
            .map(|(_, judgment)| judgment)
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
    /// with their positions in the log.
    fn closing_anywhere<'s>(
        &'s self,
        item: &'s ItemId,
    ) -> impl Iterator<Item = (usize, &'a Judgment)> + 's {
        let judgments = self.judgments;
        self.closing
            .get(item)
            .into_iter()
            .flat_map(HashMap::values)
            .flatten()
            .map(move |&position| (position, &judgments[position]))
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

/// Every gate item of the current review with its state.
#[derive(Debug)]
pub struct GateStates<'a> {
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
pub struct GateIndex<'a>(HashMap<&'a ItemId, &'a GateItem>);

impl<'a> GateIndex<'a> {
    /// Indexes `gate`, a saved review's gate items.
    #[must_use]
    pub fn new(gate: &'a [GateItem]) -> Self {
        Self(gate.iter().map(|item| (&item.id, item)).collect())
    }

    /// Indexes each gate in `related` by basis ID for [`GateStates::new`].
    #[must_use]
    pub fn of(related: &[RelatedBasis<'a>]) -> HashMap<&'a BasisId, Self> {
        related
            .iter()
            .map(|related| (related.id, Self::new(&related.basis.gate)))
            .collect()
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
    /// gates ([`GateIndex::of`]) and may contain other reviews. Callers can
    /// index each gate once for reuse. Related reviews missing from `indexes`
    /// cannot close items here.
    #[must_use]
    pub fn new(
        id: &'a BasisId,
        basis: &'a Basis,
        related: &[RelatedBasis<'a>],
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
    #[must_use]
    pub fn items(&self) -> &[JudgedItem<'a>] {
        &self.items
    }

    /// The current review's own standing completion, if any.
    #[must_use]
    pub fn own_completion(&self) -> Option<&'a Judgment> {
        self.ledger.completion(self.id)
    }

    /// Every judgment the states come from.
    #[must_use]
    pub const fn ledger(&self) -> &'a Ledger<'a> {
        self.ledger
    }

    /// Builds the `closes` map for a new completion of the current review,
    /// mapping each gate item to its closing judgment.
    ///
    /// # Errors
    ///
    /// Returns [`CompleteRefusal::AlreadyComplete`] with the current
    /// review's own standing completion, or [`CompleteRefusal::Unclosed`]
    /// with the items that are open or reopened.
    pub fn check_complete(&self) -> Result<BTreeMap<ItemId, JudgmentId>, CompleteRefusal<'a>> {
        if let Some(completion) = self.own_completion() {
            return Err(CompleteRefusal::AlreadyComplete(completion));
        }
        let unclosed: Vec<_> = self
            .items
            .iter()
            .filter(|judged| judged.state.closing().is_none())
            .map(|judged| judged.item)
            .collect();
        if !unclosed.is_empty() {
            return Err(CompleteRefusal::Unclosed(unclosed));
        }
        Ok(self
            .items
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
    fn state(&self, item: &GateItem) -> ItemState<'a> {
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
            .related_closing(&item.id)
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
    fn closers(&self, item: &GateItem) -> Vec<&'a Judgment> {
        let carried = self
            .related_closing(&item.id)
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
    /// reviews whose gates hold it. Includes each judgment's log position and
    /// the earlier item. Visits only reviews with such judgments.
    fn related_closing<'s>(
        &'s self,
        item: &'s ItemId,
    ) -> impl Iterator<Item = (usize, &'a Judgment, &'a GateItem)> + 's {
        self.ledger
            .closing_anywhere(item)
            .filter_map(move |(position, judgment)| {
                let earlier = self.related.get(&judgment.basis)?.0.get(item)?;
                Some((position, judgment, *earlier))
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
