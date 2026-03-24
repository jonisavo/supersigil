//! Test helper functions shared across workspace crates.
//!
//! Enabled by the `test-helpers` feature. Do not use in production code.

#![allow(
    clippy::must_use_candidate,
    reason = "test helper constructors — must_use is noise"
)]

use std::collections::HashMap;
use std::path::PathBuf;

use crate::{
    ACCEPTANCE_CRITERIA, CRITERION, Config, DEPENDS_ON, ExtractedComponent, Frontmatter,
    SourcePosition, SpecDocument,
};

/// Build a `SourcePosition` from a line number (`byte_offset` = line * 40).
pub fn pos(line: usize) -> SourcePosition {
    SourcePosition {
        byte_offset: line * 40,
        line,
        column: 1,
    }
}

/// Build a `SpecDocument` with path derived from id as `specs/{id}.md`.
pub fn make_doc(id: &str, components: Vec<ExtractedComponent>) -> SpecDocument {
    SpecDocument {
        path: PathBuf::from(format!("specs/{id}.md")),
        frontmatter: Frontmatter {
            id: id.to_owned(),
            doc_type: None,
            status: None,
        },
        extra: HashMap::new(),
        components,
        warnings: Vec::new(),
    }
}

/// Build a `Criterion` component.
pub fn make_criterion(id: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: CRITERION.to_owned(),
        attributes: HashMap::from([("id".to_owned(), id.to_owned())]),
        children: Vec::new(),
        body_text: Some(format!("criterion {id}")),
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: Vec::new(),
        position: pos(line),
        end_position: pos(line + 1),
    }
}

/// Build an `AcceptanceCriteria` wrapper component.
pub fn make_acceptance_criteria(
    children: Vec<ExtractedComponent>,
    line: usize,
) -> ExtractedComponent {
    ExtractedComponent {
        name: ACCEPTANCE_CRITERIA.to_owned(),
        attributes: HashMap::new(),
        children,
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: Vec::new(),
        position: pos(line),
        end_position: pos(line + 1),
    }
}

/// Build a `DependsOn` component.
pub fn make_depends_on(refs: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: DEPENDS_ON.to_owned(),
        attributes: HashMap::from([("refs".to_owned(), refs.to_owned())]),
        children: Vec::new(),
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: Vec::new(),
        position: pos(line),
        end_position: pos(line + 1),
    }
}

/// Build a default single-project `Config`.
pub fn single_project_config() -> Config {
    Config {
        paths: Some(vec!["specs/**/*.md".to_owned()]),
        ..Config::default()
    }
}
