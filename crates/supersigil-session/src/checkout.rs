//! Resolves checkout paths and checks whether one path is inside another.
//!
//! Stored checkout paths are canonical, but transcripts may use symlinks or
//! different path separators. [`placement`] accounts for these differences
//! when comparing a transcript's working directory with a checkout.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

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

/// Returns [`canonical`] if `checkout` can be resolved, or `checkout` as
/// written otherwise.
#[must_use]
pub fn resolve(checkout: &Path) -> PathBuf {
    canonical(checkout).unwrap_or_else(|_| checkout.to_path_buf())
}

/// The resolution ([`resolve`]) of each checkout a review's records name,
/// made once for the whole review, or that a sync's edits name, made once
/// for the whole sync. Every placement of a recorded file in the review,
/// and every location sync compares edits by, goes through it, so a
/// symbolic link on a checkout's path that changes meanwhile never places
/// one file two ways.
#[derive(Debug, Clone, Default)]
pub struct Resolutions(BTreeMap<PathBuf, PathBuf>);

impl Resolutions {
    /// Resolves `checkout` unless it already was.
    pub fn add(&mut self, checkout: &Path) {
        self.0
            .entry(checkout.to_path_buf())
            .or_insert_with(|| resolve(checkout));
    }

    /// The resolution of `checkout`: the one made once, or, for a checkout
    /// never added, one made now.
    #[must_use]
    pub fn resolution(&self, checkout: &Path) -> Cow<'_, Path> {
        self.0.get(checkout).map_or_else(
            || Cow::Owned(resolve(checkout)),
            |resolved| Cow::Borrowed(resolved.as_path()),
        )
    }
}

/// Where the file `path`, recorded relative to `checkout`, lies: `checkout`
/// as [`resolve`] returns it, joined with `path`, then lexically normalized
/// (`.` components removed, each `..` folded into the normal component before
/// it, dropped at a root directory, and kept when nothing precedes it to
/// fold into, as after a Windows drive without a root directory, `C:`,
/// which names the drive's current directory), with letter case as spelled.
///
/// Nothing below the checkout is read from the file system. Sync groups a
/// session's edits by this location, so edits of one file are compared
/// whether the session named it from a subdirectory, or through a symlink
/// to the checkout.
#[must_use]
pub fn file_location(checkout: &Path, path: &Path) -> PathBuf {
    file_location_in(&resolve(checkout), path)
}

/// [`file_location`] with `checkout` already resolved ([`resolve`]): only
/// the lexical normalization, so locating several files of one checkout
/// resolves it once ([`Resolutions`]).
#[must_use]
pub fn file_location_in(checkout: &Path, path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in checkout.join(path).components() {
        match component {
            Component::ParentDir => match normal.components().next_back() {
                Some(Component::Normal(_)) => {
                    normal.pop();
                }
                Some(Component::RootDir) => {}
                _ => normal.push(Component::ParentDir),
            },
            Component::CurDir => {}
            other => normal.push(other),
        }
    }
    normal
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
    fn a_missing_checkout_locates_files_by_its_normalized_spelling() {
        let location =
            |checkout: &str, path: &str| file_location(Path::new(checkout), Path::new(path));
        assert_eq!(location("/r", "sub/a.rs"), location("/r/sub", "a.rs"));
        assert_eq!(
            location("/r/./x/../sub", "./a.rs"),
            PathBuf::from("/r/sub/a.rs")
        );
        assert_eq!(location("/r/sub", "../a.rs"), PathBuf::from("/r/a.rs"));
        assert_ne!(location("/R", "a.rs"), location("/r", "a.rs"));
    }

    #[test]
    fn an_unresolved_parent_component_is_kept_in_a_location() {
        let location =
            |checkout: &str, path: &str| file_location(Path::new(checkout), Path::new(path));
        assert_eq!(
            location("../missing", "a.rs"),
            PathBuf::from("../missing/a.rs")
        );
        assert_ne!(location("../missing", "a.rs"), location("missing", "a.rs"));
        assert_eq!(location("missing", "../../a.rs"), PathBuf::from("../a.rs"));
    }

    #[cfg(unix)]
    #[test]
    fn a_parent_component_above_the_root_is_dropped() {
        assert_eq!(
            file_location(Path::new("/r/sub"), Path::new("../../../a.rs")),
            PathBuf::from("/a.rs")
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_parent_component_after_a_drive_without_a_root_is_kept() {
        let location =
            |checkout: &str, path: &str| file_location(Path::new(checkout), Path::new(path));
        // A drive alone names its current directory, whose parent is
        // unresolved.
        assert_eq!(
            location("C:../supersigil-missing", "a.rs"),
            PathBuf::from(r"C:..\supersigil-missing\a.rs")
        );
        assert_ne!(
            location("C:../supersigil-missing", "a.rs"),
            location("C:supersigil-missing", "a.rs")
        );
        // Above a drive's root directory there is nothing.
        assert_eq!(
            location(r"C:\supersigil-missing", r"..\..\a.rs"),
            PathBuf::from(r"C:\a.rs")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_checkout_and_its_real_path_locate_one_file() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let file = Path::new("a.rs");
        assert_eq!(file_location(&link, file), file_location(&real, file));
        assert_eq!(
            file_location(&link, file),
            canonical(&real).unwrap().join(file)
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
