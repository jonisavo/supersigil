#![allow(dead_code, reason = "shared test helpers — not all used yet")]
#![allow(
    missing_docs,
    clippy::must_use_candidate,
    clippy::missing_panics_doc,
    reason = "test helper constructors — panics are intentional, must_use is noise"
)]

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use supersigil_core::{
    ALTERNATIVE, CRITERION, Config, DECISION, ExtractedComponent, Frontmatter, IMPLEMENTS,
    RATIONALE, REFERENCES, SpecDocument, TASK, TRACKED_FILES, VERIFIED_BY,
};
use tempfile::TempDir;

pub use supersigil_core::test_helpers::{
    make_acceptance_criteria, make_criterion, make_depends_on, make_doc, pos, single_project_config,
};

pub fn test_config() -> Config {
    single_project_config()
}

pub fn make_doc_with_status(
    id: &str,
    status: &str,
    components: Vec<ExtractedComponent>,
) -> SpecDocument {
    SpecDocument {
        path: PathBuf::from(format!("specs/{id}.md")),
        frontmatter: Frontmatter {
            id: id.into(),
            doc_type: None,
            status: Some(status.into()),
        },
        extra: HashMap::new(),
        components,
    }
}

pub fn make_doc_typed(
    id: &str,
    doc_type: &str,
    status: Option<&str>,
    components: Vec<ExtractedComponent>,
) -> SpecDocument {
    SpecDocument {
        path: PathBuf::from(format!("specs/{id}.md")),
        frontmatter: Frontmatter {
            id: id.into(),
            doc_type: Some(doc_type.into()),
            status: status.map(Into::into),
        },
        extra: HashMap::new(),
        components,
    }
}

pub fn make_references(refs: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: REFERENCES.to_owned(),
        attributes: HashMap::from([("refs".into(), refs.into())]),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(line),
        end_position: pos(line + 1),
    }
}

pub fn make_verified_by_tag(tag: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: VERIFIED_BY.to_owned(),
        attributes: HashMap::from([
            ("strategy".into(), "tag".into()),
            ("tag".into(), tag.into()),
        ]),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(line),
        end_position: pos(line + 1),
    }
}

pub fn make_verified_by_glob(paths: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: VERIFIED_BY.to_owned(),
        attributes: HashMap::from([
            ("strategy".into(), "file-glob".into()),
            ("paths".into(), paths.into()),
        ]),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(line),
        end_position: pos(line + 1),
    }
}

pub fn make_criterion_with_verified_by(
    id: &str,
    verified_by: ExtractedComponent,
    line: usize,
) -> ExtractedComponent {
    ExtractedComponent {
        name: CRITERION.to_owned(),
        attributes: HashMap::from([("id".into(), id.into())]),
        children: vec![verified_by],
        body_text: Some(format!("criterion {id}")),
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(line),
        end_position: pos(line + 1),
    }
}

pub fn make_task(id: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: TASK.to_owned(),
        attributes: HashMap::from([("id".into(), id.into())]),
        children: vec![],
        body_text: Some(format!("task {id}")),
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(line),
        end_position: pos(line + 1),
    }
}

pub fn make_decision(children: Vec<ExtractedComponent>, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: DECISION.to_owned(),
        attributes: HashMap::new(),
        children,
        body_text: Some("a decision".into()),
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(line),
        end_position: pos(line + 1),
    }
}

pub fn make_decision_with_id(
    id: &str,
    children: Vec<ExtractedComponent>,
    line: usize,
) -> ExtractedComponent {
    ExtractedComponent {
        name: DECISION.to_owned(),
        attributes: HashMap::from([("id".into(), id.into())]),
        children,
        body_text: Some(format!("decision {id}")),
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(line),
        end_position: pos(line + 1),
    }
}

pub fn make_decision_standalone(
    id: &str,
    reason: &str,
    children: Vec<ExtractedComponent>,
    line: usize,
) -> ExtractedComponent {
    ExtractedComponent {
        name: DECISION.to_owned(),
        attributes: HashMap::from([
            ("id".into(), id.into()),
            ("standalone".into(), reason.into()),
        ]),
        children,
        body_text: Some(format!("decision {id}")),
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(line),
        end_position: pos(line + 1),
    }
}

pub fn make_rationale(line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: RATIONALE.to_owned(),
        attributes: HashMap::new(),
        children: vec![],
        body_text: Some("the rationale".into()),
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(line),
        end_position: pos(line + 1),
    }
}

pub fn make_alternative(id: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: ALTERNATIVE.to_owned(),
        attributes: HashMap::from([("id".into(), id.into())]),
        children: vec![],
        body_text: Some(format!("alternative {id}")),
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(line),
        end_position: pos(line + 1),
    }
}

pub fn make_alternative_with_status(id: &str, status: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: ALTERNATIVE.to_owned(),
        attributes: HashMap::from([("id".into(), id.into()), ("status".into(), status.into())]),
        children: vec![],
        body_text: Some(format!("alternative {id}")),
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(line),
        end_position: pos(line + 1),
    }
}

pub fn make_tracked_files(paths: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: TRACKED_FILES.to_owned(),
        attributes: HashMap::from([("paths".into(), paths.into())]),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(line),
        end_position: pos(line + 1),
    }
}

pub fn make_implements(refs: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: IMPLEMENTS.to_owned(),
        attributes: HashMap::from([("refs".into(), refs.into())]),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(line),
        end_position: pos(line + 1),
    }
}

/// Build a graph from documents using the test config. Panics on graph errors.
pub fn build_test_graph(docs: Vec<SpecDocument>) -> supersigil_core::DocumentGraph {
    let config = test_config();
    supersigil_core::build_graph(docs, &config).expect("test graph should build")
}

/// Build a graph with a custom config. Panics on graph errors.
pub fn build_test_graph_with_config(
    docs: Vec<SpecDocument>,
    config: &Config,
) -> supersigil_core::DocumentGraph {
    supersigil_core::build_graph(docs, config).expect("test graph should build")
}

// ---------------------------------------------------------------------------
// Git test helpers
// ---------------------------------------------------------------------------

fn git(dir: &TempDir, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .args(args)
        .current_dir(dir.path())
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .expect("git command")
}

/// Create a temp git repo with an initial commit. Returns (dir, initial commit OID).
pub fn init_repo() -> (TempDir, String) {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("README.md"), "init").unwrap();
    git(&dir, &["init", "-b", "main"]);
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-m", "init"]);
    let output = git(&dir, &["rev-parse", "HEAD"]);
    let oid = String::from_utf8(output.stdout).unwrap().trim().to_owned();
    (dir, oid)
}

/// Stage all changes and commit in a test repo.
pub fn git_commit(dir: &TempDir) {
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "test"]);
}

// ---------------------------------------------------------------------------
// File test helpers
// ---------------------------------------------------------------------------

/// Write a file inside a temp dir, creating parent directories as needed.
pub fn write_test_file(dir: &TempDir, name: &str, content: &str) -> PathBuf {
    let path = dir.path().join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(content.as_bytes()).unwrap();
    path
}

// ---------------------------------------------------------------------------
// Report test helpers
// ---------------------------------------------------------------------------

use crate::report::{EvidenceReportEntry, EvidenceSummary, TargetCoverage};

/// Build a sample `EvidenceSummary` for tests that need evidence data.
///
/// Contains two records targeting "req-1" with different provenance sources,
/// and a single coverage entry reflecting both.
#[must_use]
pub fn sample_evidence_summary() -> EvidenceSummary {
    EvidenceSummary {
        records: vec![
            EvidenceReportEntry {
                test_name: "test_login_flow".to_string(),
                test_file: "tests/auth.rs".to_string(),
                test_kind: "unit".to_string(),
                evidence_kind: "rust-attribute".to_string(),
                targets: vec!["req-1".to_string()],
                provenance: vec!["plugin:rust".to_string()],
                source_file: "tests/auth.rs".to_string(),
                source_line: 10,
                source_column: 1,
            },
            EvidenceReportEntry {
                test_name: "test_session_timeout".to_string(),
                test_file: "tests/auth.rs".to_string(),
                test_kind: "unit".to_string(),
                evidence_kind: "rust-attribute".to_string(),
                targets: vec!["req-1".to_string()],
                provenance: vec!["authored".to_string()],
                source_file: "tests/auth.rs".to_string(),
                source_line: 25,
                source_column: 1,
            },
        ],
        coverage: vec![TargetCoverage {
            target: "req-1".to_string(),
            test_count: 2,
        }],
        conflict_count: 0,
    }
}
