//! Blob contents, and their worktree form for attribution.
//!
//! The reviewed diff uses blob bytes: the immutable object contents, so the
//! change depends only on the two trees. Attribution uses the worktree form,
//! because transcript hashes were computed over files on disk; converting
//! depends on the current attributes and configuration, so the result is
//! evidence of this review run, not a property of the tree.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;

use crate::error::GitError;
use crate::oid::ObjectId;
use crate::path::RepoPath;
use crate::repo::Repo;

/// Returns the size of each blob with `cat-file --batch-check`.
///
/// # Errors
///
/// Returns [`GitError::Parse`] if an object is missing or the output is not
/// understood, or the errors of [`crate::Git::output_with_input`].
pub fn blob_sizes(repo: &Repo, ids: &[ObjectId]) -> Result<BTreeMap<ObjectId, u64>, GitError> {
    let unique: BTreeSet<&ObjectId> = ids.iter().collect();
    let output = repo
        .git()
        .output_with_input(["cat-file", "--batch-check"], &request(&unique))?;
    let text = String::from_utf8_lossy(&output);
    let mut sizes = BTreeMap::new();
    for line in text.lines() {
        let (id, size) = parse_header(line, repo)?;
        sizes.insert(id, size);
    }
    Ok(sizes)
}

/// Reads each blob's bytes with one `cat-file --batch` process.
///
/// # Errors
///
/// Returns [`GitError::Parse`] if an object is missing or the output is not
/// understood, or the errors of [`crate::Git::output_with_input`].
pub fn read_blobs(repo: &Repo, ids: &[ObjectId]) -> Result<BTreeMap<ObjectId, Vec<u8>>, GitError> {
    let unique: BTreeSet<&ObjectId> = ids.iter().collect();
    let output = repo
        .git()
        .output_with_input(["cat-file", "--batch"], &request(&unique))?;
    let mut blobs = BTreeMap::new();
    let mut rest: &[u8] = &output;
    while !rest.is_empty() {
        let newline = rest
            .iter()
            .position(|&b| b == b'\n')
            .ok_or_else(|| GitError::Parse("cat-file header without a newline".to_owned()))?;
        let header = String::from_utf8_lossy(&rest[..newline]);
        let (id, size) = parse_header(&header, repo)?;
        let start = newline + 1;
        let end = usize::try_from(size)
            .ok()
            .and_then(|size| start.checked_add(size))
            .filter(|&end| end < rest.len() && rest[end] == b'\n')
            .ok_or_else(|| GitError::Parse(format!("cat-file contents of {id} are truncated")))?;
        blobs.insert(id, rest[start..end].to_vec());
        rest = &rest[end + 1..];
    }
    Ok(blobs)
}

/// One object id per line.
fn request(ids: &BTreeSet<&ObjectId>) -> Vec<u8> {
    let mut input = Vec::new();
    for id in ids {
        input.extend_from_slice(id.as_str().as_bytes());
        input.push(b'\n');
    }
    input
}

/// Parses `<id> blob <size>`; anything else (a missing object, another
/// type) is an error.
fn parse_header(line: &str, repo: &Repo) -> Result<(ObjectId, u64), GitError> {
    let parts: Vec<&str> = line.split(' ').collect();
    match parts.as_slice() {
        [id, "blob", size] => {
            let size = size
                .parse()
                .map_err(|_not_a_size| GitError::Parse(format!("cat-file size in {line:?}")))?;
            Ok((ObjectId::parse(id, repo.format())?, size))
        }
        _ => Err(GitError::Parse(format!("cat-file header {line:?}"))),
    }
}

/// The worktree form of a blob, compared with its blob bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conversion {
    /// The worktree form is byte-identical to the blob.
    Identical,
    /// The worktree form differs, for example in line endings.
    Converted(Vec<u8>),
    /// Git could not convert it, for example because a required filter
    /// failed.
    Failed {
        /// Exit code, `None` when git was ended by a signal.
        status: Option<i32>,
        /// The end of standard error.
        stderr_tail: String,
    },
}

/// Characters of standard error kept in [`Conversion::Failed`].
const STDERR_TAIL_CHARS: usize = 500;

/// Converts `blob` to its worktree form for `path` with
/// `cat-file --filters --path=<path>`, one process per blob: on git 2.55,
/// `--batch --filters` writes the filtered bytes but reports the unfiltered
/// size in its header, so batch framing breaks. Attributes are read from the
/// worktree as it is now.
///
/// # Errors
///
/// Returns [`GitError::Parse`] if `path` cannot be passed to git on this
/// platform, [`GitError::Parse`] if `blob` is missing, or the errors of
/// [`crate::Git::raw`]. A conversion git refuses is
/// [`Conversion::Failed`], not an error.
pub fn worktree_form(
    repo: &Repo,
    path: &RepoPath,
    blob: &ObjectId,
    blob_bytes: &[u8],
) -> Result<Conversion, GitError> {
    let Some(os_path) = path.to_os_string() else {
        return Err(GitError::Parse(format!(
            "path {} is not representable on this platform",
            path.escaped()
        )));
    };
    let mut path_arg = OsString::from("--path=");
    path_arg.push(os_path);
    let output = repo.git().raw([
        OsString::from("cat-file"),
        OsString::from("--filters"),
        path_arg,
        OsString::from(blob.as_str()),
    ])?;
    if !output.status.success() {
        // Only a conversion git refuses is a result. If the blob itself cannot
        // be read, this is an error, not a failed conversion.
        // `cat-file -e` documents exit 1 for a missing object; any other
        // failure stays an error of git itself.
        let check = repo.git().raw(["cat-file", "-e", blob.as_str()])?;
        match check.status.code() {
            Some(0) => {}
            Some(1) => return Err(GitError::Parse(format!("object {blob} is missing"))),
            status => {
                return Err(GitError::Failed {
                    args: format!("cat-file -e {blob}"),
                    status,
                    stderr: String::from_utf8_lossy(&check.stderr).trim().to_owned(),
                });
            }
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let chars: Vec<char> = stderr.trim().chars().collect();
        let tail: String = chars[chars.len().saturating_sub(STDERR_TAIL_CHARS)..]
            .iter()
            .collect();
        return Ok(Conversion::Failed {
            status: output.status.code(),
            stderr_tail: tail,
        });
    }
    if output.stdout == blob_bytes {
        Ok(Conversion::Identical)
    } else {
        Ok(Conversion::Converted(output.stdout))
    }
}
