//! What anchor is asked to explain: a base and a target state of one file,
//! and the recorded edits that may connect them.

use std::path::PathBuf;

use supersigil_record::observations::{Content, Edit, FileState};
use supersigil_record::{ContentId, RecordId};

/// Contents of a file at one point in its history, or its absence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// The file does not exist.
    Absent,
    /// The file exists with these bytes.
    Present(Vec<u8>),
}

impl State {
    /// Returns the bytes, or `None` when the file is absent.
    #[must_use]
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Absent => None,
            Self::Present(bytes) => Some(bytes),
        }
    }

    /// Returns the content hash, or `None` when the file is absent.
    #[must_use]
    pub fn content_id(&self) -> Option<ContentId> {
        self.bytes().map(ContentId::of)
    }

    /// Compares this state with a recorded file state.
    ///
    /// Returns `Some(true)` when they agree and `Some(false)` when they
    /// contradict. Returns `None` when both are present and the recorded
    /// content is unknown, since nothing can then be checked. A recorded
    /// present file contradicts an absent state even when its content is
    /// unknown, because presence itself was recorded.
    #[must_use]
    pub fn matches(&self, recorded: &FileState) -> Option<bool> {
        match (self, recorded) {
            (Self::Absent, FileState::Absent) => Some(true),
            (Self::Absent, FileState::Present { .. }) | (Self::Present(_), FileState::Absent) => {
                Some(false)
            }
            (
                Self::Present(bytes),
                FileState::Present {
                    content: Content::Known(id),
                },
            ) => Some(ContentId::of(bytes) == *id),
            (
                Self::Present(_),
                FileState::Present {
                    content: Content::Unknown,
                },
            ) => None,
        }
    }

    /// Returns the number of bytes, 0 when the file is absent.
    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.bytes().map_or(0, <[u8]>::len)
    }
}

/// A recorded edit offered to anchor, with where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateEdit {
    /// Record the edit was read from.
    pub record: RecordId,
    /// Root of the worktree the edit's file lies in.
    pub worktree: PathBuf,
    /// The recorded edit.
    pub edit: Edit,
}

/// What kind of snapshot the target state was taken from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    /// The working tree, snapshotted after every pinned observation.
    WorkingTree,
    /// A commit, which observations may postdate.
    Commit,
}

/// Default work budget for one path: 64 MiB of bytes processed.
pub const DEFAULT_BUDGET_BYTES: u64 = 64 * 1024 * 1024;

/// One path to attribute.
#[derive(Debug, Clone)]
pub struct Request {
    /// The base state of the path.
    pub base: State,
    /// The target state of the path.
    pub target: State,
    /// Whether the target is a working tree or a commit.
    pub target_kind: TargetKind,
    /// Root of the worktree under review.
    pub reviewed_worktree: PathBuf,
    /// Candidate edits of this path, in record append order.
    pub edits: Vec<CandidateEdit>,
    /// Bytes the search may process before it is marked incomplete.
    pub budget_bytes: u64,
}
