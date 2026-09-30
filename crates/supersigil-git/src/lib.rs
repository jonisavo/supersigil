//! Runs the git CLI for supersigil review.
//!
//! Resolves a base and a target, builds the working-tree target as a tree in
//! the object database through a temporary index, lists changed paths,
//! reads blob bytes and their worktree form, and finds which worktree's own
//! HEAD reflog originated a commit. It never writes the real index, the
//! worktree, or a ref.
