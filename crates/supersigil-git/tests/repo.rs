//! Opening worktrees and resolving review ranges against real repositories.

mod common;

use common::{TestRepo, isolated, same_dir};
use supersigil_git::{
    Ancestry, Git, GitError, MIN_VERSION, ObjectFormat, Repo, ResolvedTarget, TargetSpec,
    resolve_range,
};

#[test]
fn installed_git_meets_the_minimum() {
    let repo = TestRepo::new();
    assert!(repo.repo().version() >= MIN_VERSION);
}

#[test]
fn open_finds_the_root_from_a_subdirectory() {
    let repo = TestRepo::new();
    repo.write("a/b/file.txt", b"x\n");
    let opened = Repo::open(repo.git().in_dir(repo.root.join("a/b"))).unwrap();
    assert!(same_dir(opened.root(), &repo.root));
    assert!(same_dir(opened.git().cwd(), &repo.root));
    assert_eq!(opened.format(), ObjectFormat::Sha1);
}

#[test]
fn open_outside_a_worktree_fails() {
    let dir = tempfile::tempdir().unwrap();
    let plain = dir.path().join("plain");
    std::fs::create_dir(&plain).unwrap();
    let git =
        isolated(Git::new(&plain), dir.path()).with_env("GIT_CEILING_DIRECTORIES", dir.path());
    assert!(matches!(Repo::open(git), Err(GitError::NotAWorktree(_))));
}

#[test]
fn open_inside_the_git_directory_fails() {
    let repo = TestRepo::new();
    let result = Repo::open(repo.git().in_dir(repo.root.join(".git")));
    assert!(
        matches!(result, Err(GitError::NotAWorktree(_))),
        "{result:?}"
    );
}

#[test]
fn bare_repository_is_rejected() {
    let repo = TestRepo::new();
    let bare = repo.dir.path().join("bare.git");
    repo.run(&["init", "-q", "--bare", bare.to_str().unwrap()]);
    let result = Repo::open(repo.git().in_dir(&bare));
    assert!(matches!(result, Err(GitError::Bare(_))), "{result:?}");
}

#[test]
fn sha256_repository_resolves() {
    let repo = TestRepo::new_sha256();
    repo.write("a.txt", b"a\n");
    let first = repo.commit_all("first");
    repo.write("a.txt", b"b\n");
    let second = repo.commit_all("second");
    let opened = repo.repo();
    assert_eq!(opened.format(), ObjectFormat::Sha256);
    let head = opened.resolve_commit("HEAD").unwrap();
    assert_eq!(head.as_str(), second);
    assert_eq!(head.as_str().len(), 64);
    assert_eq!(opened.empty_tree().unwrap().as_str().len(), 64);
    let range = resolve_range(&opened, None, &TargetSpec::Commit("HEAD".into())).unwrap();
    assert_eq!(range.base.commit.unwrap().as_str(), first);
    assert_eq!(range.ancestry, Ancestry::Ancestor);
}

#[test]
fn working_tree_defaults_to_a_head_base() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let head = repo.commit_all("first");
    let opened = repo.repo();
    let range = resolve_range(&opened, None, &TargetSpec::WorkingTree).unwrap();
    assert_eq!(range.base.commit.as_ref().unwrap().as_str(), head);
    let tree = repo.run(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(range.base.tree.as_str(), tree.trim());
    assert!(
        matches!(&range.target, ResolvedTarget::WorkingTree { head: Some(h) } if h.as_str() == head)
    );
    assert_eq!(range.ancestry, Ancestry::Ancestor);
    assert!(range.commits(&opened).unwrap().is_empty());
}

#[test]
fn commit_target_defaults_to_its_first_parent() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let first = repo.commit_all("first");
    repo.write("a.txt", b"b\n");
    let second = repo.commit_all("second");
    let opened = repo.repo();
    let range = resolve_range(&opened, None, &TargetSpec::Commit("HEAD".into())).unwrap();
    assert_eq!(range.base.commit.as_ref().unwrap().as_str(), first);
    assert_eq!(range.target_commit().unwrap().as_str(), second);
    let commits: Vec<String> = range
        .commits(&opened)
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(commits, [second]);
}

#[test]
fn root_commit_target_has_an_empty_tree_base() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let root = repo.commit_all("root");
    let opened = repo.repo();
    let range = resolve_range(&opened, None, &TargetSpec::Commit(root.clone())).unwrap();
    assert_eq!(range.base.commit, None);
    assert_eq!(
        range.base.tree.as_str(),
        "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
    );
    assert_eq!(range.ancestry, Ancestry::Unavailable);
    let commits: Vec<String> = range
        .commits(&opened)
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(commits, [root]);
}

#[test]
fn unborn_head_without_a_base_reviews_against_the_empty_tree() {
    let repo = TestRepo::new();
    let opened = repo.repo();
    assert_eq!(opened.head().unwrap(), None);
    let range = resolve_range(&opened, None, &TargetSpec::WorkingTree).unwrap();
    assert_eq!(range.base.commit, None);
    assert_eq!(range.base.tree, opened.empty_tree().unwrap());
    assert_eq!(range.target, ResolvedTarget::WorkingTree { head: None });
    assert_eq!(range.ancestry, Ancestry::Unavailable);
    assert!(range.commits(&opened).unwrap().is_empty());
}

#[test]
fn unborn_head_with_an_explicit_base_has_no_ancestry() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let main = repo.commit_all("first");
    repo.run(&["switch", "-q", "--orphan", "other"]);
    let opened = repo.repo();
    let range = resolve_range(&opened, Some("main"), &TargetSpec::WorkingTree).unwrap();
    assert_eq!(range.base.commit.unwrap().as_str(), main);
    assert_eq!(range.target, ResolvedTarget::WorkingTree { head: None });
    assert_eq!(range.ancestry, Ancestry::Unavailable);
}

#[test]
fn base_that_is_not_an_ancestor_is_reported() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    repo.commit_all("first");
    repo.run(&["switch", "-q", "-c", "side"]);
    repo.write("s.txt", b"s\n");
    let side = repo.commit_all("side");
    repo.run(&["switch", "-q", "main"]);
    repo.write("m.txt", b"m\n");
    repo.commit_all("main moves on");
    repo.run(&["switch", "-q", "side"]);
    let opened = repo.repo();
    let range = resolve_range(&opened, Some("main"), &TargetSpec::WorkingTree).unwrap();
    assert_eq!(range.ancestry, Ancestry::NotAncestor);
    let commits: Vec<String> = range
        .commits(&opened)
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(commits, [side]);
}

#[test]
fn revisions_that_are_not_commits_are_unknown() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    repo.commit_all("first");
    let opened = repo.repo();
    let tree = repo.run(&["rev-parse", "HEAD^{tree}"]);
    for rev in ["nope", tree.trim(), "--all", "-h"] {
        let result = resolve_range(&opened, Some(rev), &TargetSpec::WorkingTree);
        assert!(
            matches!(result, Err(GitError::UnknownRevision(ref r)) if r == rev),
            "{rev}: {result:?}"
        );
    }
}

#[test]
fn boolean_config_is_read_and_unset_is_none() {
    let repo = TestRepo::new();
    let opened = repo.repo();
    assert_eq!(opened.config_bool("core.autocrlf").unwrap(), Some(false));
    assert_eq!(opened.config_bool("supersigil.unset").unwrap(), None);
    repo.run(&["config", "core.ignorecase", "true"]);
    assert_eq!(opened.config_bool("core.ignorecase").unwrap(), Some(true));
}

#[cfg(unix)]
#[test]
fn a_worktree_directory_ending_in_a_space_is_opened_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("trailing space ");
    std::fs::create_dir(&root).unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let git = isolated(Git::new(&root), &home);
    git.output(["init", "-q", "-b", "main"]).unwrap();
    git.output(["config", "core.autocrlf", "false"]).unwrap();
    let opened = Repo::open(git).unwrap();
    assert!(opened.root().to_str().unwrap().ends_with("trailing space "));
    assert!(same_dir(opened.root(), &root));
}

#[test]
fn head_with_a_missing_commit_object_is_an_error() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let head = repo.commit_all("first");
    let opened = repo.repo();
    let (dir, rest) = head.split_at(2);
    std::fs::remove_file(repo.root.join(".git/objects").join(dir).join(rest)).unwrap();
    opened.head().unwrap_err();
    resolve_range(&opened, None, &TargetSpec::WorkingTree).unwrap_err();
}

#[test]
fn head_that_git_cannot_read_is_an_error() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    repo.commit_all("first");
    let opened = repo.repo();
    std::fs::write(repo.root.join(".git/HEAD"), b"garbage\n").unwrap();
    opened.head().unwrap_err();
}

#[test]
fn a_shallow_boundary_commit_is_not_a_root() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let first = repo.commit_all("first");
    repo.write("a.txt", b"b\n");
    repo.commit_all("second");
    let clone = repo.dir.path().join("shallow");
    let source = format!("file://{}", repo.root.display());
    repo.run(&[
        "clone",
        "-q",
        "--depth",
        "1",
        &source,
        clone.to_str().unwrap(),
    ]);
    let opened = Repo::open(repo.git().in_dir(&clone)).unwrap();
    let result = resolve_range(&opened, None, &TargetSpec::Commit("HEAD".into()));
    assert!(
        matches!(&result, Err(GitError::MissingParent { parent, .. }) if parent.as_str() == first),
        "{result:?}"
    );
}
