use std::collections::HashMap;

use supersigil_core::{DocumentGraph, ExtractedComponent, TASK};

use crate::report::{Finding, FindingDetails, RuleName};

/// Check status field consistency across the document graph.
///
/// Detects three kinds of drift:
///
/// 1. **Tasks doc drift** — a tasks document has completed tasks but its own
///    status hasn't been promoted (draft → in-progress, or in-progress → done).
/// 2. **Sibling design drift** — a tasks doc is `done` but the sibling design
///    doc (same ID prefix) is still `draft`.
/// 3. **Sibling req drift** — a tasks doc is `done` but the sibling
///    requirements doc is not `implemented`.
///
/// Coverage checking (whether criteria have verification evidence) is handled
/// by `rules::coverage` via `ArtifactGraph` and is not duplicated here.
pub fn check(graph: &DocumentGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    let done_prefixes = check_tasks_docs(graph, &mut findings);
    check_sibling_docs(graph, &done_prefixes, &mut findings);
    findings
}

/// Check tasks documents for internal status drift. Returns a map of feature
/// prefixes whose tasks doc is fully done (for sibling checks).
fn check_tasks_docs<'a>(
    graph: &'a DocumentGraph,
    findings: &mut Vec<Finding>,
) -> HashMap<&'a str, &'a str> {
    let mut done_tasks_prefixes: HashMap<&str, &str> = HashMap::new();

    for (doc_id, doc) in graph.documents() {
        if doc.frontmatter.doc_type.as_deref() != Some("tasks") {
            continue;
        }

        let mut tasks = Vec::new();
        collect_tasks(&doc.components, &mut tasks);
        if tasks.is_empty() {
            continue;
        }

        let doc_status = doc.frontmatter.status.as_deref().unwrap_or("draft");
        let any_done = tasks.contains(&"done");
        let all_done = tasks.iter().all(|s| *s == "done");

        if any_done && !all_done && doc_status == "draft" {
            findings.push(
                Finding::new(
                    RuleName::StatusInconsistency,
                    Some(doc_id.to_owned()),
                    format!(
                        "tasks document `{doc_id}` has completed tasks but status is `draft`; \
                         consider `in-progress`"
                    ),
                    None,
                )
                .with_details(FindingDetails {
                    suggestion: Some("in-progress".into()),
                    ..FindingDetails::default()
                }),
            );
        }

        if all_done && doc_status != "done" {
            findings.push(
                Finding::new(
                    RuleName::StatusInconsistency,
                    Some(doc_id.to_owned()),
                    format!(
                        "all tasks in `{doc_id}` are done but document status is `{doc_status}`; \
                         consider `done`"
                    ),
                    None,
                )
                .with_details(FindingDetails {
                    suggestion: Some("done".into()),
                    ..FindingDetails::default()
                }),
            );
        }

        if all_done
            && doc_status == "done"
            && let Some(prefix) = doc_id.rsplit_once('/').map(|(p, _)| p)
        {
            done_tasks_prefixes.insert(prefix, doc_id);
        }
    }

    done_tasks_prefixes
}

/// Check design and requirements docs whose sibling tasks doc is done.
fn check_sibling_docs(
    graph: &DocumentGraph,
    done_prefixes: &HashMap<&str, &str>,
    findings: &mut Vec<Finding>,
) {
    for (doc_id, doc) in graph.documents() {
        let Some(ref doc_type) = doc.frontmatter.doc_type else {
            continue;
        };
        let doc_status = doc.frontmatter.status.as_deref().unwrap_or("draft");
        let Some((prefix, _)) = doc_id.rsplit_once('/') else {
            continue;
        };
        let Some(tasks_doc_id) = done_prefixes.get(prefix) else {
            continue;
        };

        match doc_type.as_str() {
            "design" if doc_status == "draft" => {
                findings.push(
                    Finding::new(
                        RuleName::StatusInconsistency,
                        Some(doc_id.to_owned()),
                        format!(
                            "sibling tasks document `{tasks_doc_id}` is done but \
                             `{doc_id}` is still `draft`; consider `approved`"
                        ),
                        None,
                    )
                    .with_details(FindingDetails {
                        suggestion: Some("approved".into()),
                        ..FindingDetails::default()
                    }),
                );
            }
            "requirements" if doc_status != "implemented" => {
                findings.push(
                    Finding::new(
                        RuleName::StatusInconsistency,
                        Some(doc_id.to_owned()),
                        format!(
                            "sibling tasks document `{tasks_doc_id}` is done but \
                             `{doc_id}` status is `{doc_status}` instead of `implemented`"
                        ),
                        None,
                    )
                    .with_details(FindingDetails {
                        suggestion: Some("implemented".into()),
                        ..FindingDetails::default()
                    }),
                );
            }
            _ => {}
        }
    }
}

/// Recursively collect task statuses from all Task components.
fn collect_tasks<'a>(components: &'a [ExtractedComponent], out: &mut Vec<&'a str>) {
    for comp in components {
        if comp.name == TASK {
            let status = comp
                .attributes
                .get("status")
                .map_or("draft", String::as_str);
            out.push(status);
        }
        collect_tasks(&comp.children, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;

    // ------------------------------------------------------------------
    // Helper: build a Task component
    // ------------------------------------------------------------------

    fn make_task(id: &str, status: &str, line: usize) -> supersigil_core::ExtractedComponent {
        make_task_with_children(id, status, line, vec![])
    }

    fn make_task_with_children(
        id: &str,
        status: &str,
        line: usize,
        children: Vec<supersigil_core::ExtractedComponent>,
    ) -> supersigil_core::ExtractedComponent {
        supersigil_core::ExtractedComponent {
            name: TASK.to_owned(),
            attributes: std::collections::HashMap::from([
                ("id".into(), id.into()),
                ("status".into(), status.into()),
            ]),
            children,
            body_text: Some(format!("task {id}")),
            body_text_offset: None,
            body_text_end_offset: None,
            code_blocks: vec![],
            position: pos(line),
        }
    }

    // ------------------------------------------------------------------
    // Existing tests (preserved)
    // ------------------------------------------------------------------

    #[test]
    fn status_rule_does_not_check_coverage_via_references() {
        // Coverage is handled by coverage::check via ArtifactGraph.
        // The status rule should not emit findings for uncovered criteria.
        let docs = vec![
            make_doc_with_status(
                "req/auth",
                "implemented",
                vec![make_acceptance_criteria(
                    vec![make_criterion("req-1", 10), make_criterion("req-2", 20)],
                    9,
                )],
            ),
            make_doc(
                "design/auth",
                vec![
                    make_references("req/auth#req-1", 5),
                    // req-2 not referenced — should not matter for status rule
                ],
            ),
        ];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert!(
            findings.is_empty(),
            "status rule should not emit coverage findings; got: {findings:?}",
        );
    }

    #[test]
    fn implemented_status_emits_no_findings() {
        let docs = vec![make_doc_with_status(
            "req/auth",
            "implemented",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert!(findings.is_empty());
    }

    #[test]
    fn no_status_document_emits_no_findings() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert!(findings.is_empty());
    }

    // ------------------------------------------------------------------
    // New: tasks doc with done tasks but draft doc status
    // ------------------------------------------------------------------

    #[test]
    fn tasks_doc_with_done_tasks_but_draft_status_warns() {
        let docs = vec![make_doc_typed(
            "auth/tasks",
            "tasks",
            Some("draft"),
            vec![
                make_task("task-1", "done", 10),
                make_task("task-2", "ready", 20),
            ],
        )];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].message.contains("draft"),
            "should mention draft: {}",
            findings[0].message,
        );
        assert!(
            findings[0].message.contains("in-progress"),
            "should suggest in-progress: {}",
            findings[0].message,
        );
    }

    // ------------------------------------------------------------------
    // New: tasks doc with all tasks done but not done status
    // ------------------------------------------------------------------

    #[test]
    fn tasks_doc_all_done_but_not_done_status_warns() {
        let docs = vec![make_doc_typed(
            "auth/tasks",
            "tasks",
            Some("in-progress"),
            vec![
                make_task("task-1", "done", 10),
                make_task("task-2", "done", 20),
            ],
        )];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].message.contains("done"),
            "should suggest done: {}",
            findings[0].message,
        );
    }

    #[test]
    fn tasks_doc_all_done_with_done_status_is_clean() {
        let docs = vec![make_doc_typed(
            "auth/tasks",
            "tasks",
            Some("done"),
            vec![
                make_task("task-1", "done", 10),
                make_task("task-2", "done", 20),
            ],
        )];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert!(findings.is_empty());
    }

    #[test]
    fn tasks_doc_with_no_tasks_is_clean() {
        let docs = vec![make_doc_typed("auth/tasks", "tasks", Some("draft"), vec![])];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert!(findings.is_empty(), "no tasks means nothing to check");
    }

    // ------------------------------------------------------------------
    // New: sibling design still draft when tasks doc is done
    // ------------------------------------------------------------------

    #[test]
    fn design_draft_when_sibling_tasks_done_warns() {
        let docs = vec![
            make_doc_typed(
                "auth/tasks",
                "tasks",
                Some("done"),
                vec![make_task("task-1", "done", 10)],
            ),
            make_doc_typed("auth/design", "design", Some("draft"), vec![]),
        ];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].message.contains("auth/design"),
            "should mention design doc: {}",
            findings[0].message,
        );
        assert!(
            findings[0].message.contains("approved"),
            "should suggest approved: {}",
            findings[0].message,
        );
    }

    #[test]
    fn design_approved_when_sibling_tasks_done_is_clean() {
        let docs = vec![
            make_doc_typed(
                "auth/tasks",
                "tasks",
                Some("done"),
                vec![make_task("task-1", "done", 10)],
            ),
            make_doc_typed("auth/design", "design", Some("approved"), vec![]),
        ];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert!(findings.is_empty());
    }

    // ------------------------------------------------------------------
    // New: sibling req not implemented when tasks doc is done
    // ------------------------------------------------------------------

    #[test]
    fn req_draft_when_sibling_tasks_done_warns() {
        let docs = vec![
            make_doc_typed(
                "auth/tasks",
                "tasks",
                Some("done"),
                vec![make_task("task-1", "done", 10)],
            ),
            make_doc_typed(
                "auth/req",
                "requirements",
                Some("draft"),
                vec![make_acceptance_criteria(
                    vec![make_criterion("req-1", 10)],
                    9,
                )],
            ),
        ];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].message.contains("auth/req"),
            "should mention req doc: {}",
            findings[0].message,
        );
        assert!(
            findings[0].message.contains("implemented"),
            "should suggest implemented: {}",
            findings[0].message,
        );
    }

    #[test]
    fn req_implemented_when_sibling_tasks_done_is_clean() {
        let docs = vec![
            make_doc_typed(
                "auth/tasks",
                "tasks",
                Some("done"),
                vec![make_task("task-1", "done", 10)],
            ),
            make_doc_typed(
                "auth/req",
                "requirements",
                Some("implemented"),
                vec![make_acceptance_criteria(
                    vec![make_criterion("req-1", 10)],
                    9,
                )],
            ),
        ];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert!(findings.is_empty());
    }

    // ------------------------------------------------------------------
    // New: nested tasks all done counts correctly
    // ------------------------------------------------------------------

    #[test]
    fn nested_tasks_all_done_detects_completion() {
        let parent = make_task_with_children(
            "task-1",
            "done",
            10,
            vec![make_task("task-1-1", "done", 12)],
        );
        let docs = vec![make_doc_typed(
            "auth/tasks",
            "tasks",
            Some("in-progress"),
            vec![parent],
        )];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert_eq!(
            findings.len(),
            1,
            "should detect all tasks done even with nesting",
        );
    }

    #[test]
    fn nested_task_not_done_prevents_completion_warning() {
        let parent = make_task_with_children(
            "task-1",
            "done",
            10,
            vec![make_task("task-1-1", "ready", 12)],
        );
        let docs = vec![make_doc_typed(
            "auth/tasks",
            "tasks",
            Some("in-progress"),
            vec![parent],
        )];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert!(
            findings.is_empty(),
            "nested non-done task means not all done; got: {findings:?}",
        );
    }

    // ------------------------------------------------------------------
    // Non-tasks docs without siblings are clean
    // ------------------------------------------------------------------

    #[test]
    fn standalone_design_draft_without_sibling_tasks_is_clean() {
        let docs = vec![make_doc_typed(
            "auth/design",
            "design",
            Some("draft"),
            vec![],
        )];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert!(
            findings.is_empty(),
            "no sibling tasks doc means no status inconsistency",
        );
    }
}
