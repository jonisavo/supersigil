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

const GIT_ENV_VARS_TO_CLEAR: [&str; 5] = [
    "GIT_COMMON_DIR",
    "GIT_DIR",
    "GIT_INDEX_FILE",
    "GIT_PREFIX",
    "GIT_WORK_TREE",
];

pub fn sanitize_git_env(cmd: &mut Command) -> &mut Command {
    for name in GIT_ENV_VARS_TO_CLEAR {
        cmd.env_remove(name);
    }
    cmd
}

fn git(dir: &TempDir, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(dir.path())
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com");
    sanitize_git_env(&mut cmd);
    let output = cmd.output().expect("git command");
    assert!(
        output.status.success(),
        "git {:?} failed in {}: {}",
        args,
        dir.path().display(),
        String::from_utf8_lossy(&output.stderr),
    );
    output
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::sync::{LazyLock, Mutex};

    static GIT_ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    struct GitEnvGuard {
        saved: Vec<(&'static str, Option<OsString>)>,
    }

    impl GitEnvGuard {
        fn with_overrides(overrides: &[(&'static str, OsString)]) -> Self {
            let mut saved = Vec::with_capacity(GIT_ENV_VARS_TO_CLEAR.len());
            for name in GIT_ENV_VARS_TO_CLEAR {
                saved.push((name, std::env::var_os(name)));
                // SAFETY: Tests hold `GIT_ENV_LOCK` while mutating process env,
                // so no concurrent env access occurs within this test binary.
                unsafe { std::env::remove_var(name) };
            }
            for (name, value) in overrides {
                // SAFETY: Tests hold `GIT_ENV_LOCK` while mutating process env,
                // so no concurrent env access occurs within this test binary.
                unsafe { std::env::set_var(name, value) };
            }
            Self { saved }
        }
    }

    impl Drop for GitEnvGuard {
        fn drop(&mut self) {
            for (name, value) in self.saved.drain(..) {
                match value {
                    // SAFETY: Tests hold `GIT_ENV_LOCK` while mutating process
                    // env, so no concurrent env access occurs within this
                    // test binary.
                    Some(value) => unsafe { std::env::set_var(name, value) },
                    // SAFETY: Tests hold `GIT_ENV_LOCK` while mutating process
                    // env, so no concurrent env access occurs within this
                    // test binary.
                    None => unsafe { std::env::remove_var(name) },
                }
            }
        }
    }

    fn run_git_without_inherited_repo_env(
        dir: &std::path::Path,
        args: &[&str],
    ) -> std::process::Output {
        let mut cmd = Command::new("git");
        cmd.args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com");
        sanitize_git_env(&mut cmd);
        cmd.output().expect("git command")
    }

    #[test]
    fn init_repo_ignores_inherited_git_repo_env() {
        let _lock = GIT_ENV_LOCK.lock().unwrap();

        let outer = TempDir::new().unwrap();
        std::fs::write(outer.path().join("README.md"), "outer").unwrap();
        assert!(
            run_git_without_inherited_repo_env(outer.path(), &["init", "-b", "main"])
                .status
                .success()
        );
        assert!(
            run_git_without_inherited_repo_env(outer.path(), &["add", "."])
                .status
                .success()
        );
        assert!(
            run_git_without_inherited_repo_env(outer.path(), &["commit", "-m", "outer"])
                .status
                .success()
        );
        let outer_head = String::from_utf8(
            run_git_without_inherited_repo_env(outer.path(), &["rev-parse", "HEAD"]).stdout,
        )
        .unwrap()
        .trim()
        .to_owned();

        let _guard = GitEnvGuard::with_overrides(&[
            (
                "GIT_COMMON_DIR",
                outer.path().join(".git").as_os_str().to_os_string(),
            ),
            (
                "GIT_DIR",
                outer.path().join(".git").as_os_str().to_os_string(),
            ),
            (
                "GIT_INDEX_FILE",
                outer.path().join(".git/index").as_os_str().to_os_string(),
            ),
            ("GIT_PREFIX", OsString::from("")),
            ("GIT_WORK_TREE", outer.path().as_os_str().to_os_string()),
        ]);

        let (repo, initial) = init_repo();

        assert!(
            repo.path().join(".git").exists(),
            "init_repo should create an isolated git repo even when hook Git env vars are set"
        );
        assert!(
            !initial.is_empty(),
            "init_repo should resolve a real HEAD in the isolated temp repo"
        );

        let outer_head_after = String::from_utf8(
            run_git_without_inherited_repo_env(outer.path(), &["rev-parse", "HEAD"]).stdout,
        )
        .unwrap()
        .trim()
        .to_owned();
        assert_eq!(
            outer_head_after, outer_head,
            "init_repo should not mutate the outer repository referenced by inherited hook env"
        );
    }
}
