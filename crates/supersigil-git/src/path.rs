//! Repository-relative paths as git stores them: raw bytes.

use std::path::PathBuf;

/// A path relative to the repository root, `/`-separated, as the raw bytes
/// git prints in `-z` output. Git allows any bytes except NUL, so a path is
/// not necessarily UTF-8; the bytes stay its identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RepoPath(Vec<u8>);

impl RepoPath {
    /// Wraps raw path bytes.
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Wraps a UTF-8 path such as `src/lib.rs`.
    #[must_use]
    pub fn from_utf8(path: &str) -> Self {
        Self(path.as_bytes().to_vec())
    }

    /// Returns the raw bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Returns the path as text, or `None` when it is not UTF-8.
    #[must_use]
    pub fn to_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.0).ok()
    }

    /// Returns the path for display, replacing invalid UTF-8 with U+FFFD.
    /// Two different paths can display the same; use [`RepoPath::escaped`]
    /// where identity matters.
    #[must_use]
    pub fn display(&self) -> String {
        String::from_utf8_lossy(&self.0).into_owned()
    }

    /// Returns a reversible ASCII form: `\` becomes `\\`, and every byte
    /// outside printable ASCII (`0x20..=0x7e`) becomes `\xNN` with two
    /// lowercase hex digits.
    #[must_use]
    pub fn escaped(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(self.0.len());
        for &byte in &self.0 {
            match byte {
                b'\\' => out.push_str("\\\\"),
                0x20..=0x7e => out.push(char::from(byte)),
                _ => {
                    out.push_str("\\x");
                    out.push(char::from(HEX[usize::from(byte >> 4)]));
                    out.push(char::from(HEX[usize::from(byte & 0x0f)]));
                }
            }
        }
        out
    }

    /// Returns the path as a relative [`PathBuf`], or `None` when this
    /// platform cannot represent its bytes (non-UTF-8 on Windows).
    #[must_use]
    pub fn to_path(&self) -> Option<PathBuf> {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt as _;
            Some(PathBuf::from(std::ffi::OsString::from_vec(self.0.clone())))
        }
        #[cfg(not(unix))]
        {
            self.to_str().map(PathBuf::from)
        }
    }
}

/// Converts path bytes printed by git into a [`PathBuf`], or `None` when the
/// platform cannot represent them.
pub(crate) fn path_from_git(bytes: &[u8]) -> Option<PathBuf> {
    RepoPath::new(bytes.to_vec()).to_path()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_is_reversible_and_ascii() {
        let path = RepoPath::new(b"dir\\a b/\xff\x01\xc3\xa9.txt".to_vec());
        assert_eq!(path.escaped(), r"dir\\a b/\xff\x01\xc3\xa9.txt");
        assert!(path.escaped().is_ascii());
        assert_eq!(RepoPath::from_utf8("src/lib.rs").escaped(), "src/lib.rs");
    }

    #[test]
    fn non_utf8_paths_display_lossily_but_keep_their_bytes() {
        let a = RepoPath::new(b"x\xff".to_vec());
        let b = RepoPath::new(b"x\xfe".to_vec());
        assert_eq!(a.display(), b.display());
        assert_ne!(a, b);
        assert_ne!(a.escaped(), b.escaped());
        assert_eq!(a.to_str(), None);
        assert_eq!(RepoPath::from_utf8("é").to_str(), Some("é"));
    }
}
