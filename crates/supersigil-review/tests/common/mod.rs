//! Builders for review tests. Anchor's results are constructed directly, so
//! these tests pin the review rules without running the walk.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::collections::BTreeSet;
use std::fmt::Write as _;

use supersigil_anchor::result::ContentMatches;
use supersigil_anchor::{
    BaseLineOutcome, Chain, ChainClass, ChainEnd, Fate, LineOutcome, Origin, PathAttribution,
    PathStatus, Provenance, StopReason,
};
use supersigil_record::EventId;

/// An edit id.
pub fn eid(id: &str) -> EventId {
    EventId::new(id)
}

/// A chain over `edits` (oldest first) whose head is the last edit.
pub fn chain(id: usize, class: ChainClass, edits: &[&str], end: ChainEnd) -> Chain {
    Chain {
        id,
        class,
        head: eid(edits.last().copied().unwrap_or_default()),
        edits: edits.iter().map(|edit| eid(edit)).collect(),
        end,
    }
}

/// A target line every chain agrees on.
pub fn agreed(origins: impl IntoIterator<Item = Origin>, class: Option<ChainClass>) -> LineOutcome {
    LineOutcome::Agreed {
        provenance: Provenance {
            origins: origins.into_iter().collect(),
            ..Provenance::default()
        },
        class,
    }
}

/// A target line introduced by `edit` in a chain exact from the base.
pub fn introduced(edit: &str) -> LineOutcome {
    agreed(
        [Origin::Introduced(eid(edit))],
        Some(ChainClass::ExactFromBase),
    )
}

/// A target line that is base line `j` in a chain exact from the base.
pub fn base_line(j: usize) -> LineOutcome {
    agreed([Origin::Base(j)], Some(ChainClass::ExactFromBase))
}

/// A target line no chain explains.
pub fn unexplained() -> LineOutcome {
    agreed([Origin::Unexplained], None)
}

/// A base line with `fates`, agreed by a chain exact from the base.
pub fn fates(fates: impl IntoIterator<Item = Fate>) -> BaseLineOutcome {
    BaseLineOutcome::Agreed {
        fates: fates.into_iter().collect(),
        class: Some(ChainClass::ExactFromBase),
    }
}

/// A base line replaced by `edit`.
pub fn replaced(edit: &str) -> BaseLineOutcome {
    fates([Fate::Replaced { edit: eid(edit) }])
}

/// A base line kept as target line `line`.
pub fn kept(line: usize) -> BaseLineOutcome {
    fates([Fate::KeptAs { line }])
}

/// A base line with no fate.
pub fn no_fate() -> BaseLineOutcome {
    BaseLineOutcome::Agreed {
        fates: BTreeSet::new(),
        class: None,
    }
}

/// An attribution with `chains` and the given per-line outcomes, and no
/// content matches. Without chains the status is "not composed" with
/// `NoPredecessor`.
pub fn attribution(
    chains: Vec<Chain>,
    target: Vec<LineOutcome>,
    base: Vec<BaseLineOutcome>,
) -> PathAttribution {
    let status = if chains.is_empty() {
        PathStatus::NotComposed {
            reasons: vec![StopReason::NoPredecessor],
        }
    } else {
        PathStatus::Composed
    };
    PathAttribution {
        status,
        chains,
        set_aside: Vec::new(),
        conflicts: Vec::new(),
        accepted: Vec::new(),
        content: ContentMatches {
            target: vec![BTreeSet::new(); target.len()],
            base: vec![BTreeSet::new(); base.len()],
            incomplete: false,
        },
        target,
        base,
    }
}

/// The same escaping as the CLI's `format::escape_control`: control
/// characters other than tab become `\xNN`.
pub fn escape(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let code = u32::from(c);
        if (code <= 0x1f && c != '\t') || (0x7f..=0x9f).contains(&code) {
            let _ = write!(out, "\\x{code:02x}");
        } else {
            out.push(c);
        }
    }
    out
}
