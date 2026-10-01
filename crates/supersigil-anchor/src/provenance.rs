//! Replays one chain forward to give every target line its origins and
//! every base line its fates.
//!
//! Inside the replay, provenance travels with characters: each line carries
//! runs over its non-whitespace characters, each run with an origin,
//! whitespace-only contributors, and earlier contributors; a line without
//! any non-whitespace character carries its own. Lines of the chain's start
//! are base lines when the chain ends at the base, and unexplained
//! otherwise. Kept lines keep their runs. In a changed region whose removed
//! and added text are equal once all whitespace is deleted, characters map
//! one to one through the whitespace-free text, so each keeps its run's
//! attributes and gains the edit as a whitespace-only contributor; base-line
//! identity passes through. In any other region, every added character is
//! introduced by the edit, with earlier contributors from the removed
//! region's characters (blank lines included), and each removed base line is
//! replaced by the edit. Runs aggregate into per-line sets only in the
//! output.
//!
//! Contributor sets are shared, never copied per run: kept lines move to the
//! next state, a whitespace-only step builds one grown set for each set it
//! touches, and a replaced region builds one set for all its added lines.
//! The walk charged every step of every chain it returns, by exploring the
//! step or by copying it into a later chain, so the replay's pass over the
//! states and alignments is not charged again. What can outgrow them is
//! charged to the replay's budget: every contributor copied into a new set
//! or into the output, and the output's entry for each target line and each
//! base line.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use supersigil_record::EventId;

use crate::lines::split_lines;
use crate::result::{ChainEnd, Fate, Origin, Provenance};
use crate::step::Budget;
use crate::walk::{ChainStep, WalkedChain};

/// One chain's provenance for every target line and fates for every base
/// line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainProvenance {
    /// One provenance per target line.
    pub target: Vec<Provenance>,
    /// One set of fates per base line; all empty unless the chain ends at
    /// the base.
    pub base: Vec<BTreeSet<Fate>>,
}

/// Replays `chain` forward from its start. `base_lines` is the number of
/// lines of the base state.
///
/// Every contributor copied into a new set or into the output, and the
/// output's entry for each line, is charged to `budget` before it is made.
/// Returns `None` when the budget runs out.
///
/// # Panics
///
/// Panics when `chain` does not link: a step's alignment names a line
/// beyond that step's before- or after-state, or a step's before-state is
/// not the previous step's after-state (for the first step, `chain.start`),
/// so a whitespace-only region's characters cannot be carried one to one.
/// Every chain [`crate::walk::walk`] returns links, with alignments from
/// [`crate::align::align`].
#[must_use]
pub fn replay(
    chain: &WalkedChain,
    base_lines: usize,
    budget: &mut Budget,
) -> Option<ChainProvenance> {
    let mut replay = Replay {
        edits: chain.steps.iter().map(|step| &step.edit).collect(),
        empty: Steps::default(),
        marks: BTreeSet::new(),
        budget,
    };
    let from_base = chain.chain.end == ChainEnd::Base;
    let mut lines: Vec<Line> = split_lines(chain.start.bytes().unwrap_or_default())
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let origin = if from_base {
                Source::Base(i)
            } else {
                Source::Unexplained
            };
            Line::uniform(non_whitespace(text), replay.attrs(origin))
        })
        .collect();
    for (index, step) in chain.steps.iter().enumerate() {
        lines = replay.apply(index, step, lines)?;
    }
    replay.output(&lines, base_lines)
}

/// A set of the chain's steps, by index, shared by every run that carries
/// it.
type Steps = Rc<BTreeSet<usize>>;

/// The grown set of each set a whitespace-only step touched, by address.
/// The touched set is held too, so its address is not reused in the step.
type Respaced = HashMap<*const BTreeSet<usize>, (Steps, Steps)>;

/// What one step in a set of steps costs.
const STEP_COST: usize = size_of::<usize>();

/// Where a run's characters, or a line without any, came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    /// This line of the chain's start, which is the base.
    Base(usize),
    /// Written by the step with this index.
    Introduced(usize),
    /// A line of only whitespace the step with this index added.
    WhitespaceAdded(usize),
    /// Inherited from before the chain's start.
    Unexplained,
}

/// What a step decided about a base line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Mark {
    /// Removed by the step with this index outside a whitespace-only change.
    Replaced(usize),
    /// A line of only whitespace removed by this whitespace-only step.
    RemovedByWhitespace(usize),
}

/// What a run of characters, or a line without any, carries.
#[derive(Debug, Clone)]
struct Attrs {
    origin: Source,
    whitespace_only: Steps,
    earlier: Steps,
}

impl Attrs {
    /// Whether both carry the same origin and the same shared sets. Equal
    /// sets held apart compare unequal, which only keeps two runs apart.
    fn same(&self, other: &Self) -> bool {
        self.origin == other.origin
            && Rc::ptr_eq(&self.whitespace_only, &other.whitespace_only)
            && Rc::ptr_eq(&self.earlier, &other.earlier)
    }
}

/// A line's provenance during the replay.
#[derive(Debug)]
enum Line {
    /// Runs of (number of non-whitespace characters, their attributes).
    Runs(Vec<(usize, Attrs)>),
    /// A line without non-whitespace characters, and what it carries.
    Bare(Attrs),
}

impl Line {
    /// A line of `count` non-whitespace characters, all with `attrs`.
    fn uniform(count: usize, attrs: Attrs) -> Self {
        if count == 0 {
            Self::Bare(attrs)
        } else {
            Self::Runs(vec![(count, attrs)])
        }
    }

    /// Every attribute set the line carries: its runs', or its own.
    fn attrs(&self) -> Vec<&Attrs> {
        match self {
            Self::Runs(runs) => runs.iter().map(|(_, attrs)| attrs).collect(),
            Self::Bare(attrs) => vec![attrs],
        }
    }
}

/// The replay of one chain.
struct Replay<'a> {
    /// The edit of each step.
    edits: Vec<&'a EventId>,
    /// The empty set, shared.
    empty: Steps,
    /// What the steps decided about base lines, as (base line, mark).
    marks: BTreeSet<(usize, Mark)>,
    budget: &'a mut Budget,
}

impl Replay<'_> {
    fn charge(&mut self, bytes: usize) -> Option<()> {
        self.budget.charge(bytes).then_some(())
    }

    /// Attributes with `origin` and no contributors.
    fn attrs(&self, origin: Source) -> Attrs {
        Attrs {
            origin,
            whitespace_only: Rc::clone(&self.empty),
            earlier: Rc::clone(&self.empty),
        }
    }

    /// Records `mark` on base line `line`, charging it when it is new.
    fn mark(&mut self, line: usize, mark: Mark) -> Option<()> {
        if !self.marks.contains(&(line, mark)) {
            self.charge(size_of::<(usize, Mark)>())?;
            self.marks.insert((line, mark));
        }
        Some(())
    }

    /// Applies step `index`: moves the before-state's lines into the
    /// after-state's and records the base fates the step decides.
    fn apply(&mut self, index: usize, step: &ChainStep, lines: Vec<Line>) -> Option<Vec<Line>> {
        let before = split_lines(step.before.bytes().unwrap_or_default());
        let after = split_lines(step.after.bytes().unwrap_or_default());
        let mut old: Vec<Option<Line>> = lines.into_iter().map(Some).collect();
        let mut next: Vec<Option<Line>> =
            std::iter::repeat_with(|| None).take(after.len()).collect();
        for &(b, a) in &step.alignment.kept {
            if let (Some(slot), Some(line)) =
                (next.get_mut(a), old.get_mut(b).and_then(Option::take))
            {
                *slot = Some(line);
            }
        }
        let mut respaced = Respaced::new();
        for region in &step.alignment.regions {
            let removed: Vec<Line> = region
                .before
                .clone()
                .filter_map(|r| old.get_mut(r).and_then(Option::take))
                .collect();
            let removed_text: Vec<&[u8]> = region.before.clone().map(|r| before[r]).collect();
            let added_text: Vec<&[u8]> = region.after.clone().map(|a| after[a]).collect();
            let added = if squeeze(&removed_text) == squeeze(&added_text) {
                self.whitespace_only(index, removed, &added_text, &mut respaced)?
            } else {
                self.replaced(index, &removed, &added_text)?
            };
            for (a, line) in region.after.clone().zip(added) {
                next[a] = Some(line);
            }
        }
        Some(
            next.into_iter()
                .map(|line| line.unwrap_or_else(|| Line::Bare(self.attrs(Source::Unexplained))))
                .collect(),
        )
    }

    /// A whitespace-only region: characters keep their attributes, one to
    /// one, and gain step `index` as a whitespace-only contributor.
    fn whitespace_only(
        &mut self,
        index: usize,
        removed: Vec<Line>,
        added_text: &[&[u8]],
        respaced: &mut Respaced,
    ) -> Option<Vec<Line>> {
        // The removed characters' runs, in order.
        let mut source: Vec<(usize, Attrs)> = Vec::new();
        for line in removed {
            match line {
                Line::Runs(runs) => source.extend(runs),
                Line::Bare(attrs) => {
                    if let Source::Base(j) = attrs.origin {
                        self.mark(j, Mark::RemovedByWhitespace(index))?;
                    }
                }
            }
        }
        let mut source = source.into_iter();
        let mut pending: Option<(usize, Attrs)> = None;
        let mut out = Vec::with_capacity(added_text.len());
        for text in added_text {
            let mut needed = non_whitespace(text);
            if needed == 0 {
                let empty = Rc::clone(&self.empty);
                out.push(Line::Bare(Attrs {
                    origin: Source::WhitespaceAdded(index),
                    whitespace_only: self.respace(&empty, index, respaced)?,
                    earlier: empty,
                }));
                continue;
            }
            let mut runs: Vec<(usize, Attrs)> = Vec::new();
            while needed > 0 {
                let (count, mut attrs) = pending
                    .take()
                    .or_else(|| source.next())
                    .expect("the whitespace-free texts are equal, so the characters match");
                let taken = count.min(needed);
                if count > taken {
                    pending = Some((count - taken, attrs.clone()));
                }
                attrs.whitespace_only = self.respace(&attrs.whitespace_only, index, respaced)?;
                match runs.last_mut() {
                    Some((n, last)) if last.same(&attrs) => *n += taken,
                    _ => runs.push((taken, attrs)),
                }
                needed -= taken;
            }
            out.push(Line::Runs(runs));
        }
        Some(out)
    }

    /// `set` with step `index` added: built, and charged, once per step for
    /// each distinct set.
    fn respace(&mut self, set: &Steps, index: usize, respaced: &mut Respaced) -> Option<Steps> {
        if let Some((_, grown)) = respaced.get(&Rc::as_ptr(set)) {
            return Some(Rc::clone(grown));
        }
        self.charge(set.len().saturating_add(1).saturating_mul(STEP_COST))?;
        let mut grown = BTreeSet::clone(set);
        grown.insert(index);
        let grown = Rc::new(grown);
        respaced.insert(Rc::as_ptr(set), (Rc::clone(set), Rc::clone(&grown)));
        Some(grown)
    }

    /// Any other region: every added character is introduced by step
    /// `index`, and every edit that introduced, changed whitespace on, or
    /// was earlier on a removed line, blank ones included, is an earlier
    /// contributor of each added line.
    fn replaced(
        &mut self,
        index: usize,
        removed: &[Line],
        added_text: &[&[u8]],
    ) -> Option<Vec<Line>> {
        let mut earlier = Merged::default();
        for line in removed {
            for attrs in line.attrs() {
                match attrs.origin {
                    Source::Introduced(by) | Source::WhitespaceAdded(by) => {
                        self.charge(STEP_COST)?;
                        earlier.steps.insert(by);
                    }
                    Source::Base(j) => self.mark(j, Mark::Replaced(index))?,
                    Source::Unexplained => {}
                }
                earlier.add(&attrs.whitespace_only, self.budget)?;
                earlier.add(&attrs.earlier, self.budget)?;
            }
        }
        let earlier = earlier.steps;
        let earlier = if earlier.is_empty() {
            Rc::clone(&self.empty)
        } else {
            Rc::new(earlier)
        };
        Some(
            added_text
                .iter()
                .map(|text| {
                    Line::uniform(
                        non_whitespace(text),
                        Attrs {
                            origin: Source::Introduced(index),
                            whitespace_only: Rc::clone(&self.empty),
                            earlier: Rc::clone(&earlier),
                        },
                    )
                })
                .collect(),
        )
    }

    /// The edit of step `index`, copied out and charged.
    fn id(&mut self, index: usize) -> Option<EventId> {
        let id = self.edits[index];
        self.charge(size_of::<EventId>() + id.as_str().len())?;
        Some(id.clone())
    }

    /// The edits of `steps`, copied out and charged.
    fn ids(&mut self, steps: &BTreeSet<usize>) -> Option<BTreeSet<EventId>> {
        steps.iter().map(|&index| self.id(index)).collect()
    }

    fn origin(&mut self, source: Source) -> Option<Origin> {
        Some(match source {
            Source::Base(j) => {
                self.charge(size_of::<Origin>())?;
                Origin::Base(j)
            }
            Source::Introduced(index) => Origin::Introduced(self.id(index)?),
            Source::WhitespaceAdded(index) => Origin::WhitespaceAdded(self.id(index)?),
            Source::Unexplained => {
                self.charge(size_of::<Origin>())?;
                Origin::Unexplained
            }
        })
    }

    /// Aggregates the final lines into one provenance per target line and
    /// the fates of each base line, charging the output's entry for each
    /// line first.
    fn output(mut self, lines: &[Line], base_lines: usize) -> Option<ChainProvenance> {
        self.charge(lines.len().saturating_mul(size_of::<Provenance>()))?;
        self.charge(base_lines.saturating_mul(size_of::<BTreeSet<Fate>>()))?;
        let mut base: Vec<BTreeSet<Fate>> = vec![BTreeSet::new(); base_lines];
        for (j, mark) in std::mem::take(&mut self.marks) {
            let fate = match mark {
                Mark::Replaced(index) => Fate::Replaced {
                    edit: self.id(index)?,
                },
                Mark::RemovedByWhitespace(index) => Fate::RemovedByWhitespace {
                    edit: self.id(index)?,
                },
            };
            if let Some(fates) = base.get_mut(j) {
                fates.insert(fate);
            }
        }
        let mut target = Vec::with_capacity(lines.len());
        for (line, held) in lines.iter().enumerate() {
            let mut sources = BTreeSet::new();
            let mut whitespace_only = Merged::default();
            let mut earlier = Merged::default();
            for attrs in held.attrs() {
                sources.insert(attrs.origin);
                whitespace_only.add(&attrs.whitespace_only, self.budget)?;
                earlier.add(&attrs.earlier, self.budget)?;
                if let Source::Base(j) = attrs.origin {
                    let fate = if attrs.whitespace_only.is_empty() {
                        Fate::KeptAs { line }
                    } else {
                        Fate::CarriedTo {
                            line,
                            via: self.ids(&attrs.whitespace_only)?,
                        }
                    };
                    if let Some(fates) = base.get_mut(j) {
                        fates.insert(fate);
                    }
                }
            }
            let mut origins = BTreeSet::new();
            for source in sources {
                origins.insert(self.origin(source)?);
            }
            target.push(Provenance {
                origins,
                whitespace_only: self.ids(&whitespace_only.steps)?,
                earlier: self.ids(&earlier.steps)?,
            });
        }
        Some(ChainProvenance { target, base })
    }
}

/// A union of shared step sets that merges each distinct set once, however
/// many runs hold it, and charges what it merges.
#[derive(Default)]
struct Merged {
    steps: BTreeSet<usize>,
    /// The sets merged, by address; they are held for as long as the union
    /// is built, so no address is reused meanwhile.
    seen: HashSet<*const BTreeSet<usize>>,
}

impl Merged {
    fn add(&mut self, set: &Steps, budget: &mut Budget) -> Option<()> {
        if self.seen.insert(Rc::as_ptr(set)) {
            budget
                .charge(set.len().saturating_mul(STEP_COST))
                .then_some(())?;
            self.steps.extend(set.iter().copied());
        }
        Some(())
    }
}

/// The characters of `text` with every whitespace character deleted.
fn squeeze(text: &[&[u8]]) -> String {
    text.iter()
        .flat_map(|line| {
            String::from_utf8_lossy(line)
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn non_whitespace(line: &[u8]) -> usize {
    String::from_utf8_lossy(line)
        .chars()
        .filter(|c| !c.is_whitespace())
        .count()
}
