//! Object ids in a repository's hash format.

use std::fmt;

use crate::error::GitError;

/// Hash format of a repository's objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectFormat {
    /// SHA-1, 40 hex digits.
    Sha1,
    /// SHA-256, 64 hex digits.
    Sha256,
}

impl ObjectFormat {
    /// Returns the number of hex digits in an object id of this format.
    #[must_use]
    pub const fn hex_len(self) -> usize {
        match self {
            Self::Sha1 => 40,
            Self::Sha256 => 64,
        }
    }

    /// Parses the output of `rev-parse --show-object-format`.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::Parse`] for any other text.
    pub(crate) fn parse(text: &str) -> Result<Self, GitError> {
        match text.trim() {
            "sha1" => Ok(Self::Sha1),
            "sha256" => Ok(Self::Sha256),
            other => Err(GitError::Parse(format!("object format {other:?}"))),
        }
    }
}

/// A full object id: lowercase hex of the repository's object format.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
#[serde(transparent)]
pub struct ObjectId(String);

impl ObjectId {
    /// Validates `text` as a full object id of `format`.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::Parse`] unless `text` is exactly
    /// [`ObjectFormat::hex_len`] lowercase hex digits.
    pub fn parse(text: &str, format: ObjectFormat) -> Result<Self, GitError> {
        let valid = text.len() == format.hex_len()
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(GitError::Parse(format!("object id {text:?}")))
        }
    }

    /// Returns the hex digits.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns whether every digit is zero, git's placeholder for "no object".
    #[must_use]
    pub(crate) fn is_null(&self) -> bool {
        self.0.bytes().all(|b| b == b'0')
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Parses git output holding one object id, ignoring the trailing newline.
///
/// # Errors
///
/// Returns [`GitError::Parse`] if the output is not one id of `format`.
pub(crate) fn parse_output(output: &[u8], format: ObjectFormat) -> Result<ObjectId, GitError> {
    let text = std::str::from_utf8(output)
        .map_err(|_not_utf8| GitError::Parse("object id is not UTF-8".to_owned()))?;
    ObjectId::parse(text.trim_end(), format)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_checked_against_the_format() {
        let sha1 = "a".repeat(40);
        let sha256 = "0123456789abcdef".repeat(4);
        assert_eq!(
            ObjectId::parse(&sha1, ObjectFormat::Sha1).unwrap().as_str(),
            sha1
        );
        ObjectId::parse(&sha1, ObjectFormat::Sha256).unwrap_err();
        ObjectId::parse(&sha256, ObjectFormat::Sha256).unwrap();
        ObjectId::parse(&"A".repeat(40), ObjectFormat::Sha1).unwrap_err();
        ObjectId::parse(&"g".repeat(40), ObjectFormat::Sha1).unwrap_err();
        ObjectId::parse("", ObjectFormat::Sha1).unwrap_err();
    }

    #[test]
    fn formats_parse_and_know_their_length() {
        assert_eq!(ObjectFormat::parse("sha1\n").unwrap(), ObjectFormat::Sha1);
        assert_eq!(ObjectFormat::parse("sha256").unwrap().hex_len(), 64);
        ObjectFormat::parse("md5").unwrap_err();
    }

    #[test]
    fn the_null_id_is_recognized() {
        assert!(
            ObjectId::parse(&"0".repeat(40), ObjectFormat::Sha1)
                .unwrap()
                .is_null()
        );
        assert!(
            !ObjectId::parse(&"1".repeat(40), ObjectFormat::Sha1)
                .unwrap()
                .is_null()
        );
    }
}
