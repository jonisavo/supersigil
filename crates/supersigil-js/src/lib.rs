//! JS/TS ecosystem plugin for supersigil.
//!
//! This crate provides the JavaScript/TypeScript integration for the
//! supersigil verification framework. It handles:
//!
//! - Discovering JS/TS test files via configurable glob patterns
//! - Respecting `.gitignore` rules during file discovery
//! - Parsing `verifies()` calls from test files via `oxc`
//! - Normalizing JS/TS test results into `VerificationEvidenceRecord`s

mod discover;
pub use discover::JsPlugin;
