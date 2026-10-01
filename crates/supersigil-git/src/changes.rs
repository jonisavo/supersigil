//! Changed paths between two trees, and what kind of change each one is.

use std::ffi::OsString;

use crate::bytes::nul_fields;
use crate::error::GitError;
use crate::oid::ObjectId;
use crate::path::RepoPath;
use crate::repo::Repo;

/// A file mode as git records it in a tree, `Mode(0)` for an absent side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct Mode(pub u32);

impl Mode {
    /// No entry on this side.
    pub const ABSENT: Self = Self(0);
    /// A regular file.
    pub const REGULAR: Self = Self(0o100_644);
    /// An executable file.
    pub const EXECUTABLE: Self = Self(0o100_755);
    /// A symbolic link; the blob holds the link target.
    pub const SYMLINK: Self = Self(0o120_000);
    /// A submodule commit.
    pub const GITLINK: Self = Self(0o160_000);

    /// Returns whether this is a regular or executable file.
    #[must_use]
    pub fn is_file(self) -> bool {
        self == Self::REGULAR || self == Self::EXECUTABLE
    }
}

/// How an entry changed between the two trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeStatus {
    /// Only in the target.
    Added,
    /// Only in the base.
    Deleted,
    /// In both, with different contents or mode.
    Modified,
    /// In both, as different kinds of entry (for example a file that became
    /// a symbolic link).
    TypeChanged,
}

/// One changed entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The path, relative to the repository root.
    pub path: RepoPath,
    /// How it changed.
    pub status: ChangeStatus,
    /// Mode in the base.
    pub old_mode: Mode,
    /// Mode in the target.
    pub new_mode: Mode,
    /// Blob in the base, `None` when absent.
    pub old_blob: Option<ObjectId>,
    /// Blob in the target, `None` when absent.
    pub new_blob: Option<ObjectId>,
}

/// Lists the entries that differ between the trees `base` and `target`, with
/// `diff-tree -r -z --raw --no-renames --no-abbrev --ignore-submodules=dirty`.
///
/// Renames appear as a deletion plus an addition: rename detection is
/// derived lineage, which plan 2 does not produce. `--ignore-submodules=dirty`
/// keeps a gitlink change visible for a submodule configured `ignore = all`,
/// while submodule contents stay outside the review. `pathspecs` are literal
/// path prefixes; empty means everything.
///
/// # Errors
///
/// Returns the errors of [`crate::Git::output`], or [`GitError::Parse`] for
/// output this function does not understand or a pathspec this platform
/// cannot pass to git.
pub fn changed_paths(
    repo: &Repo,
    base: &ObjectId,
    target: &ObjectId,
    pathspecs: &[RepoPath],
) -> Result<Vec<Change>, GitError> {
    let mut args: Vec<OsString> = [
        "diff-tree",
        "-r",
        "-z",
        "--raw",
        "--no-renames",
        "--no-abbrev",
        "--ignore-submodules=dirty",
        base.as_str(),
        target.as_str(),
        "--",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    args.extend(literal_pathspecs(pathspecs)?);
    let output = repo.git().output(&args)?;
    parse_raw(&output, repo)
}

/// Turns path prefixes into literal pathspecs. A prefix this platform cannot
/// pass to git is an error: dropping it would widen the scope silently.
fn literal_pathspecs(pathspecs: &[RepoPath]) -> Result<Vec<OsString>, GitError> {
    pathspecs
        .iter()
        .map(|spec| {
            spec.literal_pathspec().ok_or_else(|| {
                GitError::Parse(format!(
                    "pathspec {} is not representable on this platform",
                    spec.escaped()
                ))
            })
        })
        .collect()
}

/// Parses `--raw -z` records: `:<old mode> <new mode> <old id> <new id> <status>`
/// then the path, each NUL-terminated.
fn parse_raw(output: &[u8], repo: &Repo) -> Result<Vec<Change>, GitError> {
    let fields: Vec<&[u8]> = nul_fields(output).collect();
    let mut changes = Vec::with_capacity(fields.len() / 2);
    for record in fields.chunks(2) {
        let [meta, path] = record else {
            return Err(GitError::Parse(
                "diff-tree output ends mid-record".to_owned(),
            ));
        };
        let meta = std::str::from_utf8(meta)
            .map_err(|_not_utf8| GitError::Parse("diff-tree record is not UTF-8".to_owned()))?;
        let parts: Vec<&str> = meta.trim_start_matches(':').split(' ').collect();
        let [old_mode, new_mode, old_id, new_id, status] = parts.as_slice() else {
            return Err(GitError::Parse(format!("diff-tree record {meta:?}")));
        };
        let id = |text: &str| -> Result<Option<ObjectId>, GitError> {
            let id = ObjectId::parse(text, repo.format())?;
            Ok((!id.is_null()).then_some(id))
        };
        changes.push(Change {
            path: RepoPath::new(path.to_vec()),
            status: match *status {
                "A" => ChangeStatus::Added,
                "D" => ChangeStatus::Deleted,
                "M" => ChangeStatus::Modified,
                "T" => ChangeStatus::TypeChanged,
                other => return Err(GitError::Parse(format!("diff-tree status {other:?}"))),
            },
            old_mode: parse_mode(old_mode)?,
            new_mode: parse_mode(new_mode)?,
            old_blob: id(old_id)?,
            new_blob: id(new_id)?,
        });
    }
    Ok(changes)
}

/// Parses an octal mode.
fn parse_mode(text: &str) -> Result<Mode, GitError> {
    u32::from_str_radix(text, 8)
        .map(Mode)
        .map_err(|_not_octal| GitError::Parse(format!("file mode {text:?}")))
}

/// What the review can do with a changed entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    /// UTF-8 text on every present side: diffed and attributed.
    Text,
    /// A NUL byte in the first 8000 bytes, or not UTF-8: listed, not diffed.
    Binary,
    /// A side larger than [`MAX_DIFF_BYTES`]: listed as too large to diff.
    TooLarge,
    /// Same blob, different mode: listed.
    ModeOnly,
    /// A symbolic link on either side: listed, not diffed, not attributed.
    Symlink,
    /// A submodule commit on either side: listed, not diffed, not attributed.
    Gitlink,
    /// The entry changed kind: listed, not diffed, not attributed.
    TypeChange,
    /// The path is not UTF-8: listed as unsupported.
    UnsupportedPath,
}

/// Largest blob the review diffs.
pub const MAX_DIFF_BYTES: u64 = 8 * 1024 * 1024;

/// Classifies what can be decided without reading blobs: an unsupported
/// path, a type change, a symbolic link, a gitlink, or a mode-only change.
/// Returns `None` when the kind depends on the bytes.
#[must_use]
pub fn classify_change(change: &Change) -> Option<FileKind> {
    let either = |mode: Mode| change.old_mode == mode || change.new_mode == mode;
    if change.path.to_str().is_none() {
        Some(FileKind::UnsupportedPath)
    } else if change.status == ChangeStatus::TypeChanged {
        Some(FileKind::TypeChange)
    } else if either(Mode::SYMLINK) {
        Some(FileKind::Symlink)
    } else if either(Mode::GITLINK) {
        Some(FileKind::Gitlink)
    } else if change.status == ChangeStatus::Modified && change.old_blob == change.new_blob {
        Some(FileKind::ModeOnly)
    } else {
        None
    }
}

/// Bytes git's binary heuristic inspects.
const SNIFF_BYTES: usize = 8000;

/// Classifies the present sides by content: [`FileKind::Binary`] when either
/// has a NUL byte in its first 8000 bytes (git's heuristic) or is not UTF-8,
/// otherwise [`FileKind::Text`].
#[must_use]
pub fn classify_bytes(old: Option<&[u8]>, new: Option<&[u8]>) -> FileKind {
    let binary = |bytes: &[u8]| {
        bytes[..bytes.len().min(SNIFF_BYTES)].contains(&0) || std::str::from_utf8(bytes).is_err()
    };
    if old.into_iter().chain(new).any(binary) {
        FileKind::Binary
    } else {
        FileKind::Text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(status: ChangeStatus, old_mode: Mode, new_mode: Mode, same_blob: bool) -> Change {
        let id =
            |c: char| ObjectId::parse(&c.to_string().repeat(40), crate::ObjectFormat::Sha1).ok();
        Change {
            path: RepoPath::from_utf8("f"),
            status,
            old_mode,
            new_mode,
            old_blob: id('a'),
            new_blob: if same_blob { id('a') } else { id('b') },
        }
    }

    #[test]
    fn kinds_decided_without_bytes() {
        use ChangeStatus::{Modified, TypeChanged};
        let cases = [
            (
                change(TypeChanged, Mode::REGULAR, Mode::SYMLINK, false),
                Some(FileKind::TypeChange),
            ),
            (
                change(Modified, Mode::SYMLINK, Mode::SYMLINK, false),
                Some(FileKind::Symlink),
            ),
            (
                change(Modified, Mode::GITLINK, Mode::GITLINK, false),
                Some(FileKind::Gitlink),
            ),
            (
                change(Modified, Mode::REGULAR, Mode::EXECUTABLE, true),
                Some(FileKind::ModeOnly),
            ),
            (change(Modified, Mode::REGULAR, Mode::REGULAR, false), None),
        ];
        for (change, expected) in cases {
            assert_eq!(classify_change(&change), expected, "{change:?}");
        }
        let mut odd = change(Modified, Mode::REGULAR, Mode::REGULAR, false);
        odd.path = RepoPath::new(b"odd\xff".to_vec());
        assert_eq!(classify_change(&odd), Some(FileKind::UnsupportedPath));
    }

    #[test]
    fn binary_follows_gits_heuristic_and_utf8() {
        assert_eq!(
            classify_bytes(Some(b"text\n"), Some(b"more\n")),
            FileKind::Text
        );
        assert_eq!(classify_bytes(None, Some(b"new\n")), FileKind::Text);
        assert_eq!(classify_bytes(Some(b"a\0b"), Some(b"x")), FileKind::Binary);
        assert_eq!(
            classify_bytes(Some(b"x"), Some(b"\xff\xfe")),
            FileKind::Binary
        );
        let mut late_nul = vec![b'a'; SNIFF_BYTES];
        late_nul.push(0);
        // Past the sniffed prefix a NUL is still not UTF-8-invalid, so the
        // file is text by git's heuristic.
        assert_eq!(classify_bytes(Some(&late_nul), None), FileKind::Text);
    }

    #[test]
    #[cfg(not(unix))]
    fn an_unrepresentable_pathspec_is_an_error() {
        let spec = RepoPath::new(b"odd\xff".to_vec());
        assert!(matches!(
            literal_pathspecs(&[spec]),
            Err(GitError::Parse(_))
        ));
    }

    #[test]
    fn representable_pathspecs_are_literal() {
        let specs = literal_pathspecs(&[RepoPath::from_utf8("a*")]).unwrap();
        assert_eq!(specs, [OsString::from(":(literal)a*")]);
    }

    #[test]
    fn file_modes_are_recognized() {
        assert!(Mode::REGULAR.is_file());
        assert!(Mode::EXECUTABLE.is_file());
        assert!(!Mode::SYMLINK.is_file());
        assert!(!Mode::ABSENT.is_file());
        assert_eq!(parse_mode("100644").unwrap(), Mode::REGULAR);
        assert_eq!(parse_mode("000000").unwrap(), Mode::ABSENT);
        parse_mode("9").unwrap_err();
    }
}
