//! Attributes the lines of a reviewed change to the recorded edits that
//! produced them.
//!
//! Anchor walks backward from a target state through recorded edits. Each
//! step proposes before-states and accepts one only when the recorded hashes
//! (where known), the retained patch (where kept and no hash decides), and
//! forward execution with the recorder's semantics agree ([`step`]). Lines
//! are aligned across a step from the byte ranges forward execution
//! replaced, never from a diff ([`align`]). Anchor is pure: it reads no
//! files and runs no processes.

pub mod align;
pub mod input;
pub mod lines;
pub mod result;
pub mod step;

pub use input::{CandidateEdit, DEFAULT_BUDGET_BYTES, Request, State, TargetKind};
pub use result::StopReason;
