//! How checkouts are spelled, and where one lies relative to another.
//!
//! The CLI and records spell a checkout [`canonical`]ly. Transcripts keep
//! the working directory Claude Code wrote, which a symlinked directory can
//! spell differently, so comparisons go through [`placement`].

use std::path::{Path, PathBuf};

/// Where a path lies relative to a root checkout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// The root itself.
    Same,
    /// Strictly inside the root, this many components below it, such as a
    /// worktree under `.claude/worktrees` (three).
    Nested(usize),
    /// Anywhere else.
    Outside,
}

/// Where `path` lies relative to `root`. When both exist on disk their
/// [`canonical`] paths decide; otherwise the paths as written are compared
/// by component, with `\` also separating components in a Windows path.
/// Inside means below `root` through normal components only, so
/// `/work/repo/../other` is outside `/work/repo`.
#[must_use]
pub fn placement(path: &Path, root: &Path) -> Placement {
    match (canonical(path), canonical(root)) {
        (Ok(path), Ok(root)) => placement_as_written(&path, &root),
        _ => placement_as_written(path, root),
    }
}

/// `path` made absolute with every symlink resolved, as
/// [`std::fs::canonicalize`] does, but without the verbatim prefix that
/// function adds on Windows: `\\?\C:\x` becomes `C:\x`, and
/// `\\?\UNC\server\share` becomes `\\server\share`. Claude Code never
/// writes that prefix, so neither its transcripts nor its project directory
/// names would match a checkout spelled with it.
///
/// # Errors
///
/// Returns the I/O error if `path` does not exist or cannot be resolved.
pub fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path).map(without_verbatim_prefix)
}

/// `path` without a Windows verbatim prefix; other paths as they are.
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

/// [`placement`] of two paths as written, by [`comparable`] components.
fn placement_as_written(path: &Path, root: &Path) -> Placement {
    let (path, root) = (comparable(path), comparable(root));
    match path.strip_prefix(root.as_slice()) {
        Some([]) => Placement::Same,
        Some(rest) if rest.iter().all(|c| c != "..") => Placement::Nested(rest.len()),
        _ => Placement::Outside,
    }
}

/// The components of `path` for comparison, alike on every platform.
///
/// In a Windows path (one starting with a drive letter or `\\`, or any path
/// on Windows) `\` separates components as `/` does. `.` and empty
/// components are dropped, and a drive letter is upper-cased. The first component records the root: `/`, `//` for a UNC
/// path, or nothing.
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

/// Whether `text` starts with a drive letter and a colon, as in `C:`.
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
