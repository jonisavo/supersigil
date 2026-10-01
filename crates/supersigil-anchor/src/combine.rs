//! Combines the chains of one path line by line.
//!
//! Chains are alternative histories of the same bytes, not complementary
//! pieces. A target line is agreed only when every chain gives it an
//! identical provenance: the same origins, the same whitespace-only
//! contributors, and the same earlier contributors. "Unexplained" is an
//! origin like any other, so a line one chain explains and another does not
//! is ambiguous, and so is a line two chains explain through different
//! edits. A base line is agreed only when every chain gives it the same
//! fates. Nothing is unioned across chains: competing contributors are
//! never presented as a joint one.

use std::collections::BTreeSet;

use crate::provenance::ChainProvenance;
use crate::result::{BaseLineOutcome, Chain, LineOutcome, Origin, Provenance, Reading};

/// Combines the provenance of `chains` into one outcome per line.
///
/// Agreed lines carry the provenance every chain gives them and the
/// strongest class among the chains; otherwise every chain's full
/// provenance is listed. Without chains, every target line is agreed
/// unexplained and every base line has no fate, with no class. Each chain's
/// provenance moves into the outcomes, so combining copies none of it.
#[must_use]
pub fn combine(
    chains: Vec<(&Chain, ChainProvenance)>,
    target_lines: usize,
    base_lines: usize,
) -> (Vec<LineOutcome>, Vec<BaseLineOutcome>) {
    let strongest = chains.iter().map(|(chain, _)| chain.class).min();
    let (mut targets, mut bases): (Vec<_>, Vec<_>) = chains
        .into_iter()
        .map(|(chain, p)| ((chain, p.target.into_iter()), (chain, p.base.into_iter())))
        .unzip();
    let target = (0..target_lines)
        .map(|_| match agreement(&mut targets) {
            Agreement::NoChain => LineOutcome::Agreed {
                provenance: Provenance {
                    origins: BTreeSet::from([Origin::Unexplained]),
                    ..Provenance::default()
                },
                class: None,
            },
            Agreement::Agreed(provenance) => LineOutcome::Agreed {
                provenance,
                class: strongest,
            },
            Agreement::Disputed(readings) => LineOutcome::Ambiguous { readings },
        })
        .collect();
    let base = (0..base_lines)
        .map(|_| match agreement(&mut bases) {
            Agreement::NoChain => BaseLineOutcome::Agreed {
                fates: BTreeSet::new(),
                class: None,
            },
            Agreement::Agreed(fates) => BaseLineOutcome::Agreed {
                fates,
                class: strongest,
            },
            Agreement::Disputed(readings) => BaseLineOutcome::Ambiguous { readings },
        })
        .collect();
    (target, base)
}

/// What the chains say about one line.
enum Agreement<T> {
    /// There is no chain.
    NoChain,
    /// Every chain says this.
    Agreed(T),
    /// The chains disagree; each one's reading.
    Disputed(Vec<Reading<T>>),
}

/// Takes every chain's value of the next line and compares them. A chain
/// without one (which replay never produces) reads as empty.
fn agreement<T: Default + PartialEq>(
    chains: &mut [(&Chain, std::vec::IntoIter<T>)],
) -> Agreement<T> {
    let values: Vec<(&Chain, T)> = chains
        .iter_mut()
        .map(|(chain, values)| (*chain, values.next().unwrap_or_default()))
        .collect();
    if values.windows(2).all(|pair| pair[0].1 == pair[1].1) {
        return values
            .into_iter()
            .next()
            .map_or(Agreement::NoChain, |(_, agreed)| Agreement::Agreed(agreed));
    }
    Agreement::Disputed(
        values
            .into_iter()
            .map(|(chain, value)| Reading {
                chain: chain.id,
                class: chain.class,
                value,
            })
            .collect(),
    )
}
