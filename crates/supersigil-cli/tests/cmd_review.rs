//! `supersigil review` and `supersigil why` end to end, against temporary
//! git repositories and fixture transcripts.

mod common;

use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use assert_cmd::assert::OutputAssertExt;
use predicates::prelude::*;
use serde_json::{Value, json};
use supersigil_record::store::Store;
use supersigil_record::{EventId, SessionId};

/// Session id of the slice fixture.
const SLICE_SESSION: &str = "11111111-1111-4111-8111-111111111111";
/// `src/lib.rs` before the slice session.
const LIB_BASE: &str = "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n";
/// `src/lib.rs` after the session and the manual line it never recorded.
const LIB_FINAL: &str = "// manual\npub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\npub fn greet(name: &str) -> String {\n    format!(\"Hello, {name}!\")\n}\n";
/// `src/new.rs` as the slice session wrote it.
const NEW_RS: &str = "pub const VERSION: &str = \"1\";\n";

/// A temporary repository, Claude home, and records directory, with git
/// isolated from the machine's configuration.
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    repo: PathBuf,
    claude: PathBuf,
    records: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let (dir, root) = common::canonical_tempdir();
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let fixture = Self {
            claude: root.join("claude"),
            records: root.join("records"),
            repo,
            root,
            _dir: dir,
        };
        fixture.git(&["init", "-q", "-b", "main"]);
        fixture.git(&["config", "core.autocrlf", "false"]);
        fixture
    }

    /// Git isolation and the records directory. The
    /// variables git sets for hooks are removed, as `supersigil_git::Git`
    /// removes them: a test run from a pre-commit hook would otherwise
    /// point git at the outer repository's index. The variables that pass
    /// configuration through the environment are removed too, so neither
    /// git here nor the git supersigil starts reads any.
    fn isolate(&self, command: &mut Command) {
        for key in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_COMMON_DIR",
            "GIT_CONFIG_GLOBAL",
            "GIT_CONFIG_PARAMETERS",
            "GIT_CONFIG_COUNT",
        ] {
            command.env_remove(key);
        }
        command
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", &self.root)
            .env("XDG_CONFIG_HOME", self.root.join("xdg"))
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .env("SUPERSIGIL_RECORD_DIR", &self.records)
            .env_remove("XDG_DATA_HOME");
    }

    /// Runs git in `dir`; panics with its stderr on failure.
    fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        let mut command = Command::new("git");
        self.isolate(&mut command);
        let output = command.current_dir(dir).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.repo, args)
    }

    /// Stages everything in `dir`, commits, and returns the commit id.
    fn commit(&self, dir: &Path, message: &str) -> String {
        self.git_in(dir, &["add", "-A"]);
        self.git_in(dir, &["commit", "-q", "-m", message]);
        self.git_in(dir, &["rev-parse", "HEAD"]).trim().to_owned()
    }

    /// Writes `text` as transcript `name` in the Claude Code project
    /// directory of `cwd`, with the fixtures' `/work/repo` replaced by `cwd`
    /// (escaped for a JSON string: a Windows path has backslashes), and
    /// returns its path.
    fn transcript(&self, cwd: &Path, name: &str, text: &str) -> PathBuf {
        let quoted = serde_json::to_string(&cwd.to_string_lossy()).unwrap();
        let text = text.replace("/work/repo", &quoted[1..quoted.len() - 1]);
        common::project_transcript(&self.claude, cwd, name, &text)
    }

    /// `supersigil <args>` in `dir`, isolated, as typed.
    fn plain(&self, dir: &Path, args: &[&str]) -> Command {
        let mut command = common::supersigil_cmd();
        self.isolate(&mut command);
        command.current_dir(dir).args(args);
        command
    }

    /// `supersigil <subcommand> --claude-home <claude> <rest>` in `dir`.
    fn supersigil(&self, dir: &Path, args: &[&str]) -> Command {
        let mut command = common::supersigil_cmd();
        self.isolate(&mut command);
        command
            .current_dir(dir)
            .arg(args[0])
            .arg("--claude-home")
            .arg(&self.claude)
            .args(&args[1..]);
        command
    }

    /// Runs a command expected to succeed and parses its JSON output.
    fn json(&self, dir: &Path, args: &[&str]) -> Value {
        let output = self.supersigil(dir, args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    /// Runs a command with `<key>=1` in supersigil's own environment, which
    /// every git it starts inherits unless supersigil removes it. Returns
    /// the parsed JSON output, or standard error when the command fails.
    fn json_inheriting(&self, key: &str, dir: &Path, args: &[&str]) -> Result<Value, String> {
        let output = self.supersigil(dir, args).env(key, "1").output().unwrap();
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        Ok(serde_json::from_slice(&output.stdout).unwrap())
    }
}

/// The variables that set how git reads every pathspec of a command. A
/// parent environment can hold any of them; supersigil builds its own
/// literal pathspecs.
const PATHSPEC_MODES: [&str; 4] = [
    "GIT_LITERAL_PATHSPECS",
    "GIT_GLOB_PATHSPECS",
    "GIT_NOGLOB_PATHSPECS",
    "GIT_ICASE_PATHSPECS",
];

/// `check(mode)` for each of [`PATHSPEC_MODES`], paired with the mode, so
/// an assertion over the whole list shows every mode that differs.
fn under_each_pathspec_mode<T>(check: impl Fn(&str) -> T) -> Vec<(&'static str, T)> {
    PATHSPEC_MODES
        .iter()
        .map(|&mode| (mode, check(mode)))
        .collect()
}

/// The paths a review lists, in order.
fn paths(review: &Value) -> Vec<String> {
    review["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap().to_owned())
        .collect()
}

/// The untracked paths a review's scope excluded, in order.
fn excluded(review: &Value) -> Vec<String> {
    review["scope"]["untracked_excluded"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["path"].as_str().unwrap().to_owned())
        .collect()
}

/// Writes `content` to `path` under `dir`, creating directories.
fn write(dir: &Path, path: &str, content: &str) {
    let path = dir.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// Builds the records of one generated session, each chained to the last.
struct Session<'a> {
    id: &'a str,
    cwd: &'a Path,
    text: String,
    count: u32,
    parent: Option<String>,
}

impl<'a> Session<'a> {
    fn new(id: &'a str, cwd: &'a Path) -> Self {
        Self {
            id,
            cwd,
            text: String::new(),
            count: 0,
            parent: None,
        }
    }

    fn push(&mut self, kind: &str, content: &Value, result: Option<Value>) -> &mut Self {
        let uuid = format!("{}-{}", self.id, self.count);
        let mut value = json!({
            "type": kind, "uuid": uuid, "parentUuid": self.parent, "sessionId": self.id,
            "cwd": self.cwd.to_string_lossy(),
            "timestamp": format!("2026-09-29T10:00:{:02}.000Z", self.count),
            "isSidechain": false, "message": {"role": kind, "content": content},
        });
        if let Some(result) = result {
            value["toolUseResult"] = result;
        }
        self.text.push_str(&value.to_string());
        self.text.push('\n');
        self.parent = Some(uuid);
        self.count += 1;
        self
    }

    fn prompt(&mut self, text: &str) -> &mut Self {
        self.push("user", &json!(text), None)
    }

    /// An Edit call on a file over 10 KB and its result, as Claude Code
    /// records it: no `originalFile`, and the patch of replacing `old` with
    /// `new` in `before`, the file as the edit found it.
    fn edit(&mut self, tool: &str, file: &Path, before: &str, old: &str, new: &str) -> &mut Self {
        let file = file.to_string_lossy();
        let call = json!([{"type": "tool_use", "id": tool, "name": "Edit",
            "input": {"file_path": file, "old_string": old, "new_string": new}}]);
        let done = json!([{"type": "tool_result", "tool_use_id": tool, "content": "updated"}]);
        let result = json!({"filePath": file, "oldString": old, "newString": new,
            "originalFile": null, "structuredPatch": structured_patch(before, old, new),
            "userModified": false, "replaceAll": false});
        self.push("assistant", &call, None)
            .push("user", &done, Some(result))
    }

    /// A Write call that creates `file`, and its result.
    fn create(&mut self, tool: &str, file: &Path, content: &str) -> &mut Self {
        let file = file.to_string_lossy();
        let call = json!([{"type": "tool_use", "id": tool, "name": "Write",
            "input": {"file_path": file, "content": content}}]);
        let done = json!([{"type": "tool_result", "tool_use_id": tool, "content": "File created"}]);
        let result =
            json!({"type": "create", "filePath": file, "content": content, "structuredPatch": []});
        self.push("assistant", &call, None)
            .push("user", &done, Some(result))
    }

    /// A Bash call running `command` and its result, as Claude Code records
    /// it: with `report` as the result's `bashEditDiff` when there is one.
    fn bash(&mut self, tool: &str, command: &str, report: Option<Value>) -> &mut Self {
        let call = json!([{"type": "tool_use", "id": tool, "name": "Bash",
            "input": {"command": command}}]);
        let done = json!([{"type": "tool_result", "tool_use_id": tool, "content": ""}]);
        let mut result = json!({"stdout": "", "stderr": "", "interrupted": false,
            "isImage": false, "noOutputExpected": false});
        if let Some(report) = report {
            result["bashEditDiff"] = report;
        }
        self.push("assistant", &call, None)
            .push("user", &done, Some(result))
    }

    /// A `NotebookEdit` call, which the capture cannot read, and its result.
    fn notebook_edit(&mut self, tool: &str) -> &mut Self {
        let notebook = self.cwd.join("nb.ipynb");
        let call = json!([{"type": "tool_use", "id": tool, "name": "NotebookEdit",
            "input": {"notebook_path": notebook.to_string_lossy(), "new_source": "x"}}]);
        let done = json!([{"type": "tool_result", "tool_use_id": tool, "content": "ok"}]);
        self.push("assistant", &call, None)
            .push("user", &done, None)
    }

    fn text(&self) -> String {
        self.text.clone()
    }
}

/// Claude Code's `structuredPatch` for replacing `old` with `new` once in
/// `before`: one hunk with three lines of context, as jsdiff builds it, with
/// tabs shown as two spaces.
fn structured_patch(before: &str, old: &str, new: &str) -> Value {
    let after = before.replacen(old, new, 1);
    let old_lines: Vec<&str> = before.lines().collect();
    let new_lines: Vec<&str> = after.lines().collect();
    let prefix = old_lines
        .iter()
        .zip(&new_lines)
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = old_lines[prefix..]
        .iter()
        .rev()
        .zip(new_lines[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (old_changed, new_changed) = (old_lines.len() - suffix, new_lines.len() - suffix);
    let start = prefix.saturating_sub(3);
    let old_end = (old_changed + 3).min(old_lines.len());
    let shown = |marker: &str, lines: &[&str]| -> Vec<String> {
        lines
            .iter()
            .map(|line| format!("{marker}{}", line.replace('\t', "  ")))
            .collect()
    };
    let mut lines = shown(" ", &old_lines[start..prefix]);
    lines.extend(shown("-", &old_lines[prefix..old_changed]));
    lines.extend(shown("+", &new_lines[prefix..new_changed]));
    lines.extend(shown(" ", &old_lines[old_changed..old_end]));
    let after_context = old_end - old_changed;
    json!([{"oldStart": start + 1, "oldLines": old_end - start,
        "newStart": start + 1, "newLines": new_changed + after_context - start,
        "lines": lines}])
}

fn slice_text() -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../supersigil-session/tests/fixtures/slice.jsonl"),
    )
    .unwrap()
}

/// The slice fixture: the files the session found, committed,
/// then the working tree after the session plus the manual line it never
/// recorded. `old.txt` appears only in a command, so it is seeded here.
fn slice(f: &Fixture) {
    write(&f.repo, "src/lib.rs", LIB_BASE);
    write(&f.repo, "notes.txt", "draft\n");
    write(&f.repo, "old.txt", "obsolete\n");
    f.commit(&f.repo, "base");
    write(&f.repo, "src/lib.rs", LIB_FINAL);
    write(&f.repo, "src/new.rs", NEW_RS);
    std::fs::remove_file(f.repo.join("old.txt")).unwrap();
    f.transcript(&f.repo, "slice.jsonl", &slice_text());
}

/// A limitations-only transcript: one `NotebookEdit` call, no edit recorded.
fn limits(f: &Fixture) {
    let text = Session::new("s-limits", &f.repo)
        .prompt("Edit the notebook.")
        .notebook_edit("toolu_nb")
        .text();
    f.transcript(&f.repo, "limits.jsonl", &text);
}

fn edit_id(session: &str, tool: &str) -> String {
    EventId::derive("edit", &SessionId::new(session), tool)
        .as_str()
        .to_owned()
}

fn file<'v>(review: &'v Value, path: &str) -> &'v Value {
    review["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == path)
        .unwrap_or_else(|| panic!("no file {path} in {review:#}"))
}

/// `(line text, outcome)` for every diff line of `file` on `side`
/// (`"target"` for added lines, `"base"` for removed ones), in order.
fn side_lines(file: &Value, side: &str) -> Vec<(String, Value)> {
    let (start_key, texts_key) = if side == "target" {
        ("target_start", "added")
    } else {
        ("base_start", "removed")
    };
    let mut lines = Vec::new();
    for hunk in file["hunks"].as_array().unwrap() {
        let first = hunk[start_key].as_u64().unwrap();
        for span in hunk["spans"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["side"] == side)
        {
            for offset in 0..span["count"].as_u64().unwrap() {
                let index =
                    usize::try_from(span["start"].as_u64().unwrap() + offset - first).unwrap();
                let text = hunk[texts_key][index].as_str().unwrap().to_owned();
                lines.push((text, span["outcome"].clone()));
            }
        }
    }
    lines
}

/// Every edit id `value` names, outside conflict references and mentions.
fn edit_ids(value: &Value, ids: &mut BTreeSet<String>) {
    match value {
        Value::String(text) if text.starts_with("edit:") => {
            ids.insert(text.clone());
        }
        Value::Array(items) => items.iter().for_each(|item| edit_ids(item, ids)),
        Value::Object(map) => {
            for (key, item) in map {
                if key != "conflicting_edits" && key != "mentions" {
                    edit_ids(item, ids);
                }
            }
        }
        _ => {}
    }
}

/// Asserts that `edits` holds exactly the edits `analysis` names.
fn assert_edits_are_the_referenced_ones(analysis: &Value, edits: &Value) {
    let mut referenced = BTreeSet::new();
    edit_ids(analysis, &mut referenced);
    let keys: BTreeSet<String> = edits.as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, referenced);
}

/// 400 lines, about 23 KB: over the size above which Claude Code records no
/// `originalFile`, so the edits to it carry no hashes.
fn big_file() -> String {
    (1..=400).map(big_line).collect()
}

fn big_line(i: u32) -> String {
    format!("line {i:03}: the quick brown fox jumps over the lazy dog\n")
}

#[test]
fn review_slice_fixture() {
    let f = Fixture::new();
    slice(&f);
    let review = f.json(&f.repo, &["review", "--format", "json"]);

    let lib = file(&review, "src/lib.rs");
    assert_eq!(lib["status"], "modified");
    let chains = lib["attribution"]["chains"].as_array().unwrap();
    assert_eq!(chains.len(), 1);
    assert_eq!(chains[0]["class"], "exact_from_start");
    assert_eq!(chains[0]["head"], edit_id(SLICE_SESSION, "toolu_05"));
    // Every base line survives, so the diff only adds: the manual line and
    // the four lines of `greet` and the blank line before it.
    let added = side_lines(lib, "target");
    assert_eq!(added.len(), 5, "{added:?}");
    let hello = added
        .iter()
        .filter(|(text, _)| text.contains("Hello, {name}!"))
        .count();
    assert_eq!(hello, 1, "{added:?}");
    for (text, outcome) in added {
        if text.contains("Hello, {name}!") {
            assert_eq!(outcome["kind"], "attributed", "{text}");
            assert_eq!(
                outcome["edits"][0]["edit"],
                edit_id(SLICE_SESSION, "toolu_05")
            );
            assert_eq!(outcome["edits"][0]["relation"], "introduced");
        } else {
            // The manual line, and the earlier edit's surviving lines: the
            // later edit rewrote part of that block, so no whole block of it
            // occurs in the target and the fallback has nothing to match.
            assert_eq!(outcome["kind"], "unattributed", "{text}");
        }
    }
    let prompt = &review["edits"][edit_id(SLICE_SESSION, "toolu_05")]["prompt"];
    assert_eq!(prompt["turn"], "u1");
    assert_eq!(prompt["role"], "human");
    assert_edits_are_the_referenced_ones(&review["files"], &review["edits"]);

    let old = file(&review, "old.txt");
    assert_eq!(old["status"], "deleted");
    let removed = side_lines(old, "base");
    assert!(!removed.is_empty());
    assert!(removed.iter().all(|(_, o)| o["kind"] == "unattributed"));
    let mentions = old["mentions"].as_array().unwrap();
    assert!(
        mentions.iter().any(|m| m["text"] == "rm old.txt"),
        "{mentions:?}"
    );

    let paths: Vec<&Value> = review["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| &f["path"])
        .collect();
    // The restored notes.txt has no net change; the created file is untracked.
    assert!(!paths.contains(&&json!("notes.txt")));
    assert!(!paths.contains(&&json!("src/new.rs")));
    let untracked = &review["scope"]["untracked_excluded"][0];
    assert_eq!(untracked["path"], "src/new.rs");
    assert_eq!(untracked["recorded_edit"], true);
    assert_eq!(untracked["include_flag"], "--include-untracked src/new.rs");
    assert_eq!(
        review["unattributed"]["untracked_with_recorded_edits"],
        json!(["src/new.rs"])
    );
    assert_eq!(review["records"][0]["reconciled"], true);
}

#[test]
fn review_slice_with_include_untracked() {
    let f = Fixture::new();
    slice(&f);
    let review = f.json(
        &f.repo,
        &[
            "review",
            "--format",
            "json",
            "--include-untracked",
            "src/new.rs",
        ],
    );

    let new = file(&review, "src/new.rs");
    assert_eq!(new["status"], "added");
    assert_eq!(new["attribution"]["chains"][0]["class"], "exact_from_base");
    let added = side_lines(new, "target");
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].1["kind"], "attributed");
    assert_eq!(
        added[0].1["edits"][0]["edit"],
        edit_id(SLICE_SESSION, "toolu_02")
    );
    assert!(
        review["scope"]["untracked_excluded"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn review_large_file_single_session() {
    let f = Fixture::new();
    write(&f.repo, "big.txt", &big_file());
    f.commit(&f.repo, "base");
    let (old, new) = (big_line(200), "line 200: a changed line\n".to_owned());
    write(&f.repo, "big.txt", &big_file().replacen(&old, &new, 1));
    let text = Session::new("s-big", &f.repo)
        .prompt("Change line 200.")
        .edit(
            "toolu_big",
            &f.repo.join("big.txt"),
            &big_file(),
            &old,
            &new,
        )
        .text();
    f.transcript(&f.repo, "big.jsonl", &text);

    let review = f.json(&f.repo, &["review", "--format", "json"]);

    let big = file(&review, "big.txt");
    assert_eq!(big["attribution"]["chains"][0]["class"], "exact_from_base");
    let added = side_lines(big, "target");
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].0, new);
    assert_eq!(added[0].1["kind"], "attributed");
    assert_eq!(
        added[0].1["edits"][0]["edit"],
        edit_id("s-big", "toolu_big")
    );
    let removed = side_lines(big, "base");
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].1["kind"], "attributed");
    assert_eq!(removed[0].1["edits"][0]["relation"], "replaced");
}

#[test]
fn review_large_file_two_sessions() {
    let f = Fixture::new();
    write(&f.repo, "big.txt", &big_file());
    f.commit(&f.repo, "base");
    let changed = big_file()
        .replacen(&big_line(100), "line 100: changed by one\n", 1)
        .replacen(&big_line(300), "line 300: changed by two\n", 1);
    write(&f.repo, "big.txt", &changed);
    let big = f.repo.join("big.txt");
    // Lines 100 and 300 are far apart, so each patch is the same whichever
    // session edited first.
    let one = Session::new("s-one", &f.repo)
        .prompt("Change line 100.")
        .edit(
            "toolu_one",
            &big,
            &big_file(),
            &big_line(100),
            "line 100: changed by one\n",
        )
        .text();
    let two = Session::new("s-two", &f.repo)
        .prompt("Change line 300.")
        .edit(
            "toolu_two",
            &big,
            &big_file(),
            &big_line(300),
            "line 300: changed by two\n",
        )
        .text();
    f.transcript(&f.repo, "one.jsonl", &one);
    f.transcript(&f.repo, "two.jsonl", &two);

    let review = f.json(&f.repo, &["review", "--format", "json"]);

    // No recorded hash joins the two transcripts, so each yields one
    // consistent chain, but their unknown hashes cannot join them.
    let big = file(&review, "big.txt");
    let chains = big["attribution"]["chains"].as_array().unwrap();
    assert_eq!(chains.len(), 2);
    assert!(chains.iter().all(|c| c["class"] == "consistent"));
    let added = side_lines(big, "target");
    assert_eq!(added.len(), 2);
    assert!(
        added.iter().all(|(_, o)| o["kind"] == "ambiguous"),
        "{added:?}"
    );
    // Neither chain reaches the base, so no removed line has a fate; each
    // edit's old text is a whole block of the base.
    let removed = side_lines(big, "base");
    assert_eq!(removed.len(), 2);
    assert!(
        removed.iter().all(|(_, o)| o["kind"] == "content_match"),
        "{removed:?}"
    );
}

#[test]
fn review_commit_target_uses_the_reflog_origin() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    let text = Session::new("s-create", &f.repo)
        .prompt("Add a version.")
        .create("toolu_create", &f.repo.join("src/new.rs"), NEW_RS)
        .text();
    f.transcript(&f.repo, "create.jsonl", &text);
    write(&f.repo, "src/new.rs", NEW_RS);
    let head = f.commit(&f.repo, "add new.rs");

    let review = f.json(&f.repo, &["review", "--target", "HEAD", "--format", "json"]);

    assert_eq!(review["origins"]["commits"][0]["commit"], head);
    assert_eq!(
        review["origins"]["commits"][0]["worktrees"],
        json!([f.repo.display().to_string()])
    );
    let new = file(&review, "src/new.rs");
    let added = side_lines(new, "target");
    assert_eq!(added[0].1["kind"], "attributed");
    assert_eq!(
        added[0].1["edits"][0]["edit"],
        edit_id("s-create", "toolu_create")
    );
}

#[test]
fn review_commit_target_without_reflog_is_unattributed() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    let text = Session::new("s-create", &f.repo)
        .prompt("Add a version.")
        .create("toolu_create", &f.repo.join("src/new.rs"), NEW_RS)
        .text();
    f.transcript(&f.repo, "create.jsonl", &text);
    write(&f.repo, "src/new.rs", NEW_RS);
    let head = f.commit(&f.repo, "add new.rs");
    f.git(&["reflog", "expire", "--expire=now", "--all"]);

    let review = f.json(&f.repo, &["review", "--target", "HEAD", "--format", "json"]);

    assert_eq!(review["origins"]["without_origin"], json!([head]));
    assert!(review["records"].as_array().unwrap().is_empty());
    let added = side_lines(file(&review, "src/new.rs"), "target");
    assert_eq!(added[0].1["kind"], "unattributed");
}

#[test]
fn review_commit_from_sibling_worktree() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    let sib = f.root.join("sib");
    let sib_arg = sib.to_string_lossy().into_owned();
    f.git(&["worktree", "add", "-q", "-b", "feature", sib_arg.as_str()]);
    let content = "pub fn feature() {}\n";
    let text = Session::new("s-sib", &sib)
        .prompt("Add the feature.")
        .create("toolu_sib", &sib.join("src/feature.rs"), content)
        .text();
    f.transcript(&sib, "sib.jsonl", &text);
    write(&sib, "src/feature.rs", content);
    f.commit(&sib, "add feature");

    let review = f.json(
        &f.repo,
        &[
            "review", "--base", "main", "--target", "feature", "--format", "json",
        ],
    );

    assert_eq!(
        review["origins"]["commits"][0]["worktrees"],
        json!([sib.display().to_string()])
    );
    // Reconcile created the sibling's record from its transcripts.
    assert_eq!(review["records"].as_array().unwrap().len(), 1);
    let added = side_lines(file(&review, "src/feature.rs"), "target");
    assert_eq!(added[0].1["kind"], "attributed");
    assert_eq!(
        added[0].1["edits"][0]["edit"],
        edit_id("s-sib", "toolu_sib")
    );
}

#[test]
fn a_sibling_whose_reflog_cannot_be_read_is_unavailable_not_absent() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    let sib = f.root.join("sib");
    let sib_arg = sib.to_string_lossy().into_owned();
    f.git(&["worktree", "add", "-q", "-b", "feature", sib_arg.as_str()]);
    let content = "pub fn feature() {}\n";
    let text = Session::new("s-sib", &sib)
        .prompt("Add the feature.")
        .create("toolu_sib", &sib.join("src/feature.rs"), content)
        .text();
    f.transcript(&sib, "sib.jsonl", &text);
    write(&sib, "src/feature.rs", content);
    let feature = f.commit(&sib, "add feature");
    // A date format git rejects, for the sibling only: its reflog exists,
    // but `log -g` exits 128 there.
    f.git(&["config", "extensions.worktreeConfig", "true"]);
    f.git_in(&sib, &["config", "--worktree", "log.date", "not-a-date"]);
    let args = [
        "review", "--base", "main", "--target", "feature", "--format",
    ];

    let review = f.json(&f.repo, &[&args[..], &["json"]].concat());

    assert_eq!(review["origins"]["without_origin"], json!([feature]));
    let unavailable = review["origins"]["unavailable"].as_array().unwrap();
    assert_eq!(unavailable.len(), 1, "{unavailable:?}");
    assert_eq!(unavailable[0]["worktree"], sib.display().to_string());
    let reason = unavailable[0]["reason"].as_str().unwrap();
    assert!(
        reason.starts_with("HEAD reflog could not be read (exit 128): "),
        "{reason}"
    );
    let added = side_lines(file(&review, "src/feature.rs"), "target");
    assert_eq!(added[0].1["kind"], "unattributed");

    let output = f
        .supersigil(&f.repo, &[&args[..], &["terminal"]].concat())
        .output()
        .unwrap();
    assert!(output.status.success());
    let summary = String::from_utf8(output.stdout).unwrap();
    let line = summary
        .lines()
        .find(|l| l.contains("HEAD reflog could not be read"))
        .unwrap_or_else(|| panic!("{summary}"));
    assert_eq!(
        line.matches("origin evidence unavailable").count(),
        1,
        "{line}"
    );
}

#[test]
fn review_session_editing_a_nested_worktree() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    f.git(&["worktree", "add", "-q", "-b", "x", ".claude/worktrees/x"]);
    let nested = f.repo.join(".claude/worktrees/x");
    // A session in the main checkout writes a file inside the nested worktree.
    let content = "pub fn n() {}\n";
    let text = Session::new("s-parent", &f.repo)
        .prompt("Add n to the nested worktree.")
        .create("toolu_parent", &nested.join("src/n.rs"), content)
        .text();
    f.transcript(&f.repo, "parent.jsonl", &text);
    write(&nested, "src/n.rs", content);
    // No record exists yet: reconcile finds the main checkout's project
    // directory as an ancestor of the nested worktree.
    assert!(!f.records.exists());

    let review = f.json(
        &nested,
        &[
            "review",
            "--format",
            "json",
            "--include-untracked",
            "src/n.rs",
        ],
    );

    let added = side_lines(file(&review, "src/n.rs"), "target");
    assert_eq!(added[0].1["kind"], "attributed");
    assert_eq!(
        added[0].1["edits"][0]["edit"],
        edit_id("s-parent", "toolu_parent")
    );
}

#[test]
fn review_reports_locked_record() {
    let f = Fixture::new();
    slice(&f);
    f.json(&f.repo, &["review", "--format", "json"]);
    let record = std::fs::read_dir(&f.records)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.join("manifest.json").exists())
        .unwrap();
    // The stop hook, say, holding the record's writer lock.
    let held = Store::open(&record).unwrap();
    let tx = held.begin().unwrap();

    let started = Instant::now();
    let review = f.json(&f.repo, &["review", "--format", "json"]);

    assert!(started.elapsed() < Duration::from_secs(30));
    assert_eq!(review["records"][0]["reconciled"], false);
    assert_eq!(review["records"][0]["reason"], "locked by another writer");
    // The pinned revision is still read.
    let chains = &file(&review, "src/lib.rs")["attribution"]["chains"];
    assert_eq!(chains[0]["head"], edit_id(SLICE_SESSION, "toolu_05"));
    drop(tx);
}

#[test]
fn review_reports_unreconciled_checkout() {
    let f = Fixture::new();
    slice(&f);
    std::fs::create_dir_all(&f.records).unwrap();
    let guard = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(f.records.join(".lock"))
        .unwrap();
    guard.lock().unwrap();

    let started = Instant::now();
    let review = f.json(&f.repo, &["review", "--format", "json"]);

    assert!(started.elapsed() < Duration::from_secs(30));
    assert!(review["records"].as_array().unwrap().is_empty());
    let unreconciled = &review["evidence"]["unreconciled_checkouts"][0];
    assert_eq!(unreconciled["checkout"], f.repo.display().to_string());
    assert_eq!(unreconciled["transcripts"], 1);
    assert_eq!(
        unreconciled["reason"],
        "records directory locked by another writer"
    );
    drop(guard);
}

#[test]
fn review_keeps_capture_limitations_when_everything_is_attributed() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    let text = Session::new("s-create", &f.repo)
        .prompt("Add a version.")
        .create("toolu_create", &f.repo.join("src/new.rs"), NEW_RS)
        .text();
    f.transcript(&f.repo, "create.jsonl", &text);
    write(&f.repo, "src/new.rs", NEW_RS);
    limits(&f);
    let args = ["review", "--include-untracked", "src/new.rs", "--format"];
    let mut json_args = args.to_vec();
    json_args.push("json");
    let mut terminal_args = args.to_vec();
    terminal_args.push("terminal");

    let review = f.json(&f.repo, &json_args);

    let added = side_lines(file(&review, "src/new.rs"), "target");
    assert_eq!(added.len(), 1);
    for (text, outcome) in added {
        assert_eq!(outcome["kind"], "attributed", "{text}");
    }
    let transcripts = review["evidence"]["candidate_transcripts"]
        .as_array()
        .unwrap();
    let limited = transcripts
        .iter()
        .find(|t| t["transcript"].as_str().unwrap().ends_with("limits.jsonl"))
        .unwrap();
    assert!(
        !limited["capture_limitations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(limited["localized"], false);
    f.supersigil(&f.repo, &terminal_args)
        .assert()
        .success()
        .stdout(predicate::str::contains("limits.jsonl"))
        .stdout(predicate::str::contains("not localized to a path"));
}

#[test]
fn why_on_a_changed_line() {
    let f = Fixture::new();
    slice(&f);
    let why = f.json(&f.repo, &["why", "src/lib.rs:7", "--format", "json"]);

    assert_eq!(why["on_disk"]["state"], "captured");
    let line = &why["line"];
    assert!(line["text"].as_str().unwrap().contains("Hello, {name}!"));
    assert_eq!(line["differs_from_head"], "yes");
    assert_eq!(line["outcome"]["kind"], "attributed");
    assert_eq!(
        line["outcome"]["edits"][0]["edit"],
        edit_id(SLICE_SESSION, "toolu_05")
    );
}

#[test]
fn why_on_an_unchanged_line_explains_back_to_creation() {
    let f = Fixture::new();
    let text = Session::new("s-create", &f.repo)
        .prompt("Add a version.")
        .create("toolu_create", &f.repo.join("src/new.rs"), NEW_RS)
        .text();
    f.transcript(&f.repo, "create.jsonl", &text);
    write(&f.repo, "src/new.rs", NEW_RS);
    f.commit(&f.repo, "add new.rs");

    let why = f.json(&f.repo, &["why", "src/new.rs:1", "--format", "json"]);

    let line = &why["line"];
    assert_eq!(line["differs_from_head"], "no");
    assert_eq!(line["chains"][0]["class"], "exact_from_base");
    assert_eq!(line["outcome"]["kind"], "attributed");
    assert_eq!(
        line["outcome"]["edits"][0]["edit"],
        edit_id("s-create", "toolu_create")
    );
}

#[test]
fn why_prints_the_creation_a_later_edit_replaced() {
    let f = Fixture::new();
    let file = f.repo.join("src/a.rs");
    let text = Session::new("s-ef", &f.repo)
        .prompt("Create a, then change it.")
        .create("toolu_create", &file, "a\n")
        .edit("toolu_edit", &file, "a\n", "a\n", "b\n")
        .text();
    f.transcript(&f.repo, "ef.jsonl", &text);
    write(&f.repo, "src/a.rs", "b\n");
    f.commit(&f.repo, "add a.rs");
    let (create, edit) = (
        edit_id("s-ef", "toolu_create"),
        edit_id("s-ef", "toolu_edit"),
    );

    let why = f.json(&f.repo, &["why", "src/a.rs:1", "--format", "json"]);
    assert_eq!(why["line"]["outcome"]["kind"], "attributed");
    assert_eq!(
        why["line"]["provenance"]["provenance"]["earlier"],
        json!([create])
    );

    // The terminal shows the replacing edit and the creation it replaced.
    f.supersigil(&f.repo, &["why", "src/a.rs:1", "--format", "terminal"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "  edit {edit} (introduced) at "
        )))
        .stdout(predicate::str::contains(format!(
            "  edit {create} (earlier) at "
        )));
}

#[test]
fn why_on_assume_unchanged_file_is_not_captured() {
    let f = Fixture::new();
    slice(&f);
    limits(&f);
    write(&f.repo, "notes.txt", "edited on disk\n");
    f.git(&["update-index", "--assume-unchanged", "notes.txt"]);

    let why = f.json(&f.repo, &["why", "notes.txt:1", "--format", "json"]);

    assert_eq!(why["on_disk"]["state"], "not_captured");
    assert!(why["line"].is_null());
    // The evidence context survives the early return.
    assert_eq!(why["records"].as_array().unwrap().len(), 1);
    let transcripts = why["evidence"]["candidate_transcripts"].as_array().unwrap();
    assert!(transcripts.iter().any(|t| {
        t["transcript"].as_str().unwrap().ends_with("limits.jsonl")
            && !t["capture_limitations"].as_array().unwrap().is_empty()
    }));
    // No line was explained, so nothing refers to an edit.
    assert_eq!(why["edits"], json!({}));
    f.supersigil(&f.repo, &["why", "notes.txt:1", "--format", "terminal"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not captured"))
        .stdout(predicate::str::contains("limits.jsonl"))
        .stdout(predicate::str::contains("  edit ").not());
}

#[test]
fn why_beyond_the_end_of_the_file_fails() {
    let f = Fixture::new();
    slice(&f);
    f.supersigil(&f.repo, &["why", "src/lib.rs:99", "--format", "json"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("beyond the end"));
}

#[test]
fn review_format_auto_prints_json_when_piped() {
    let f = Fixture::new();
    slice(&f);
    let review = f.json(&f.repo, &["review"]);
    assert_eq!(review["schema"], "supersigil.review/1");
}

#[test]
fn why_inside_a_coarse_hunk_does_not_claim_a_difference() {
    let f = Fixture::new();
    let middle: String = (2..=6001).map(|i| i.to_string() + "\n").collect();
    write(&f.repo, "big.txt", &format!("first\n{middle}last\n"));
    f.commit(&f.repo, "base");
    // Only the first and last lines change: nothing is common at either end,
    // so the 6,002 remaining lines exceed the cutoff and the diff is coarse.
    write(&f.repo, "big.txt", &format!("FIRST\n{middle}LAST\n"));

    let why = f.json(&f.repo, &["why", "big.txt:3000", "--format", "json"]);

    assert_eq!(why["line"]["differs_from_head"], "unknown");
    assert_eq!(why["line"]["differs_from_head_reason"], "coarse diff");
}

#[cfg(unix)]
#[test]
fn why_compares_the_disk_with_converted_bytes_that_do_not_map_line_for_line() {
    let f = Fixture::new();
    // A reversible filter: checkout doubles every line, staging keeps the
    // odd ones. The worktree form therefore has twice the blob's lines.
    f.git(&["config", "filter.dup.smudge", "sed p"]);
    f.git(&["config", "filter.dup.clean", "awk 'NR%2==1'"]);
    write(&f.repo, ".gitattributes", "*.dup filter=dup\n");
    write(&f.repo, "f.dup", "a\na\nb\nb\n");
    f.commit(&f.repo, "base");

    let why = f.json(&f.repo, &["why", "f.dup:1", "--format", "json"]);

    assert_eq!(why["on_disk"]["state"], "captured");
    assert_eq!(why["target"]["attribution_bytes"], "converted");
    let outcome = &why["line"]["outcome"];
    assert_eq!(outcome["kind"], "unattributed");
    assert_eq!(outcome["reason"]["kind"], "attribution_unavailable");
    assert_eq!(
        outcome["reason"]["reason"],
        "conversion changed line structure"
    );
}

#[test]
fn why_reads_no_more_of_the_disk_than_could_match_the_capture() {
    let f = Fixture::new();
    // A clean filter that keeps only the first line: a large file on disk
    // is captured as a five-byte blob, and its worktree form is the same.
    f.git(&["config", "filter.first.clean", "sed -n 1p"]);
    write(&f.repo, ".gitattributes", "*.first filter=first\n");
    write(
        &f.repo,
        "f.first",
        &format!("keep\n{}", "x\n".repeat(32_768)),
    );
    f.commit(&f.repo, "base");

    let why = f.json(&f.repo, &["why", "f.first:1", "--format", "json"]);

    // A file matching `keep\n` up to line endings holds at most six bytes,
    // so the read stops at the seventh.
    assert_eq!(why["on_disk"]["state"], "not_captured");
    let reason = why["on_disk"]["reason"].as_str().unwrap();
    assert!(
        reason.starts_with("the file on disk is larger than the 6 bytes "),
        "{reason}"
    );
    assert!(why["line"].is_null());
}

#[test]
fn a_root_path_selector_reviews_everything() {
    let f = Fixture::new();
    write(&f.repo, "src/lib.rs", "a\n");
    write(&f.repo, "README.md", "r\n");
    f.commit(&f.repo, "base");
    write(&f.repo, "src/lib.rs", "b\n");
    write(&f.repo, "README.md", "s\n");

    let review = f.json(&f.repo, &["review", "--format", "json", "--", "src", "."]);

    let paths: Vec<&str> = review["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["README.md", "src/lib.rs"]);
    assert!(review["scope"]["paths"].as_array().unwrap().is_empty());
    assert!(
        review["scope"]["path_note"]
            .as_str()
            .unwrap()
            .contains("whole worktree")
    );
    // `--include-untracked` never treats the root as a selector.
    f.supersigil(
        &f.repo,
        &["review", "--format", "json", "--include-untracked", "."],
    )
    .assert()
    .failure()
    .code(1)
    .stderr(predicate::str::contains("names the worktree"));
}

/// Records session `s-dup`'s tool use `toolu_dup` creating `checkout/name`
/// in a record of its own for `checkout`, and writes the file: one sighting
/// of an edit id whose other sightings name other files.
fn record_dup_sighting(f: &Fixture, checkout: &Path, name: &str) {
    let transcripts = f.root.join("transcripts");
    std::fs::create_dir_all(&transcripts).unwrap();
    std::fs::create_dir_all(checkout).unwrap();
    let text = Session::new("s-dup", checkout)
        .prompt("Write it.")
        .create("toolu_dup", &checkout.join(name), "pub fn f() {}\n")
        .text();
    let transcript = transcripts.join(format!("{}.jsonl", name.replace('.', "-")));
    std::fs::write(&transcript, text).unwrap();
    let checkout_arg = checkout.to_string_lossy().into_owned();
    let transcript_arg = transcript.to_string_lossy().into_owned();
    let args = [
        "session",
        "--checkout",
        checkout_arg.as_str(),
        "sync",
        "--transcript",
        transcript_arg.as_str(),
    ];
    f.plain(&f.repo, &args).assert().success();
    write(checkout, name, "pub fn f() {}\n");
}

#[test]
fn a_conflicting_edit_id_attributes_neither_path() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    // One session and tool-use id recorded with different files in two
    // records: the same edit id with conflicting payloads.
    record_dup_sighting(&f, &f.repo.join("x"), "a.rs");
    record_dup_sighting(&f, &f.repo.join("y"), "b.rs");

    let args = [
        "review",
        "--format",
        "json",
        "--include-untracked",
        "x/a.rs",
        "--include-untracked",
        "y/b.rs",
    ];
    let review = f.json(&f.repo, &args);

    let id = edit_id("s-dup", "toolu_dup");
    for path in ["x/a.rs", "y/b.rs"] {
        let file = file(&review, path);
        let added = side_lines(file, "target");
        assert_eq!(added.len(), 1, "{path}");
        assert!(
            added.iter().all(|(_, o)| o["kind"] == "unattributed"),
            "{path}: {added:?}"
        );
        assert_eq!(file["conflicting_edits"][0]["edit"], id);
    }
    assert_eq!(review["evidence"]["conflicting_edits"][0]["edit"], id);
    assert!(review["edits"].get(&id).is_none());
}

#[test]
fn a_conflict_with_a_non_candidate_worktree_attributes_neither() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    // The second sighting lies in a registered nested worktree, which a
    // working-tree review of the main checkout does not make a candidate:
    // the edit id still conflicts, so the candidate sighting is excluded.
    f.git(&["worktree", "add", "-q", "-b", "y", "y"]);
    record_dup_sighting(&f, &f.repo.join("x"), "a.rs");
    record_dup_sighting(&f, &f.repo.join("y"), "b.rs");
    // A file at the nested sighting's worktree-relative path, in the
    // reviewed worktree: a different file, which the conflict never touched.
    write(&f.repo, "b.rs", "pub fn f() {}\n");

    let args = [
        "review",
        "--format",
        "json",
        "--include-untracked",
        "x/a.rs",
        "--include-untracked",
        "b.rs",
    ];
    let review = f.json(&f.repo, &args);

    let id = edit_id("s-dup", "toolu_dup");
    let a = file(&review, "x/a.rs");
    let added = side_lines(a, "target");
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].1["kind"], "unattributed", "{added:?}");
    assert_eq!(a["conflicting_edits"][0]["edit"], id);
    let b = file(&review, "b.rs");
    assert_eq!(b["conflicting_edits"], json!([]));
    assert_eq!(review["evidence"]["conflicting_edits"][0]["edit"], id);
    assert_eq!(
        review["evidence"]["conflicting_edits"][0]["records"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(review["edits"].get(&id).is_none());
}

#[test]
fn include_untracked_is_refused_for_a_commit_target() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    write(&f.repo, "new.rs", "pub fn n() {}\n");
    // A commit has no untracked files: accepting the flag would list a path
    // in the scope as included when nothing was.
    f.supersigil(
        &f.repo,
        &[
            "review",
            "--target",
            "HEAD",
            "--include-untracked",
            "new.rs",
            "--format",
            "json",
        ],
    )
    .assert()
    .failure()
    .code(2)
    .stderr(predicate::str::contains("cannot be used with"));
}

#[cfg(unix)]
#[test]
fn why_on_a_symlink_fails() {
    let f = Fixture::new();
    write(&f.repo, "target.txt", "a\n");
    std::os::unix::fs::symlink("target.txt", f.repo.join("link")).unwrap();
    f.commit(&f.repo, "base");

    // The link's blob holds its target path, not lines of the file:
    // symbolic links are never attributed.
    f.supersigil(&f.repo, &["why", "link:1", "--format", "json"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("symbolic link"));
}

#[cfg(unix)]
#[test]
fn a_conflict_is_listed_on_a_file_whose_attribution_is_unavailable() {
    let f = Fixture::new();
    // Checkout doubles every line, so the converted bytes cannot be mapped
    // onto the blob lines and the file's attribution is unavailable.
    f.git(&["config", "filter.dup.smudge", "sed p"]);
    f.git(&["config", "filter.dup.clean", "awk 'NR%2==1'"]);
    write(&f.repo, ".gitattributes", "*.dup filter=dup\n");
    f.commit(&f.repo, "base");
    record_dup_sighting(&f, &f.repo.join("x"), "a.dup");
    record_dup_sighting(&f, &f.repo.join("y"), "b.rs");

    let args = [
        "review",
        "--format",
        "json",
        "--include-untracked",
        "x/a.dup",
    ];
    let review = f.json(&f.repo, &args);

    let a = file(&review, "x/a.dup");
    assert_eq!(
        a["attribution"]["unavailable"],
        "conversion changed line structure"
    );
    assert_eq!(
        a["conflicting_edits"][0]["edit"],
        edit_id("s-dup", "toolu_dup")
    );
}

#[cfg(unix)]
#[test]
fn an_edit_through_a_symlinked_directory_attributes_no_unrelated_file() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    std::fs::create_dir_all(f.repo.join("src")).unwrap();
    std::fs::create_dir_all(f.repo.join("one")).unwrap();
    std::os::unix::fs::symlink(f.repo.join("src"), f.repo.join("one/link")).unwrap();
    f.commit(&f.repo, "base");
    // The session wrote `one/link/a.rs`, which is `src/a.rs` on disk.
    let content = "pub fn a() {}\n";
    let text = Session::new("s-link", &f.repo)
        .prompt("Add a.")
        .create("toolu_link", &f.repo.join("one/link/a.rs"), content)
        .text();
    f.transcript(&f.repo, "link.jsonl", &text);
    write(&f.repo, "src/a.rs", content);
    // An unrelated file with the same bytes at `link/a.rs`.
    write(&f.repo, "link/a.rs", content);

    let args = [
        "review",
        "--format",
        "json",
        "--include-untracked",
        "link/a.rs",
    ];
    let review = f.json(&f.repo, &args);

    let added = side_lines(file(&review, "link/a.rs"), "target");
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].1["kind"], "unattributed", "{added:?}");
}

#[test]
fn edit_maps_hold_only_the_edits_the_analysis_refers_to() {
    let f = Fixture::new();
    write(&f.repo, "big.txt", &big_file());
    f.commit(&f.repo, "base");
    let (old, new) = (big_line(200), "line 200: a changed line\n".to_owned());
    write(&f.repo, "big.txt", &big_file().replacen(&old, &new, 1));
    let big = f.repo.join("big.txt");
    let used = Session::new("s-big", &f.repo)
        .prompt("Change line 200.")
        .edit("toolu_big", &big, &big_file(), &old, &new)
        .text();
    // An edit to line 300 that the file no longer shows: offered to anchor
    // for the path, rejected as a head, and named by nothing.
    let unused = Session::new("s-unused", &f.repo)
        .prompt("Change line 300.")
        .edit(
            "toolu_unused",
            &big,
            &big_file(),
            &big_line(300),
            "line 300: never kept\n",
        )
        .text();
    f.transcript(&f.repo, "big.jsonl", &used);
    f.transcript(&f.repo, "unused.jsonl", &unused);
    let (used, unused) = (
        edit_id("s-big", "toolu_big"),
        edit_id("s-unused", "toolu_unused"),
    );

    let review = f.json(&f.repo, &["review", "--format", "json"]);
    assert!(review["edits"].get(&used).is_some());
    assert!(
        review["edits"].get(&unused).is_none(),
        "{:#}",
        review["edits"]
    );
    assert_edits_are_the_referenced_ones(&review["files"], &review["edits"]);

    let why = f.json(&f.repo, &["why", "big.txt:200", "--format", "json"]);
    assert_eq!(why["line"]["outcome"]["kind"], "attributed");
    assert!(why["edits"].get(&unused).is_none(), "{:#}", why["edits"]);
    assert_edits_are_the_referenced_ones(&why["line"], &why["edits"]);
    f.supersigil(&f.repo, &["why", "big.txt:200", "--format", "terminal"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "  edit {used} (introduced) at "
        )))
        .stdout(predicate::str::contains(unused.as_str()).not());

    // Line 1 predates every recorded edit: no contributor is printed.
    f.supersigil(&f.repo, &["why", "big.txt:1", "--format", "terminal"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not attributed"))
        .stdout(predicate::str::contains("  edit ").not());
}

#[test]
fn a_conflict_is_listed_on_a_file_that_is_not_diffed() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    record_dup_sighting(&f, &f.repo.join("x"), "a.bin");
    record_dup_sighting(&f, &f.repo.join("y"), "b.rs");
    // The file on disk is binary: listed, not diffed, with no attribution.
    std::fs::write(f.repo.join("x/a.bin"), b"\0\x01binary\n").unwrap();

    let args = [
        "review",
        "--format",
        "json",
        "--include-untracked",
        "x/a.bin",
    ];
    let review = f.json(&f.repo, &args);

    let a = file(&review, "x/a.bin");
    assert_eq!(a["kind"], "binary");
    assert!(a["attribution"].is_null());
    assert_eq!(
        a["conflicting_edits"][0]["edit"],
        edit_id("s-dup", "toolu_dup")
    );
}

#[cfg(unix)]
#[test]
fn a_failed_conversion_keeps_its_exit_status() {
    let f = Fixture::new();
    // Staging works; converting a blob to worktree form fails.
    f.git(&["config", "filter.bad.clean", "cat"]);
    f.git(&["config", "filter.bad.smudge", "false"]);
    f.git(&["config", "filter.bad.required", "true"]);
    write(&f.repo, ".gitattributes", "*.bad filter=bad\n");
    write(&f.repo, "a.bad", "a\n");
    f.commit(&f.repo, "base");
    write(&f.repo, "a.bad", "b\n");

    let review = f.json(&f.repo, &["review", "--format", "json"]);

    let a = file(&review, "a.bad");
    let status = a["attribution_bytes"]["target"].as_str().unwrap();
    assert!(status.starts_with("failed (exit "), "{status}");
    let reason = a["attribution"]["unavailable"].as_str().unwrap();
    assert!(reason.starts_with("conversion failed (exit "), "{reason}");
}

#[cfg(unix)]
#[test]
fn a_conversion_over_the_size_limit_is_unavailable_with_the_reason() {
    let f = Fixture::new();
    // Repeats its input five million times: each two-byte blob becomes ten
    // million bytes in worktree form, over the size limit.
    f.git(&[
        "config",
        "filter.grow.smudge",
        "x=$(cat); yes \"$x\" | head -n 5000000",
    ]);
    write(&f.repo, ".gitattributes", "*.grow filter=grow\n");
    write(&f.repo, "a.grow", "a\n");
    f.commit(&f.repo, "base");
    write(&f.repo, "a.grow", "b\n");

    let review = f.json(&f.repo, &["review", "--format", "json"]);

    let a = file(&review, "a.grow");
    assert_eq!(a["attribution_bytes"]["base"], "too large");
    assert_eq!(a["attribution_bytes"]["target"], "too large");
    let limit = supersigil_git::changes::MAX_DIFF_BYTES;
    assert_eq!(
        a["attribution"]["unavailable"],
        format!("conversion output is larger than {limit} bytes")
    );
}

/// One line of text one byte longer than the largest blob the review reads.
fn oversized() -> String {
    let limit = usize::try_from(supersigil_git::changes::MAX_DIFF_BYTES).unwrap();
    "x".repeat(limit) + "\n"
}

#[test]
fn why_refuses_a_file_over_the_size_limit_before_reading_it() {
    let f = Fixture::new();
    write(&f.repo, "big.txt", &oversized());
    f.commit(&f.repo, "base");

    f.supersigil(&f.repo, &["why", "big.txt:1", "--format", "json"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("larger than"));
}

#[test]
fn why_does_not_compare_with_a_head_version_over_the_size_limit() {
    let f = Fixture::new();
    write(&f.repo, "big.txt", &oversized());
    f.commit(&f.repo, "base");
    write(&f.repo, "big.txt", "small\n");

    let why = f.json(&f.repo, &["why", "big.txt:1", "--format", "json"]);

    assert_eq!(why["line"]["differs_from_head"], "unknown");
    let reason = why["line"]["differs_from_head_reason"].as_str().unwrap();
    assert!(reason.contains("larger than"), "{reason}");
}

#[test]
fn why_on_a_file_that_replaced_a_submodule_has_no_head_baseline() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    let base = f.commit(&f.repo, "base");
    // HEAD records `sub` as a submodule commit; the index and the disk now
    // hold a regular text file there.
    let cacheinfo = format!("160000,{base},sub");
    f.git(&["update-index", "--add", "--cacheinfo", cacheinfo.as_str()]);
    f.git(&["commit", "-q", "-m", "add a submodule"]);
    f.git(&["rm", "-q", "--cached", "sub"]);
    write(&f.repo, "sub", "text\n");
    f.git(&["add", "sub"]);

    let why = f.json(&f.repo, &["why", "sub:1", "--format", "json"]);

    assert_eq!(why["on_disk"]["state"], "captured");
    assert_eq!(why["line"]["differs_from_head"], "unknown");
    assert_eq!(
        why["line"]["differs_from_head_reason"],
        "the path is not a regular file in HEAD"
    );
}

#[test]
fn a_scoped_review_keeps_its_file_whatever_pathspec_mode_it_inherits() {
    let f = Fixture::new();
    write(&f.repo, "f.txt", "a\n");
    write(&f.repo, "g.txt", "a\n");
    f.commit(&f.repo, "base");
    write(&f.repo, "f.txt", "b\n");
    write(&f.repo, "g.txt", "b\n");
    let args = ["review", "--format", "json", "--", "f.txt"];

    let listed = under_each_pathspec_mode(|mode| {
        f.json_inheriting(mode, &f.repo, &args)
            .map(|review| paths(&review))
    });

    assert_eq!(
        listed,
        under_each_pathspec_mode(|_| Ok(vec!["f.txt".to_owned()]))
    );
}

#[test]
fn why_finds_its_file_whatever_pathspec_mode_it_inherits() {
    let f = Fixture::new();
    write(&f.repo, "f.txt", "a\n");
    f.commit(&f.repo, "base");
    write(&f.repo, "f.txt", "b\n");
    let args = ["why", "f.txt:1", "--format", "json"];

    let found = under_each_pathspec_mode(|mode| {
        f.json_inheriting(mode, &f.repo, &args).map(|why| {
            (
                why["on_disk"]["state"].clone(),
                why["line"]["text"].clone(),
                why["line"]["differs_from_head"].clone(),
            )
        })
    });

    // The file is found in the captured tree and in HEAD.
    assert_eq!(
        found,
        under_each_pathspec_mode(|_| Ok((json!("captured"), json!("b\n"), json!("yes"))))
    );
}

#[test]
fn an_untracked_file_is_included_whatever_pathspec_mode_it_inherits() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    write(&f.repo, "new.txt", "n\n");
    write(&f.repo, "other.txt", "o\n");
    let review_args = [
        "review",
        "--format",
        "json",
        "--include-untracked",
        "new.txt",
    ];
    let why_args = ["why", "new.txt:1", "--format", "json"];

    let included = under_each_pathspec_mode(|mode| {
        let review = f
            .json_inheriting(mode, &f.repo, &review_args)
            .map(|review| (paths(&review), excluded(&review)));
        let why = f
            .json_inheriting(mode, &f.repo, &why_args)
            .map(|why| why["line"]["text"].clone());
        (review, why)
    });

    let expected = (
        Ok((vec!["new.txt".to_owned()], vec!["other.txt".to_owned()])),
        Ok(json!("n\n")),
    );
    assert_eq!(included, under_each_pathspec_mode(|_| expected.clone()));
}

/// Needs a filesystem where `f.txt` and `F.txt` are two files.
#[cfg(target_os = "linux")]
#[test]
fn an_inherited_pathspec_mode_does_not_widen_a_scope_to_another_case() {
    let f = Fixture::new();
    write(&f.repo, "f.txt", "a\n");
    write(&f.repo, "F.txt", "a\n");
    f.commit(&f.repo, "base");
    write(&f.repo, "f.txt", "b\n");
    write(&f.repo, "F.txt", "b\n");
    write(&f.repo, "n.txt", "n\n");
    write(&f.repo, "N.txt", "n\n");
    let scoped = ["review", "--format", "json", "--", "f.txt"];
    let included = [
        "review",
        "--format",
        "json",
        "--include-untracked",
        "n.txt",
        "--",
        "f.txt",
        "n.txt",
    ];

    let listed = under_each_pathspec_mode(|mode| {
        let scoped = f
            .json_inheriting(mode, &f.repo, &scoped)
            .map(|review| paths(&review));
        let included = f
            .json_inheriting(mode, &f.repo, &included)
            .map(|review| (paths(&review), excluded(&review)));
        (scoped, included)
    });

    // `N.txt` is outside the scope, so it is not even listed as excluded.
    let expected = (
        Ok(vec!["f.txt".to_owned()]),
        Ok((vec!["f.txt".to_owned(), "n.txt".to_owned()], Vec::new())),
    );
    assert_eq!(listed, under_each_pathspec_mode(|_| expected.clone()));
}

#[test]
fn a_bracketed_name_stays_literal_whatever_pathspec_mode_it_inherits() {
    let f = Fixture::new();
    // As a glob, `[ab].txt` would match `a.txt`, and `[n].txt` `n.txt`.
    write(&f.repo, "[ab].txt", "a\n");
    write(&f.repo, "a.txt", "a\n");
    f.commit(&f.repo, "base");
    write(&f.repo, "[ab].txt", "b\n");
    write(&f.repo, "a.txt", "b\n");
    write(&f.repo, "[n].txt", "n\n");
    write(&f.repo, "n.txt", "n\n");
    let scoped = ["review", "--format", "json", "--", "[ab].txt"];
    let included = [
        "review",
        "--format",
        "json",
        "--include-untracked",
        "[n].txt",
    ];

    let listed = under_each_pathspec_mode(|mode| {
        let scoped = f
            .json_inheriting(mode, &f.repo, &scoped)
            .map(|review| paths(&review));
        let included = f
            .json_inheriting(mode, &f.repo, &included)
            .map(|review| (paths(&review), excluded(&review)));
        (scoped, included)
    });

    let expected = (
        Ok(vec!["[ab].txt".to_owned()]),
        Ok((
            vec![
                "[ab].txt".to_owned(),
                "[n].txt".to_owned(),
                "a.txt".to_owned(),
            ],
            vec!["n.txt".to_owned()],
        )),
    );
    assert_eq!(listed, under_each_pathspec_mode(|_| expected.clone()));
}

/// The id `rev` names, read before any replacement ref exists.
fn rev_parse(f: &Fixture, rev: &str) -> String {
    f.git(&["rev-parse", rev]).trim().to_owned()
}

#[test]
fn a_replaced_blob_is_reviewed_with_its_own_bytes() {
    let f = Fixture::new();
    write(&f.repo, "f.txt", "before\n");
    f.commit(&f.repo, "base");
    write(&f.repo, "f.txt", "after\n");
    f.commit(&f.repo, "change");
    let (before, after) = (rev_parse(&f, "HEAD~1:f.txt"), rev_parse(&f, "HEAD:f.txt"));
    // A replacement ref makes git serve `before\n` for the target blob's id.
    f.git(&["replace", &after, &before]);

    let review = f.json(&f.repo, &["review", "--target", "HEAD", "--format", "json"]);
    let why = f.json(&f.repo, &["why", "f.txt:1", "--format", "json"]);

    let changed = file(&review, "f.txt");
    assert_eq!(changed["new_blob"], after);
    let lines = |side| -> Vec<String> {
        side_lines(changed, side)
            .into_iter()
            .map(|(text, _)| text)
            .collect()
    };
    assert_eq!(
        (lines("base"), lines("target")),
        (vec!["before\n".to_owned()], vec!["after\n".to_owned()])
    );
    assert_eq!(why["target"]["blob"], after);
    assert_eq!(why["on_disk"]["state"], "captured");
    assert_eq!(why["line"]["text"], "after\n");
}

#[test]
fn a_replaced_tree_still_shows_its_change() {
    let f = Fixture::new();
    write(&f.repo, "f.txt", "before\n");
    f.commit(&f.repo, "base");
    write(&f.repo, "f.txt", "after\n");
    f.commit(&f.repo, "change");
    let tree = rev_parse(&f, "HEAD^{tree}");
    // A replacement ref makes git serve the base tree for the target's.
    f.git(&["replace", "HEAD^{tree}", "HEAD~1^{tree}"]);

    let review = f.json(
        &f.repo,
        &[
            "review", "--base", "HEAD~1", "--target", "HEAD", "--format", "json",
        ],
    );

    assert_eq!(review["target"]["tree"], tree);
    assert_eq!(paths(&review), ["f.txt"]);
    let added = side_lines(file(&review, "f.txt"), "target");
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].0, "after\n");
}

#[test]
fn a_replaced_commit_resolves_to_its_own_tree_and_parent() {
    let f = Fixture::new();
    write(&f.repo, "f.txt", "a\n");
    f.commit(&f.repo, "first");
    write(&f.repo, "f.txt", "b\n");
    let second = f.commit(&f.repo, "second");
    write(&f.repo, "f.txt", "c\n");
    let third = f.commit(&f.repo, "third");
    let tree = rev_parse(&f, "HEAD^{tree}");
    // A replacement ref makes git serve the second commit, with its tree
    // and its parent, for the third.
    f.git(&["replace", &third, &second]);
    let implicit = ["review", "--target", "HEAD", "--format", "json"];
    let explicit = [
        "review", "--base", "HEAD~1", "--target", "HEAD", "--format", "json",
    ];

    for args in [&implicit[..], &explicit[..]] {
        let review = f.json(&f.repo, args);
        let resolved = (
            &review["base"]["commit"],
            &review["target"]["commit"],
            &review["target"]["tree"],
        );
        assert_eq!(
            resolved,
            (&json!(second), &json!(third), &json!(tree)),
            "{args:?}"
        );
        let added = side_lines(file(&review, "f.txt"), "target");
        assert_eq!(added.len(), 1, "{args:?}");
        assert_eq!(added[0].0, "c\n", "{args:?}");
    }
}

#[test]
fn a_capture_limitation_two_records_hold_is_counted_once() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    // One transcript from a directory inside the worktree, synced into the
    // record of that directory and into the record of the worktree: both
    // hold its one capture limitation, an unsupported tool use. The nested
    // record comes first: once the worktree has a record, a sync for the
    // directory inside it would use that one.
    let nested = f.repo.join("x");
    std::fs::create_dir_all(&nested).unwrap();
    let text = Session::new("s-limits", &nested)
        .prompt("Edit the notebook.")
        .notebook_edit("toolu_nb")
        .text();
    let transcript = f.root.join("transcripts").join("limits.jsonl");
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    std::fs::write(&transcript, text).unwrap();
    let transcript_arg = transcript.to_string_lossy().into_owned();
    for checkout in [&nested, &f.repo] {
        let checkout_arg = checkout.to_string_lossy().into_owned();
        let args = [
            "session",
            "--checkout",
            checkout_arg.as_str(),
            "sync",
            "--transcript",
            transcript_arg.as_str(),
        ];
        f.plain(&f.repo, &args).assert().success();
    }

    let review = f.json(&f.repo, &["review", "--format", "json"]);

    assert_eq!(review["records"].as_array().unwrap().len(), 2);
    let transcripts = review["evidence"]["candidate_transcripts"]
        .as_array()
        .unwrap();
    let limited = transcripts
        .iter()
        .find(|t| t["transcript"] == transcript_arg.as_str())
        .unwrap_or_else(|| panic!("{transcripts:#?}"));
    let reports = limited["capture_limitations"].as_array().unwrap();
    let output = f
        .supersigil(&f.repo, &["review", "--format", "terminal"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let summary = String::from_utf8(output.stdout).unwrap();
    let header = summary
        .lines()
        .find(|line| line.contains("capture limitations in"))
        .unwrap_or_else(|| panic!("{summary}"));
    let expected = format!(
        "  capture limitations in {transcript_arg} (session s-limits): 1 unsupported tool uses; not localized to a path"
    );
    assert_eq!((reports.len(), header), (1, expected.as_str()));
    assert_eq!(reports[0]["unsupported_tool_uses"], 1);
}

/// `why <location>` in both formats, each expected to succeed: the parsed
/// JSON and the terminal text.
fn why_outputs(f: &Fixture, location: &str) -> (Value, String) {
    let json = f.json(&f.repo, &["why", location, "--format", "json"]);
    let output = f
        .supersigil(&f.repo, &["why", location, "--format", "terminal"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (json, String::from_utf8(output.stdout).unwrap())
}

/// The tree a capture of the working tree holds, found without supersigil:
/// what `git add -u`, then `git add` of each of `untracked`, stages in a
/// copy of the index with the original's modification time, written with
/// `write-tree`. The real index is untouched.
fn captured_tree(f: &Fixture, untracked: &[&str]) -> String {
    let source = f.repo.join(".git").join("index");
    let index = f.root.join("expected-index");
    std::fs::copy(&source, &index).unwrap();
    let modified = std::fs::metadata(&source).unwrap().modified().unwrap();
    OpenOptions::new()
        .write(true)
        .open(&index)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    let git = |args: &[&str]| -> String {
        let mut command = Command::new("git");
        f.isolate(&mut command);
        let output = command
            .env("GIT_INDEX_FILE", &index)
            .current_dir(&f.repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    git(&["add", "-u"]);
    for path in untracked {
        git(&["add", "--", path]);
    }
    let tree = git(&["write-tree"]).trim().to_owned();
    std::fs::remove_file(&index).unwrap();
    tree
}

/// A session `s-create` that writes `content` to `path` after the prompt
/// "Write the file.", recorded as `create.jsonl`. Returns the transcript's
/// path and the edit's id.
fn record_creation(f: &Fixture, path: &str, content: &str) -> (PathBuf, String) {
    let text = Session::new("s-create", &f.repo)
        .prompt("Write the file.")
        .create("toolu_create", &f.repo.join(path), content)
        .text();
    let transcript = f.transcript(&f.repo, "create.jsonl", &text);
    (transcript, edit_id("s-create", "toolu_create"))
}

/// The JSON `why` prints for `path` when the captured `tree` holds it and
/// the one record holds the one transcript of [`record_creation`]. The
/// record's id is random, so it is taken from `why`; everything else is
/// what the case expects.
fn expected_why(
    f: &Fixture,
    why: &Value,
    (path, tree): (&str, &str),
    transcript: &Path,
    (on_disk, line, edits): (Value, Value, Value),
) -> Value {
    json!({
        "schema": "supersigil.why/1",
        "path": path,
        "target": {
            "worktree": f.repo.to_string_lossy(),
            "tree": tree,
            "blob": rev_parse(f, &format!("{tree}:{path}")),
            "attribution_bytes": "identical",
        },
        "records": [{
            "id": why["records"][0]["id"].as_str().unwrap(),
            "revision": 1,
            "reconciled": true,
            "reason": null,
        }],
        "evidence": {
            "candidate_transcripts": [{
                "transcript": transcript.to_string_lossy(),
                "session": "s-create",
                "capture_limitations": [],
                "localized": false,
            }],
            "unplaced_edits": 0,
            "unreconciled_checkouts": [],
            "conflicting_edits": [],
        },
        "on_disk": on_disk,
        "line": line,
        "edits": edits,
        "conflicting_edits": [],
    })
}

/// The analysis of line `number`, `text`, which the creation `id`
/// introduced, exactly from the base.
fn created_line(number: u64, text: &str, (differs, reason): (&str, Value), id: &str) -> Value {
    json!({
        "number": number,
        "text": text,
        "differs_from_head": differs,
        "differs_from_head_reason": reason,
        "status": {"kind": "composed"},
        "chains": [{
            "id": 0,
            "class": "exact_from_base",
            "head": id,
            "edits": [id],
            "end": {"kind": "base"},
        }],
        "set_aside": [],
        "outcome": {
            "kind": "attributed",
            "edits": [{"edit": id, "relation": "introduced", "classes": ["exact_from_base"]}],
        },
        "provenance": {
            "class": "exact_from_base",
            "kind": "agreed",
            "provenance": {
                "earlier": [],
                "origins": [{"kind": "introduced", "value": id}],
                "whitespace_only": [],
            },
        },
    })
}

/// The edits map holding only the creation `id` of [`record_creation`].
fn creation_edits(f: &Fixture, transcript: &Path, id: &str) -> Value {
    json!({
        id: {
            "session": "s-create",
            "transcript": transcript.to_string_lossy(),
            "turn": "s-create-1",
            "time": "2026-09-29T10:00:02.000Z",
            "worktree": f.repo.to_string_lossy(),
            "operation": "write",
            "origin": {"kind": "tool"},
            "prompt": {
                "turn": "s-create-0",
                "role": "human",
                "excerpt": {"state": "retained", "value": "Write the file."},
            },
        },
    })
}

/// The terminal text for line `number`, `text`, which the creation `id`
/// introduced.
fn created_line_text(path: &str, number: u64, text: &str, differs: &str, id: &str) -> String {
    format!(
        "{path}:{number}: {text}\n  introduced by edit {id} (exact from the base)\n  differs from HEAD: {differs}\n  edit {id} (introduced) at 2026-09-29T10:00:02.000Z, session s-create: after \"Write the file.\"\n"
    )
}

#[test]
fn why_on_a_tracked_file_reads_the_capture_of_tracked_changes() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    write(&f.repo, "t.txt", "one\n");
    f.commit(&f.repo, "base");
    let (transcript, id) = record_creation(&f, "t.txt", "one\ntwo\n");
    write(&f.repo, "t.txt", "one\ntwo\n");
    write(&f.repo, "other.txt", "o\n");

    let (why, terminal) = why_outputs(&f, "t.txt:2");

    // The capture holds the tracked change and no untracked file.
    let tree = captured_tree(&f, &[]);
    assert_ne!(tree, rev_parse(&f, "HEAD^{tree}"));
    let line = created_line(2, "two\n", ("yes", Value::Null), &id);
    let analysis = (
        json!({"state": "captured"}),
        line,
        creation_edits(&f, &transcript, &id),
    );
    let expected = expected_why(&f, &why, ("t.txt", &tree), &transcript, analysis);
    assert_eq!(why, expected);
    assert_eq!(terminal, created_line_text("t.txt", 2, "two", "yes", &id));
}

#[test]
fn why_on_an_untracked_file_captures_it_with_the_tracked_changes() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    let (transcript, id) = record_creation(&f, "new.txt", "n1\nn2\n");
    write(&f.repo, "new.txt", "n1\nn2\n");
    write(&f.repo, "README.md", "changed\n");
    write(&f.repo, "other.txt", "o\n");

    let (why, terminal) = why_outputs(&f, "new.txt:2");

    // The capture holds the tracked change and the requested untracked
    // file, and no other untracked file.
    let tree = captured_tree(&f, &["new.txt"]);
    assert_ne!(tree, captured_tree(&f, &[]));
    let not_in_head = json!("the file is not in HEAD");
    let line = created_line(2, "n2\n", ("unknown", not_in_head), &id);
    let analysis = (
        json!({"state": "captured"}),
        line,
        creation_edits(&f, &transcript, &id),
    );
    let expected = expected_why(&f, &why, ("new.txt", &tree), &transcript, analysis);
    assert_eq!(why, expected);
    assert_eq!(
        terminal,
        created_line_text("new.txt", 2, "n2", "unknown (the file is not in HEAD)", &id)
    );
}

/// Checks `why` on a committed file whose line on disk was edited after
/// `flag` (an `update-index` option) was set on it: the capture keeps the
/// index's version, which the recorded creation explains, and the line on
/// disk is not that version, so nothing is explained.
fn a_flagged_line_explains_nothing(flag: &str, cause: &str) {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    let (transcript, _) = record_creation(&f, "t.txt", "one\n");
    write(&f.repo, "t.txt", "one\n");
    f.commit(&f.repo, "base");
    write(&f.repo, "t.txt", "edited\n");
    f.git(&["update-index", flag, "t.txt"]);

    let (why, terminal) = why_outputs(&f, "t.txt:1");

    let tree = captured_tree(&f, &[]);
    assert_eq!(tree, rev_parse(&f, "HEAD^{tree}"));
    let reason = format!("present on disk; contents not captured ({cause})");
    let analysis = (
        json!({"state": "not_captured", "reason": reason}),
        Value::Null,
        json!({}),
    );
    let expected = expected_why(&f, &why, ("t.txt", &tree), &transcript, analysis);
    assert_eq!(why, expected);
    assert_eq!(
        terminal,
        format!("t.txt: not captured: {reason}; no line explained\n")
    );
}

#[test]
fn why_on_an_assume_unchanged_line_explains_nothing() {
    a_flagged_line_explains_nothing("--assume-unchanged", "assume_unchanged");
}

#[test]
fn why_on_a_skip_worktree_line_explains_nothing() {
    // The path is tracked, so the capture must not add it as an untracked
    // file: git refuses to add a skip-worktree path.
    a_flagged_line_explains_nothing("--skip-worktree", "skip_worktree");
}

#[test]
fn why_on_a_line_whose_disk_state_differs_from_the_capture_explains_nothing() {
    let f = Fixture::new();
    // A clean filter stages `x` as `y`, and nothing converts it back: the
    // captured blob holds `y\n` while the file on disk holds `x\n`.
    f.git(&["config", "filter.swap.clean", "sed s/x/y/"]);
    write(&f.repo, ".gitattributes", "*.swap filter=swap\n");
    let (transcript, _) = record_creation(&f, "f.swap", "x\n");
    write(&f.repo, "f.swap", "x\n");
    f.commit(&f.repo, "base");

    let (why, terminal) = why_outputs(&f, "f.swap:1");

    let tree = captured_tree(&f, &[]);
    let reason = "the file on disk differs from the captured target: its on-disk state was not captured, or it changed during the command";
    let analysis = (
        json!({"state": "not_captured", "reason": reason}),
        Value::Null,
        json!({}),
    );
    let expected = expected_why(&f, &why, ("f.swap", &tree), &transcript, analysis);
    assert_eq!(why, expected);
    assert_eq!(
        terminal,
        format!("f.swap: not captured: {reason}; no line explained\n")
    );
}

#[test]
fn a_record_in_an_older_format_fails_with_what_to_do() {
    let f = Fixture::new();
    slice(&f);
    let stale = f.records.join("stale");
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::write(
        stale.join("manifest.json"),
        r#"{"record_id": "d435b3f4-5a03-4c70-a4c8-009547bee6a1", "schema_version": 1,
            "revision": 1, "associations": [{"checkout": "/elsewhere"}],
            "logs": {}, "documents": {}, "cursors": {}}"#,
    )
    .unwrap();
    f.supersigil(&f.repo, &["review", "--format", "json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(stale.display().to_string()))
        .stderr(predicate::str::contains(
            "uses record format 1; this build reads format 2",
        ))
        .stderr(predicate::str::contains("Delete that directory"));
}

#[test]
fn isolation_drops_configuration_passed_through_the_environment() {
    let f = Fixture::new();
    let leak = f.root.join("leak.cfg");
    std::fs::write(&leak, "[core]\n\tleak = global\n").unwrap();
    let mut command = Command::new("git");
    command
        .env("GIT_CONFIG_GLOBAL", &leak)
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "core.leak")
        .env("GIT_CONFIG_VALUE_0", "count")
        .env("GIT_CONFIG_PARAMETERS", "'core.leak=parameters'");
    f.isolate(&mut command);
    let output = command
        .current_dir(&f.repo)
        .args(["config", "--get-all", "core.leak"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
}

/// A change report naming `file` with one entry: `created` or modified,
/// and one hunk at line 1 holding `lines`, with the header counts the
/// harness gives it.
fn change_report(file: &Path, created: bool, lines: &[&str]) -> Value {
    let file = file.to_string_lossy();
    let count = |markers: &[char]| lines.iter().filter(|l| l.starts_with(markers)).count();
    let mut entry = json!({"filePath": file, "hunks": [{"oldStart": 1,
        "oldLines": count(&[' ', '-']), "newStart": 1, "newLines": count(&[' ', '+']),
        "lines": lines}]});
    if created {
        entry["created"] = json!(true);
    }
    json!({"changedFiles": [file], "files": [entry], "moreFiles": 0})
}

const SHELL_SESSION: &str = "s-shell";
const HEREDOC_CREATE: &str = "mkdir -p src && cat > src/gen.rs <<'EOF'\npub fn a() {}\nEOF\n";
const HEREDOC_APPEND: &str = "cat >> src/gen.rs <<'EOF'\npub fn b() {}\nEOF\n";
const SED: &str = "sed -i 's/draft/final/' notes.txt";

/// A session that works through the shell only: a heredoc creates
/// `src/gen.rs`, another appends to it, `sed` rewrites `notes.txt`, and a
/// last heredoc has no change report. The working tree holds the result.
fn shell_session(f: &Fixture) {
    write(&f.repo, "notes.txt", "draft\n");
    f.commit(&f.repo, "base");
    let gen_rs = f.repo.join("src/gen.rs");
    let notes = f.repo.join("notes.txt");
    let text = Session::new(SHELL_SESSION, &f.repo)
        .prompt("Generate the module.")
        .bash(
            "t_new",
            HEREDOC_CREATE,
            Some(change_report(&gen_rs, true, &["+pub fn a() {}"])),
        )
        .bash(
            "t_app",
            HEREDOC_APPEND,
            Some(change_report(
                &gen_rs,
                false,
                &[" pub fn a() {}", "+pub fn b() {}"],
            )),
        )
        .bash(
            "t_sed",
            SED,
            Some(change_report(&notes, false, &["-draft", "+final"])),
        )
        .bash("t_unc", "cat > scratch.txt <<'EOF'\nx\nEOF\n", None)
        .text();
    f.transcript(&f.repo, "shell.jsonl", &text);
    write(&f.repo, "src/gen.rs", "pub fn a() {}\npub fn b() {}\n");
    write(&f.repo, "notes.txt", "final\n");
}

fn command_id(session: &str, tool: &str) -> String {
    EventId::derive("command", &SessionId::new(session), tool)
        .as_str()
        .to_owned()
}

const SHELL_REVIEW: [&str; 5] = [
    "review",
    "--include-untracked",
    "src/gen.rs",
    "--format",
    "json",
];

#[test]
fn heredoc_writes_attribute_like_edits_and_name_their_command() {
    let f = Fixture::new();
    shell_session(&f);
    let review = f.json(&f.repo, &SHELL_REVIEW);

    let created = edit_id(SHELL_SESSION, "t_new#0");
    let appended = edit_id(SHELL_SESSION, "t_app#0");
    let gen_rs = file(&review, "src/gen.rs");
    assert_eq!(gen_rs["attribution"]["status"]["kind"], "composed");
    assert_eq!(
        gen_rs["attribution"]["chains"][0]["class"],
        "exact_from_base"
    );
    let lines = side_lines(gen_rs, "target");
    assert_eq!(lines.len(), 2);
    for ((text, outcome), id) in lines.iter().zip([&created, &appended]) {
        assert_eq!(outcome["kind"], "attributed", "{text}");
        assert_eq!(outcome["edits"][0]["edit"], id.as_str(), "{text}");
        assert_eq!(outcome["edits"][0]["relation"], "introduced", "{text}");
    }
    assert_eq!(review["edits"][&created]["operation"], "write");
    assert_eq!(
        review["edits"][&created]["origin"],
        json!({"kind": "shell", "command": command_id(SHELL_SESSION, "t_new"),
            "text": HEREDOC_CREATE})
    );
    assert_eq!(review["edits"][&appended]["operation"], "append");
    assert_eq!(
        review["edits"][&appended]["origin"]["command"],
        command_id(SHELL_SESSION, "t_app")
    );
    assert_edits_are_the_referenced_ones(&review["files"], &review["edits"]);
}

#[test]
fn a_review_lists_the_commands_that_changed_each_file() {
    let f = Fixture::new();
    shell_session(&f);
    let review = f.json(&f.repo, &SHELL_REVIEW);

    // Newest first; the heredoc commands are listed although their writes
    // are also edits.
    let gen_rs = file(&review, "src/gen.rs");
    let listed: Vec<(&str, &str)> = gen_rs["command_changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["command"].as_str().unwrap(), c["kind"].as_str().unwrap()))
        .collect();
    assert_eq!(
        listed,
        vec![
            (command_id(SHELL_SESSION, "t_app").as_str(), "modified"),
            (command_id(SHELL_SESSION, "t_new").as_str(), "created"),
        ]
    );

    // `sed` wrote no recorded edit: the line stays unattributed, and the
    // command is listed with what the harness reported.
    let notes = file(&review, "notes.txt");
    let (_, outcome) = &side_lines(notes, "target")[0];
    assert_eq!(outcome["kind"], "unattributed");
    let changes = notes["command_changes"].as_array().unwrap();
    assert_eq!(changes.len(), 1);
    let transcript = f
        .claude
        .join("projects")
        .join(supersigil_session::discover::encode_project_dir(&f.repo))
        .join("shell.jsonl");
    assert_eq!(
        changes[0],
        json!({
            "command": command_id(SHELL_SESSION, "t_sed"),
            "session": SHELL_SESSION,
            "turn": format!("{SHELL_SESSION}-5"),
            "transcript": transcript.to_string_lossy(),
            "time": "2026-09-29T10:00:05.000Z",
            "checkout": f.repo.to_string_lossy(),
            "worktree": f.repo.to_string_lossy(),
            "text": SED,
            "kind": "modified",
            "patch": {"state": "retained", "value": [{"old_start": 1, "old_lines": 1,
                "new_start": 1, "new_lines": 1, "lines": ["-draft", "+final"]}]},
            "unlisted": 0,
            "outside": 0,
            "flags": [],
        })
    );
    // Its text names the path, but it is not listed twice.
    assert_eq!(notes["mentions"], json!([]));

    // The unconfirmed heredoc is a capture limitation, not an edit.
    let limitations = &review["evidence"]["candidate_transcripts"][0]["capture_limitations"];
    assert_eq!(limitations[0]["unconfirmed_shell_writes"], 1);
    let unconfirmed = edit_id(SHELL_SESSION, "t_unc#0");
    assert!(review["edits"].get(&unconfirmed).is_none());
}

#[test]
fn a_commit_review_attributes_shell_edits_and_lists_command_changes() {
    let f = Fixture::new();
    shell_session(&f);
    let head = f.commit(&f.repo, "generate");

    let review = f.json(&f.repo, &["review", "--target", "HEAD", "--format", "json"]);

    assert_eq!(review["origins"]["commits"][0]["commit"], head);
    let gen_rs = file(&review, "src/gen.rs");
    let lines = side_lines(gen_rs, "target");
    assert_eq!(lines[0].1["kind"], "attributed");
    assert_eq!(
        lines[0].1["edits"][0]["edit"],
        edit_id(SHELL_SESSION, "t_new#0")
    );
    assert_eq!(lines[1].1["kind"], "attributed");
    assert_eq!(gen_rs["command_changes"].as_array().unwrap().len(), 2);
    let notes = file(&review, "notes.txt");
    assert_eq!(
        notes["command_changes"][0]["command"],
        command_id(SHELL_SESSION, "t_sed")
    );
}

/// A session whose one command skips its heredoc (`false &&`) and creates
/// the file with `echo`; the harness reports the file created, with
/// `report_lines` as its hunk when there are any.
fn skipped_heredoc(f: &Fixture, report_lines: Option<&[&str]>) -> Value {
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    let made = f.repo.join("made.txt");
    let command = "false && cat > made.txt <<'EOF'\nfrom the heredoc\nEOF\necho other > made.txt\n";
    let mut report = change_report(&made, true, report_lines.unwrap_or_default());
    if report_lines.is_none() {
        report["files"][0]["hunks"] = json!([]);
    }
    let text = Session::new("s-skip", &f.repo)
        .prompt("Make the file.")
        .bash("t_skip", command, Some(report))
        .text();
    f.transcript(&f.repo, "skip.jsonl", &text);
    write(&f.repo, "made.txt", "other\n");
    f.json(
        &f.repo,
        &[
            "review",
            "--include-untracked",
            "made.txt",
            "--format",
            "json",
        ],
    )
}

#[test]
fn a_heredoc_the_reported_diff_contradicts_is_not_an_edit() {
    let f = Fixture::new();
    let review = skipped_heredoc(&f, Some(&["+other"]));
    let made = file(&review, "made.txt");
    let (_, outcome) = &side_lines(made, "target")[0];
    assert_eq!(outcome["kind"], "unattributed");
    assert_eq!(review["edits"], json!({}));
    let limitations = &review["evidence"]["candidate_transcripts"][0]["capture_limitations"];
    assert_eq!(limitations[0]["unconfirmed_shell_writes"], 1);
    // The command is still what the reviewer is pointed at.
    assert_eq!(
        made["command_changes"][0]["command"],
        command_id("s-skip", "t_skip")
    );
}

#[test]
fn a_heredoc_edit_whose_statement_did_not_run_attributes_nothing() {
    // Without hunks nothing speaks against the write, so the record holds
    // an edit that did not happen: the limit of a confirmation per command.
    // Its text is not the file's bytes, so it attributes nothing.
    let f = Fixture::new();
    let review = skipped_heredoc(&f, None);
    let made = file(&review, "made.txt");
    let (_, outcome) = &side_lines(made, "target")[0];
    assert_eq!(outcome["kind"], "unattributed");
    assert_eq!(made["attribution"]["status"]["kind"], "not_composed");
    assert_eq!(
        made["attribution"]["status"]["reasons"][0]["edit"],
        edit_id("s-skip", "t_skip#0")
    );
    assert_eq!(
        made["command_changes"][0]["command"],
        command_id("s-skip", "t_skip")
    );
}

#[test]
fn the_terminal_summary_counts_commands_and_unconfirmed_shell_writes() {
    let f = Fixture::new();
    shell_session(&f);
    let output = f
        .supersigil(
            &f.repo,
            &[
                "review",
                "--include-untracked",
                "src/gen.rs",
                "--format",
                "terminal",
            ],
        )
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("1 unconfirmed shell writes; not localized to a path"),
        "{text}"
    );
    assert!(
        text.contains(
            "M notes.txt  +1 -1  2 unattributed: no surviving chain, changed by 1 recorded command\n"
        ),
        "{text}"
    );
    // A fully attributed file names no command on its line.
    let gen_line = text
        .lines()
        .find(|line| line.contains("src/gen.rs  +2"))
        .unwrap_or_else(|| panic!("{text}"));
    assert!(!gen_line.contains("changed by"), "{text}");
}
