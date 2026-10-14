//! Git access for supersigil review, through the git CLI.
//!
//! [`Repo`] opens a worktree and [`resolve_range`] resolves the base and
//! target of a review. The working-tree target is written by
//! [`snapshot::snapshot_working_tree`] through a temporary index, so it is the
//! tree `git add -u` would produce, and nothing here ever writes the real
//! index or the worktree. [`changes`] lists changed paths, [`bytes`] reads
//! blob contents and their worktree form, [`worktree`] lists worktrees, and
//! [`origin`] reads which worktree created a commit. [`retention`] names the
//! repository and the person for a judgment and writes the only ref this
//! crate writes: one that keeps a reviewed snapshot, created once and never
//! moved.
//!
//! Git runs only through [`Git`], which scrubs the variables that could point
//! it at another repository, index, or object store or change how it reads
//! pathspecs, disables replacement refs so every id names its own object,
//! and honors the user's configuration.

pub mod bytes;
pub mod changes;
pub mod error;
pub mod oid;
pub mod origin;
pub mod path;
pub mod repo;
pub mod retention;
pub mod run;
pub mod snapshot;
pub mod worktree;

pub use error::GitError;
pub use oid::{ObjectFormat, ObjectId};
pub use path::RepoPath;
pub use repo::{Ancestry, Base, Repo, ResolvedRange, ResolvedTarget, TargetSpec, resolve_range};
pub use run::{Git, GitVersion, MIN_VERSION};
