//! Rust ecosystem plugin for supersigil.
//!
//! This crate provides the Rust-specific integration for the supersigil
//! verification framework. It handles:
//!
//! - Parsing criterion targets from `#[verifies(...)]` attributes
//! - Discovering evidence in Rust source files via `syn`
//! - Normalizing Rust test results into `VerificationEvidenceRecord`s
//! - Resolving single-project and multi-project Cargo workspace layouts
//!
//! Consumers depend on this crate alone; the proc-macro from
//! `supersigil-rust-macros` is re-exported here.

pub mod build_support;
mod discover;
pub use discover::RustPlugin;
pub mod scope;

// Re-export the proc macro so consumers can write `#[supersigil_rust::verifies(...)]`.
pub use supersigil_rust_macros::verifies;
