//! Reflog origins produced by real git workflows.

mod common;

use std::path::{Path, PathBuf};

use common::TestRepo;
use supersigil_git::origin::{Origins, find_origins, head_reflog};
use supersigil_git::worktree::list_worktrees;
use supersigil_git::{ObjectFormat, ObjectId, Repo};

fn id(text: &str) -> ObjectId {
    ObjectId::parse(text.trim(), ObjectFormat::Sha1).unwrap()
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap()
}

/// Origins of `commits`, seen from the repository at `root`.
fn origins_of(repo: &TestRepo, commits: &[&str]) -> Origins {
    let opened = repo.repo();
    let worktrees = list_worktrees(&opened).unwrap();
    let commits: Vec<ObjectId> = commits.iter().map(|c| id(c)).collect();
    find_origins(&opened, &worktrees, &commits).unwrap()
}

/// The canonical origin worktrees of `commit`, empty when it has none.
fn origin_dirs(origins: &Origins, commit: &str) -> Vec<PathBuf> {
    origins
        .commits
        .iter()
        .find(|o| o.commit.as_str() == commit.trim())
        .map(|o| o.worktrees.iter().map(|w| canonical(w)).collect())
        .unwrap_or_default()
}

fn head(repo: &TestRepo, dir: &Path) -> String {
    repo.run_in(dir, &["rev-parse", "HEAD"]).trim().to_owned()
}

/// Asserts that the reflog entry for `commit` has a subject starting `prefix`.
fn assert_subject(entries: &[supersigil_git::origin::ReflogEntry], commit: &str, prefix: &str) {
    assert!(
        entries
            .iter()
            .any(|e| e.commit.as_str() == commit && e.subject.starts_with(prefix)),
        "{prefix} {entries:?}"
    );
}

#[test]
fn commit_and_amend_are_origins() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let first = repo.commit_all("first");
    repo.write("a.txt", b"b\n");
    repo.run(&["commit", "-q", "-a", "--amend", "-m", "first, amended"]);
    let amended = head(&repo, &repo.root);
    let origins = origins_of(&repo, &[&first, &amended]);
    assert_eq!(origin_dirs(&origins, &first), [canonical(&repo.root)]);
    assert_eq!(origin_dirs(&origins, &amended), [canonical(&repo.root)]);
    assert!(origins.without_origin.is_empty());
}

#[test]
fn a_commit_titled_fast_forward_is_still_an_origin() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let commit = repo.commit_all("Fast-forward handling fix");
    let origins = origins_of(&repo, &[&commit]);
    assert_eq!(origin_dirs(&origins, &commit), [canonical(&repo.root)]);
}

#[test]
fn a_merge_commit_is_an_origin_but_a_fast_forward_is_not() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    repo.commit_all("base");
    let linked = repo.dir.path().join("linked");
    repo.run(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feat",
        linked.to_str().unwrap(),
    ]);
    std::fs::write(linked.join("f.txt"), b"f\n").unwrap();
    repo.run_in(&linked, &["add", "f.txt"]);
    repo.run_in(&linked, &["commit", "-q", "-m", "feature"]);
    let feature = head(&repo, &linked);
    // Fast-forward main onto the feature commit made in the linked worktree.
    repo.run(&["merge", "-q", "--ff-only", "feat"]);
    repo.write("m.txt", b"m\n");
    let side = repo.commit_all("main moves on");
    repo.run_in(
        &linked,
        &["commit", "-q", "--allow-empty", "-m", "feature two"],
    );
    let feature_two = head(&repo, &linked);
    repo.run(&["merge", "-q", "--no-edit", "--no-ff", "feat"]);
    let merge = head(&repo, &repo.root);
    let origins = origins_of(&repo, &[&feature, &side, &feature_two, &merge]);
    assert_eq!(origin_dirs(&origins, &feature), [canonical(&linked)]);
    assert_eq!(origin_dirs(&origins, &side), [canonical(&repo.root)]);
    assert_eq!(origin_dirs(&origins, &feature_two), [canonical(&linked)]);
    assert_eq!(origin_dirs(&origins, &merge), [canonical(&repo.root)]);
}

#[test]
fn cherry_pick_and_revert_are_origins() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    repo.commit_all("base");
    repo.run(&["switch", "-q", "-c", "side"]);
    repo.write("s.txt", b"s\n");
    let side = repo.commit_all("side");
    repo.run(&["switch", "-q", "main"]);
    // Main moves on, so the replay lands on a different parent.
    repo.write("m.txt", b"m\n");
    repo.commit_all("main moves on");
    repo.run(&["cherry-pick", side.as_str()]);
    let picked = head(&repo, &repo.root);
    assert_ne!(picked, side);
    repo.run(&["revert", "--no-edit", "HEAD"]);
    let reverted = head(&repo, &repo.root);
    assert_ne!(reverted, picked);
    let entries = head_reflog(&repo.repo(), &repo.root).unwrap();
    assert_subject(&entries, &picked, "cherry-pick: ");
    assert_subject(&entries, &reverted, "revert: ");
    let origins = origins_of(&repo, &[&picked, &reverted]);
    assert_eq!(origin_dirs(&origins, &picked), [canonical(&repo.root)]);
    assert_eq!(origin_dirs(&origins, &reverted), [canonical(&repo.root)]);
}

#[test]
fn an_applied_patch_is_an_origin() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    repo.commit_all("base");
    repo.run(&["switch", "-q", "-c", "side"]);
    repo.write("s.txt", b"s\n");
    let side_commit = repo.commit_all("side");
    let patches = repo.dir.path().join("patches");
    repo.run(&[
        "format-patch",
        "-q",
        "-1",
        "HEAD",
        "-o",
        patches.to_str().unwrap(),
    ]);
    repo.run(&["switch", "-q", "main"]);
    // Main moves on, so the applied patch lands on a different parent.
    repo.write("m.txt", b"m\n");
    repo.commit_all("main moves on");
    let patch = std::fs::read_dir(&patches)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    repo.run(&["am", "-q", patch.to_str().unwrap()]);
    let applied = head(&repo, &repo.root);
    assert_ne!(applied, side_commit);
    let entries = head_reflog(&repo.repo(), &repo.root).unwrap();
    assert_subject(&entries, &applied, "am: ");
    let origins = origins_of(&repo, &[&applied]);
    assert_eq!(origin_dirs(&origins, &applied), [canonical(&repo.root)]);
}

#[test]
fn a_rebased_commit_is_an_origin() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    repo.commit_all("base");
    repo.run(&["switch", "-q", "-c", "side"]);
    repo.write("s.txt", b"s\n");
    repo.commit_all("side");
    repo.run(&["switch", "-q", "main"]);
    repo.write("m.txt", b"m\n");
    repo.commit_all("main");
    repo.run(&["switch", "-q", "side"]);
    repo.run(&["rebase", "-q", "main"]);
    let rebased = head(&repo, &repo.root);
    let entries = head_reflog(&repo.repo(), &repo.root).unwrap();
    assert!(
        entries
            .iter()
            .any(|e| e.commit.as_str() == rebased && e.subject.starts_with("rebase (pick)")),
        "{entries:?}"
    );
    let origins = origins_of(&repo, &[&rebased]);
    assert_eq!(origin_dirs(&origins, &rebased), [canonical(&repo.root)]);
}

#[test]
fn pulls_that_create_commits_are_origins_in_the_pulling_worktree() {
    let upstream = TestRepo::new();
    upstream.write("a.txt", b"a\n");
    upstream.commit_all("base");
    let clone = upstream.dir.path().join("clone");
    upstream.run(&[
        "clone",
        "-q",
        "-c",
        "core.autocrlf=false",
        upstream.root.to_str().unwrap(),
        clone.to_str().unwrap(),
    ]);

    // pull --rebase replays a local commit onto new upstream work.
    std::fs::write(clone.join("local.txt"), b"local\n").unwrap();
    upstream.run_in(&clone, &["add", "local.txt"]);
    upstream.run_in(&clone, &["commit", "-q", "-m", "local"]);
    upstream.write("up.txt", b"up\n");
    upstream.commit_all("upstream");
    upstream.run_in(&clone, &["pull", "-q", "--rebase", "origin", "main"]);
    let rebased = head(&upstream, &clone);

    // pull --no-rebase makes a merge commit.
    std::fs::write(clone.join("local2.txt"), b"local2\n").unwrap();
    upstream.run_in(&clone, &["add", "local2.txt"]);
    upstream.run_in(&clone, &["commit", "-q", "-m", "local two"]);
    upstream.write("up2.txt", b"up2\n");
    upstream.commit_all("upstream two");
    upstream.run_in(
        &clone,
        &["pull", "-q", "--no-rebase", "--no-edit", "origin", "main"],
    );
    let merged = head(&upstream, &clone);

    let opened = Repo::open(upstream.git().in_dir(&clone)).unwrap();
    let worktrees = list_worktrees(&opened).unwrap();
    let origins = find_origins(&opened, &worktrees, &[id(&rebased), id(&merged)]).unwrap();
    assert_eq!(origin_dirs(&origins, &rebased), [canonical(&clone)]);
    assert_eq!(origin_dirs(&origins, &merged), [canonical(&clone)]);
}

#[test]
fn a_linked_worktree_keeps_its_own_reflog() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let base = repo.commit_all("base");
    let linked = repo.root.join(".claude/worktrees/feature");
    repo.run(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feature",
        linked.to_str().unwrap(),
    ]);
    std::fs::write(linked.join("f.txt"), b"f\n").unwrap();
    repo.run_in(&linked, &["add", "f.txt"]);
    repo.run_in(&linked, &["commit", "-q", "-m", "feature"]);
    let feature = head(&repo, &linked);
    let origins = origins_of(&repo, &[&base, &feature]);
    assert_eq!(origin_dirs(&origins, &base), [canonical(&repo.root)]);
    assert_eq!(origin_dirs(&origins, &feature), [canonical(&linked)]);
    assert_eq!(
        origins
            .origin_worktrees()
            .iter()
            .map(|w| canonical(w))
            .collect::<Vec<_>>()
            .len(),
        2
    );
}

#[test]
fn a_prunable_worktree_is_unavailable_and_its_commits_have_no_origin() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    repo.commit_all("base");
    let linked = repo.dir.path().join("gone");
    repo.run(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "gone",
        linked.to_str().unwrap(),
    ]);
    std::fs::write(linked.join("g.txt"), b"g\n").unwrap();
    repo.run_in(&linked, &["add", "g.txt"]);
    repo.run_in(
        &linked,
        &["commit", "-q", "-m", "made in the gone worktree"],
    );
    let made = head(&repo, &linked);
    std::fs::remove_dir_all(&linked).unwrap();
    let origins = origins_of(&repo, &[&made]);
    assert_eq!(origins.without_origin, [id(&made)]);
    assert_eq!(origins.unavailable.len(), 1);
    assert_eq!(
        origins.unavailable[0].reason,
        "worktree directory is missing"
    );
}

#[test]
fn an_expired_reflog_leaves_commits_without_origin() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let commit = repo.commit_all("first");
    repo.run(&["reflog", "expire", "--expire=now", "--all"]);
    assert!(head_reflog(&repo.repo(), &repo.root).unwrap().is_empty());
    let origins = origins_of(&repo, &[&commit]);
    assert!(origins.commits.is_empty());
    assert_eq!(origins.without_origin, [id(&commit)]);
}

#[test]
fn an_unborn_worktree_has_no_reflog_entries() {
    let repo = TestRepo::new();
    assert!(head_reflog(&repo.repo(), &repo.root).unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn an_unreadable_worktree_directory_is_an_error_not_a_missing_one() {
    use std::os::unix::fs::PermissionsExt;
    use supersigil_git::GitError;
    use supersigil_git::worktree::Worktree;

    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let commit = repo.commit_all("first");
    let locked = repo.dir.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let probe = locked.join("inner");
    let can_see = std::fs::metadata(&probe).map_or_else(
        |e| e.kind() != std::io::ErrorKind::PermissionDenied,
        |_| true,
    );
    let worktrees = [Worktree {
        path: probe,
        head: None,
        bare: false,
        prunable: false,
    }];
    let result = find_origins(&repo.repo(), &worktrees, &[id(&commit)]);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    if can_see {
        return; // running as a user who ignores permissions
    }
    assert!(matches!(result, Err(GitError::Io { .. })), "{result:?}");
}
