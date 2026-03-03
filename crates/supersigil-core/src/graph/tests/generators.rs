//! Proptest generators and deterministic helpers for graph module tests.
//!
//! Generators produce the core input types (`Frontmatter`,
//! `ExtractedComponent`, `SpecDocument`, `Config`) needed by property tests.
//!
//! Deterministic helpers (`make_*`, `single_project_config`) build concrete
//! test fixtures for both property tests and unit tests.

use std::collections::HashMap;
use std::path::PathBuf;

use proptest::prelude::*;

use crate::graph::{ACCEPTANCE_CRITERIA, CRITERION, DEPENDS_ON, TASK, TRACKED_FILES};
use crate::{
    Config, DocumentsConfig, EcosystemConfig, ExtractedComponent, Frontmatter, HooksConfig,
    ProjectConfig, SourcePosition, SpecDocument, TestResultsConfig, VerifyConfig,
};

// ---------------------------------------------------------------------------
// ID generation
// ---------------------------------------------------------------------------

/// Strategy for document IDs: segments of `[a-z0-9]` joined by `/` or `-`.
/// Produces IDs like `"auth/req/login"`, `"core-utils"`, `"a"`.
pub fn arb_id() -> impl Strategy<Value = String> {
    let segment = "[a-z][a-z0-9]{0,7}";
    proptest::collection::vec(segment, 1..=4).prop_map(|segs| segs.join("/"))
}

/// Strategy for component IDs (fragments): simple `[a-z][a-z0-9-]{0,11}`.
pub fn arb_component_id() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9\\-]{0,11}"
}

// ---------------------------------------------------------------------------
// Frontmatter
// ---------------------------------------------------------------------------

/// Generate a `Frontmatter` with a specific ID.
pub fn arb_frontmatter_with_id(id: String) -> impl Strategy<Value = Frontmatter> {
    (
        proptest::option::of("[a-z]{3,8}"),
        proptest::option::of(prop_oneof!["draft", "active", "done", "deprecated",]),
    )
        .prop_map(move |(doc_type, status)| Frontmatter {
            id: id.clone(),
            doc_type,
            status,
        })
}

// ---------------------------------------------------------------------------
// SpecDocument
// ---------------------------------------------------------------------------

/// Generate a `SpecDocument` with a specific ID and given components.
pub fn arb_spec_document_with_id(
    id: String,
    components: Vec<ExtractedComponent>,
) -> impl Strategy<Value = SpecDocument> {
    arb_frontmatter_with_id(id).prop_map(move |frontmatter| SpecDocument {
        path: PathBuf::from(format!("specs/{}.mdx", frontmatter.id)),
        frontmatter,
        extra: HashMap::new(),
        components: components.clone(),
    })
}

/// Generate `n` documents with guaranteed unique IDs and no components.
pub fn arb_document_set(n: usize) -> impl Strategy<Value = Vec<SpecDocument>> {
    proptest::collection::hash_set(arb_id(), n..=n).prop_flat_map(move |ids| {
        let strats: Vec<_> = ids
            .into_iter()
            .map(|id| arb_spec_document_with_id(id, Vec::new()))
            .collect();
        strats
    })
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Generate a `Config` with optional multi-project setup and default `ComponentDefs`.
pub fn arb_config() -> impl Strategy<Value = Config> {
    proptest::bool::ANY.prop_flat_map(|multi_project| {
        if multi_project {
            arb_multi_project_config().boxed()
        } else {
            arb_single_project_config().boxed()
        }
    })
}

fn arb_single_project_config() -> impl Strategy<Value = Config> {
    Just(single_project_config())
}

fn arb_multi_project_config() -> impl Strategy<Value = Config> {
    (proptest::bool::ANY,).prop_map(|(isolated,)| {
        let mut projects = HashMap::new();
        projects.insert(
            "project-a".to_owned(),
            ProjectConfig {
                paths: vec!["project-a/specs/**/*.mdx".to_owned()],
                tests: Vec::new(),
                isolated,
            },
        );
        projects.insert(
            "project-b".to_owned(),
            ProjectConfig {
                paths: vec!["project-b/specs/**/*.mdx".to_owned()],
                tests: Vec::new(),
                isolated: false,
            },
        );
        Config {
            paths: None,
            tests: None,
            projects: Some(projects),
            id_pattern: None,
            documents: DocumentsConfig {
                types: HashMap::new(),
            },
            components: HashMap::new(),
            verify: VerifyConfig {
                strictness: None,
                rules: HashMap::new(),
            },
            ecosystem: EcosystemConfig {
                plugins: vec!["rust".to_owned()],
            },
            hooks: HooksConfig::default(),
            test_results: TestResultsConfig {
                formats: Vec::new(),
                paths: Vec::new(),
            },
        }
    })
}

// ---------------------------------------------------------------------------
// DAG generation
// ---------------------------------------------------------------------------

/// A generated DAG with named nodes and edges.
#[derive(Debug, Clone)]
pub struct GeneratedDag {
    /// Node names in topological order (index 0 has no dependencies).
    pub nodes: Vec<String>,
    /// Edges as `(from, to)` pairs where `from` depends on `to`
    /// (i.e., `to` must come before `from`).
    pub edges: Vec<(String, String)>,
}

/// Generate a random DAG with `n` nodes.
///
/// Nodes are named `"t0"`, `"t1"`, ..., `"t{n-1}"`. Edges only go from
/// higher-indexed nodes to lower-indexed nodes, guaranteeing acyclicity.
/// Each possible edge is included with 30% probability.
pub fn arb_dag(n: usize) -> impl Strategy<Value = GeneratedDag> {
    // For n nodes, there are at most n*(n-1)/2 possible forward edges.
    let max_edges = n * n.saturating_sub(1) / 2;
    proptest::collection::vec(proptest::bool::weighted(0.3), max_edges).prop_map(
        move |coin_flips| {
            let nodes: Vec<String> = (0..n).map(|i| format!("t{i}")).collect();
            let mut edges = Vec::new();
            let mut flip_idx = 0;
            for i in 1..n {
                for j in 0..i {
                    if coin_flips.get(flip_idx).copied().unwrap_or(false) {
                        // Node i depends on node j (j must come before i).
                        edges.push((nodes[i].clone(), nodes[j].clone()));
                    }
                    flip_idx += 1;
                }
            }
            GeneratedDag { nodes, edges }
        },
    )
}

// ---------------------------------------------------------------------------
// Deterministic test helpers
// ---------------------------------------------------------------------------

/// Build a `SourcePosition` from a line number (`byte_offset` = line * 40).
pub fn pos(line: usize) -> SourcePosition {
    SourcePosition {
        byte_offset: line * 40,
        line,
        column: 1,
    }
}

/// Build a `SpecDocument` with path derived from id as `specs/{id}.mdx`.
pub fn make_doc(id: &str, components: Vec<ExtractedComponent>) -> SpecDocument {
    SpecDocument {
        path: PathBuf::from(format!("specs/{id}.mdx")),
        frontmatter: Frontmatter {
            id: id.to_owned(),
            doc_type: None,
            status: None,
        },
        extra: HashMap::new(),
        components,
    }
}

/// Build a `SpecDocument` with an explicit path.
pub fn make_doc_with_path(
    id: &str,
    path: &str,
    components: Vec<ExtractedComponent>,
) -> SpecDocument {
    SpecDocument {
        path: PathBuf::from(path),
        frontmatter: Frontmatter {
            id: id.to_owned(),
            doc_type: None,
            status: None,
        },
        extra: HashMap::new(),
        components,
    }
}

/// Build a `SpecDocument` with optional `doc_type` and status.
pub fn make_doc_full(
    id: &str,
    doc_type: Option<&str>,
    status: Option<&str>,
    components: Vec<ExtractedComponent>,
) -> SpecDocument {
    SpecDocument {
        path: PathBuf::from(format!("specs/{id}.mdx")),
        frontmatter: Frontmatter {
            id: id.to_owned(),
            doc_type: doc_type.map(str::to_owned),
            status: status.map(str::to_owned),
        },
        extra: HashMap::new(),
        components,
    }
}

/// Build a `Criterion` component.
pub fn make_criterion(id: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: CRITERION.to_owned(),
        attributes: HashMap::from([("id".to_owned(), id.to_owned())]),
        children: Vec::new(),
        body_text: Some(format!("criterion {id}")),
        position: pos(line),
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
        position: pos(line),
    }
}

/// Build a component with a `refs` attribute (Validates, Implements, etc.).
pub fn make_refs_component(name: &str, refs: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: name.to_owned(),
        attributes: HashMap::from([("refs".to_owned(), refs.to_owned())]),
        children: Vec::new(),
        body_text: None,
        position: pos(line),
    }
}

/// Build a `Task` component with optional attributes.
pub fn make_task(
    id: &str,
    status: Option<&str>,
    implements: Option<&str>,
    depends: Option<&str>,
    line: usize,
) -> ExtractedComponent {
    let mut attributes = HashMap::from([("id".to_owned(), id.to_owned())]);
    if let Some(s) = status {
        attributes.insert("status".to_owned(), s.to_owned());
    }
    if let Some(i) = implements {
        attributes.insert("implements".to_owned(), i.to_owned());
    }
    if let Some(d) = depends {
        attributes.insert("depends".to_owned(), d.to_owned());
    }
    ExtractedComponent {
        name: TASK.to_owned(),
        attributes,
        children: Vec::new(),
        body_text: Some(format!("task {id}")),
        position: pos(line),
    }
}

/// Build a `TrackedFiles` component.
pub fn make_tracked_files_component(paths: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: TRACKED_FILES.to_owned(),
        attributes: HashMap::from([("paths".to_owned(), paths.to_owned())]),
        children: Vec::new(),
        body_text: None,
        position: pos(line),
    }
}

/// Build a `DependsOn` component.
pub fn make_depends_on(refs: &str, line: usize) -> ExtractedComponent {
    make_refs_component(DEPENDS_ON, refs, line)
}

/// Build a two-project `Config` with configurable isolation flags.
///
/// Creates `project-a` and `project-b` with paths `project-{x}/specs/**/*.mdx`.
pub fn two_project_config(a_isolated: bool, b_isolated: bool) -> Config {
    let mut projects = HashMap::new();
    projects.insert(
        "project-a".to_owned(),
        ProjectConfig {
            paths: vec!["project-a/specs/**/*.mdx".to_owned()],
            tests: Vec::new(),
            isolated: a_isolated,
        },
    );
    projects.insert(
        "project-b".to_owned(),
        ProjectConfig {
            paths: vec!["project-b/specs/**/*.mdx".to_owned()],
            tests: Vec::new(),
            isolated: b_isolated,
        },
    );
    Config {
        paths: None,
        tests: None,
        projects: Some(projects),
        id_pattern: None,
        documents: DocumentsConfig {
            types: HashMap::new(),
        },
        components: HashMap::new(),
        verify: VerifyConfig {
            strictness: None,
            rules: HashMap::new(),
        },
        ecosystem: EcosystemConfig {
            plugins: vec!["rust".to_owned()],
        },
        hooks: HooksConfig::default(),
        test_results: TestResultsConfig {
            formats: Vec::new(),
            paths: Vec::new(),
        },
    }
}

/// Build a default single-project `Config`.
pub fn single_project_config() -> Config {
    Config {
        paths: Some(vec!["specs/**/*.mdx".to_owned()]),
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
        ecosystem: EcosystemConfig {
            plugins: vec!["rust".to_owned()],
        },
        hooks: HooksConfig::default(),
        test_results: TestResultsConfig {
            formats: Vec::new(),
            paths: Vec::new(),
        },
    }
}
