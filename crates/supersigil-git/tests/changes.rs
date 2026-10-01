//! Changed paths, classification, and bytes against real repositories.

mod common;

use common::TestRepo;
use supersigil_git::bytes::{
    Blob, Conversion, blob_sizes, read_blobs, read_blobs_within, worktree_form,
};
use supersigil_git::changes::{
    Change, ChangeStatus, FileKind, Mode, changed_paths, classify_change,
};
use supersigil_git::{GitError, ObjectFormat, ObjectId, RepoPath};

/// A conversion limit no test output reaches.
const NO_LIMIT: u64 = u64::MAX;

fn id(text: &str) -> ObjectId {
    ObjectId::parse(text.trim(), ObjectFormat::Sha1).unwrap()
}

fn tree(repo: &TestRepo, rev: &str) -> ObjectId {
    id(&repo.run(&["rev-parse", &format!("{rev}^{{tree}}")]))
}

fn summary(changes: &[Change]) -> Vec<(String, ChangeStatus)> {
    changes
        .iter()
        .map(|c| (c.path.display(), c.status))
        .collect()
}

#[test]
fn changed_paths_report_status_modes_and_blobs() {
    let repo = TestRepo::new();
    for name in ["a", "b", "c"] {
        repo.write(&format!("{name}.txt"), format!("{name}\n").as_bytes());
    }
    repo.commit_all("base");
    repo.write("a.txt", b"a changed\n");
    std::fs::remove_file(repo.root.join("b.txt")).unwrap();
    repo.write("d.txt", b"d\n");
    repo.run(&["add", "-A"]);
    repo.run(&["update-index", "--chmod=+x", "c.txt"]);
    repo.run(&["commit", "-q", "-m", "target"]);
    let changes = changed_paths(
        &repo.repo(),
        &tree(&repo, "HEAD~1"),
        &tree(&repo, "HEAD"),
        &[],
    )
    .unwrap();
    assert_eq!(
        summary(&changes),
        [
            ("a.txt".to_owned(), ChangeStatus::Modified),
            ("b.txt".to_owned(), ChangeStatus::Deleted),
            ("c.txt".to_owned(), ChangeStatus::Modified),
            ("d.txt".to_owned(), ChangeStatus::Added),
        ]
    );
    let [a, b, c, d] = changes.as_slice() else {
        unreachable!()
    };
    assert_eq!(
        a.old_blob.as_ref().unwrap(),
        &id(&repo.run(&["rev-parse", "HEAD~1:a.txt"]))
    );
    assert_eq!(
        a.new_blob.as_ref().unwrap(),
        &id(&repo.run(&["rev-parse", "HEAD:a.txt"]))
    );
    assert_eq!((b.new_mode, b.new_blob.as_ref()), (Mode::ABSENT, None));
    assert_eq!((c.old_mode, c.new_mode), (Mode::REGULAR, Mode::EXECUTABLE));
    assert_eq!(classify_change(c), Some(FileKind::ModeOnly));
    assert_eq!((d.old_mode, d.old_blob.as_ref()), (Mode::ABSENT, None));
    assert_eq!(classify_change(a), None);
}

#[test]
fn a_rename_is_a_deletion_plus_an_addition_even_with_rename_config() {
    let repo = TestRepo::new();
    repo.write("old.txt", b"same content\n");
    repo.commit_all("base");
    repo.run(&["config", "diff.renames", "true"]);
    repo.run(&["mv", "old.txt", "new.txt"]);
    repo.run(&["commit", "-q", "-m", "rename"]);
    let changes = changed_paths(
        &repo.repo(),
        &tree(&repo, "HEAD~1"),
        &tree(&repo, "HEAD"),
        &[],
    )
    .unwrap();
    assert_eq!(
        summary(&changes),
        [
            ("new.txt".to_owned(), ChangeStatus::Added),
            ("old.txt".to_owned(), ChangeStatus::Deleted),
        ]
    );
}

#[test]
fn pathspecs_are_literal_prefixes() {
    let repo = TestRepo::new();
    repo.write("src/a.rs", b"a\n");
    repo.write("src*/b.rs", b"b\n");
    repo.write("docs/c.md", b"c\n");
    repo.commit_all("base");
    for path in ["src/a.rs", "src*/b.rs", "docs/c.md"] {
        repo.write(path, b"changed\n");
    }
    repo.commit_all("target");
    let only = |spec: &str| {
        let changes = changed_paths(
            &repo.repo(),
            &tree(&repo, "HEAD~1"),
            &tree(&repo, "HEAD"),
            &[RepoPath::from_utf8(spec)],
        )
        .unwrap();
        summary(&changes)
            .into_iter()
            .map(|(p, _)| p)
            .collect::<Vec<_>>()
    };
    assert_eq!(only("src"), ["src/a.rs"]);
    assert_eq!(only("src*"), ["src*/b.rs"]);
}

#[test]
fn a_gitlink_change_is_listed_for_a_submodule_configured_ignore_all() {
    let repo = TestRepo::new();
    repo.write(
        ".gitmodules",
        b"[submodule \"sm\"]\n\tpath = sm\n\turl = ./sm\n\tignore = all\n",
    );
    let old = "1".repeat(40);
    let new = "2".repeat(40);
    repo.run(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{old},sm"),
    ]);
    repo.run(&["add", ".gitmodules"]);
    repo.run(&["commit", "-q", "-m", "old gitlink"]);
    repo.run(&["update-index", "--cacheinfo", &format!("160000,{new},sm")]);
    repo.run(&["commit", "-q", "-m", "new gitlink"]);
    let (old_tree, new_tree) = (tree(&repo, "HEAD~1"), tree(&repo, "HEAD"));
    // Plain diff-tree hides the change; this is why the flag is passed.
    let plain = repo.run(&[
        "diff-tree",
        "-r",
        "--raw",
        old_tree.as_str(),
        new_tree.as_str(),
    ]);
    assert_eq!(plain, "");
    let changes = changed_paths(&repo.repo(), &old_tree, &new_tree, &[]).unwrap();
    assert_eq!(
        summary(&changes),
        [("sm".to_owned(), ChangeStatus::Modified)]
    );
    assert_eq!(changes[0].old_mode, Mode::GITLINK);
    assert_eq!(classify_change(&changes[0]), Some(FileKind::Gitlink));
}

#[cfg(unix)]
#[test]
fn symlinks_and_type_changes_are_classified() {
    let repo = TestRepo::new();
    repo.write("target.txt", b"t\n");
    repo.write("was-file", b"f\n");
    repo.commit_all("base");
    std::os::unix::fs::symlink("target.txt", repo.root.join("link")).unwrap();
    std::fs::remove_file(repo.root.join("was-file")).unwrap();
    std::os::unix::fs::symlink("target.txt", repo.root.join("was-file")).unwrap();
    repo.commit_all("target");
    let changes = changed_paths(
        &repo.repo(),
        &tree(&repo, "HEAD~1"),
        &tree(&repo, "HEAD"),
        &[],
    )
    .unwrap();
    let kinds: Vec<(String, ChangeStatus, Option<FileKind>)> = changes
        .iter()
        .map(|c| (c.path.display(), c.status, classify_change(c)))
        .collect();
    assert_eq!(
        kinds,
        [
            (
                "link".to_owned(),
                ChangeStatus::Added,
                Some(FileKind::Symlink)
            ),
            (
                "was-file".to_owned(),
                ChangeStatus::TypeChanged,
                Some(FileKind::TypeChange)
            ),
        ]
    );
}

#[test]
fn a_non_utf8_path_in_a_tree_is_parsed_and_unsupported() {
    let repo = TestRepo::new();
    let blob = repo.run_input(&["hash-object", "-w", "--stdin"], b"odd\n");
    let index = repo.dir.path().join("odd-index");
    let mut record = format!("100644 {}\t", blob.trim()).into_bytes();
    record.extend_from_slice(b"odd\xffname.txt\0");
    let git = repo.git().with_env("GIT_INDEX_FILE", &index);
    git.output_with_input(["update-index", "--add", "-z", "--index-info"], &record)
        .unwrap();
    let written = id(&String::from_utf8(git.output(["write-tree"]).unwrap()).unwrap());
    let opened = repo.repo();
    let changes = changed_paths(&opened, &opened.empty_tree().unwrap(), &written, &[]).unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path.as_bytes(), b"odd\xffname.txt");
    assert_eq!(changes[0].path.escaped(), r"odd\xffname.txt");
    assert_eq!(
        classify_change(&changes[0]),
        Some(FileKind::UnsupportedPath)
    );
}

#[test]
fn blobs_are_read_in_one_batch_with_exact_bytes_and_sizes() {
    let repo = TestRepo::new();
    let contents: [&[u8]; 3] = [b"line\nwith\nnewlines\n", b"bin\0ary\n\0", b""];
    let ids: Vec<ObjectId> = contents
        .iter()
        .map(|c| id(&repo.run_input(&["hash-object", "-w", "--stdin"], c)))
        .collect();
    let opened = repo.repo();
    let blobs = read_blobs(&opened, &ids).unwrap();
    let sizes = blob_sizes(&opened, &ids).unwrap();
    for (id, content) in ids.iter().zip(contents) {
        assert_eq!(blobs[id], content);
        assert_eq!(sizes[id], content.len() as u64);
    }
}

#[test]
fn a_missing_object_is_an_error() {
    let repo = TestRepo::new();
    let missing = id(&"3".repeat(40));
    let opened = repo.repo();
    assert!(matches!(
        read_blobs(&opened, std::slice::from_ref(&missing)),
        Err(GitError::Parse(_))
    ));
    assert!(matches!(
        blob_sizes(&opened, &[missing]),
        Err(GitError::Parse(_))
    ));
}

#[test]
fn blobs_within_the_limit_are_read_and_larger_ones_only_sized() {
    let repo = TestRepo::new();
    let contents: [&[u8]; 4] = [b"", b"abc", b"abcd", b"bin\0ary\n\0"];
    let ids: Vec<ObjectId> = contents
        .iter()
        .map(|c| id(&repo.run_input(&["hash-object", "-w", "--stdin"], c)))
        .collect();
    let opened = repo.repo();
    // The limit is inclusive: a blob of exactly `max` bytes is read.
    let blobs = read_blobs_within(&opened, &ids, 3).unwrap();
    assert_eq!(blobs.len(), ids.len());
    assert_eq!(blobs[&ids[0]], Blob::Read(Vec::new()));
    assert_eq!(blobs[&ids[1]], Blob::Read(b"abc".to_vec()));
    assert_eq!(blobs[&ids[2]], Blob::TooLarge(4));
    assert_eq!(blobs[&ids[3]], Blob::TooLarge(9));
}

#[test]
fn blobs_within_list_each_id_once_and_need_none() {
    let repo = TestRepo::new();
    let blob = id(&repo.run_input(&["hash-object", "-w", "--stdin"], b"a\n"));
    let opened = repo.repo();
    let blobs = read_blobs_within(&opened, &[blob.clone(), blob.clone()], 2).unwrap();
    assert_eq!(blobs.len(), 1);
    assert_eq!(blobs[&blob], Blob::Read(b"a\n".to_vec()));
    assert!(read_blobs_within(&opened, &[], 2).unwrap().is_empty());
}

#[test]
fn blobs_within_a_limit_still_fail_on_a_missing_object() {
    let repo = TestRepo::new();
    let missing = id(&"3".repeat(40));
    let opened = repo.repo();
    // Missing whether it would be read or only sized.
    for max in [0, u64::MAX] {
        assert!(matches!(
            read_blobs_within(&opened, std::slice::from_ref(&missing), max),
            Err(GitError::Parse(_))
        ));
    }
}

#[test]
fn the_worktree_form_is_identical_without_conversion() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\nb\n");
    repo.commit_all("base");
    let blob = id(&repo.run(&["rev-parse", "HEAD:a.txt"]));
    let opened = repo.repo();
    let bytes = read_blobs(&opened, std::slice::from_ref(&blob))
        .unwrap()
        .remove(&blob)
        .unwrap();
    assert_eq!(
        worktree_form(
            &opened,
            &RepoPath::from_utf8("a.txt"),
            &blob,
            &bytes,
            NO_LIMIT
        )
        .unwrap(),
        Conversion::Identical
    );
}

#[test]
fn the_worktree_form_converts_line_endings_under_autocrlf() {
    let repo = TestRepo::new();
    repo.run(&["config", "core.autocrlf", "true"]);
    repo.write("a.txt", b"a\r\nb\r\n");
    repo.commit_all("crlf on disk");
    let blob = id(&repo.run(&["rev-parse", "HEAD:a.txt"]));
    let opened = repo.repo();
    let bytes = read_blobs(&opened, std::slice::from_ref(&blob))
        .unwrap()
        .remove(&blob)
        .unwrap();
    assert_eq!(bytes, b"a\nb\n");
    assert_eq!(
        worktree_form(
            &opened,
            &RepoPath::from_utf8("a.txt"),
            &blob,
            &bytes,
            NO_LIMIT
        )
        .unwrap(),
        Conversion::Converted(b"a\r\nb\r\n".to_vec())
    );
}

#[test]
fn the_worktree_form_follows_an_eol_attribute() {
    let repo = TestRepo::new();
    repo.write(".gitattributes", b"*.txt eol=crlf\n");
    repo.write("a.txt", b"a\nb\n");
    repo.commit_all("eol attribute");
    let blob = id(&repo.run(&["rev-parse", "HEAD:a.txt"]));
    let opened = repo.repo();
    assert_eq!(
        worktree_form(
            &opened,
            &RepoPath::from_utf8("a.txt"),
            &blob,
            b"a\nb\n",
            NO_LIMIT
        )
        .unwrap(),
        Conversion::Converted(b"a\r\nb\r\n".to_vec())
    );
}

#[cfg(unix)]
#[test]
fn a_failed_required_filter_is_reported_not_raised() {
    let repo = TestRepo::new();
    repo.write(".gitattributes", b"*.bad filter=boom\n");
    repo.run(&["config", "filter.boom.clean", "cat"]);
    repo.run(&["config", "filter.boom.smudge", "false"]);
    repo.run(&["config", "filter.boom.required", "true"]);
    let blob = id(&repo.run_input(&["hash-object", "-w", "--stdin"], b"x\n"));
    let result = worktree_form(
        &repo.repo(),
        &RepoPath::from_utf8("x.bad"),
        &blob,
        b"x\n",
        NO_LIMIT,
    )
    .unwrap();
    assert!(
        matches!(&result, Conversion::Failed { status: Some(code), stderr_tail } if *code != 0 && !stderr_tail.is_empty()),
        "{result:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_conversion_past_the_limit_is_too_large() {
    let repo = TestRepo::new();
    repo.write(".gitattributes", b"*.grow filter=grow\n");
    // Repeats its input a thousand times: `x\n` becomes 2,000 bytes.
    repo.run(&[
        "config",
        "filter.grow.smudge",
        "x=$(cat); yes \"$x\" | head -n 1000",
    ]);
    let blob = id(&repo.run_input(&["hash-object", "-w", "--stdin"], b"x\n"));
    let opened = repo.repo();
    let path = RepoPath::from_utf8("x.grow");
    // The limit is inclusive: a worktree form of exactly `max` bytes is read.
    assert_eq!(
        worktree_form(&opened, &path, &blob, b"x\n", 2_000).unwrap(),
        Conversion::Converted(b"x\n".repeat(1_000))
    );
    assert_eq!(
        worktree_form(&opened, &path, &blob, b"x\n", 1_999).unwrap(),
        Conversion::TooLarge
    );
}

#[test]
fn a_failing_diff_tree_is_an_error() {
    let repo = TestRepo::new();
    let missing = id(&"4".repeat(40));
    let opened = repo.repo();
    assert!(matches!(
        changed_paths(&opened, &missing, &missing, &[]),
        Err(GitError::Failed { .. })
    ));
}

#[test]
fn a_missing_blob_is_an_error_not_a_failed_conversion() {
    let repo = TestRepo::new();
    let missing = id(&"5".repeat(40));
    let result = worktree_form(
        &repo.repo(),
        &RepoPath::from_utf8("a.txt"),
        &missing,
        b"a\n",
        NO_LIMIT,
    );
    assert!(matches!(result, Err(GitError::Parse(_))), "{result:?}");
}
