//! A temporary repository the `review` and `judge` tests share, and the
//! slice fixture's working tree and transcript.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use supersigil_record::ReviewStore;
use supersigil_record::reviews;
use supersigil_review::summary::Shell;

/// `src/lib.rs` before the slice session.
pub const LIB_BASE: &str = "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n";
/// `src/lib.rs` after the session and the manual line it never recorded.
pub const LIB_FINAL: &str = "// manual\npub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\npub fn greet(name: &str) -> String {\n    format!(\"Hello, {name}!\")\n}\n";
/// `src/new.rs` as the slice session wrote it.
pub const NEW_RS: &str = "pub const VERSION: &str = \"1\";\n";

/// A temporary repository, Claude home, and records directory, with git
/// isolated from the machine's configuration.
pub struct Fixture {
    _dir: tempfile::TempDir,
    /// The temporary directory, canonical; also the isolated `HOME`.
    pub root: PathBuf,
    /// The repository's main worktree.
    pub repo: PathBuf,
    /// The Claude home discovery reads.
    pub claude: PathBuf,
    /// The records directory.
    pub records: PathBuf,
}

impl Fixture {
    /// An empty repository on `main` without line-ending conversion.
    pub fn new() -> Self {
        let (dir, root) = super::canonical_tempdir();
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

    /// Isolates git configuration and sets the records directory.
    /// Removes git hook variables, as `supersigil_git::Git` does, so pre-commit test
    /// runs do not use the outer repository's index. Removes environment-passed
    /// configuration from git and from git processes started by supersigil.
    pub fn isolate(&self, command: &mut Command) {
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
    pub fn git_in(&self, dir: &Path, args: &[&str]) -> String {
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

    /// Runs git in the repository; panics with its stderr on failure.
    pub fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.repo, args)
    }

    /// The default reviews directory, under the isolated `HOME`.
    pub fn reviews(&self) -> PathBuf {
        self.root.join(".local/share/supersigil/reviews")
    }

    /// Stages everything in `dir`, commits, and returns the commit id.
    pub fn commit(&self, dir: &Path, message: &str) -> String {
        self.git_in(dir, &["add", "-A"]);
        self.git_in(dir, &["commit", "-q", "-m", message]);
        self.git_in(dir, &["rev-parse", "HEAD"]).trim().to_owned()
    }

    /// Writes transcript `name` in `cwd`'s Claude Code project directory and returns
    /// its path. Replaces `/work/repo` in `text` with JSON-escaped `cwd`, including
    /// backslash escapes for Windows paths.
    pub fn transcript(&self, cwd: &Path, name: &str, text: &str) -> PathBuf {
        let quoted = serde_json::to_string(&cwd.to_string_lossy()).unwrap();
        let text = text.replace("/work/repo", &quoted[1..quoted.len() - 1]);
        super::project_transcript(&self.claude, cwd, name, &text)
    }

    /// Writes `text` as the transcript of agent `agent` of session `session`
    /// in the Claude Code project directory of `cwd`, and returns its path.
    pub fn agent_transcript(&self, cwd: &Path, session: &str, agent: &str, text: &str) -> PathBuf {
        self.transcript(
            cwd,
            &format!("{session}/subagents/agent-{agent}.jsonl"),
            text,
        )
    }

    /// `supersigil <args>` in `dir`, isolated, as typed.
    pub fn plain(&self, dir: &Path, args: &[&str]) -> Command {
        let mut command = super::supersigil_cmd();
        self.isolate(&mut command);
        command.current_dir(dir).args(args);
        command
    }

    /// `supersigil <subcommand> --claude-home <claude> <rest>` in `dir`.
    pub fn supersigil(&self, dir: &Path, args: &[&str]) -> Command {
        let mut command = super::supersigil_cmd();
        self.isolate(&mut command);
        command
            .current_dir(dir)
            .arg(args[0])
            .arg("--claude-home")
            .arg(&self.claude)
            .args(&args[1..]);
        command
    }

    /// The `--claude-home` flag [`Self::supersigil`] passes, as printed
    /// `review` commands repeat it.
    pub fn claude_flag(&self) -> String {
        format!("--claude-home={}", word(&self.claude.display().to_string()))
    }

    /// Runs a command expected to succeed and parses its JSON output.
    pub fn json(&self, dir: &Path, args: &[&str]) -> Value {
        let output = self.supersigil(dir, args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    /// Runs a command with `<key>=1` in supersigil's environment. Git subprocesses
    /// inherit it unless supersigil removes it. Returns the parsed JSON output.
    ///
    /// # Errors
    ///
    /// Returns standard error when the command fails.
    pub fn json_inheriting(&self, key: &str, dir: &Path, args: &[&str]) -> Result<Value, String> {
        let output = self.supersigil(dir, args).env(key, "1").output().unwrap();
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        Ok(serde_json::from_slice(&output.stdout).unwrap())
    }
}

/// `text` as commands print it in this platform's shells.
pub fn word(text: &str) -> String {
    Shell::current()
        .word(text)
        .unwrap_or_else(|| panic!("{text:?} prints as given here"))
}

/// The review store of `f`'s repository in `reviews`.
pub fn review_store(f: &Fixture, reviews: &Path) -> ReviewStore {
    reviews::find(reviews, &f.repo.join(".git"))
        .unwrap()
        .unwrap_or_else(|| panic!("no review store in {}", reviews.display()))
}

/// The id of the basis `review` saved.
pub fn basis_id(review: &Value) -> String {
    review["basis"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("no saved basis in {review:#}"))
        .to_owned()
}

/// The item named `name` in the gate of `output`, a review or a judgment.
pub fn gate_row<'v>(output: &'v Value, name: &str) -> &'v Value {
    output["gate"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == name)
        .unwrap_or_else(|| panic!("no gate item {name} in {:#}", output["gate"]))
}

/// Writes `content` to `path` under `dir`, creating directories.
pub fn write(dir: &Path, path: &str, content: &str) {
    let path = dir.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// The slice session's transcript, recorded in `/work/repo`.
pub fn slice_text() -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../supersigil-session/tests/fixtures/slice.jsonl"),
    )
    .unwrap()
}

/// Creates and commits the slice session's starting files, then writes its final
/// working tree plus the unrecorded manual line. Seeds `old.txt`, which appears
/// only in a command.
pub fn slice(f: &Fixture) {
    write(&f.repo, "src/lib.rs", LIB_BASE);
    write(&f.repo, "notes.txt", "draft\n");
    write(&f.repo, "old.txt", "obsolete\n");
    f.commit(&f.repo, "base");
    write(&f.repo, "src/lib.rs", LIB_FINAL);
    write(&f.repo, "src/new.rs", NEW_RS);
    std::fs::remove_file(f.repo.join("old.txt")).unwrap();
    f.transcript(&f.repo, "slice.jsonl", &slice_text());
}
