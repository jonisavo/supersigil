use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::json;
use supersigil_core::{
    AlternativeContext, ContextOutput, DecisionContext, DocRef, LinkedDecision, OutstandingTarget,
    PlanOutput, SpecDocument, TargetContext, TaskInfo,
};

#[test]
fn spec_document_serializes_to_json() {
    let doc = SpecDocument {
        path: PathBuf::from("specs/auth/req.md"),
        frontmatter: supersigil_core::Frontmatter {
            id: "auth/req/login".into(),
            doc_type: Some("requirements".into()),
            status: Some("approved".into()),
        },
        extra: HashMap::new(),
        components: vec![],
    };
    let json = serde_json::to_value(&doc).expect("serialize SpecDocument");

    assert_eq!(json["path"], "specs/auth/req.md");
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
            path: PathBuf::from("test.md"),
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
        decisions: vec![DecisionContext {
            id: "d1".into(),
            body_text: Some("use approach A".into()),
            rationale_text: Some("faster".into()),
            alternatives: vec![AlternativeContext {
                id: "alt1".into(),
                status: "rejected".into(),
                body_text: Some("approach B".into()),
            }],
        }],
        linked_decisions: vec![LinkedDecision {
            source_doc_id: "adr/1".into(),
            decision_id: "d-ext".into(),
            body_text: Some("external decision".into()),
        }],
        implemented_by: vec![],
        referenced_by: vec![],
        tasks: vec![],
    };
    let json = serde_json::to_value(&ctx).expect("serialize ContextOutput");

    assert_eq!(json["document"]["path"], "test.md");
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
    assert_eq!(
        json["decisions"],
        json!([{
            "id": "d1",
            "body_text": "use approach A",
            "rationale_text": "faster",
            "alternatives": [{ "id": "alt1", "status": "rejected", "body_text": "approach B" }],
        }])
    );
    assert_eq!(
        json["linked_decisions"],
        json!([{
            "source_doc_id": "adr/1",
            "decision_id": "d-ext",
            "body_text": "external decision",
        }])
    );
    assert_eq!(json["implemented_by"], json!([]));
    assert_eq!(json["referenced_by"], json!([]));
    assert_eq!(json["tasks"], json!([]));
}

#[test]
fn decision_context_json_roundtrip() {
    let decision = DecisionContext {
        id: "d1".into(),
        body_text: Some("pick REST".into()),
        rationale_text: Some("simpler".into()),
        alternatives: vec![
            AlternativeContext {
                id: "alt-grpc".into(),
                status: "rejected".into(),
                body_text: Some("use gRPC".into()),
            },
            AlternativeContext {
                id: "alt-graphql".into(),
                status: "deferred".into(),
                body_text: None,
            },
        ],
    };

    let json_str = serde_json::to_string(&decision).expect("serialize DecisionContext");
    let roundtripped: DecisionContext =
        serde_json::from_str(&json_str).expect("deserialize DecisionContext");

    assert_eq!(roundtripped, decision);
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
