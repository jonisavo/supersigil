//! Attributes the lines of a reviewed change to the recorded edits that
//! produced them.
//!
//! Anchor walks backward from a target state through recorded edits. Each
//! step proposes before-states and accepts one only when the recorded hashes
//! (where known), the retained patch (where kept and no hash decides), and
//! forward execution with the recorder's semantics agree ([`step`]). Lines
//! are aligned across a step from the byte ranges forward execution
//! replaced, never from a diff ([`align`]). The walk collects every chain
//! the evidence supports and classes it by what verifies it ([`walk`]);
//! each chain is replayed into per-line provenance ([`provenance`]) and the
//! chains are combined line by line ([`combine`]), with a labeled
//! content-match fallback ([`content`]). [`attribute()`] runs all of it for
//! one path. Anchor is pure: it reads no files and runs no processes.

pub mod align;
pub mod attribute;
pub mod combine;
pub mod content;
pub mod input;
pub mod lines;
pub mod provenance;
pub mod result;
pub mod step;
pub mod walk;

pub use attribute::attribute;
pub use input::{CandidateEdit, DEFAULT_BUDGET_BYTES, Request, State, TargetKind};
pub use result::{
    BaseLineOutcome, Chain, ChainClass, ChainEnd, Conflict, Fate, LineOutcome, Origin,
    PathAttribution, PathStatus, Provenance, Reading, StopReason, one_based,
};
