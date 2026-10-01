//! Involved records and bounded reconcile, tested against the library.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use supersigil_cli::reconcile::{LOCK_WAIT, involved_records, reconcile, transcripts_below};
use supersigil_cli::record_dir::open_or_create_record;
use supersigil_record::store::{Association, Store};
use supersigil_session::checkout::canonical;
use supersigil_session::discover::encode_project_dir;
use supersigil_session::sync::sync;

/// A records directory, a Claude home, and a worktree directory, all under
/// one canonical temporary directory.
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    records: PathBuf,
    home: PathBuf,
    repo: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = canonical(dir.path()).unwrap();
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    Fixture {
        records: root.join("records"),
        home: root.join("claude"),
        repo,
        root,
        _dir: dir,
    }
}

/// A two-record transcript of `session` working in `cwd`.
fn transcript(session: &str, cwd: &Path) -> String {
    let cwd = cwd.to_string_lossy();
    let line = |uuid: &str, parent: Option<&str>, kind: &str, content: serde_json::Value| {
        let mut text = serde_json::json!({
            "type": kind,
            "uuid": uuid,
            "parentUuid": parent,
            "sessionId": session,
            "cwd": cwd,
            "timestamp": format!("2026-09-29T10:00:0{}.000Z", uuid.len()),
            "isSidechain": false,
            "message": {"role": kind, "content": content},
        })
        .to_string();
        text.push('\n');
        text
    };
    line("u1", None, "user", serde_json::json!("hello"))
        + &line(
            "a1",
            Some("u1"),
            "assistant",
            serde_json::json!([{"type": "text", "text": "hi"}]),
        )
}

/// Writes `text` as `<name>` in the Claude Code project directory of `cwd`.
fn project_transcript(home: &Path, cwd: &Path, name: &str, text: &str) -> PathBuf {
    let dir = home.join("projects").join(encode_project_dir(cwd));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn sessions(store: &Store) -> Vec<String> {
    store
        .snapshot()
        .unwrap()
        .sessions()
        .iter()
        .map(|s| s.as_str().to_owned())
        .collect()
}

fn create_record(records: &Path, name: &str, checkout: &Path) -> Store {
    Store::create(
        &records.join(name),
        Association {
            checkout: checkout.to_path_buf(),
        },
    )
    .unwrap()
}

#[test]
fn involved_records_follow_associations() {
    let f = fixture();
    let src = f.repo.join("src");
    let other = f.root.join("other");
    let exact = create_record(&f.records, "a", &f.repo);
    let inside = create_record(&f.records, "b", &src);
    let containing = create_record(&f.records, "c", &f.root);
    create_record(&f.records, "d", &other);

    let roots: Vec<PathBuf> = involved_records(&f.records, std::slice::from_ref(&f.repo))
        .unwrap()
        .iter()
        .map(|s| s.root().to_path_buf())
        .collect();
    assert_eq!(
        roots,
        vec![
            exact.root().to_path_buf(),
            inside.root().to_path_buf(),
            containing.root().to_path_buf(),
        ]
    );
    assert!(
        involved_records(&f.root.join("no-records"), std::slice::from_ref(&f.repo))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn subdirectory_record_is_involved() {
    let f = fixture();
    let src = f.repo.join("src");
    std::fs::create_dir_all(&src).unwrap();
    // What `session sync --checkout <repo>/src` leaves behind.
    let store = open_or_create_record(&f.records, &src).unwrap();
    project_transcript(&f.home, &src, "sub.jsonl", &transcript("s-sub", &src));

    let result = reconcile(
        &f.records,
        Some(&f.home),
        std::slice::from_ref(&f.repo),
        &f.repo,
        LOCK_WAIT,
    )
    .unwrap();

    assert_eq!(result.records.len(), 1);
    let involved = &result.records[0];
    assert_eq!(involved.store.root(), store.root());
    assert_eq!(involved.record_id, store.manifest().unwrap().record_id);
    assert_eq!(involved.not_reconciled, None);
    assert_eq!(sessions(&involved.store), vec!["s-sub"]);
    assert!(result.unreconciled.is_empty());
}

#[test]
fn two_records_cover_one_worktree() {
    let f = fixture();
    let a = f.repo.join("a");
    let b = f.repo.join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    open_or_create_record(&f.records, &a).unwrap();
    open_or_create_record(&f.records, &b).unwrap();
    project_transcript(&f.home, &a, "a.jsonl", &transcript("s-a", &a));
    project_transcript(&f.home, &b, "b.jsonl", &transcript("s-b", &b));

    let result = reconcile(
        &f.records,
        Some(&f.home),
        std::slice::from_ref(&f.repo),
        &f.repo,
        LOCK_WAIT,
    )
    .unwrap();

    let mut all: Vec<String> = result
        .records
        .iter()
        .flat_map(|r| sessions(&r.store))
        .collect();
    all.sort();
    assert_eq!(result.records.len(), 2);
    assert_eq!(all, vec!["s-a", "s-b"]);
}

#[test]
fn cursor_only_transcript_is_resynced() {
    let f = fixture();
    let elsewhere = f.root.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let path = elsewhere.join("t.jsonl");
    std::fs::write(&path, transcript("s-cursor", &f.repo)).unwrap();
    let store = open_or_create_record(&f.records, &f.repo).unwrap();
    sync(&store, &f.repo, std::slice::from_ref(&path)).unwrap();
    let before = store.manifest().unwrap();

    let mut more = std::fs::read_to_string(&path).unwrap();
    more.push_str(
        &serde_json::json!({
            "type": "user",
            "uuid": "u2",
            "parentUuid": "a1",
            "sessionId": "s-cursor",
            "cwd": f.repo.to_string_lossy(),
            "timestamp": "2026-09-29T10:01:00.000Z",
            "isSidechain": false,
            "message": {"role": "user", "content": "again"},
        })
        .to_string(),
    );
    more.push('\n');
    std::fs::write(&path, more).unwrap();

    // No Claude home: the transcript is reachable only through the cursor.
    let result = reconcile(
        &f.records,
        None,
        std::slice::from_ref(&f.repo),
        &f.repo,
        LOCK_WAIT,
    )
    .unwrap();

    let after = result.records[0].store.manifest().unwrap();
    let key = canonical(&path).unwrap().display().to_string();
    assert!(after.revision > before.revision);
    assert!(after.cursors[&key].offset > before.cursors[&key].offset);
}

#[test]
fn transcripts_below_finds_the_worktree_and_nested_project_dirs() {
    let f = fixture();
    let nested = f.repo.join(".claude").join("worktrees").join("x");
    let lookalike = f.root.join("repo-other");
    let own = project_transcript(&f.home, &f.repo, "a.jsonl", "");
    let inner = project_transcript(&f.home, &nested, "b.jsonl", "");
    let similar = project_transcript(&f.home, &lookalike, "c.jsonl", "");
    project_transcript(&f.home, &f.root.join("unrelated"), "d.jsonl", "");

    assert_eq!(
        transcripts_below(&f.home, &f.repo).unwrap(),
        vec![own, inner, similar]
    );
    assert!(
        transcripts_below(&f.root.join("no-home"), &f.repo)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn unowned_nested_transcript_gets_a_record() {
    let f = fixture();
    let nested = f.repo.join(".claude").join("worktrees").join("x");
    std::fs::create_dir_all(&nested).unwrap();
    project_transcript(
        &f.home,
        &nested,
        "n.jsonl",
        &transcript("s-nested", &nested),
    );

    let result = reconcile(
        &f.records,
        Some(&f.home),
        std::slice::from_ref(&f.repo),
        &f.repo,
        LOCK_WAIT,
    )
    .unwrap();

    assert_eq!(result.records.len(), 1);
    assert_eq!(sessions(&result.records[0].store), vec!["s-nested"]);
    let associations = result.records[0].store.manifest().unwrap().associations;
    assert_eq!(associations.len(), 1);
    assert_eq!(associations[0].checkout, nested);
}

#[test]
fn lookalike_project_dir_is_not_admitted() {
    let f = fixture();
    let lookalike = f.root.join("repo-other");
    std::fs::create_dir_all(&lookalike).unwrap();
    project_transcript(
        &f.home,
        &lookalike,
        "o.jsonl",
        &transcript("s-other", &lookalike),
    );

    let result = reconcile(
        &f.records,
        Some(&f.home),
        std::slice::from_ref(&f.repo),
        &f.repo,
        LOCK_WAIT,
    )
    .unwrap();

    assert!(result.records.is_empty());
    assert!(result.unreconciled.is_empty());
}

#[test]
fn writer_lock_contention_is_reported() {
    let f = fixture();
    let store = open_or_create_record(&f.records, &f.repo).unwrap();
    project_transcript(&f.home, &f.repo, "t.jsonl", &transcript("s-held", &f.repo));
    // The stop hook, say, holding the record's writer lock.
    let held = store.begin().unwrap();

    let started = Instant::now();
    let result = reconcile(
        &f.records,
        Some(&f.home),
        std::slice::from_ref(&f.repo),
        &f.repo,
        Duration::from_millis(200),
    )
    .unwrap();

    // A watchdog only: timing is never asserted against the retry bound.
    assert!(started.elapsed() < Duration::from_secs(30));
    assert_eq!(result.records.len(), 1);
    assert_eq!(
        result.records[0].not_reconciled.as_deref(),
        Some("locked by another writer")
    );
    assert!(sessions(&result.records[0].store).is_empty());
    drop(held);
}

#[test]
fn records_dir_lock_contention_is_reported() {
    let f = fixture();
    project_transcript(&f.home, &f.repo, "t.jsonl", &transcript("s-new", &f.repo));
    std::fs::create_dir_all(&f.records).unwrap();
    let guard = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(f.records.join(".lock"))
        .unwrap();
    guard.lock().unwrap();

    let started = Instant::now();
    let result = reconcile(
        &f.records,
        Some(&f.home),
        std::slice::from_ref(&f.repo),
        &f.repo,
        Duration::from_millis(200),
    )
    .unwrap();

    assert!(started.elapsed() < Duration::from_secs(30));
    assert!(result.records.is_empty());
    assert_eq!(result.unreconciled.len(), 1);
    let unreconciled = &result.unreconciled[0];
    assert_eq!(unreconciled.checkout, f.repo);
    assert_eq!(unreconciled.transcripts, 1);
    assert_eq!(
        unreconciled.reason,
        "records directory locked by another writer"
    );
    drop(guard);
}

#[test]
fn ancestor_project_dir_is_reconciled_for_a_nested_worktree() {
    let f = fixture();
    let nested = f.repo.join(".claude").join("worktrees").join("x");
    std::fs::create_dir_all(&nested).unwrap();
    // A session in the main checkout: its project directory is named after
    // the main checkout, not after the nested worktree under review.
    project_transcript(
        &f.home,
        &f.repo,
        "main.jsonl",
        &transcript("s-main", &f.repo),
    );
    // Above the main worktree is outside the repository: never read.
    project_transcript(
        &f.home,
        &f.root,
        "above.jsonl",
        &transcript("s-above", &f.root),
    );

    let result = reconcile(
        &f.records,
        Some(&f.home),
        std::slice::from_ref(&nested),
        &f.repo,
        LOCK_WAIT,
    )
    .unwrap();

    assert_eq!(result.records.len(), 1);
    assert_eq!(sessions(&result.records[0].store), vec!["s-main"]);
    let associations = result.records[0].store.manifest().unwrap().associations;
    assert_eq!(associations[0].checkout, f.repo);
    assert!(result.unreconciled.is_empty());
}

/// Makes `dir` unreadable, or returns `false` when the process can still
/// read it (running as root), so the caller can skip a test that would
/// otherwise prove nothing.
#[cfg(unix)]
fn deny_access(dir: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o000)).unwrap();
    std::fs::metadata(dir.join("t.jsonl")).is_err()
}

#[cfg(unix)]
#[test]
fn unreadable_cursor_transcript_is_an_error_not_an_absence() {
    use std::os::unix::fs::PermissionsExt;
    let f = fixture();
    let elsewhere = f.root.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let path = elsewhere.join("t.jsonl");
    std::fs::write(&path, transcript("s-cursor", &f.repo)).unwrap();
    let store = open_or_create_record(&f.records, &f.repo).unwrap();
    sync(&store, &f.repo, std::slice::from_ref(&path)).unwrap();
    assert!(!store.manifest().unwrap().cursors.is_empty());

    let denied = deny_access(&elsewhere);
    let result = reconcile(
        &f.records,
        None,
        std::slice::from_ref(&f.repo),
        &f.repo,
        LOCK_WAIT,
    );
    std::fs::set_permissions(&elsewhere, std::fs::Permissions::from_mode(0o755)).unwrap();
    if denied {
        assert!(result.is_err(), "an unreadable transcript was skipped");
    } else {
        eprintln!("skipped: this process ignores file permissions");
    }
}
