//! Opens a worktree and resolves the base and target of a review.

use std::path::{Path, PathBuf};

use crate::error::GitError;
use crate::oid::{ObjectFormat, ObjectId, parse_output};
use crate::path::path_from_line;
use crate::run::{Git, GitVersion, MIN_VERSION, failed};

/// A git worktree opened for review: its root, object format, and git version.
#[derive(Debug, Clone)]
pub struct Repo {
    git: Git,
    root: PathBuf,
    format: ObjectFormat,
    version: GitVersion,
}

impl Repo {
    /// Opens the worktree containing `git`'s directory.
    ///
    /// Checks the git version first, then that the directory is inside a
    /// worktree of a non-bare repository.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::NotFound`] or [`GitError::TooOld`] for an unusable
    /// git, [`GitError::Bare`] for a bare repository,
    /// [`GitError::NotAWorktree`] outside any worktree (including inside a
    /// `.git` directory), or the errors of [`Git::output`].
    pub fn open(git: Git) -> Result<Self, GitError> {
        let version = git.version()?;
        if version < MIN_VERSION {
            return Err(GitError::TooOld {
                found: version,
                required: MIN_VERSION,
            });
        }
        let bare = git.raw(["rev-parse", "--is-bare-repository"])?;
        if !bare.status.success() {
            return Err(GitError::NotAWorktree(git.cwd().to_path_buf()));
        }
        if bare.stdout.trim_ascii() == b"true" {
            return Err(GitError::Bare(git.cwd().to_path_buf()));
        }
        let top = git.raw(["rev-parse", "--show-toplevel"])?;
        // `--show-toplevel` has no `-z` form: strip exactly the one newline it
        // prints, so a directory name ending in spaces survives.
        let root = top
            .status
            .success()
            .then(|| path_from_line(&top.stdout))
            .flatten()
            .filter(|root| !root.as_os_str().is_empty())
            .ok_or_else(|| GitError::NotAWorktree(git.cwd().to_path_buf()))?;
        let format_output = git.output(["rev-parse", "--show-object-format"])?;
        let format = ObjectFormat::parse(&String::from_utf8_lossy(&format_output))?;
        Ok(Self {
            git: git.moved_to(&root),
            root,
            format,
            version,
        })
    }

    /// Returns a runner whose directory is the worktree root.
    #[must_use]
    pub fn git(&self) -> &Git {
        &self.git
    }

    /// Returns the worktree root as git prints it.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the repository's object format.
    #[must_use]
    pub fn format(&self) -> ObjectFormat {
        self.format
    }

    /// Returns the git version found when the repository was opened.
    #[must_use]
    pub fn version(&self) -> GitVersion {
        self.version
    }

    /// Resolves `rev` to a commit id.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::UnknownRevision`] if `rev` does not name a commit,
    /// or the errors of [`Git::raw`].
    pub fn resolve_commit(&self, rev: &str) -> Result<ObjectId, GitError> {
        let spec = format!("{rev}^{{commit}}");
        let output = self.git.raw([
            "rev-parse",
            "--verify",
            "--quiet",
            "--end-of-options",
            &spec,
        ])?;
        if !output.status.success() {
            return Err(GitError::UnknownRevision(rev.to_owned()));
        }
        parse_output(&output.stdout, self.format)
    }

    /// Returns the tree of `commit`.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`Git::output`], or [`GitError::Parse`] for
    /// output that is not an object id.
    pub fn commit_tree(&self, commit: &ObjectId) -> Result<ObjectId, GitError> {
        let spec = format!("{commit}^{{tree}}");
        let output = self.git.output(["rev-parse", "--verify", &spec])?;
        parse_output(&output, self.format)
    }

    /// Returns the commit HEAD points at, or `None` when HEAD is a symbolic
    /// ref to a branch that does not exist yet (an unborn branch).
    ///
    /// # Errors
    ///
    /// Returns [`GitError::Failed`] when HEAD cannot be read for any other
    /// reason: a detached or symbolic HEAD whose commit object is missing, a
    /// corrupt HEAD, or a fatal git failure. Also returns the errors of
    /// [`Git::raw`], or [`GitError::Parse`] for output that is not an object
    /// id.
    pub fn head(&self) -> Result<Option<ObjectId>, GitError> {
        let args = ["rev-parse", "--verify", "--quiet", "HEAD^{commit}"];
        let output = self.git.raw(args)?;
        if output.status.success() {
            return parse_output(&output.stdout, self.format).map(Some);
        }
        if output.status.code() != Some(1) {
            return Err(failed(&args, &output));
        }
        // Exit 1 means HEAD did not resolve to a commit: unborn only if HEAD
        // is a symbolic ref whose branch does not exist.
        let symbolic = self.git.raw(["symbolic-ref", "--quiet", "HEAD"])?;
        if !symbolic.status.success() {
            return Err(failed(&args, &output));
        }
        let name = String::from_utf8_lossy(symbolic.stdout.trim_ascii()).into_owned();
        let show = ["show-ref", "--verify", "--quiet", "--end-of-options", &name];
        let exists = self.git.raw(show)?;
        match exists.status.code() {
            Some(1) => Ok(None),
            _ => Err(failed(&args, &output)),
        }
    }

    /// Returns the first parent of `commit`, or `None` for a root commit.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::MissingParent`] when the commit names a parent
    /// whose object cannot be read (a shallow boundary or otherwise
    /// unavailable history), never treating it as a root. Also returns the
    /// errors of [`Git::output`], or [`GitError::Parse`] for a commit or id
    /// this crate cannot read.
    pub(crate) fn first_parent(&self, commit: &ObjectId) -> Result<Option<ObjectId>, GitError> {
        let output = self.git.output(["cat-file", "commit", commit.as_str()])?;
        // Headers end at the first blank line; a commit message is not parsed.
        let header_end = output
            .windows(2)
            .position(|w| w == b"\n\n")
            .map_or(output.len(), |i| i + 1);
        let parent = output[..header_end]
            .split(|&b| b == b'\n')
            .find_map(|line| line.strip_prefix(b"parent "));
        let Some(parent) = parent else {
            return Ok(None);
        };
        let parent = ObjectId::parse(&String::from_utf8_lossy(parent), self.format)?;
        let spec = format!("{parent}^{{commit}}");
        let readable = self.git.raw(["cat-file", "-e", &spec])?;
        if !readable.status.success() {
            return Err(GitError::MissingParent {
                commit: commit.clone(),
                parent,
            });
        }
        Ok(Some(parent))
    }

    /// Writes the empty tree and returns its id. The id depends on the object
    /// format, so it is asked of git rather than hard-coded.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`Git::output_with_input`], or
    /// [`GitError::Parse`] for output that is not an object id.
    pub fn empty_tree(&self) -> Result<ObjectId, GitError> {
        let output = self.git.output_with_input(["mktree"], b"")?;
        parse_output(&output, self.format)
    }

    /// Returns whether `ancestor` is an ancestor of `descendant` (a commit is
    /// its own ancestor).
    ///
    /// # Errors
    ///
    /// Returns [`GitError::Failed`] for an exit status other than 0 or 1, or
    /// the errors of [`Git::raw`].
    pub(crate) fn is_ancestor(
        &self,
        ancestor: &ObjectId,
        descendant: &ObjectId,
    ) -> Result<bool, GitError> {
        self.git.probe([
            "merge-base",
            "--is-ancestor",
            ancestor.as_str(),
            descendant.as_str(),
        ])
    }

    /// Returns the commits reachable from `target` but not from `base`,
    /// newest first; every commit reachable from `target` when `base` is
    /// `None`.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`Git::output`], or [`GitError::Parse`] for
    /// output that is not a list of object ids.
    pub(crate) fn commits_in_range(
        &self,
        base: Option<&ObjectId>,
        target: &ObjectId,
    ) -> Result<Vec<ObjectId>, GitError> {
        let mut args = vec!["rev-list".to_owned(), target.to_string()];
        if let Some(base) = base {
            args.push(format!("^{base}"));
        }
        let output = self.git.output(&args)?;
        String::from_utf8_lossy(&output)
            .lines()
            .map(|line| ObjectId::parse(line, self.format))
            .collect()
    }

    /// Reads a boolean configuration value, or `None` when it is unset.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::Failed`] when git rejects the key or the value is
    /// not a boolean, or the errors of [`Git::raw`].
    pub fn config_bool(&self, key: &str) -> Result<Option<bool>, GitError> {
        let output = self
            .git
            .probe_output(["config", "--type=bool", "--get", key])?;
        output
            .map(|stdout| match stdout.trim_ascii() {
                b"true" => Ok(true),
                b"false" => Ok(false),
                other => Err(GitError::Parse(format!(
                    "config {key}: {}",
                    String::from_utf8_lossy(other)
                ))),
            })
            .transpose()
    }
}

/// What the review compares against the base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetSpec {
    /// The worktree's on-disk state, captured by
    /// [`crate::snapshot::snapshot_working_tree`].
    WorkingTree,
    /// A revision naming a commit.
    Commit(String),
}

/// The base of a review: a commit and its tree, or the empty tree with no
/// commit (for a root commit target or an unborn HEAD).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base {
    /// The base commit, `None` for the empty tree.
    pub commit: Option<ObjectId>,
    /// The base tree.
    pub tree: ObjectId,
}

/// Whether the base commit is an ancestor of the target commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Ancestry {
    /// The base is an ancestor: the diff shows only changes made on the
    /// target side.
    Ancestor,
    /// The base is not an ancestor: the diff also shows changes made on the
    /// base side.
    NotAncestor,
    /// One endpoint is not a commit (an empty-tree base or an unborn HEAD).
    Unavailable,
}

/// The resolved target of a review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedTarget {
    /// The working tree; its tree is written later by
    /// [`crate::snapshot::snapshot_working_tree`].
    WorkingTree {
        /// The commit HEAD points at, `None` when unborn.
        head: Option<ObjectId>,
    },
    /// A commit.
    Commit {
        /// The target commit.
        commit: ObjectId,
        /// Its tree.
        tree: ObjectId,
    },
}

/// Base, target, and their ancestry for one review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRange {
    /// The base.
    pub base: Base,
    /// The target.
    pub target: ResolvedTarget,
    /// Whether the base commit is an ancestor of the target commit.
    pub ancestry: Ancestry,
}

impl ResolvedRange {
    /// Returns the target commit, or HEAD for a working-tree target; `None`
    /// when HEAD is unborn.
    #[must_use]
    pub fn target_commit(&self) -> Option<&ObjectId> {
        match &self.target {
            ResolvedTarget::WorkingTree { head } => head.as_ref(),
            ResolvedTarget::Commit { commit, .. } => Some(commit),
        }
    }

    /// Returns the commits in base..target, newest first: every commit
    /// reachable from the target commit when the base is the empty tree, and
    /// none when HEAD is unborn.
    ///
    /// # Errors
    ///
    /// Returns the errors of the range walk this lists commits with.
    pub fn commits(&self, repo: &Repo) -> Result<Vec<ObjectId>, GitError> {
        match self.target_commit() {
            Some(target) => repo.commits_in_range(self.base.commit.as_ref(), target),
            None => Ok(Vec::new()),
        }
    }
}

/// Resolves the base and target of a review.
///
/// The base defaults to HEAD for a working-tree target and to the first
/// parent for a commit target; it is the empty tree for a root commit target
/// or an unborn HEAD. An explicit base is used as given (two-dot, no
/// merge-base). Ancestry is checked only when both endpoints are commits.
///
/// # Errors
///
/// Returns [`GitError::UnknownRevision`] if the base or target does not name
/// a commit, or the errors of the [`Repo`] calls it makes.
pub fn resolve_range(
    repo: &Repo,
    base: Option<&str>,
    target: &TargetSpec,
) -> Result<ResolvedRange, GitError> {
    let target = match target {
        TargetSpec::WorkingTree => ResolvedTarget::WorkingTree { head: repo.head()? },
        TargetSpec::Commit(rev) => {
            let commit = repo.resolve_commit(rev)?;
            let tree = repo.commit_tree(&commit)?;
            ResolvedTarget::Commit { commit, tree }
        }
    };
    let base_commit = match (base, &target) {
        (Some(rev), _) => Some(repo.resolve_commit(rev)?),
        (None, ResolvedTarget::WorkingTree { head }) => head.clone(),
        (None, ResolvedTarget::Commit { commit, .. }) => repo.first_parent(commit)?,
    };
    let base = match base_commit {
        Some(commit) => Base {
            tree: repo.commit_tree(&commit)?,
            commit: Some(commit),
        },
        None => Base {
            commit: None,
            tree: repo.empty_tree()?,
        },
    };
    let mut range = ResolvedRange {
        base,
        target,
        ancestry: Ancestry::Unavailable,
    };
    if let (Some(base), Some(target)) = (&range.base.commit, range.target_commit()) {
        range.ancestry = if repo.is_ancestor(base, target)? {
            Ancestry::Ancestor
        } else {
            Ancestry::NotAncestor
        };
    }
    Ok(range)
}
