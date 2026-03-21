//! Supersigil Language Server Protocol implementation.

use std::path::{Path, PathBuf};

use lsp_types::Url;
use supersigil_core::DiagnosticsTier;

pub mod commands;
pub mod completion;
pub mod definition;
pub mod diagnostics;
pub mod hover;
pub mod position;
pub mod state;

pub(crate) const REF_ATTRS: &[&str] = &["refs", "implements", "depends"];

pub(crate) const DIAGNOSTIC_SOURCE: &str = "supersigil";

pub(crate) fn parse_tier(s: &str) -> Option<DiagnosticsTier> {
    match s {
        "lint" => Some(DiagnosticsTier::Lint),
        "verify" => Some(DiagnosticsTier::Verify),
        _ => None,
    }
}

pub(crate) fn path_to_url(path: &Path) -> Option<Url> {
    if path.is_absolute() {
        Url::from_file_path(path).ok()
    } else {
        let abs = PathBuf::from("/").join(path);
        Url::from_file_path(&abs).ok()
    }
}
