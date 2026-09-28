//! Transcript sources that turn agent sessions into record observations.
//!
//! Claude Code is the first source. A source parses a transcript tolerantly,
//! classifies every record, and emits observations; sync appends them to a
//! record incrementally from a per-transcript cursor.
