//! Repository paths as raw bytes.

use serde::ser::SerializeMap as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A repository path's raw bytes.
///
/// Serializes as a JSON string for UTF-8 bytes and as
/// `{"hex": "<hex digits>"}` otherwise, preserving paths that print alike.
/// Sorts by raw bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PathBytes(Vec<u8>);

impl PathBytes {
    /// Wraps a repository path's bytes.
    #[must_use]
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    /// Returns the path's bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Returns the path as text, or `None` if its bytes are not UTF-8.
    #[must_use]
    pub fn to_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.0).ok()
    }
}

impl Serialize for PathBytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Some(text) = self.to_str() {
            serializer.serialize_str(text)
        } else {
            let mut map = serializer.serialize_map(Some(1))?;
            map.serialize_entry("hex", &hex::encode(&self.0))?;
            map.end()
        }
    }
}

/// The two spellings [`PathBytes`] reads.
#[derive(Deserialize)]
#[serde(untagged)]
enum Spelling {
    Text(String),
    Hex(Hex),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Hex {
    hex: String,
}

impl<'de> Deserialize<'de> for PathBytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match Spelling::deserialize(deserializer)? {
            Spelling::Text(text) => Ok(Self(text.into_bytes())),
            Spelling::Hex(Hex { hex }) => {
                hex::decode(hex).map(Self).map_err(serde::de::Error::custom)
            }
        }
    }
}
