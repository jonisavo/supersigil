//! Where one checkout path lies relative to another.
//!
//! Transcripts, the CLI, and records spell the same directory differently:
//! a transcript writes `C:\work\repo` where canonicalization on Windows
//! yields `\\?\C:\work\repo`, and a symlinked directory has two spellings on
//! any platform. Comparisons go through this module; stored paths keep the
//! spelling their source used.

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
/// canonical paths decide; otherwise the paths as written are compared by
/// component, after [`normalized`] and with `\` also separating components
/// in a Windows path. Inside means below `root` through normal components
/// only, so `/work/repo/../other` is outside `/work/repo`.
#[must_use]
pub fn placement(path: &Path, root: &Path) -> Placement {
    match (std::fs::canonicalize(path), std::fs::canonicalize(root)) {
        (Ok(path), Ok(root)) => placement_as_written(&path, &root),
        _ => placement_as_written(path, root),
    }
}

/// `path` without a Windows verbatim prefix: `\\?\C:\x` becomes `C:\x`, and
/// `\\?\UNC\server\share` becomes `\\server\share`. Other paths are
/// returned as they are.
#[must_use]
pub fn normalized(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
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
/// The path is [`normalized`]. In a Windows path (one starting with a drive
/// letter or `\\`, or any path on Windows) `\` separates components as `/`
/// does. `.` and empty components are dropped, and a drive letter is
/// upper-cased. The first component records the root: `/`, `//` for a UNC
/// path, or nothing.
fn comparable(path: &Path) -> Vec<String> {
    let text = normalized(path).to_string_lossy().into_owned();
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
    fn windows_verbatim_prefixes_do_not_change_the_placement() {
        assert_eq!(
            relation(r"\\?\C:\work\repo", r"C:\work\repo"),
            Placement::Same
        );
        assert_eq!(
            relation(r"C:\work\repo", r"\\?\C:\work\repo"),
            Placement::Same
        );
        assert_eq!(
            relation(r"C:\work\repo\.claude\worktrees\x", r"\\?\C:\work\repo"),
            Placement::Nested(3)
        );
        assert_eq!(
            relation(r"C:\work\repo\.claude\worktrees\x", r"C:\work\repo"),
            Placement::Nested(3)
        );
        assert_eq!(
            relation(r"\\?\UNC\server\share\repo", r"\\server\share\repo"),
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
    fn normalized_drops_only_verbatim_prefixes() {
        assert_eq!(
            normalized(Path::new(r"\\?\C:\work\repo")),
            PathBuf::from(r"C:\work\repo")
        );
        assert_eq!(
            normalized(Path::new(r"\\?\UNC\server\share")),
            PathBuf::from(r"\\server\share")
        );
        assert_eq!(
            normalized(Path::new("/work/repo")),
            PathBuf::from("/work/repo")
        );
    }
}
