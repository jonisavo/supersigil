//! Judgment rules: how saved reviews relate, which judgments stand, each
//! gate item's state, when a review may be completed, and what an undo
//! withdraws.
//!
//! Pure over what the CLI loads from the review store: the current basis,
//! the bases that have a judgment, and every judgment.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;
use supersigil_record::JudgmentId;
use supersigil_record::reviews::{
    Basis, BasisId, Fingerprint, GateItem, ItemId, Judgment, JudgmentKind, Origin, PathBytes,
    Withdrawal,
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

/// The reviews among `others` related to the current review `id`, whose
/// basis is `basis`, in the order given. The current review is not related
/// to itself.
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
            relation(basis, other).map(|relation| RelatedBasis {
                id: other_id,
                basis: other,
                relation,
            })
        })
        .collect()
}

/// How `other` relates to `current`, or `None` when it does not.
fn relation(current: &Basis, other: &Basis) -> Option<Relation> {
    if current.base.tree != other.base.tree || current.scope != other.scope {
        None
    } else if current.target.tree == other.target.tree {
        Some(Relation::SameContent)
    } else if current.worktree == other.worktree {
        Some(Relation::CodeChanged)
    } else {
        None
    }
}

/// Every judgment in a review store, with what stands of each.
///
/// An acknowledgment or dismissal stands for an item until an undo
/// withdraws that (judgment, item) pair; a comment or completion stands
/// until an undo withdraws it whole.
#[derive(Debug)]
pub struct Ledger<'a> {
    /// Every judgment, in the order recorded.
    judgments: &'a [Judgment],
    /// Every withdrawal of every undo: a pair, or a whole judgment with no
    /// item.
    withdrawn: HashSet<(&'a JudgmentId, Option<&'a ItemId>)>,
    /// Positions in `judgments` of the judgments on each basis.
    by_basis: HashMap<&'a BasisId, Vec<usize>>,
    /// Positions in `judgments` of the standing acknowledgments and
    /// dismissals of each item, by the basis they were recorded on, in the
    /// order recorded, so deciding an item's state never scans a judgment's
    /// list of items nor visits a related review that never closed it.
    closing: HashMap<&'a ItemId, HashMap<&'a BasisId, Vec<usize>>>,
}

impl<'a> Ledger<'a> {
    /// The ledger of `judgments`, every judgment in the store in the order
    /// recorded.
    #[must_use]
    pub fn new(judgments: &'a [Judgment]) -> Self {
        let mut withdrawn = HashSet::new();
        let mut by_basis: HashMap<_, Vec<_>> = HashMap::new();
        for (position, judgment) in judgments.iter().enumerate() {
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
            by_basis,
            closing,
        }
    }

    /// Whether any of `judgment` stands: for an acknowledgment or
    /// dismissal, its judgment of any item; for a comment or completion,
    /// the whole. An undo always stands, since nothing withdraws one.
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

    /// Whether `judgment` acknowledges or dismisses `item` and no undo
    /// withdrew that pair or the whole judgment.
    #[must_use]
    pub fn stands_for(&self, judgment: &Judgment, item: &ItemId) -> bool {
        matches!(
            judgment.kind,
            JudgmentKind::Acknowledge | JudgmentKind::Dismiss
        ) && judgment.items.contains(item)
            && self.not_withdrawn(judgment, item)
    }

    /// Whether no undo withdrew the pair of `judgment` and `item`, one of
    /// its items, nor the whole judgment: [`Self::stands_for`] for an
    /// acknowledgment or dismissal and an item it names, without searching
    /// its items again.
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

    /// Resolves `prefix`, a full judgment ID or the first 8 or more
    /// characters of one in either case, among every judgment.
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

    /// What `undo --judgment` withdraws of `judgment`: a standing comment
    /// or completion whole; for an acknowledgment or dismissal, each item
    /// it still stands for, then every standing completion that counts one
    /// of those.
    ///
    /// # Errors
    ///
    /// Returns [`WithdrawRefusal::Undo`] for an undo and
    /// [`WithdrawRefusal::Withdrawn`] for a judgment of which nothing
    /// stands.
    pub fn plan_withdrawal(
        &self,
        judgment: &'a Judgment,
    ) -> Result<Vec<PlannedWithdrawal<'a>>, WithdrawRefusal> {
        match judgment.kind {
            JudgmentKind::Undo => Err(WithdrawRefusal::Undo),
            _ if !self.stands(judgment) => Err(WithdrawRefusal::Withdrawn),
            JudgmentKind::Comment | JudgmentKind::Complete => Ok(vec![PlannedWithdrawal {
                withdrawal: Withdrawal {
                    judgment: judgment.id.clone(),
                    item: None,
                },
                judgment,
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

    /// The withdrawals of `pairs`, standing (acknowledgment or dismissal,
    /// item) pairs, each once, then of every standing completion that
    /// counts one of them, in the order recorded.
    fn withdraw_pairs<'i>(
        &self,
        pairs: impl IntoIterator<Item = (&'a Judgment, &'i ItemId)>,
    ) -> Vec<PlannedWithdrawal<'a>> {
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
            .map(|(judgment, item)| PlannedWithdrawal {
                withdrawal: Withdrawal {
                    judgment: judgment.id.clone(),
                    item,
                },
                judgment,
            })
            .collect()
    }

    /// The judgments recorded on `basis`, with their positions in the log.
    fn on(&self, basis: &BasisId) -> impl Iterator<Item = (usize, &'a Judgment)> {
        let judgments = self.judgments;
        self.by_basis
            .get(basis)
            .into_iter()
            .flatten()
            .map(move |&position| (position, &judgments[position]))
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
    /// Nothing of the judgment stands any more.
    Withdrawn,
}

/// A withdrawal an undo would record, with the judgment it withdraws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedWithdrawal<'a> {
    /// What the undo records.
    pub withdrawal: Withdrawal,
    /// The withdrawn judgment, which gives its kind, the review it was
    /// made on (`basis`), and its date.
    pub judgment: &'a Judgment,
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
    related: HashMap<&'a BasisId, Cow<'a, GateIndex<'a>>>,
    /// The current review's gate items with their states, in gate order.
    items: Vec<JudgedItem<'a>>,
    /// Each item's position in `items`, by ID.
    positions: HashMap<&'a ItemId, usize>,
}

/// A saved review's gate items by ID, built once for the [`GateStates`]
/// of every review it is related to.
#[derive(Debug, Clone)]
pub struct GateIndex<'a>(HashMap<&'a ItemId, &'a GateItem>);

impl<'a> GateIndex<'a> {
    /// Indexes `gate`, a saved review's gate items.
    #[must_use]
    pub fn new(gate: &'a [GateItem]) -> Self {
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
    /// No judgment on the current review names it, but a standing
    /// acknowledgment or dismissal on a related review whose gate holds it
    /// with a matching fingerprint closes it, carried from that review.
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
    /// No standing judgment names it.
    Open,
}

impl<'a> ItemState<'a> {
    /// The judgment that closes the item: its own acknowledgment or
    /// dismissal, or the one carried. `None` when it is open or reopened.
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
    /// The state of each gate item of the review `id`, whose basis is
    /// `basis`, from `related`, the reviews related to it, and the
    /// judgments in `ledger`.
    #[must_use]
    pub fn new(
        id: &'a BasisId,
        basis: &'a Basis,
        related: &[RelatedBasis<'a>],
        ledger: &'a Ledger<'a>,
    ) -> Self {
        Self::indexed(id, basis, related, None, ledger)
    }

    /// As [`Self::new`], taking each related review's gate index from
    /// `indexes`, by basis ID, when it holds one, so a caller deciding the
    /// states of many reviews indexes each gate once.
    #[must_use]
    pub fn with_indexes(
        id: &'a BasisId,
        basis: &'a Basis,
        related: &[RelatedBasis<'a>],
        indexes: &'a HashMap<&'a BasisId, GateIndex<'a>>,
        ledger: &'a Ledger<'a>,
    ) -> Self {
        Self::indexed(id, basis, related, Some(indexes), ledger)
    }

    /// The states [`Self::with_indexes`] decides, indexing here the gate of
    /// each related review that `indexes` lacks.
    fn indexed(
        id: &'a BasisId,
        basis: &'a Basis,
        related: &[RelatedBasis<'a>],
        indexes: Option<&'a HashMap<&'a BasisId, GateIndex<'a>>>,
        ledger: &'a Ledger<'a>,
    ) -> Self {
        let related = related
            .iter()
            .map(|related| {
                let index = indexes.and_then(|indexes| indexes.get(related.id));
                let gate = index.map_or_else(
                    || Cow::Owned(GateIndex::new(&related.basis.gate)),
                    Cow::Borrowed,
                );
                (related.id, gate)
            })
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

    /// The `closes` map of a new completion of the current review: each
    /// gate item to the judgment that closes it.
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

    /// Checks that a new acknowledgment or dismissal may name each of
    /// `items`, IDs of the current review's gate items (others are
    /// ignored).
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

    /// What `undo <items>` on the current review withdraws: for each of
    /// `items`, IDs of its gate items (others are ignored), every standing
    /// acknowledgment or dismissal that closes it here (its own, and every
    /// carried one, not only the most recent), then every standing
    /// completion, on any review, that counts one of those.
    ///
    /// # Errors
    ///
    /// Returns every named item that nothing closes, with its state, in
    /// the order named.
    pub fn plan_undo(
        &self,
        items: &[ItemId],
    ) -> Result<Vec<PlannedWithdrawal<'a>>, Vec<NotClosed<'a>>> {
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

    /// The state of `item`: closed by the most recent standing judgment of
    /// its own, else carried by the most recent one with a matching
    /// fingerprint, else reopened by the most recent other one.
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

    /// The current review's item `id` with its state.
    fn judged(&self, id: &ItemId) -> Option<&JudgedItem<'a>> {
        self.positions
            .get(id)
            .map(|&position| &self.items[position])
    }

    /// The standing acknowledgments and dismissals of `item` on related
    /// reviews whose gate holds it, each with its position in the log and
    /// that review's item. Visits only the reviews with such a judgment.
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

/// A gate item a new acknowledgment or dismissal may not name, since a
/// judgment already closes it.
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

/// A gate item of the current review that an earlier review's gate did not
/// hold as it is now.
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

/// The items of `gate`, the current review's, that `completed`, an earlier
/// review's gate, does not hold with a matching fingerprint, in gate
/// order. A completion of that review covers the current gate only when
/// there are none; items only `completed` holds do not matter.
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

/// The reviewed files of `current` whose explanation differs from what
/// `earlier` explained, in path order, leaving out `changed`: the paths
/// whose content differs between the two reviews, whose explanations
/// differ with the code. A file `earlier` did not explain differs.
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
