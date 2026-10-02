//! Temporary git repositories isolated from the machine's git configuration.
//!
//! Every git process here, and every [`Git`] handed to the crate under test,
//! gets `GIT_CONFIG_NOSYSTEM=1`, `HOME` and `XDG_CONFIG_HOME` pointing into the
//! test's temporary directory, and a fixed identity, so a developer's or a
//! CI runner's configuration (Git for Windows sets `core.autocrlf` system-wide)
//! cannot change what a test sees. Environment-passed configuration is
//! also neutralized: `GIT_CONFIG_GLOBAL` names the file implied by
//! `HOME`, `GIT_CONFIG_COUNT` is zero, and `GIT_CONFIG_PARAMETERS` is
//! empty.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use supersigil_git::{Git, Repo};

/// Adds the isolation environment to `git`.
pub fn isolated(git: Git, home: &Path) -> Git {
    git.with_env("GIT_CONFIG_NOSYSTEM", "1")
        .with_env("GIT_CONFIG_GLOBAL", home.join(".gitconfig"))
        .with_env("GIT_CONFIG_COUNT", "0")
        .with_env("GIT_CONFIG_PARAMETERS", "")
        .with_env("HOME", home)
        .with_env("XDG_CONFIG_HOME", home)
        .with_env("GIT_AUTHOR_NAME", "Test")
        .with_env("GIT_AUTHOR_EMAIL", "test@example.com")
        .with_env("GIT_COMMITTER_NAME", "Test")
        .with_env("GIT_COMMITTER_EMAIL", "test@example.com")
}

/// Whether two paths name the same directory once symlinks are resolved.
pub fn same_dir(a: &Path, b: &Path) -> bool {
    std::fs::canonicalize(a).unwrap() == std::fs::canonicalize(b).unwrap()
}

/// A repository in a temporary directory, with `core.autocrlf=false`.
pub struct TestRepo {
    /// Holds the temporary directory: `home/` for configuration, `repo/` for
    /// the worktree, and room for linked worktrees beside it.
    pub dir: tempfile::TempDir,
    /// The worktree root.
    pub root: PathBuf,
}

impl TestRepo {
    /// `git init -b main` with SHA-1 objects.
    pub fn new() -> Self {
        Self::init(&[])
    }

    /// `git init -b main --object-format=sha256`.
    pub fn new_sha256() -> Self {
        Self::init(&["--object-format=sha256"])
    }

    /// `git init -b main --ref-format=reftable`, or `None` when this git
    /// cannot create a reftable repository (before 2.45).
    pub fn new_reftable() -> Option<Self> {
        Self::try_init(&["--ref-format=reftable"]).ok()
    }

    fn init(extra: &[&str]) -> Self {
        Self::try_init(extra).unwrap_or_else(|stderr| panic!("git init {extra:?}: {stderr}"))
    }

    /// `git init -b main` with `extra`, or git's stderr when it fails.
    fn try_init(extra: &[&str]) -> Result<Self, String> {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("home")).unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        let repo = Self { dir, root };
        let mut args = vec!["init", "-q", "-b", "main"];
        args.extend_from_slice(extra);
        let init = repo.git().raw(&args).unwrap();
        if !init.status.success() {
            return Err(String::from_utf8_lossy(&init.stderr).into_owned());
        }
        repo.run(&["config", "core.autocrlf", "false"]);
        Ok(repo)
    }

    /// The directory used as `HOME`.
    pub fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    /// A [`Git`] for the worktree root with the isolation environment.
    pub fn git(&self) -> Git {
        isolated(Git::new(&self.root), &self.home())
    }

    /// Opens the worktree root as a [`Repo`].
    pub fn repo(&self) -> Repo {
        Repo::open(self.git()).unwrap()
    }

    /// Runs git in the worktree root; panics on failure; returns stdout.
    pub fn run(&self, args: &[&str]) -> String {
        self.run_in(&self.root, args)
    }

    /// Runs git in `dir` with the isolation environment; panics on failure.
    pub fn run_in(&self, dir: &Path, args: &[&str]) -> String {
        self.run_with_input(dir, args, None)
    }

    /// Runs git in the worktree root with `input` on stdin; panics on failure.
    pub fn run_input(&self, args: &[&str], input: &[u8]) -> String {
        self.run_with_input(&self.root, args, Some(input))
    }

    fn run_with_input(&self, dir: &Path, args: &[&str], input: Option<&[u8]>) -> String {
        let git = self.git().in_dir(dir);
        let output = match input {
            Some(input) => git.output_with_input(args, input),
            None => git.output(args),
        }
        .unwrap_or_else(|e| panic!("git {args:?} in {}: {e}", dir.display()));
        String::from_utf8(output).unwrap()
    }

    /// Writes `bytes` to `path` under the worktree root, creating directories.
    pub fn write(&self, path: &str, bytes: &[u8]) {
        let full = self.root.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, bytes).unwrap();
    }

    /// `add -A` and commit; returns the new commit id.
    pub fn commit_all(&self, message: &str) -> String {
        self.run(&["add", "-A"]);
        self.run(&["commit", "-q", "--allow-empty", "-m", message]);
        self.run(&["rev-parse", "HEAD"]).trim().to_owned()
    }

    /// Every file in `tree`, by path, with its contents as text.
    pub fn tree_files(&self, tree: &str) -> BTreeMap<String, String> {
        let names = self.run(&["ls-tree", "-r", "-z", "--name-only", tree]);
        names
            .split('\0')
            .filter(|name| !name.is_empty())
            .map(|name| {
                let spec = format!("{tree}:{name}");
                (name.to_owned(), self.run(&["cat-file", "blob", &spec]))
            })
            .collect()
    }
}
