//! The repository's common directory, the person's identity, the tree check,
//! and retention refs against real repositories.

mod common;

use common::{TestRepo, same_dir};
use supersigil_git::retention::{Identity, Retained, common_dir, identity, retain, tree_exists};
use supersigil_git::{GitError, ObjectId, Repo};

/// Parses git output naming one object of `repo`'s format.
fn id(repo: &Repo, output: &str) -> ObjectId {
    ObjectId::parse(output.trim(), repo.format()).unwrap()
}

/// The identity retention commits are written with in these tests, unlike
/// the fixture's `Test` author and committer.
fn person() -> Identity {
    Identity {
        name: "Ada Lovelace".to_owned(),
        email: "ada@example.com".to_owned(),
    }
}

/// A repository with a base commit and a target commit changing `a.txt`.
struct Range {
    repo: TestRepo,
    opened: Repo,
    base: ObjectId,
    target: ObjectId,
    /// The target commit's tree.
    tree: ObjectId,
}

impl Range {
    fn new() -> Self {
        let repo = TestRepo::new();
        repo.write("a.txt", b"a\n");
        let base = repo.commit_all("base");
        repo.write("a.txt", b"b\n");
        let target = repo.commit_all("target");
        let opened = repo.repo();
        Self {
            base: id(&opened, &base),
            target: id(&opened, &target),
            tree: id(&opened, &repo.run(&["rev-parse", "HEAD^{tree}"])),
            repo,
            opened,
        }
    }

    /// Retains `tree` under `reference` with `parents`, as [`person`] and
    /// with a fixed message.
    fn keep(
        &self,
        reference: &str,
        tree: &ObjectId,
        parents: &[ObjectId],
    ) -> Result<Retained, GitError> {
        retain(&self.opened, reference, tree, parents, "message", &person())
    }

    /// The tree and parents of the commit `reference` points at, one per
    /// line, then its author, committer, and message.
    fn kept(&self, reference: &str) -> String {
        self.repo.run(&[
            "log",
            "-1",
            "--format=%T%n%P%n%an <%ae>%n%cn <%ce>%n%B",
            reference,
        ])
    }

    /// The commit `reference` points at.
    fn points_at(&self, reference: &str) -> String {
        self.repo.run(&["rev-parse", reference]).trim().to_owned()
    }
}

/// Commits `a.txt` in `repo` and adds a linked worktree beside it; returns
/// the linked worktree's directory.
fn with_linked_worktree(repo: &TestRepo) -> std::path::PathBuf {
    repo.write("a.txt", b"a\n");
    repo.commit_all("first");
    let linked = repo.dir.path().join("linked");
    repo.run(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feature",
        linked.to_str().unwrap(),
    ]);
    linked
}

#[test]
fn every_worktree_of_a_repository_has_the_same_common_directory() {
    let repo = TestRepo::new();
    let linked = with_linked_worktree(&repo);
    let main = common_dir(&repo.repo()).unwrap();
    let other = common_dir(&Repo::open(repo.git().in_dir(&linked)).unwrap()).unwrap();
    assert!(same_dir(&main, &other));
    assert!(same_dir(&main, &repo.root.join(".git")));
    assert!(main.is_absolute());
    assert!(!main.to_string_lossy().starts_with(r"\\?\"));
}

#[cfg(unix)]
#[test]
fn a_symlinked_git_directory_resolves_to_one_directory() {
    let repo = TestRepo::new();
    let store = repo.dir.path().join("store.git");
    std::fs::rename(repo.root.join(".git"), &store).unwrap();
    std::os::unix::fs::symlink(&store, repo.root.join(".git")).unwrap();
    let linked = with_linked_worktree(&repo);
    // The main worktree names its git directory `.git`, through the link;
    // a linked worktree names the directory the link resolves to.
    let main = common_dir(&repo.repo()).unwrap();
    let other = common_dir(&Repo::open(repo.git().in_dir(&linked)).unwrap()).unwrap();
    assert!(same_dir(&main, &store));
    assert!(same_dir(&main, &other));
}

#[test]
fn identity_is_the_name_and_email_git_resolves_in_the_repository() {
    let repo = TestRepo::new();
    std::fs::write(
        repo.home().join(".gitconfig"),
        "[user]\n\tname = Global Name\n\temail = ada@example.com\n",
    )
    .unwrap();
    repo.run(&["config", "user.name", "Ada Lovelace"]);
    assert_eq!(
        identity(&repo.repo()).unwrap(),
        Some(Identity {
            name: "Ada Lovelace".to_owned(),
            email: "ada@example.com".to_owned(),
        })
    );
}

#[test]
fn identity_needs_both_a_name_and_an_email() {
    // The fixture's author and committer variables are not configuration,
    // so none of these repositories has an identity until it sets one.
    let missing = TestRepo::new();
    assert_eq!(identity(&missing.repo()).unwrap(), None);

    let without_email = TestRepo::new();
    without_email.run(&["config", "user.name", "Ada Lovelace"]);
    assert_eq!(identity(&without_email.repo()).unwrap(), None);

    let without_name = TestRepo::new();
    without_name.run(&["config", "user.email", "ada@example.com"]);
    assert_eq!(identity(&without_name.repo()).unwrap(), None);

    let empty_name = TestRepo::new();
    empty_name.run(&["config", "user.name", " "]);
    empty_name.run(&["config", "user.email", "ada@example.com"]);
    assert_eq!(identity(&empty_name.repo()).unwrap(), None);
}

#[test]
fn only_a_tree_present_in_the_repository_exists_as_a_tree() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let commit = repo.commit_all("first");
    let opened = repo.repo();
    let tree = id(&opened, &repo.run(&["rev-parse", "HEAD^{tree}"]));
    let blob = id(&opened, &repo.run(&["rev-parse", "HEAD:a.txt"]));
    assert!(tree_exists(&opened, &tree).unwrap());
    assert!(!tree_exists(&opened, &blob).unwrap());
    assert!(!tree_exists(&opened, &id(&opened, &commit)).unwrap());
    assert!(!tree_exists(&opened, &id(&opened, &"1".repeat(40))).unwrap());
}

#[test]
fn a_pruned_tree_no_longer_exists() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    repo.commit_all("first");
    let opened = repo.repo();
    // A tree nothing reaches, as a captured working tree is until retained.
    let blob = repo.run(&["rev-parse", "HEAD:a.txt"]);
    let entry = format!("100644 blob {}\tb.txt\n", blob.trim());
    let unreachable = id(&opened, &repo.run_input(&["mktree"], entry.as_bytes()));
    assert!(tree_exists(&opened, &unreachable).unwrap());
    repo.run(&["prune", "--expire=now"]);
    assert!(!tree_exists(&opened, &unreachable).unwrap());
}

#[test]
fn a_snapshot_is_kept_under_a_new_ref_with_its_tree_parents_and_identity() {
    let range = Range::new();
    let reference = "refs/supersigil/reviews/one";
    let parents = [range.base.clone(), range.target.clone()];
    let kept = retain(
        &range.opened,
        reference,
        &range.tree,
        &parents,
        "supersigil review one",
        &person(),
    )
    .unwrap();
    let Retained::Created(commit) = &kept else {
        panic!("expected a new commit, got {kept:?}");
    };
    assert_eq!(range.points_at(reference), commit.as_str());
    assert_eq!(
        range.kept(reference),
        format!(
            "{}\n{} {}\nAda Lovelace <ada@example.com>\nAda Lovelace <ada@example.com>\n\
             supersigil review one\n\n",
            range.tree, range.base, range.target
        )
    );
}

#[test]
fn parents_keep_their_order_without_duplicates() {
    let range = Range::new();
    let (base, target) = (&range.base, &range.target);
    let cases: [(&str, Vec<ObjectId>, String); 3] = [
        (
            "refs/supersigil/reviews/two",
            vec![target.clone(), base.clone(), target.clone()],
            format!("{target} {base}"),
        ),
        (
            "refs/supersigil/reviews/one",
            vec![target.clone(), target.clone()],
            target.to_string(),
        ),
        ("refs/supersigil/reviews/none", Vec::new(), String::new()),
    ];
    for (reference, parents, expected) in cases {
        range.keep(reference, &range.tree, &parents).unwrap();
        let kept = range.kept(reference);
        assert_eq!(kept.lines().nth(1), Some(expected.as_str()), "{reference}");
    }
}

#[test]
fn retaining_again_reuses_the_ref_and_writes_nothing() {
    let range = Range::new();
    let reference = "refs/supersigil/reviews/one";
    let parents = [range.base.clone(), range.target.clone()];
    let first = range.keep(reference, &range.tree, &parents).unwrap();
    let objects = range.repo.run(&["count-objects"]);
    // Only the tree and parents are checked, after duplicates are removed.
    let someone_else = Identity {
        name: "Someone Else".to_owned(),
        email: "else@example.com".to_owned(),
    };
    let again = [range.base.clone(), range.target.clone(), range.base.clone()];
    let second = retain(
        &range.opened,
        reference,
        &range.tree,
        &again,
        "another message",
        &someone_else,
    )
    .unwrap();
    assert_eq!(second, Retained::Existing(first.commit().clone()));
    assert_eq!(range.points_at(reference), first.commit().as_str());
    assert_eq!(range.repo.run(&["count-objects"]), objects);
}

#[test]
fn a_ref_holding_another_snapshot_is_refused_and_never_moved() {
    let range = Range::new();
    let reference = "refs/supersigil/reviews/one";
    let (base, target) = (&range.base, &range.target);
    let parents = [base.clone(), target.clone()];
    let kept = range.keep(reference, &range.tree, &parents).unwrap();
    let base_tree = id(
        &range.opened,
        &range.repo.run(&["rev-parse", "HEAD~1^{tree}"]),
    );
    let commit = kept.commit();
    let refusals = [
        (
            &base_tree,
            vec![base.clone(), target.clone()],
            format!(
                "points at commit {commit} with tree {}, not {base_tree}",
                range.tree
            ),
        ),
        (
            &range.tree,
            vec![target.clone(), base.clone()],
            format!("points at commit {commit} with parents {base} {target}, not {target} {base}"),
        ),
        (
            &range.tree,
            vec![target.clone()],
            format!("points at commit {commit} with parents {base} {target}, not {target}"),
        ),
        (
            &range.tree,
            Vec::new(),
            format!("points at commit {commit} with parents {base} {target}, not none"),
        ),
    ];
    for (tree, parents, reason) in refusals {
        let err = range.keep(reference, tree, &parents).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("{reference} already exists and {reason}")
        );
        assert_eq!(range.points_at(reference), commit.as_str());
    }
}

#[test]
fn a_ref_to_something_other_than_a_commit_is_refused_and_never_moved() {
    let range = Range::new();
    let reference = "refs/supersigil/reviews/tree";
    let tree = range.tree.as_str();
    range.repo.run(&["update-ref", reference, tree]);
    let err = range
        .keep(reference, &range.tree, std::slice::from_ref(&range.target))
        .unwrap_err();
    assert!(
        matches!(&err, GitError::RefMismatch { reference: r, .. } if r == reference),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        format!("{reference} already exists and points at a tree, not a commit")
    );
    assert_eq!(range.points_at(reference), tree);
}

#[test]
fn a_symbolic_ref_is_refused_and_never_moved_even_when_it_leads_to_the_snapshot() {
    let range = Range::new();
    // A branch at a commit with exactly the tree and parents to keep: moving
    // or deleting the branch would leave the snapshot to git's pruning.
    let commit = range.repo.run(&[
        "commit-tree",
        "-m",
        "message",
        "-p",
        range.target.as_str(),
        range.tree.as_str(),
    ]);
    let commit = commit.trim();
    range.repo.run(&["update-ref", "refs/heads/kept", commit]);
    let reference = "refs/supersigil/reviews/alias";
    range
        .repo
        .run(&["symbolic-ref", reference, "refs/heads/kept"]);
    let err = range
        .keep(reference, &range.tree, std::slice::from_ref(&range.target))
        .unwrap_err();
    assert!(
        matches!(&err, GitError::RefMismatch { reference: r, .. } if r == reference),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        format!("{reference} already exists and is a symbolic reference to refs/heads/kept")
    );
    assert_eq!(
        range.repo.run(&["symbolic-ref", reference]).trim(),
        "refs/heads/kept"
    );
    assert_eq!(range.points_at("refs/heads/kept"), commit);
}

#[test]
fn a_symbolic_ref_to_a_missing_branch_is_never_followed() {
    let range = Range::new();
    let reference = "refs/supersigil/reviews/dangling";
    range
        .repo
        .run(&["symbolic-ref", reference, "refs/heads/missing"]);
    let objects = range.repo.run(&["count-objects", "-v"]);
    let err = range
        .keep(reference, &range.tree, std::slice::from_ref(&range.target))
        .unwrap_err();
    assert!(
        matches!(&err, GitError::RefMismatch { reference: r, .. } if r == reference),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        format!("{reference} already exists and is a symbolic reference to refs/heads/missing")
    );
    assert_eq!(range.repo.run(&["count-objects", "-v"]), objects);
    // Neither the branch it names nor the reference itself was written.
    let exists = |name: &str| {
        let output = range
            .repo
            .git()
            .raw(["show-ref", "--verify", "--quiet", name])
            .unwrap();
        output.status.success()
    };
    assert!(!exists("refs/heads/missing"));
    assert_eq!(
        range.repo.run(&["symbolic-ref", reference]).trim(),
        "refs/heads/missing"
    );
}

#[test]
fn a_ref_below_the_name_is_not_taken_for_it() {
    let range = Range::new();
    let parents = [range.target.clone()];
    let elsewhere = range
        .keep("refs/supersigil/reviews/elsewhere", &range.tree, &parents)
        .unwrap();
    // A matching snapshot under `one/`, which a name match up to a slash
    // would read as `one` itself; `one` cannot be created beside it.
    let below = "refs/supersigil/reviews/one/x";
    range
        .repo
        .run(&["update-ref", below, elsewhere.commit().as_str()]);
    let reference = "refs/supersigil/reviews/one";
    let err = range.keep(reference, &range.tree, &parents).unwrap_err();
    assert!(matches!(err, GitError::Failed { .. }), "{err:?}");
    let exists = range
        .repo
        .git()
        .raw(["show-ref", "--verify", "--quiet", reference])
        .unwrap();
    assert!(!exists.status.success());
}

#[test]
fn a_held_ref_lock_fails_the_write_within_a_bound_whatever_the_configuration() {
    let range = Range::new();
    range
        .repo
        .run(&["config", "core.filesRefLockTimeout", "10000"]);
    let git_dir = range.repo.root.join(".git");
    let lock = git_dir.join("refs/supersigil/reviews/locked.lock");
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    std::fs::write(&lock, b"").unwrap();
    let started = std::time::Instant::now();
    let err = range
        .keep(
            "refs/supersigil/reviews/locked",
            &range.tree,
            std::slice::from_ref(&range.target),
        )
        .unwrap_err();
    assert!(matches!(err, GitError::Failed { .. }), "{err:?}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "waited {:?}",
        started.elapsed()
    );
}
