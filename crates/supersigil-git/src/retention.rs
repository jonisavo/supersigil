//! Reads the git common directory and user identity, and retains judged trees.

use std::path::PathBuf;
use std::slice;

use crate::bytes::request;
use crate::error::GitError;
use crate::oid::{ObjectId, parse_output};
use crate::path::path_from_line;
use crate::repo::Repo;

/// Returns the absolute git common directory shared by `repo`'s worktrees.
/// The path is not resolved. Canonicalize it before comparing repositories,
/// since worktrees may reach it through different symlinks.
///
/// # Errors
///
/// Returns [`GitError::Parse`] if the platform cannot represent the path git
/// prints, or the errors of [`crate::Git::output`].
pub fn common_dir(repo: &Repo) -> Result<PathBuf, GitError> {
    let output = repo.git().output(["rev-parse", "--git-common-dir"])?;
    // Git has no `-z` form here. Keep trailing spaces in the path.
    let path = path_from_line(&output).ok_or_else(|| {
        GitError::Parse("common directory not representable on this platform".to_owned())
    })?;
    // A relative path is relative to the directory git ran in, the root.
    Ok(repo.root().join(path))
}

/// The person's git identity, from `user.name` and `user.email`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// `user.name`.
    pub name: String,
    /// `user.email`.
    pub email: String,
}

/// Returns `user.name` and `user.email` as git resolves them in `repo`, or
/// `None` when either is unset or blank.
///
/// # Errors
///
/// Returns [`GitError::Failed`] when git cannot read the configuration,
/// [`GitError::Parse`] if a value is not UTF-8, or the errors of
/// [`crate::Git::raw`].
pub fn identity(repo: &Repo) -> Result<Option<Identity>, GitError> {
    let name = config_text(repo, "user.name")?;
    let email = config_text(repo, "user.email")?;
    Ok(name
        .zip(email)
        .map(|(name, email)| Identity { name, email }))
}

/// Trims a git configuration value; returns `None` if unset or blank.
fn config_text(repo: &Repo, key: &str) -> Result<Option<String>, GitError> {
    let Some(output) = repo.git().probe_output(["config", "--get", key])? else {
        return Ok(None);
    };
    let text = String::from_utf8(output)
        .map_err(|_not_utf8| GitError::Parse(format!("config {key} is not UTF-8")))?;
    let text = text.trim();
    Ok((!text.is_empty()).then(|| text.to_owned()))
}

/// Returns `false` for missing objects, including pruned captured trees,
/// and objects of any type other than tree.
///
/// # Errors
///
/// Returns the errors of [`trees_exist`].
pub fn tree_exists(repo: &Repo, tree: &ObjectId) -> Result<bool, GitError> {
    Ok(trees_exist(repo, slice::from_ref(tree))? == [true])
}

/// Checks `trees` in order with one git call, using [`tree_exists`] rules.
/// An empty input runs no git command.
///
/// # Errors
///
/// Returns [`GitError::Parse`] if git does not answer once for each tree,
/// and the errors of [`crate::Git::output_with_input`].
pub fn trees_exist(repo: &Repo, trees: &[ObjectId]) -> Result<Vec<bool>, GitError> {
    if trees.is_empty() {
        return Ok(Vec::new());
    }
    // `--batch-check` answers `<id> missing` for an absent object where
    // `cat-file -t` would fail like any other error.
    let output = repo
        .git()
        .output_with_input(["cat-file", "--batch-check=%(objecttype)"], &request(trees))?;
    let answers: Vec<bool> = output
        .strip_suffix(b"\n")
        .unwrap_or(&output)
        .split(|&byte| byte == b'\n')
        .map(|answer| answer.trim_ascii_end() == b"tree")
        .collect();
    if answers.len() != trees.len() {
        return Err(GitError::Parse(format!(
            "cat-file answered {} lines for {} objects",
            answers.len(),
            trees.len()
        )));
    }
    Ok(answers)
}

/// The commit retained by [`retain`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Retained {
    /// The new commit and ref.
    Created(ObjectId),
    /// An existing commit with the expected tree and parents. Nothing was written.
    Existing(ObjectId),
}

impl Retained {
    /// Returns the commit the reference points at.
    #[must_use]
    pub fn commit(&self) -> &ObjectId {
        match self {
            Self::Created(commit) | Self::Existing(commit) => commit,
        }
    }
}

/// Retains `tree` under a full ref name, such as `refs/supersigil/reviews/<id>`.
/// The commit has `parents` in order without duplicates and `message`, with
/// `who` as both author and committer.
///
/// Creates a direct ref only if absent and never moves it. An existing ref
/// writes nothing and is reused only if it points directly to a commit with
/// exactly that tree and those parents, regardless of message or identity.
/// Symbolic refs are always refused because their targets can move.
///
/// # Errors
///
/// Returns [`GitError::RefMismatch`] if the reference exists and is
/// symbolic or points at anything else, or the errors of the git commands
/// it runs, including [`GitError::Failed`] when the reference cannot be
/// created.
pub fn retain(
    repo: &Repo,
    reference: &str,
    tree: &ObjectId,
    parents: &[ObjectId],
    message: &str,
    who: &Identity,
) -> Result<Retained, GitError> {
    let mut unique: Vec<&ObjectId> = Vec::with_capacity(parents.len());
    for parent in parents {
        if !unique.contains(&parent) {
            unique.push(parent);
        }
    }
    // `for-each-ref` omits a symbolic reference to a missing branch, so ask
    // for symbolic references first, before anything is written.
    let symbolic = repo
        .git()
        .probe_output(["symbolic-ref", "--quiet", reference])?;
    if let Some(target) = symbolic {
        let target = String::from_utf8_lossy(&target).trim_end().to_owned();
        return Pointed::Symbolic { target }.matching(reference, tree, &unique);
    }
    if let Some(existing) = read_ref(repo, reference)? {
        return existing.matching(reference, tree, &unique);
    }
    let mut args = vec!["commit-tree", "-m", message];
    for parent in &unique {
        args.extend(["-p", parent.as_str()]);
    }
    args.push(tree.as_str());
    // The identity is the person's, whatever the environment says.
    let git = repo
        .git()
        .clone()
        .with_env("GIT_AUTHOR_NAME", &who.name)
        .with_env("GIT_AUTHOR_EMAIL", &who.email)
        .with_env("GIT_COMMITTER_NAME", &who.name)
        .with_env("GIT_COMMITTER_EMAIL", &who.email);
    let commit = parse_output(&git.output(&args)?, repo.format())?;
    // The empty old value requires an absent ref. `--no-deref` rejects any
    // symbolic ref, including one to a missing branch, without following it.
    // Override configured lock waits to keep the total within 2 s. Older git
    // ignores the reftable key.
    repo.git().output([
        "-c",
        "core.filesRefLockTimeout=1000",
        "-c",
        "core.packedRefsTimeout=1000",
        "-c",
        "reftable.lockTimeout=1000",
        "update-ref",
        "--no-deref",
        reference,
        commit.as_str(),
        "",
    ])?;
    Ok(Retained::Created(commit))
}

/// What an existing reference points at.
enum Pointed {
    /// A commit, with its tree and its parents in order.
    Commit {
        id: ObjectId,
        tree: ObjectId,
        parents: Vec<ObjectId>,
    },
    /// An object of another type, such as `tree`.
    Other { kind: String },
    /// A symbolic ref whose target can move.
    Symbolic { target: String },
}

impl Pointed {
    /// Returns [`Retained::Existing`] if this is a commit with `tree` and
    /// `parents`, or [`GitError::RefMismatch`] naming the difference.
    fn matching(
        self,
        reference: &str,
        tree: &ObjectId,
        parents: &[&ObjectId],
    ) -> Result<Retained, GitError> {
        let reason = match self {
            Self::Symbolic { target } => format!("is a symbolic reference to {target}"),
            Self::Other { kind } => format!("points at a {kind}, not a commit"),
            Self::Commit {
                id, tree: found, ..
            } if found != *tree => {
                format!("points at commit {id} with tree {found}, not {tree}")
            }
            Self::Commit {
                id, parents: found, ..
            } if !found.iter().eq(parents.iter().copied()) => format!(
                "points at commit {id} with parents {}, not {}",
                listed(found.iter()),
                listed(parents.iter().copied()),
            ),
            Self::Commit { id, .. } => return Ok(Retained::Existing(id)),
        };
        Err(GitError::RefMismatch {
            reference: reference.to_owned(),
            reason,
        })
    }
}

/// Lists ids separated by spaces, or `none`.
fn listed<'a>(ids: impl Iterator<Item = &'a ObjectId>) -> String {
    let text = ids.map(ObjectId::as_str).collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        "none".to_owned()
    } else {
        text
    }
}

/// Reads exactly `reference`; returns `None` if absent.
fn read_ref(repo: &Repo, reference: &str) -> Result<Option<Pointed>, GitError> {
    // `for-each-ref` also lists `reference/` descendants and accepts absent
    // refs. Tabs preserve an empty `symref` field; ref names contain no tabs
    // and parent ids use spaces.
    let output = repo.git().output([
        "for-each-ref",
        "--format=%(refname)%09%(symref)%09%(objecttype)%09%(objectname)%09%(tree)%09%(parent)",
        reference,
    ])?;
    let text = std::str::from_utf8(&output)
        .map_err(|_not_utf8| GitError::Parse("for-each-ref output is not UTF-8".to_owned()))?;
    let Some(line) = text
        .lines()
        .find(|line| line.split('\t').next() == Some(reference))
    else {
        return Ok(None);
    };
    let unexpected = || GitError::Parse(format!("for-each-ref line {line:?}"));
    let [_name, target, kind, id, tree, parents] = line
        .split('\t')
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_fields| unexpected())?;
    if !target.is_empty() {
        return Ok(Some(Pointed::Symbolic {
            target: target.to_owned(),
        }));
    }
    if kind != "commit" {
        return Ok(Some(Pointed::Other {
            kind: kind.to_owned(),
        }));
    }
    let parse = |field: &str| ObjectId::parse(field, repo.format());
    Ok(Some(Pointed::Commit {
        id: parse(id)?,
        tree: parse(tree)?,
        parents: parents
            .split_ascii_whitespace()
            .map(parse)
            .collect::<Result<_, _>>()?,
    }))
}
