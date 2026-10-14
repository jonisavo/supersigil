//! Stores agent session history and computes restores and gaps between edits.
//!
//! [`Observation`] records turns, edits, commands, and capture limitations in
//! append-only session logs. [`derive::derive`] compares the recorded file states
//! to find restores and discontinuities. Each [`DerivationSet`] identifies the
//! input revision and algorithm version used to compute it.
//!
//! [`Store`] saves these records as JSON files. A [`RecordSnapshot`] reads the
//! log lengths and document names listed in one manifest, so later writes do
//! not change what that snapshot returns.
//!
//! [`ReviewStore`] keeps a repository's saved reviews under the same
//! protocol: each review's [`reviews::Basis`] and which one each worktree
//! selected.

pub mod derivations;
pub mod derive;
mod files;
pub mod ids;
pub mod observations;
pub mod reviews;
pub mod store;
pub mod time;

pub use derivations::DerivationSet;
pub use ids::{
    ContentId, EventId, JudgmentId, RecordId, Revision, SessionId, StoreId, Timestamp, TurnId,
};
pub use observations::Observation;
pub use reviews::{ReviewSnapshot, ReviewStore, ReviewTx};
pub use store::{RecordSnapshot, Store, StoreError, WriteTx};
