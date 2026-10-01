//! Maps a recorded edit onto the git worktree whose file it changed.
//!
//! An edit stores the transcript's working directory (`checkout`: not
//! canonical, possibly a subdirectory) and a path relative to it. The file it
//! changed belongs to the innermost worktree containing `checkout/path`,
//! which is how a session in `/repo` editing
//! `.claude/worktrees/feature/src/lib.rs` lands in the `feature` worktree as
//! `src/lib.rs`.

use std::path::{Path, PathBuf};

use supersigil_record::observations::Edit;
use supersigil_session::checkout::{Placement, canonical, placement_as_written};

/// The worktree an edit changed a file in, and the file's path inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedEdit {
    /// Root of the innermost worktree containing the edited file, as given in
    /// `worktree_roots`.
    pub worktree: PathBuf,
    /// The file's path relative to that root, components joined with `/`.
    pub path: String,
}

/// Maps `edit` to the innermost of `worktree_roots` that contains the file
/// it edited.
///
/// The edit's checkout directory is canonicalized when it exists and taken
/// as written otherwise, and its already-validated relative path is
/// appended lexically. Nothing below the checkout is resolved: containment
/// and the remainder are both read from that one spelling
/// ([`placement_as_written`]), so a symlinked file keeps the path the agent
/// wrote, a symlinked directory in the relative path is never followed into
/// another file's name, and a deleted file still maps. Roots are compared as
/// given (callers pass them canonical); with `ignore_case` (the repository
/// sets `core.ignorecase`), a root that does not contain the file as
/// spelled is tried again with both sides ASCII-lowercased. Returns `None`
/// when no root contains the file or its path is not UTF-8.
#[must_use]
pub fn map_edit(edit: &Edit, worktree_roots: &[PathBuf], ignore_case: bool) -> Option<MappedEdit> {
    let checkout = canonical(&edit.checkout).unwrap_or_else(|_| edit.checkout.clone());
    let file = checkout.join(&edit.path);
    let mut best: Option<(usize, &PathBuf)> = None;
    for root in worktree_roots {
        // A file lies at least one component below the root that holds it.
        let Some(depth) = depth_below(&file, root, ignore_case).filter(|depth| *depth > 0) else {
            continue;
        };
        if best.is_none_or(|(shallowest, _)| depth < shallowest) {
            best = Some((depth, root));
        }
    }
    let (depth, root) = best?;
    let mut parts = file
        .components()
        .rev()
        .take(depth)
        .map(|c| c.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()?;
    parts.reverse();
    Some(MappedEdit {
        worktree: root.clone(),
        path: parts.join("/"),
    })
}

/// How many components `path` lies below `root`, as written, or `None` when
/// it is outside.
fn depth_below(path: &Path, root: &Path, ignore_case: bool) -> Option<usize> {
    let depth = |place| match place {
        Placement::Same => Some(0),
        Placement::Nested(depth) => Some(depth),
        Placement::Outside => None,
    };
    depth(placement_as_written(path, root)).or_else(|| {
        ignore_case
            .then(|| depth(placement_as_written(&lowercase(path), &lowercase(root))))
            .flatten()
    })
}

/// `path` with ASCII letters lowercased, for `core.ignorecase` comparisons.
fn lowercase(path: &Path) -> PathBuf {
    PathBuf::from(path.to_string_lossy().to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use supersigil_record::observations::{Edit, EditOperation, FileState, Material};
    use supersigil_record::{EventId, SessionId, Timestamp, TurnId};
    use supersigil_session::checkout::canonical;

    use super::{MappedEdit, map_edit};

    fn edit(checkout: &Path, path: &str) -> Edit {
        Edit {
            id: EventId::new("e1"),
            turn: TurnId::new("a1"),
            session: SessionId::new("s1"),
            path: PathBuf::from(path),
            before: FileState::unknown(),
            after: FileState::unknown(),
            patch: Material::unavailable("test"),
            old_text: Material::unavailable("test"),
            new_text: Material::unavailable("test"),
            replace_all: false,
            operation: EditOperation::Replace,
            checkout: checkout.to_path_buf(),
            time: Timestamp::new("2026-09-29T10:00:00.000Z"),
            source_ordinal: 0,
            agent_id: None,
            transcript: None,
        }
    }

    /// A repository with a nested worktree, under a canonical temporary directory.
    fn dirs() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path()).unwrap();
        let repo = root.join("repo");
        let nested = repo.join(".claude").join("worktrees").join("feature");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(nested.join("src")).unwrap();
        (dir, repo, nested)
    }

    #[test]
    fn a_parent_session_edit_lands_in_the_nested_worktree() {
        let (_dir, repo, nested) = dirs();
        let roots = [repo.clone(), nested.clone()];
        let mapped = map_edit(
            &edit(&repo, ".claude/worktrees/feature/src/lib.rs"),
            &roots,
            false,
        );
        assert_eq!(
            mapped,
            Some(MappedEdit {
                worktree: nested,
                path: "src/lib.rs".to_owned()
            })
        );
    }

    #[test]
    fn a_subdirectory_checkout_maps_to_a_repository_path() {
        let (_dir, repo, _nested) = dirs();
        let mapped = map_edit(
            &edit(&repo.join("src"), "lib.rs"),
            std::slice::from_ref(&repo),
            false,
        );
        assert_eq!(mapped.map(|m| m.path), Some("src/lib.rs".to_owned()));
    }

    #[test]
    fn an_edit_outside_every_worktree_is_unplaced() {
        let (dir, repo, _nested) = dirs();
        let elsewhere = canonical(dir.path()).unwrap().join("elsewhere");
        assert_eq!(map_edit(&edit(&elsewhere, "a.rs"), &[repo], false), None);
    }

    #[test]
    fn a_missing_checkout_is_compared_as_written() {
        let (_dir, repo, _nested) = dirs();
        let mapped = map_edit(
            &edit(&repo.join("gone"), "a.rs"),
            std::slice::from_ref(&repo),
            false,
        );
        assert_eq!(mapped.map(|m| m.path), Some("gone/a.rs".to_owned()));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_file_keeps_the_written_path() {
        let (_dir, repo, _nested) = dirs();
        std::fs::write(repo.join("src/real.rs"), "").unwrap();
        std::os::unix::fs::symlink(repo.join("src/real.rs"), repo.join("link.rs")).unwrap();
        let mapped = map_edit(&edit(&repo, "link.rs"), std::slice::from_ref(&repo), false);
        assert_eq!(mapped.map(|m| m.path), Some("link.rs".to_owned()));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_directory_in_the_edit_path_is_not_resolved() {
        let (_dir, repo, _nested) = dirs();
        std::fs::create_dir_all(repo.join("one")).unwrap();
        std::os::unix::fs::symlink(repo.join("src"), repo.join("one/link")).unwrap();
        let mapped = map_edit(
            &edit(&repo, "one/link/a.rs"),
            std::slice::from_ref(&repo),
            false,
        );
        // Resolving the link and cutting the resolved depth from the written
        // path would name `link/a.rs`, an unrelated file.
        assert_eq!(mapped.map(|m| m.path), Some("one/link/a.rs".to_owned()));
    }

    #[test]
    fn case_is_ignored_only_when_asked() {
        let roots = [PathBuf::from("/work/repo")];
        let checkout = Path::new("/work/Repo/src");
        assert_eq!(map_edit(&edit(checkout, "a.rs"), &roots, false), None);
        let mapped = map_edit(&edit(checkout, "a.rs"), &roots, true);
        assert_eq!(mapped.map(|m| m.path), Some("src/a.rs".to_owned()));
    }
}
