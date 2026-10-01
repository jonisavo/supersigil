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

/// Maps "not found" to `None`, so a missing file or directory is an absence
/// and any other I/O failure stays an error for the caller to give context.
///
/// # Errors
///
/// Returns the error unless its kind is [`std::io::ErrorKind::NotFound`].
pub fn found<T>(result: std::io::Result<T>) -> std::io::Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Error, ErrorKind};

    use super::*;

    #[test]
    fn found_turns_only_not_found_into_none() {
        assert_eq!(found(Ok(1)).unwrap(), Some(1));
        assert_eq!(
            found::<u8>(Err(Error::from(ErrorKind::NotFound))).unwrap(),
            None
        );
        let err = found::<u8>(Err(Error::from(ErrorKind::PermissionDenied))).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::PermissionDenied);
    }
}
