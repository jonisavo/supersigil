//! The working-tree snapshot against real repositories.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{TestRepo, isolated};
use supersigil_git::snapshot::{
    NotCaptured, NotCapturedCause, OnDisk, SnapshotOptions, snapshot_working_tree,
};
use supersigil_git::{Git, GitError, Repo, RepoPath};

fn files(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

fn paths(list: &[RepoPath]) -> Vec<String> {
    list.iter().map(RepoPath::display).collect()
}

fn listed(entries: &[NotCaptured]) -> Vec<(String, OnDisk, NotCapturedCause)> {
    entries
        .iter()
        .map(|e| (e.path.display(), e.on_disk, e.cause))
        .collect()
}

/// A repository with four committed files.
fn committed() -> TestRepo {
    let repo = TestRepo::new();
    for name in ["a", "b", "c", "d"] {
        repo.write(&format!("{name}.txt"), format!("{name}\n").as_bytes());
    }
    repo.commit_all("four files");
    repo
}

#[test]
fn snapshot_takes_the_on_disk_state_of_tracked_files() {
    let repo = committed();
    repo.write("a.txt", b"a changed\n");
    std::fs::remove_file(repo.root.join("b.txt")).unwrap();
    repo.write("c.txt", b"c staged\n");
    repo.run(&["add", "c.txt"]);
    repo.write("c.txt", b"c staged then changed\n");
    repo.write("new.txt", b"untracked\n");
    let snapshot = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    assert_eq!(
        repo.tree_files(snapshot.tree.as_str()),
        files(&[
            ("a.txt", "a changed\n"),
            ("c.txt", "c staged then changed\n"),
            ("d.txt", "d\n"),
        ])
    );
    assert_eq!(paths(&snapshot.untracked), ["new.txt"]);
    assert!(snapshot.not_captured.is_empty());
    assert!(snapshot.unmerged.is_empty());
    assert_eq!(snapshot.skip_worktree_absent, 0);
}

#[test]
fn the_real_index_is_byte_identical_and_keeps_its_mtime() {
    let repo = committed();
    repo.write("a.txt", b"a changed\n");
    repo.write("c.txt", b"c staged\n");
    repo.run(&["add", "c.txt"]);
    let index = repo.root.join(".git/index");
    let bytes = std::fs::read(&index).unwrap();
    let modified = std::fs::metadata(&index).unwrap().modified().unwrap();
    snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    assert_eq!(std::fs::read(&index).unwrap(), bytes);
    assert_eq!(
        std::fs::metadata(&index).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(repo.run(&["diff", "--cached", "--name-only"]), "c.txt\n");
}

#[cfg(unix)]
#[test]
fn snapshot_never_runs_hooks() {
    use std::os::unix::fs::PermissionsExt as _;
    let repo = committed();
    let marker = repo.dir.path().join("hook-ran");
    let hook = repo.root.join(".git/hooks/post-index-change");
    std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    repo.write("a.txt", b"a changed\n");
    snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    assert!(!marker.exists(), "a hook ran during the snapshot");
    // The hook itself works: a normal `git add` runs it.
    repo.run(&["add", "a.txt"]);
    assert!(marker.exists());
}

#[test]
fn a_fresh_repository_without_an_index_reads_as_empty() {
    let repo = TestRepo::new();
    repo.write("first.txt", b"hello\n");
    assert!(!repo.root.join(".git/index").exists());
    let opened = repo.repo();
    let snapshot = snapshot_working_tree(&opened, &SnapshotOptions::default()).unwrap();
    assert_eq!(snapshot.tree, opened.empty_tree().unwrap());
    assert_eq!(paths(&snapshot.untracked), ["first.txt"]);
    let options = SnapshotOptions {
        include_untracked: vec![RepoPath::from_utf8("first.txt")],
        ..SnapshotOptions::default()
    };
    let included = snapshot_working_tree(&opened, &options).unwrap();
    assert_eq!(
        repo.tree_files(included.tree.as_str()),
        files(&[("first.txt", "hello\n")])
    );
    assert!(included.untracked.is_empty());
    assert!(!repo.root.join(".git/index").exists());
}

#[test]
fn listed_untracked_paths_are_included_and_bad_ones_refused() {
    let repo = committed();
    repo.write("new.txt", b"new\n");
    repo.write("other.txt", b"other\n");
    repo.write(".gitignore", b"ignored.txt\n");
    repo.write("ignored.txt", b"secret\n");
    let opened = repo.repo();
    let options = SnapshotOptions {
        include_untracked: vec![RepoPath::from_utf8("new.txt")],
        ..SnapshotOptions::default()
    };
    let snapshot = snapshot_working_tree(&opened, &options).unwrap();
    let tree = repo.tree_files(snapshot.tree.as_str());
    assert_eq!(tree.get("new.txt").map(String::as_str), Some("new\n"));
    assert!(!tree.contains_key("other.txt"));
    assert_eq!(paths(&snapshot.untracked), [".gitignore", "other.txt"]);

    for (path, reason) in [("missing.txt", "no such file"), ("ignored.txt", "ignored")] {
        let options = SnapshotOptions {
            include_untracked: vec![RepoPath::from_utf8(path)],
            ..SnapshotOptions::default()
        };
        let result = snapshot_working_tree(&opened, &options);
        assert!(
            matches!(&result, Err(GitError::Untracked { path: p, reason: r }) if p == path && r == reason),
            "{path}: {result:?}"
        );
    }
}

#[test]
fn a_split_index_is_read_without_writing_shared_indexes() {
    let repo = committed();
    repo.run(&["config", "core.splitIndex", "true"]);
    repo.run(&["update-index", "--split-index"]);
    repo.write("a.txt", b"a changed\n");
    let shared = |repo: &TestRepo| {
        std::fs::read_dir(repo.root.join(".git"))
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("sharedindex.")
            })
            .count()
    };
    let before = shared(&repo);
    assert!(before >= 1);
    let index = std::fs::read(repo.root.join(".git/index")).unwrap();
    let snapshot = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    assert_eq!(shared(&repo), before);
    assert_eq!(std::fs::read(repo.root.join(".git/index")).unwrap(), index);
    assert_eq!(
        repo.tree_files(snapshot.tree.as_str())
            .get("a.txt")
            .unwrap(),
        "a changed\n"
    );
}

#[test]
fn assume_unchanged_entries_are_listed_present_or_missing() {
    let repo = committed();
    repo.run(&["update-index", "--assume-unchanged", "a.txt", "b.txt"]);
    repo.write("a.txt", b"a hidden change\n");
    std::fs::remove_file(repo.root.join("b.txt")).unwrap();
    let snapshot = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    let tree = repo.tree_files(snapshot.tree.as_str());
    assert_eq!(tree.get("a.txt").unwrap(), "a\n");
    assert_eq!(tree.get("b.txt").unwrap(), "b\n");
    assert_eq!(
        listed(&snapshot.not_captured),
        [
            (
                "a.txt".to_owned(),
                OnDisk::Present,
                NotCapturedCause::AssumeUnchanged
            ),
            (
                "b.txt".to_owned(),
                OnDisk::Missing,
                NotCapturedCause::AssumeUnchanged
            ),
        ]
    );
}

#[test]
fn skip_worktree_entries_are_listed_when_present_and_counted_when_absent() {
    let repo = committed();
    repo.run(&["update-index", "--skip-worktree", "a.txt", "b.txt"]);
    repo.write("a.txt", b"a local override\n");
    std::fs::remove_file(repo.root.join("b.txt")).unwrap();
    let snapshot = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    assert_eq!(
        listed(&snapshot.not_captured),
        [(
            "a.txt".to_owned(),
            OnDisk::Present,
            NotCapturedCause::SkipWorktree
        )]
    );
    assert_eq!(snapshot.skip_worktree_absent, 1);
    assert_eq!(
        repo.tree_files(snapshot.tree.as_str())
            .get("a.txt")
            .unwrap(),
        "a\n"
    );
}

#[test]
fn a_sparse_file_recreated_outside_the_patterns_is_listed_not_captured() {
    let repo = TestRepo::new();
    repo.write("in/f.txt", b"inside\n");
    repo.write("out/g.txt", b"outside\n");
    repo.commit_all("two directories");
    repo.run(&["sparse-checkout", "set", "--no-cone", "/in/"]);
    assert!(!repo.root.join("out/g.txt").exists());
    repo.write("out/g.txt", b"recreated with new contents\n");
    repo.write("in/f.txt", b"inside changed\n");
    let snapshot = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    let tree = repo.tree_files(snapshot.tree.as_str());
    assert_eq!(tree.get("in/f.txt").unwrap(), "inside changed\n");
    assert_eq!(tree.get("out/g.txt").unwrap(), "outside\n");
    // Newer git clears the skip-worktree bit of a file that reappears, and
    // `diff-files` reports it; older git keeps the bit, and the flag listing
    // reports it. Either way it is listed as present and not captured.
    let entry = snapshot
        .not_captured
        .iter()
        .find(|e| e.path.display() == "out/g.txt")
        .expect("out/g.txt is listed");
    assert_eq!(entry.on_disk, OnDisk::Present);
    assert!(matches!(
        entry.cause,
        NotCapturedCause::NotStaged | NotCapturedCause::SkipWorktree
    ));
}

#[test]
fn a_submodule_configured_ignore_all_is_listed_not_captured() {
    let repo = TestRepo::new();
    let sub = TestRepo::new();
    sub.write("s.txt", b"one\n");
    sub.commit_all("sub one");
    repo.write("a.txt", b"a\n");
    repo.run(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        "-q",
        sub.root.to_str().unwrap(),
        "sm",
    ]);
    repo.run(&["config", "-f", ".gitmodules", "submodule.sm.ignore", "all"]);
    repo.commit_all("with submodule");
    let old = repo.run(&["rev-parse", "HEAD:sm"]);
    let sm = repo.root.join("sm");
    repo.run_in(&sm, &["config", "core.autocrlf", "false"]);
    repo.write("sm/s.txt", b"two\n");
    repo.run_in(&sm, &["commit", "-q", "-am", "sub two"]);
    let new = repo.run_in(&sm, &["rev-parse", "HEAD"]);
    let snapshot = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    let spec = format!("{}:sm", snapshot.tree);
    let captured = repo.run(&["rev-parse", &spec]);
    // Newer git's `add -u` honors `ignore = all` and keeps the old gitlink,
    // which `diff-files` then lists; git 2.36 stages the advanced gitlink, so
    // nothing is left to list. Either is coherent; a kept gitlink that is not
    // listed, or a captured one that is, is not.
    if captured == old {
        assert_eq!(
            listed(&snapshot.not_captured),
            [(
                "sm".to_owned(),
                OnDisk::Present,
                NotCapturedCause::NotStaged
            )]
        );
    } else {
        assert_eq!(captured, new);
        assert!(listed(&snapshot.not_captured).is_empty());
    }
}

#[test]
fn an_unmerged_path_is_taken_on_disk_and_listed() {
    let repo = TestRepo::new();
    repo.write("f.txt", b"base\n");
    repo.commit_all("base");
    repo.run(&["switch", "-q", "-c", "side"]);
    repo.write("f.txt", b"side\n");
    repo.commit_all("side");
    repo.run(&["switch", "-q", "main"]);
    repo.write("f.txt", b"main\n");
    repo.commit_all("main");
    let merge = repo.git().raw(["merge", "-q", "side"]).unwrap();
    assert!(!merge.status.success(), "the merge conflicts");
    let on_disk = std::fs::read_to_string(repo.root.join("f.txt")).unwrap();
    assert!(on_disk.contains("<<<<<<<"));
    let snapshot = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    assert_eq!(paths(&snapshot.unmerged), ["f.txt"]);
    assert_eq!(
        repo.tree_files(snapshot.tree.as_str())
            .get("f.txt")
            .unwrap(),
        &on_disk
    );
    assert!(
        repo.run(&["ls-files", "-u"]).contains("f.txt"),
        "the real index stays unmerged"
    );
}

#[test]
fn an_intent_to_add_entry_is_staged() {
    let repo = committed();
    repo.write("planned.txt", b"planned\n");
    repo.run(&["add", "-N", "planned.txt"]);
    let snapshot = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    assert_eq!(
        repo.tree_files(snapshot.tree.as_str())
            .get("planned.txt")
            .unwrap(),
        "planned\n"
    );
}

#[test]
fn a_changed_modification_time_alone_is_not_listed() {
    let repo = committed();
    let path = repo.root.join("a.txt");
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(later)
        .unwrap();
    let snapshot = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    assert!(
        snapshot.not_captured.is_empty(),
        "{:?}",
        snapshot.not_captured
    );
}

#[test]
fn pathspecs_limit_the_listings_but_not_the_tree() {
    let repo = committed();
    repo.write("src/new.rs", b"fn main() {}\n");
    repo.write("docs/new.md", b"# doc\n");
    repo.write("a.txt", b"a changed\n");
    let options = SnapshotOptions {
        pathspecs: vec![RepoPath::from_utf8("src")],
        ..SnapshotOptions::default()
    };
    let snapshot = snapshot_working_tree(&repo.repo(), &options).unwrap();
    assert_eq!(paths(&snapshot.untracked), ["src/new.rs"]);
    assert_eq!(
        repo.tree_files(snapshot.tree.as_str())
            .get("a.txt")
            .unwrap(),
        "a changed\n"
    );
}

#[test]
fn a_linked_worktree_is_snapshotted_from_its_own_index() {
    let repo = committed();
    let linked = repo.dir.path().join("linked");
    repo.run(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feature",
        linked.to_str().unwrap(),
    ]);
    std::fs::write(linked.join("a.txt"), b"a in linked\n").unwrap();
    let opened = supersigil_git::Repo::open(repo.git().in_dir(&linked)).unwrap();
    let snapshot = snapshot_working_tree(&opened, &SnapshotOptions::default()).unwrap();
    assert_eq!(
        repo.tree_files(snapshot.tree.as_str())
            .get("a.txt")
            .unwrap(),
        "a in linked\n"
    );
    let main = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    assert_eq!(
        repo.tree_files(main.tree.as_str()).get("a.txt").unwrap(),
        "a\n"
    );
}

#[cfg(windows)]
#[test]
fn a_pathspec_this_platform_cannot_represent_is_refused() {
    let repo = committed();
    // A lone surrogate is not UTF-8, so the path has no Windows form.
    let options = SnapshotOptions {
        pathspecs: vec![RepoPath::new(b"x\xff".to_vec())],
        ..SnapshotOptions::default()
    };
    let result = snapshot_working_tree(&repo.repo(), &options);
    assert!(result.is_err(), "{result:?}");
}

#[cfg(unix)]
#[test]
fn a_magic_looking_ignored_name_is_ignored_and_refused() {
    let repo = committed();
    repo.write(".gitignore", b"/:(literal)x\n");
    repo.write(":(literal)x", b"secret\n");
    let options = SnapshotOptions {
        include_untracked: vec![RepoPath::from_utf8(":(literal)x")],
        ..SnapshotOptions::default()
    };
    let result = snapshot_working_tree(&repo.repo(), &options);
    assert!(
        matches!(&result, Err(GitError::Untracked { reason, .. }) if reason == "ignored"),
        "{result:?}"
    );
}

#[cfg(unix)]
#[test]
fn an_unreadable_directory_is_an_error_not_a_missing_file() {
    use std::os::unix::fs::PermissionsExt as _;
    let repo = TestRepo::new();
    repo.write("dir/f.txt", b"f\n");
    repo.commit_all("dir");
    repo.run(&["update-index", "--assume-unchanged", "dir/f.txt"]);
    let dir = repo.root.join("dir");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).unwrap();
    let restore = || std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755));
    if std::fs::symlink_metadata(dir.join("f.txt")).is_ok() {
        restore().unwrap();
        return; // running as root: permissions do not apply
    }
    let result = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default());
    restore().unwrap();
    assert!(
        matches!(result, Err(GitError::Io { .. })),
        "{:?}",
        result.map(|s| s.not_captured)
    );
}

#[test]
fn a_fatal_ignore_check_is_an_error_not_a_clean_path() {
    let repo = committed();
    // Beside the worktree: it exists, but `check-ignore` refuses it as
    // outside the repository (exit 128).
    std::fs::write(repo.dir.path().join("outside"), b"x\n").unwrap();
    let options = SnapshotOptions {
        include_untracked: vec![RepoPath::from_utf8("../outside")],
        ..SnapshotOptions::default()
    };
    let result = snapshot_working_tree(&repo.repo(), &options);
    assert!(
        matches!(&result, Err(GitError::Failed { args, status: Some(128), stderr }) if args == "check-ignore" && stderr.contains("outside repository")),
        "{:?}",
        result.map(|s| s.tree)
    );
}

/// Set only in the child run of
/// [`an_inherited_object_directory_receives_no_snapshot_objects`]: the
/// temporary directory of the repository the child snapshots.
const SNAPSHOT_CHILD: &str = "SUPERSIGIL_TEST_SNAPSHOT_CHILD";

/// Every file below `dir`, recursively.
fn files_below(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(files_below(&path));
        } else {
            files.push(path);
        }
    }
    files
}

/// The snapshot's objects (the changed file's blob and the tree) go to the
/// repository's own object store even when the process running the runner
/// inherits variables naming another one. The variables are set only in
/// the environment of a child run of this test, which every git the runner
/// starts there would inherit; this process's environment never changes.
#[test]
fn an_inherited_object_directory_receives_no_snapshot_objects() {
    if let Some(dir) = std::env::var_os(SNAPSHOT_CHILD) {
        let dir = PathBuf::from(dir);
        let git = isolated(Git::new(dir.join("repo")), &dir.join("home"));
        let repo = Repo::open(git).unwrap();
        let snapshot = snapshot_working_tree(&repo, &SnapshotOptions::default()).unwrap();
        println!("snapshot tree {}", snapshot.tree);
        return;
    }
    let repo = committed();
    repo.write("a.txt", b"a changed\n");
    let elsewhere = repo.dir.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    let own = repo.root.join(".git").join("objects");

    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "an_inherited_object_directory_receives_no_snapshot_objects",
            "--nocapture",
        ])
        .env(SNAPSHOT_CHILD, repo.dir.path())
        .env("GIT_OBJECT_DIRECTORY", &elsewhere)
        .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", &own)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&child.stdout);
    assert!(
        child.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&child.stderr)
    );
    let tree = stdout
        .lines()
        .find_map(|line| line.strip_prefix("snapshot tree "))
        .unwrap_or_else(|| panic!("no tree in {stdout}"));
    assert_eq!(files_below(&elsewhere), Vec::<PathBuf>::new());
    assert_eq!(repo.tree_files(tree)["a.txt"], "a changed\n");
}
