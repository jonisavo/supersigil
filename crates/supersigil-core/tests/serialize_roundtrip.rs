use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::json;
use supersigil_core::{
    ContextOutput, CriterionContext, DocRef, IllustrationRef, OutstandingCriterion, PlanOutput,
    SpecDocument, TaskInfo,
};

#[test]
fn spec_document_serializes_to_json() {
    let doc = SpecDocument {
        path: PathBuf::from("specs/auth/req.mdx"),
        frontmatter: supersigil_core::Frontmatter {
            id: "auth/req/login".into(),
            doc_type: Some("requirement".into()),
            status: Some("approved".into()),
        },
        extra: HashMap::new(),
        components: vec![],
    };
    let json = serde_json::to_value(&doc).expect("serialize SpecDocument");

    assert_eq!(json["path"], "specs/auth/req.mdx");
    assert_eq!(json["frontmatter"]["id"], "auth/req/login");
    assert_eq!(json["frontmatter"]["type"], "requirement");
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
        criteria: vec![CriterionContext {
            id: "c1".into(),
            body_text: Some("criterion text".into()),
            validated_by: vec![DocRef {
                doc_id: "prop/1".into(),
                status: Some("verified".into()),
            }],
            illustrated_by: vec![],
        }],
        implemented_by: vec![],
        illustrated_by: vec![],
        tasks: vec![],
    };
    let json = serde_json::to_value(&ctx).expect("serialize ContextOutput");

    assert_eq!(json["document"]["path"], "test.mdx");
    assert_eq!(json["document"]["frontmatter"], json!({ "id": "test/doc" }));
    assert_eq!(
        json["criteria"],
        json!([{
            "id": "c1",
            "body_text": "criterion text",
            "validated_by": [{ "doc_id": "prop/1", "status": "verified" }],
            "illustrated_by": [],
        }])
    );
    assert_eq!(json["implemented_by"], json!([]));
    assert_eq!(json["illustrated_by"], json!([]));
    assert_eq!(json["tasks"], json!([]));
}

#[test]
fn plan_output_serializes_to_json() {
    let plan = PlanOutput {
        outstanding_criteria: vec![OutstandingCriterion {
            doc_id: "req/1".into(),
            criterion_id: "c1".into(),
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
        illustrated_by: vec![IllustrationRef {
            doc_id: "ex/1".into(),
            target_doc_id: "req/1".into(),
            target_fragment: Some("c1".into()),
        }],
    };
    let json = serde_json::to_value(&plan).expect("serialize PlanOutput");

    assert_eq!(
        json["outstanding_criteria"],
        json!([{
            "doc_id": "req/1",
            "criterion_id": "c1",
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
    assert_eq!(
        json["illustrated_by"],
        json!([{
            "doc_id": "ex/1",
            "target_doc_id": "req/1",
            "target_fragment": "c1",
        }])
    );
}
