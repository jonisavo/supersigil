//! Content hashes, record and event IDs, timestamps, and revision numbers.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// SHA-256 hash of a file's bytes, stored as `sha256:<hex>`.
///
/// An empty file has a content hash. A missing file is represented by
/// [`crate::observations::FileState::Absent`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentId(String);

impl ContentId {
    /// Computes the SHA-256 hash of `bytes` and prefixes its hex encoding with `sha256:`.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        Self(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
    }

    /// Returns the stored hash string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Stores `value` without validating its format.
            #[must_use]
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Returns the stored string.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

string_id!(
    /// Session ID supplied by the source, such as Claude Code's `sessionId`.
    SessionId
);
string_id!(
    /// Turn ID supplied by the source, such as a Claude Code record's `uuid`.
    TurnId
);
string_id!(
    /// Edit or command ID derived from its kind, session, and source tool-use ID.
    EventId
);
string_id!(
    /// ID of a stored development history, shared by its associated checkouts.
    RecordId
);
string_id!(
    /// Timestamp string expected to contain an RFC 3339 UTC value.
    ///
    /// Values are stored without parsing and compared as strings. This gives
    /// chronological order for Claude Code's fixed-width UTC timestamps.
    Timestamp
);

impl EventId {
    /// Builds an event ID from `kind`, `session`, and the source's tool-use ID.
    ///
    /// Hashes the three strings with NUL separators using SHA-256. The ID is
    /// `kind:<hex>`, where `<hex>` encodes the first eight bytes of the hash.
    /// The same inputs produce the same ID when a transcript is read again.
    #[must_use]
    pub fn derive(kind: &str, session: &SessionId, source_id: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(kind.as_bytes());
        hasher.update(b"\0");
        hasher.update(session.as_str().as_bytes());
        hasher.update(b"\0");
        hasher.update(source_id.as_bytes());
        let digest = hasher.finalize();
        Self(format!("{kind}:{}", hex::encode(&digest[..8])))
    }
}

impl RecordId {
    /// Generates a random UUID v4 as the record ID.
    #[must_use]
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

/// Record version number, incremented each time a write transaction commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Revision(u64);

impl Revision {
    /// Initial revision of a newly created record.
    pub const ZERO: Self = Self(0);

    /// Returns this revision plus one.
    ///
    /// # Panics
    ///
    /// Panics if the revision would exceed [`u64::MAX`].
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.checked_add(1).expect("revision counter overflow"))
    }

    /// Returns the revision number as a `u64`.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_id_is_sha256_hex_with_prefix() {
        let id = ContentId::of(b"hello\n");
        assert_eq!(
            id.as_str(),
            "sha256:5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
        );
    }

    #[test]
    fn content_id_distinguishes_empty_from_absent_by_being_a_value() {
        let empty = ContentId::of(b"");
        assert_eq!(
            empty.as_str(),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn event_id_is_stable_for_same_inputs() {
        let session = SessionId::new("s1");
        let a = EventId::derive("edit", &session, "toolu_01");
        let b = EventId::derive("edit", &session, "toolu_01");
        assert_eq!(a, b);
        assert!(a.as_str().starts_with("edit:"));
        assert_eq!(a.as_str().len(), "edit:".len() + 16);
    }

    #[test]
    fn event_id_differs_by_kind_session_and_source() {
        let s1 = SessionId::new("s1");
        let s2 = SessionId::new("s2");
        let base = EventId::derive("edit", &s1, "toolu_01");
        assert_ne!(base, EventId::derive("command", &s1, "toolu_01"));
        assert_ne!(base, EventId::derive("edit", &s2, "toolu_01"));
        assert_ne!(base, EventId::derive("edit", &s1, "toolu_02"));
    }

    #[test]
    fn revision_increments() {
        assert_eq!(Revision::ZERO.next().get(), 1);
        assert_eq!(Revision::ZERO.next().next().get(), 2);
    }

    #[test]
    #[should_panic(expected = "revision counter overflow")]
    fn revision_next_panics_on_overflow() {
        let max: Revision = serde_json::from_str("18446744073709551615").unwrap();
        let _ = max.next();
    }

    #[test]
    fn timestamps_order_lexicographically() {
        let earlier = Timestamp::new("2026-09-28T10:00:00.000Z");
        let later = Timestamp::new("2026-09-28T10:00:05.000Z");
        assert!(earlier < later);
    }

    #[test]
    fn record_id_is_uuid_shaped_and_unique() {
        let a = RecordId::generate();
        let b = RecordId::generate();
        assert_ne!(a, b);
        assert_eq!(a.as_str().len(), 36);
    }
}
