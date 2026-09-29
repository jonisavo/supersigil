//! Reads Claude Code transcripts and saves session events in a Supersigil record.
//!
//! [`parse_transcript`] extracts turns, edits, and commands, and counts records
//! it cannot capture. [`sync()`] appends new observations, saves the position
//! reached in each transcript, and recomputes restores and discontinuities.

pub mod checkout;
pub mod claude_code;

pub use claude_code::{ParseOutcome, parse_transcript};
pub mod discover;
pub mod sync;

pub use sync::{SyncError, SyncReport, TranscriptReport, sync};
