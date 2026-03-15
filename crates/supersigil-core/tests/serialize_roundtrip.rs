use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::json;
use supersigil_core::{
    ContextOutput, DocRef, OutstandingTarget, PlanOutput, SpecDocument, TargetContext, TaskInfo,
};

#[test]
fn spec_document_serializes_to_json() {
    let doc = SpecDocument {
        path: PathBuf::from("specs/auth/req.mdx"),
        frontmatter: supersigil_core::Frontmatter {
            id: "auth/req/login".into(),
            doc_type: Some("requirements".into()),
            status: Some("approved".into()),
        },
        extra: HashMap::new(),
        components: vec![],
    };
    let json = serde_json::to_value(&doc).expect("serialize SpecDocument");

    assert_eq!(json["path"], "specs/auth/req.mdx");
    assert_eq!(json["frontmatter"]["id"], "auth/req/login");
    assert_eq!(json["frontmatter"]["type"], "requirements");
    assert_eq!(json["frontmatter"]["status"], "approved");
    assert_eq!(json["extra"], json!({}));
    assert_eq!(json["components"], json!([]));
}

#[test]
fn context_output_serializes_to_json() {
    let ctx = ContextOutput {
        document: SpecDocument {
            path: PathBuf::from("test.mdx"),
            frontmatter: supersigil_core::Frontmatter {
                id: "test/doc".into(),
                doc_type: None,
                status: None,
            },
            extra: HashMap::new(),
            components: vec![],
        },
        criteria: vec![TargetContext {
            id: "c1".into(),
            target_ref: "test/doc#c1".into(),
            body_text: Some("criterion text".into()),
            referenced_by: vec![DocRef {
                doc_id: "prop/1".into(),
                status: Some("verified".into()),
            }],
        }],
        implemented_by: vec![],
        referenced_by: vec![],
        tasks: vec![],
    };
    let json = serde_json::to_value(&ctx).expect("serialize ContextOutput");

    assert_eq!(json["document"]["path"], "test.mdx");
    assert_eq!(json["document"]["frontmatter"], json!({ "id": "test/doc" }));
    assert_eq!(
        json["criteria"],
        json!([{
            "id": "c1",
            "target_ref": "test/doc#c1",
            "body_text": "criterion text",
            "referenced_by": [{ "doc_id": "prop/1", "status": "verified" }],
        }])
    );
    assert_eq!(json["implemented_by"], json!([]));
    assert_eq!(json["referenced_by"], json!([]));
    assert_eq!(json["tasks"], json!([]));
}

#[test]
fn plan_output_serializes_to_json() {
    let plan = PlanOutput {
        outstanding_targets: vec![OutstandingTarget {
            doc_id: "req/1".into(),
            target_id: "c1".into(),
            body_text: Some("uncovered".into()),
        }],
        pending_tasks: vec![TaskInfo {
            tasks_doc_id: "tasks/1".into(),
            task_id: "t1".into(),
            status: Some("ready".into()),
            body_text: None,
            implements: vec![("req/1".into(), "c1".into())],
            depends_on: vec![],
        }],
        completed_tasks: vec![],
        actionable_tasks: vec!["t1".into()],
        blocked_tasks: vec![],
    };
    let json = serde_json::to_value(&plan).expect("serialize PlanOutput");

    assert_eq!(
        json["outstanding_targets"],
        json!([{
            "doc_id": "req/1",
            "target_id": "c1",
            "body_text": "uncovered",
        }])
    );
    assert_eq!(
        json["pending_tasks"],
        json!([{
            "tasks_doc_id": "tasks/1",
            "task_id": "t1",
            "status": "ready",
            "body_text": null,
            "implements": [["req/1", "c1"]],
            "depends_on": [],
        }])
    );
    assert_eq!(json["completed_tasks"], json!([]));
    assert_eq!(json["actionable_tasks"], json!(["t1"]));
    assert_eq!(json["blocked_tasks"], json!([]));
}

#[test]
fn task_info_none_status_serializes_as_pending() {
    let task = TaskInfo {
        tasks_doc_id: "tasks/1".into(),
        task_id: "t1".into(),
        status: None,
        body_text: None,
        implements: vec![],
        depends_on: vec![],
    };
    let json = serde_json::to_value(&task).expect("serialize TaskInfo");

    assert_eq!(json["status"], "pending");
}

#[test]
fn task_info_explicit_status_serializes_as_is() {
    let task = TaskInfo {
        tasks_doc_id: "tasks/1".into(),
        task_id: "t1".into(),
        status: Some("done".into()),
        body_text: None,
        implements: vec![],
        depends_on: vec![],
    };
    let json = serde_json::to_value(&task).expect("serialize TaskInfo");

    assert_eq!(json["status"], "done");
}
