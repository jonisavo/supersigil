//! Listing worktrees against real repositories.

mod common;

use common::{TestRepo, same_dir};
use supersigil_git::worktree::list_worktrees;

#[test]
fn worktrees_are_listed_main_first_with_their_heads() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let head = repo.commit_all("first");
    let linked = repo.dir.path().join("linked tree");
    repo.run(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feature",
        linked.to_str().unwrap(),
    ]);
    let worktrees = list_worktrees(&repo.repo()).unwrap();
    assert_eq!(worktrees.len(), 2);
    assert!(same_dir(&worktrees[0].path, &repo.root));
    assert!(same_dir(&worktrees[1].path, &linked));
    for worktree in &worktrees {
        assert_eq!(worktree.head.as_ref().unwrap().as_str(), head);
        assert!(!worktree.bare);
        assert!(!worktree.prunable);
    }
}

#[test]
fn a_worktree_whose_directory_is_gone_is_prunable() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    repo.commit_all("first");
    let linked = repo.dir.path().join("gone");
    repo.run(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "gone",
        linked.to_str().unwrap(),
    ]);
    std::fs::remove_dir_all(&linked).unwrap();
    let worktrees = list_worktrees(&repo.repo()).unwrap();
    assert_eq!(worktrees.len(), 2);
    assert!(!worktrees[0].prunable);
    assert!(worktrees[1].prunable);
}

#[test]
fn an_unborn_worktree_has_no_head() {
    let repo = TestRepo::new();
    let worktrees = list_worktrees(&repo.repo()).unwrap();
    assert_eq!(worktrees.len(), 1);
    assert_eq!(worktrees[0].head, None);
}

#[test]
fn a_nested_worktree_is_listed_with_its_own_path() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    repo.commit_all("first");
    let nested = repo.root.join(".claude/worktrees/feature");
    repo.run(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feature",
        nested.to_str().unwrap(),
    ]);
    let worktrees = list_worktrees(&repo.repo()).unwrap();
    assert!(worktrees.iter().any(|w| same_dir(&w.path, &nested)));
    assert!(worktrees.iter().any(|w| same_dir(&w.path, &repo.root)));
}
