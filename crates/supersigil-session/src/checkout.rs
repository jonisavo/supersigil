//! Resolves checkout paths and checks whether one path is inside another.
//!
//! Stored checkout paths are canonical, but transcripts may use symlinks or
//! different path separators. [`placement`] accounts for these differences
//! when comparing a transcript's working directory with a checkout.

use std::path::{Path, PathBuf};

/// Whether a path is the checkout root, a descendant, or outside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// The root itself.
    Same,
    /// A descendant of the root, with its depth in path components.
    /// For example, `.claude/worktrees/example` has depth 3.
    Nested(usize),
    /// Neither the root nor a descendant of it.
    Outside,
}

/// Checks whether `path` equals `root`, is inside it, or is outside it.
///
/// Compares canonical paths if both can be resolved. Otherwise compares the
/// supplied path components, treating `\` as a separator in Windows paths.
/// In that fallback, any `..` after the root makes the path outside, so
/// `/work/repo/../other` is outside `/work/repo`.
#[must_use]
pub fn placement(path: &Path, root: &Path) -> Placement {
    match (canonical(path), canonical(root)) {
        (Ok(path), Ok(root)) => placement_as_written(&path, &root),
        _ => placement_as_written(path, root),
    }
}

/// Checks whether `path` equals `root` or is inside it, by [`placement`].
#[must_use]
pub fn within(path: &Path, root: &Path) -> bool {
    placement(path, root) != Placement::Outside
}

/// Checks whether either path equals or is inside the other, by [`placement`].
#[must_use]
pub fn overlaps(a: &Path, b: &Path) -> bool {
    within(a, b) || within(b, a)
}

/// Returns [`canonical`], or `path` as written when it does not exist.
///
/// # Errors
///
/// Returns any I/O error other than [`std::io::ErrorKind::NotFound`].
pub fn canonical_or_written(path: &Path) -> std::io::Result<PathBuf> {
    Ok(crate::found(canonical(path))?.unwrap_or_else(|| path.to_path_buf()))
}

/// Returns an absolute path with symlinks resolved and no Windows verbatim prefix.
///
/// Calls [`std::fs::canonicalize`], then converts `\\?\C:\x` to `C:\x` and
/// `\\?\UNC\server\share` to `\\server\share`. This matches the path format
/// used in Claude Code transcripts and project directory names.
///
/// # Errors
///
/// Returns the I/O error if `path` does not exist or cannot be resolved.
pub fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path).map(without_verbatim_prefix)
}

/// Removes a Windows verbatim prefix, leaving other paths unchanged.
fn without_verbatim_prefix(path: PathBuf) -> PathBuf {
    let stripped = {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            Some(format!(r"\\{rest}"))
        } else {
            text.strip_prefix(r"\\?\").map(str::to_owned)
        }
    };
    stripped.map_or(path, PathBuf::from)
}

/// Checks whether `path` equals `root`, is inside it, or is outside it, as
/// written: normalized components are compared without accessing the file
/// system, so no symlink is resolved on either side.
///
/// Normalization is [`placement`]'s fallback: `\` is a separator in Windows
/// paths, empty and `.` components are dropped, drive letters are compared
/// uppercased, and any `..` after the root makes the path outside.
#[must_use]
pub fn placement_as_written(path: &Path, root: &Path) -> Placement {
    let (path, root) = (comparable(path), comparable(root));
    match path.strip_prefix(root.as_slice()) {
        Some([]) => Placement::Same,
        Some(rest) if rest.iter().all(|c| c != "..") => Placement::Nested(rest.len()),
        _ => Placement::Outside,
    }
}

/// Splits `path` into components for comparison across platforms.
///
/// Treats `\` as a separator on Windows and in paths starting with a drive
/// letter or `\\`. Removes empty and `.` components and uppercases drive
/// letters. The first component is `/`, `//` for UNC paths, or an empty string
/// for paths without either root.
fn comparable(path: &Path) -> Vec<String> {
    let text = path.to_string_lossy();
    let windows = cfg!(windows) || text.starts_with(r"\\") || has_drive(&text);
    let separators: &[char] = if windows { &['/', '\\'] } else { &['/'] };
    let root = if windows && (text.starts_with(r"\\") || text.starts_with("//")) {
        "//"
    } else if text.starts_with(separators) {
        "/"
    } else {
        ""
    };
    let mut components = vec![root.to_owned()];
    for (i, component) in text
        .split(separators)
        .filter(|c| !c.is_empty() && *c != ".")
        .enumerate()
    {
        if i == 0 && has_drive(component) {
            components.push(component.to_ascii_uppercase());
        } else {
            components.push(component.to_owned());
        }
    }
    components
}

/// Returns whether `text` starts with an ASCII drive letter and a colon, such as `C:`.
fn has_drive(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relation(path: &str, root: &str) -> Placement {
        placement_as_written(Path::new(path), Path::new(root))
    }

    #[test]
    fn within_and_overlaps_follow_placement() {
        let (root, inner, other) = (
            Path::new("/work/repo"),
            Path::new("/work/repo/sub"),
            Path::new("/work/other"),
        );
        assert!(within(root, root));
        assert!(within(inner, root));
        assert!(!within(root, inner));
        assert!(!within(other, root));
        assert!(overlaps(root, inner) && overlaps(inner, root));
        assert!(!overlaps(root, other));
    }

    #[test]
    fn canonical_or_written_falls_back_only_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        assert_eq!(canonical_or_written(&missing).unwrap(), missing);
        assert_eq!(
            canonical_or_written(dir.path()).unwrap(),
            canonical(dir.path()).unwrap()
        );
    }

    // Unix reports a path beneath a regular file as "not a directory", an
    // error; Windows reports it as not found, so there the written path is kept.
    #[cfg(unix)]
    #[test]
    fn canonical_or_written_reports_a_path_beneath_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        std::fs::write(&file, b"").unwrap();
        let err = canonical_or_written(&file.join("child")).unwrap_err();
        assert_ne!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn windows_paths_are_compared_by_component() {
        assert_eq!(relation(r"C:\work\repo", r"C:\work\repo"), Placement::Same);
        assert_eq!(relation(r"c:\work\repo", r"C:\work\repo"), Placement::Same);
        assert_eq!(relation("C:/work/repo", r"C:\work\repo"), Placement::Same);
        assert_eq!(
            relation(r"C:\work\repo\.claude\worktrees\x", r"C:\work\repo"),
            Placement::Nested(3)
        );
        assert_eq!(
            relation(r"\\server\share\repo", r"\\server\share\repo"),
            Placement::Same
        );
        assert_eq!(
            relation(r"C:\work\repo2", r"C:\work\repo"),
            Placement::Outside
        );
        assert_eq!(
            relation(r"C:\work\repo\..\other", r"C:\work\repo"),
            Placement::Outside
        );
    }

    #[test]
    fn unix_paths_are_compared_by_component() {
        assert_eq!(relation("/work/repo", "/work/repo"), Placement::Same);
        assert_eq!(
            relation("/work/repo/.claude/worktrees/x", "/work/repo"),
            Placement::Nested(3)
        );
        assert_eq!(relation("/other/repo", "/work/repo"), Placement::Outside);
        assert_eq!(relation("/work/repo2", "/work/repo"), Placement::Outside);
        assert_eq!(
            relation("/work/repo/../other", "/work/repo"),
            Placement::Outside
        );
        assert_eq!(relation("/work", "/work/repo"), Placement::Outside);
    }

    #[test]
    fn verbatim_prefixes_are_dropped() {
        assert_eq!(
            without_verbatim_prefix(PathBuf::from(r"\\?\C:\work\repo")),
            PathBuf::from(r"C:\work\repo")
        );
        assert_eq!(
            without_verbatim_prefix(PathBuf::from(r"\\?\UNC\server\share")),
            PathBuf::from(r"\\server\share")
        );
        assert_eq!(
            without_verbatim_prefix(PathBuf::from("/work/repo")),
            PathBuf::from("/work/repo")
        );
    }

    #[test]
    fn canonical_paths_are_placed_like_their_directories() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = canonical(dir.path()).unwrap();
        assert!(!canonical.to_string_lossy().starts_with(r"\\?\"));
        assert_eq!(placement(&canonical, dir.path()), Placement::Same);
        assert_eq!(placement(dir.path(), &canonical), Placement::Same);
    }
}
