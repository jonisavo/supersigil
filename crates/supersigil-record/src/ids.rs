//! Identifiers shared by every layer of the record.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Hash of file content. Absence of a file is never represented by a content
/// id; see [`crate::observations::FileState`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentId(String);

impl ContentId {
    /// Hashes `bytes` with SHA-256.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        Self(format!("sha256:{}", hex::encode(digest)))
    }

    /// The `sha256:<hex>` form.
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
            /// Wraps an existing identifier string.
            #[must_use]
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// The identifier as a string slice.
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
    /// Identifier of a session, taken from the source (Claude Code's `sessionId`).
    SessionId
);
string_id!(
    /// Identifier of a turn, taken from the source (Claude Code's record `uuid`).
    TurnId
);
string_id!(
    /// Identifier of an edit or command event, derived from its source id.
    EventId
);
string_id!(
    /// Identifier of a record (one per repository association set).
    RecordId
);
string_id!(
    /// RFC 3339 UTC timestamp string. Ordering is lexicographic, which is
    /// correct for the fixed-width form Claude Code writes.
    Timestamp
);

impl EventId {
    /// Derives a stable event id from the event kind, its session, and the
    /// source's own id for it (for example a `tool_use` id). Re-syncing the
    /// same transcript yields the same ids.
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
    /// Generates a fresh random record id.
    #[must_use]
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

/// Monotonic revision number of a record manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Revision(u64);

impl Revision {
    /// The revision of an empty record before any commit.
    pub const ZERO: Self = Self(0);

    /// The following revision.
    ///
    /// # Panics
    ///
    /// Panics if the revision would exceed [`u64::MAX`].
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.checked_add(1).expect("revision counter overflow"))
    }

    /// The numeric value.
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
