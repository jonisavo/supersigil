//! Git access for supersigil review, through the git CLI.
//!
//! [`Repo`] opens a worktree and [`resolve_range`] resolves the base and
//! target of a review; [`worktree`] lists worktrees. Nothing here ever
//! writes the real index, the worktree, or a ref.
//!
//! Git runs only through [`Git`], which scrubs the variables that could point
//! it at another repository or index and honors the user's configuration.

pub mod bytes;
pub mod changes;
pub mod error;
pub mod oid;
pub mod path;
pub mod repo;
pub mod run;
pub mod snapshot;
pub mod worktree;

pub use error::GitError;
pub use oid::{ObjectFormat, ObjectId};
pub use path::RepoPath;
pub use repo::{Ancestry, Base, Repo, ResolvedRange, ResolvedTarget, TargetSpec, resolve_range};
pub use run::{Git, GitVersion, MIN_VERSION};
