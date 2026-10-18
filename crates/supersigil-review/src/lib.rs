//! Builds the review model: the reviewed diff between two snapshots, one
//! outcome per changed line, and the JSON and terminal forms of both.
//!
//! Everything here is pure; the CLI gathers the inputs. [`diff::diff_lines`]
//! computes the reviewed diff over blob bytes. [`mapping::lines_correspond`]
//! decides whether anchor's provenance, computed over worktree-form bytes,
//! maps line for line onto that diff. [`outcome`] turns each diff line's
//! provenance into one outer outcome by its ordered rules, and
//! [`model::file_review`] assembles hunks and spans. [`summary`] and [`why`]
//! render terminal forms through [`printed`], which escapes them and
//! addresses the commands they print. [`gate`] builds gate items,
//! fingerprints for carrying judgments, and saved evidence digests.
//! [`names`] names gate items and resolves typed names. [`judging`] resolves
//! item states from judgments recorded on saved reviews. [`annotating`]
//! checks an annotation document against its session's observations before
//! the record stores it. [`decisions`] finds the annotated decisions that
//! explain the change, turns open questions into gate items, and pins what
//! a basis read of annotations.

pub mod annotating;
pub mod decisions;
pub mod diff;
pub mod gate;
pub mod judging;
pub mod mapping;
pub mod model;
pub mod names;
pub mod outcome;
pub mod printed;

pub use model::{REVIEW_SCHEMA, Review, WHY_SCHEMA};
pub mod summary;
pub mod why;
