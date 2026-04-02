//! Unit tests for concrete examples and edge cases.
//!
//! These complement the property tests by covering:
//! - The `auth/req/login` example from the supersigil design document
//! - Edge cases: empty collections, no components, self-cycles, empty queries
//! - Cross-phase error aggregation

use crate::SpecDocument;
use crate::graph::query::PlanQuery;
use crate::graph::tests::generators::{
    make_acceptance_criteria, make_criterion, make_doc, make_doc_full, make_example,
    make_refs_component, make_task, make_tracked_files_component, single_project_config,
};
use crate::graph::{GraphError, IMPLEMENTS, REFERENCES, build_graph};

/// Build the auth/req/login scenario from the design document:
///
/// - `auth/req/login` — requirement with 3 criteria (valid-creds,
///   invalid-password, rate-limit) and `TrackedFiles`
/// - `auth/prop/token-generation` — references `auth/req/login#valid-creds`
/// - `auth/design/login-flow` — implements `auth/req/login`
/// - `auth/tasks/login` — tasks doc with 4 tasks in dependency chain,
///   one implementing `#valid-creds`
/// - `auth/example/login-happy-path` — references `auth/req/login#valid-creds`
#[allow(
    clippy::too_many_lines,
    reason = "test scenario builder with many documents"
)]
pub(super) fn build_auth_login_scenario() -> (
    crate::graph::DocumentGraph,
    SpecDocument, // req doc (for assertion)
) {
    let config = single_project_config();

    // Requirement document: auth/req/login
    let req_doc = make_doc_full(
        "auth/req/login",
        Some("requirements"),
        Some("approved"),
        vec![
            make_tracked_files_component("src/auth/**/*.rs", 1),
            make_acceptance_criteria(
                vec![
                    make_criterion("valid-creds", 3),
                    make_criterion("invalid-password", 4),
                    make_criterion("rate-limit", 5),
                ],
                2,
            ),
        ],
    );

    // Property document: references valid-creds
    let prop_doc = make_doc_full(
        "auth/prop/token-generation",
        Some("design"),
        Some("verified"),
        vec![make_refs_component(
            REFERENCES,
            "auth/req/login#valid-creds",
            1,
        )],
    );

    // Design document: implements auth/req/login
    let design_doc = make_doc_full(
        "auth/design/login-flow",
        Some("design"),
        Some("approved"),
        vec![make_refs_component(IMPLEMENTS, "auth/req/login", 1)],
    );

    // Tasks document with dependency chain:
    // type-alignment (done) → adapter-code (in-progress, implements #valid-creds)
    //   → switch-over (ready) → cleanup (draft)
    let tasks_doc = make_doc(
        "auth/tasks/login",
        vec![
            make_task("type-alignment", Some("done"), None, None, 1),
            make_task(
                "adapter-code",
                Some("in-progress"),
                Some("auth/req/login#valid-creds"),
                Some("type-alignment"),
                2,
            ),
            make_task("switch-over", Some("ready"), None, Some("adapter-code"), 3),
            make_task("cleanup", Some("draft"), None, Some("switch-over"), 4),
        ],
    );

    // Example document: references valid-creds criterion
    let example_doc = make_doc(
        "auth/example/login-happy-path",
        vec![make_refs_component(
            REFERENCES,
            "auth/req/login#valid-creds",
            1,
        )],
    );

    let graph = build_graph(
        vec![
            req_doc.clone(),
            prop_doc,
            design_doc,
            tasks_doc,
            example_doc,
        ],
        &config,
    )
    .expect("auth/req/login scenario should build without errors");

    (graph, req_doc)
}

mod accessors;
mod concrete;
mod edge_cases;
mod task_implements;
