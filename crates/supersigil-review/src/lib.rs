//! Builds the review model: the reviewed diff between two snapshots, one
//! outcome per changed line, and the JSON and terminal forms of both.
//!
//! Everything here is pure; the CLI gathers the inputs. [`diff::diff_lines`]
//! computes the reviewed diff over blob bytes. [`mapping::lines_correspond`]
//! decides whether anchor's provenance, computed over worktree-form bytes,
//! maps line for line onto that diff. [`outcome`] turns each diff line's
//! provenance into one outer outcome by its ordered rules, and
//! [`model::file_review`] assembles hunks and spans. [`summary`] and [`why`]
//! render the terminal forms.

pub mod diff;
pub mod mapping;
pub mod model;
pub mod outcome;

pub use model::{REVIEW_SCHEMA, Review, WHY_SCHEMA};
pub mod summary;
pub mod why;
