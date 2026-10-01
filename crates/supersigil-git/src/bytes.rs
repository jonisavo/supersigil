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
use crate::run::{Bounded, stderr_tail};

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

/// A blob as [`read_blobs_within`] returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blob {
    /// The blob's bytes.
    Read(Vec<u8>),
    /// The blob is larger than the limit; its size, with nothing read.
    TooLarge(u64),
}

/// Reads each blob of at most `max` bytes and only sizes the larger ones:
/// every size is asked of git ([`blob_sizes`]) before anything is read
/// ([`read_blobs`]), so the limit bounds what is read. Every id gets an
/// entry.
///
/// # Errors
///
/// Returns [`GitError::Parse`] if an object is missing, whether or not it
/// would be read, and the errors of [`blob_sizes`] and [`read_blobs`].
pub fn read_blobs_within(
    repo: &Repo,
    ids: &[ObjectId],
    max: u64,
) -> Result<BTreeMap<ObjectId, Blob>, GitError> {
    let sizes = blob_sizes(repo, ids)?;
    let small: Vec<ObjectId> = sizes
        .iter()
        .filter(|(_, size)| **size <= max)
        .map(|(id, _)| id.clone())
        .collect();
    let mut read = if small.is_empty() {
        BTreeMap::new()
    } else {
        read_blobs(repo, &small)?
    };
    let mut blobs = BTreeMap::new();
    for id in ids.iter().collect::<BTreeSet<_>>() {
        let size = *sizes
            .get(id)
            .ok_or_else(|| GitError::Parse(format!("cat-file reported no size for {id}")))?;
        let blob = if size <= max {
            Blob::Read(
                read.remove(id)
                    .ok_or_else(|| GitError::Parse(format!("cat-file returned no {id}")))?,
            )
        } else {
            Blob::TooLarge(size)
        };
        blobs.insert(id.clone(), blob);
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
    /// The worktree form is larger than the limit: reading stopped one byte
    /// past it, and git was stopped.
    TooLarge,
    /// Git could not convert it, for example because a required filter
    /// failed.
    Failed {
        /// Exit code, `None` when git was ended by a signal.
        status: Option<i32>,
        /// The end of standard error.
        stderr_tail: String,
    },
}

/// Converts `blob` to its worktree form for `path` with
/// `cat-file --filters --path=<path>`, one process per blob: on git 2.55,
/// `--batch --filters` writes the filtered bytes but reports the unfiltered
/// size in its header, so batch framing breaks. Attributes are read from the
/// worktree as it is now.
///
/// At most `max` bytes of the worktree form are read: a filter can expand a
/// small blob without limit, so one more byte ends the read and stops git,
/// and the result is [`Conversion::TooLarge`].
///
/// # Errors
///
/// Returns [`GitError::Parse`] if `path` cannot be passed to git on this
/// platform, [`GitError::Parse`] if `blob` is missing, or
/// [`GitError::NotFound`] or [`GitError::Io`] if git cannot be started,
/// read from, or waited for. A conversion git refuses is
/// [`Conversion::Failed`], not an error.
pub fn worktree_form(
    repo: &Repo,
    path: &RepoPath,
    blob: &ObjectId,
    blob_bytes: &[u8],
    max: u64,
) -> Result<Conversion, GitError> {
    let Some(os_path) = path.to_os_string() else {
        return Err(GitError::Parse(format!(
            "path {} is not representable on this platform",
            path.escaped()
        )));
    };
    let mut path_arg = OsString::from("--path=");
    path_arg.push(os_path);
    let args = [
        OsString::from("cat-file"),
        OsString::from("--filters"),
        path_arg,
        OsString::from(blob.as_str()),
    ];
    let output = match repo.git().output_within(args, max)? {
        Bounded::Finished(output) => output,
        Bounded::TooLarge => return Ok(Conversion::TooLarge),
    };
    if !output.status.success() {
        // Only a conversion git refuses is a result. If the blob itself cannot
        // be read, this is an error, not a failed conversion.
        // `cat-file -e` documents exit 1 for a missing object; any other
        // failure stays an error of git itself.
        if !repo.git().probe(["cat-file", "-e", blob.as_str()])? {
            return Err(GitError::Parse(format!("object {blob} is missing")));
        }
        return Ok(Conversion::Failed {
            status: output.status.code(),
            stderr_tail: stderr_tail(&String::from_utf8_lossy(&output.stderr)),
        });
    }
    if output.stdout == blob_bytes {
        Ok(Conversion::Identical)
    } else {
        Ok(Conversion::Converted(output.stdout))
    }
}

/// Splits NUL-terminated output at every NUL, keeping empty fields: callers
/// that read an empty field as a record separator need them.
pub(crate) fn nul_split(output: &[u8]) -> impl Iterator<Item = &[u8]> {
    output.split(|&b| b == 0)
}

/// Splits NUL-terminated output into its non-empty fields.
pub(crate) fn nul_fields(output: &[u8]) -> impl Iterator<Item = &[u8]> {
    nul_split(output).filter(|field| !field.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nul_fields_skip_the_terminator() {
        let fields: Vec<&[u8]> = nul_fields(b"a\0b c\0").collect();
        assert_eq!(fields, [&b"a"[..], &b"b c"[..]]);
        assert_eq!(nul_fields(b"").count(), 0);
    }

    #[test]
    fn nul_split_keeps_empty_fields() {
        let fields: Vec<&[u8]> = nul_split(b"a\0\0b\0").collect();
        assert_eq!(fields, [&b"a"[..], b"", b"b", b""]);
    }
}
