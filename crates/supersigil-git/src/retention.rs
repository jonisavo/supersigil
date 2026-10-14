//! What judging a review needs from git: the repository's identity across
//! worktrees, the person's identity, and a ref that keeps the reviewed
//! snapshot.

use std::path::PathBuf;

use crate::error::GitError;
use crate::oid::{ObjectId, parse_output};
use crate::path::path_from_line;
use crate::repo::Repo;
use crate::run::io_error;

/// Returns the canonical path of the repository's git common directory,
/// which every worktree of the repository shares: symlinks resolved and, on
/// Windows, no verbatim prefix, as `supersigil-session` canonicalizes
/// checkouts.
///
/// # Errors
///
/// Returns [`GitError::Parse`] if the platform cannot represent the path git
/// prints, [`GitError::Io`] if it cannot be resolved, or the errors of
/// [`crate::Git::output`].
pub fn common_dir(repo: &Repo) -> Result<PathBuf, GitError> {
    let output = repo.git().output(["rev-parse", "--git-common-dir"])?;
    // No `-z` form: strip only the line terminator, since a path may end in
    // spaces.
    let path = path_from_line(&output).ok_or_else(|| {
        GitError::Parse("common directory not representable on this platform".to_owned())
    })?;
    // A relative path is relative to the directory git ran in, the root.
    let path = repo.root().join(path);
    std::fs::canonicalize(&path)
        .map(without_verbatim_prefix)
        .map_err(|e| io_error(&format!("cannot resolve {}", path.display()), e))
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

/// Reads a configuration value as text without surrounding whitespace, as
/// git writes it into an identity, or `None` when it is unset or blank.
fn config_text(repo: &Repo, key: &str) -> Result<Option<String>, GitError> {
    let Some(output) = repo.git().probe_output(["config", "--get", key])? else {
        return Ok(None);
    };
    let text = String::from_utf8(output)
        .map_err(|_not_utf8| GitError::Parse(format!("config {key} is not UTF-8")))?;
    let text = text.trim();
    Ok((!text.is_empty()).then(|| text.to_owned()))
}

/// Returns whether `tree` names a tree in the repository: `false` for a
/// missing object, such as a captured tree git has pruned, and for an object
/// of another type.
///
/// # Errors
///
/// Returns the errors of [`crate::Git::output_with_input`].
pub fn tree_exists(repo: &Repo, tree: &ObjectId) -> Result<bool, GitError> {
    // `--batch-check` answers `<id> missing` for an absent object where
    // `cat-file -t` would fail like any other error.
    let input = format!("{tree}\n");
    let output = repo.git().output_with_input(
        ["cat-file", "--batch-check=%(objecttype)"],
        input.as_bytes(),
    )?;
    Ok(output.trim_ascii_end() == b"tree")
}

/// What [`retain`] left the reference pointing at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Retained {
    /// The reference did not exist: this commit was written and the
    /// reference created.
    Created(ObjectId),
    /// The reference already pointed at this commit, which has the expected
    /// tree and parents; nothing was written.
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

/// Keeps `tree` reachable under `reference`, a full reference name such as
/// `refs/supersigil/reviews/<id>`, through a commit with that tree,
/// `parents` in order without duplicates, and `message`, authored and
/// committed by `who`.
///
/// The reference is a direct one, created only if it does not exist and
/// never moved. When it exists, nothing is written: it is reused if it
/// points directly at a commit with exactly that tree and those parents,
/// whatever its message and identity, and refused otherwise, as a symbolic
/// reference always is: what it keeps moves with the reference it names.
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
    // An empty old value creates the reference only if it does not exist;
    // without dereferencing, a symbolic reference left at the name, even
    // one to a missing branch, counts as existing and is never followed.
    repo.git()
        .output(["update-ref", "--no-deref", reference, commit.as_str(), ""])?;
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
    /// Another reference, by name: what it keeps moves with that one.
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

/// Reads what the reference named exactly `reference` points at, or `None`
/// when there is no such reference.
fn read_ref(repo: &Repo, reference: &str) -> Result<Option<Pointed>, GitError> {
    // `for-each-ref` also lists refs below `reference/` and never fails for
    // a missing one. Fields are tab-separated, since a direct reference's
    // `symref` is empty; reference names hold no tabs, and the parents are
    // separated by spaces.
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

/// Converts `\\?\C:\x` to `C:\x` and `\\?\UNC\server\share` to
/// `\\server\share`, leaving other paths unchanged.
fn without_verbatim_prefix(path: PathBuf) -> PathBuf {
    let stripped = {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            Some(format!(r"\\{rest}"))
        } else {
            text.strip_prefix(r"\\?\").map(str::to_owned)
        }
    };
    stripped.map_or(path, PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_prefixes_are_dropped() {
        assert_eq!(
            without_verbatim_prefix(PathBuf::from(r"\\?\C:\work\repo\.git")),
            PathBuf::from(r"C:\work\repo\.git")
        );
        assert_eq!(
            without_verbatim_prefix(PathBuf::from(r"\\?\UNC\server\share\.git")),
            PathBuf::from(r"\\server\share\.git")
        );
        assert_eq!(
            without_verbatim_prefix(PathBuf::from("/work/repo/.git")),
            PathBuf::from("/work/repo/.git")
        );
    }
}
