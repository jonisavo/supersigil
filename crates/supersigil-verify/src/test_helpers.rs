#![cfg(test)]
#![allow(dead_code, reason = "shared test helpers — not all used yet")]

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use supersigil_core::{
    Config, DocumentsConfig, EcosystemConfig, ExtractedComponent, Frontmatter, HooksConfig,
    SourcePosition, SpecDocument, TestResultsConfig, VerifyConfig,
};
use tempfile::TempDir;

pub fn pos(line: usize) -> SourcePosition {
    SourcePosition {
        byte_offset: line * 40,
        line,
        column: 1,
    }
}

pub fn test_config() -> Config {
    Config {
        paths: Some(vec!["specs/**/*.mdx".into()]),
        tests: None,
        projects: None,
        id_pattern: None,
        documents: DocumentsConfig {
            types: HashMap::new(),
        },
        components: HashMap::new(),
        verify: VerifyConfig {
            strictness: None,
            rules: HashMap::new(),
        },
        ecosystem: EcosystemConfig { plugins: vec![] },
        hooks: HooksConfig::default(),
        test_results: TestResultsConfig {
            formats: vec![],
            paths: vec![],
        },
    }
}

pub fn make_doc(id: &str, components: Vec<ExtractedComponent>) -> SpecDocument {
    SpecDocument {
        path: PathBuf::from(format!("specs/{id}.mdx")),
        frontmatter: Frontmatter {
            id: id.into(),
            doc_type: None,
            status: None,
        },
        extra: HashMap::new(),
        components,
    }
}

pub fn make_doc_with_status(
    id: &str,
    status: &str,
    components: Vec<ExtractedComponent>,
) -> SpecDocument {
    SpecDocument {
        path: PathBuf::from(format!("specs/{id}.mdx")),
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
        path: PathBuf::from(format!("specs/{id}.mdx")),
        frontmatter: Frontmatter {
            id: id.into(),
            doc_type: Some(doc_type.into()),
            status: status.map(Into::into),
        },
        extra: HashMap::new(),
        components,
    }
}

pub fn make_criterion(id: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: "Criterion".into(),
        attributes: HashMap::from([("id".into(), id.into())]),
        children: vec![],
        body_text: Some(format!("criterion {id}")),
        position: pos(line),
    }
}

pub fn make_acceptance_criteria(
    children: Vec<ExtractedComponent>,
    line: usize,
) -> ExtractedComponent {
    ExtractedComponent {
        name: "AcceptanceCriteria".into(),
        attributes: HashMap::new(),
        children,
        body_text: None,
        position: pos(line),
    }
}

pub fn make_validates(refs: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: "Validates".into(),
        attributes: HashMap::from([("refs".into(), refs.into())]),
        children: vec![],
        body_text: None,
        position: pos(line),
    }
}

pub fn make_illustrates(refs: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: "Illustrates".into(),
        attributes: HashMap::from([("refs".into(), refs.into())]),
        children: vec![],
        body_text: None,
        position: pos(line),
    }
}

pub fn make_verified_by_tag(tag: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: "VerifiedBy".into(),
        attributes: HashMap::from([
            ("strategy".into(), "tag".into()),
            ("tag".into(), tag.into()),
        ]),
        children: vec![],
        body_text: None,
        position: pos(line),
    }
}

pub fn make_verified_by_glob(paths: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: "VerifiedBy".into(),
        attributes: HashMap::from([
            ("strategy".into(), "file-glob".into()),
            ("paths".into(), paths.into()),
        ]),
        children: vec![],
        body_text: None,
        position: pos(line),
    }
}

pub fn make_tracked_files(paths: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: "TrackedFiles".into(),
        attributes: HashMap::from([("paths".into(), paths.into())]),
        children: vec![],
        body_text: None,
        position: pos(line),
    }
}

pub fn make_implements(refs: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: "Implements".into(),
        attributes: HashMap::from([("refs".into(), refs.into())]),
        children: vec![],
        body_text: None,
        position: pos(line),
    }
}

pub fn make_depends_on(refs: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: "DependsOn".into(),
        attributes: HashMap::from([("refs".into(), refs.into())]),
        children: vec![],
        body_text: None,
        position: pos(line),
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
