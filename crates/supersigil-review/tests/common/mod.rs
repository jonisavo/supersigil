//! Builders for review tests. Anchor's results are constructed directly, so
//! these tests pin the review rules without running the walk.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::collections::BTreeSet;
use std::sync::mpsc;
use std::time::Duration;

use supersigil_anchor::result::ContentMatches;
use supersigil_anchor::{
    BaseLineOutcome, Chain, ChainClass, ChainEnd, Fate, LineOutcome, Origin, PathAttribution,
    PathStatus, Provenance, StopReason,
};
use supersigil_record::interpretations::AnnotationRevision;
use supersigil_record::reviews::DecisionRef;
use supersigil_record::{EventId, RecordId, SessionId};

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

/// Runs `work` on another thread and returns its result, failing the test
/// when it takes longer than `limit` times [`time_limit_scale`]. A hang
/// becomes a failure instead of a stuck test run; the thread is left to the
/// process's exit.
pub fn finishes_within<T: Send + 'static>(
    limit: Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (done, result) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(work());
    });
    result
        .recv_timeout(limit * time_limit_scale())
        .expect("the work finishes within its time limit")
}

/// The whole number in `SUPERSIGIL_TIME_LIMIT_SCALE`, or 1 when it is unset,
/// so a slow CI runner can allow more time while others keep the limits.
///
/// # Panics
///
/// Panics if the variable is set to anything but a positive whole number.
fn time_limit_scale() -> u32 {
    std::env::var("SUPERSIGIL_TIME_LIMIT_SCALE").map_or(1, |scale| {
        scale
            .parse()
            .ok()
            .filter(|&scale| scale > 0)
            .expect("SUPERSIGIL_TIME_LIMIT_SCALE is a positive whole number")
    })
}

/// Owned copies of `texts`.
pub fn strings(texts: &[&str]) -> Vec<String> {
    texts.iter().map(|text| (*text).to_owned()).collect()
}

/// An event id for each of `texts`.
pub fn ids(texts: &[&str]) -> Vec<EventId> {
    texts.iter().map(|text| eid(text)).collect()
}

/// Annotation revision `number`.
pub fn revision(number: u32) -> AnnotationRevision {
    AnnotationRevision::new(number)
}

/// A reference to decision `id` of `session`'s `revision` in `record`.
pub fn reference(record: &str, session: &str, revision: u32, id: &str) -> DecisionRef {
    DecisionRef {
        record: RecordId::new(record),
        session: SessionId::new(session),
        revision: AnnotationRevision::new(revision),
        id: id.to_owned(),
    }
}
