//! Local, revisioned record of development history.
//!
//! The record has layers with different rules. Observations are immutable
//! events appended to per-session logs. Derivations are rebuildable findings
//! computed from observations and written as immutable documents tagged with
//! the observation revision and algorithm version that produced them. Later
//! plans add interpretations and contributions on the same store.
//!
//! Readers only ever see what the manifest pins, so a reader never mixes two
//! revisions even while a writer is appending.

pub mod derivations;
pub mod derive;
pub mod ids;
pub mod observations;
pub mod store;

pub use derivations::DerivationSet;
pub use ids::{
    ContentHasher, ContentId, EventId, RecordId, Revision, SessionId, Timestamp, TurnId,
};
pub use observations::Observation;
pub use store::{RecordSnapshot, Store, StoreError, WriteTx};
