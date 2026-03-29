use std::path::PathBuf;

use serde_json::json;
use supersigil_import::{Diagnostic, ImportPlan, ImportSummary, PlannedDocument};

#[test]
fn import_plan_serializes_to_json() {
    let plan = ImportPlan {
        documents: vec![PlannedDocument {
            output_path: PathBuf::from("specs/auth/auth.req.md"),
            document_id: "auth/req".into(),
            content: "---\nsupersigil:\n  id: auth/req\n---\n".into(),
        }],
        ambiguity_count: 1,
        summary: ImportSummary {
            criteria_converted: 3,
            validates_resolved: 2,
            tasks_converted: 5,
            features_processed: 1,
        },
        diagnostics: vec![Diagnostic::Warning {
            message: "test warning".into(),
        }],
    };
    let json = serde_json::to_value(&plan).expect("serialize ImportPlan");

    assert_eq!(
        json["documents"],
        json!([{
            "output_path": "specs/auth/auth.req.md",
            "document_id": "auth/req",
            "content": "---\nsupersigil:\n  id: auth/req\n---\n",
        }])
    );
    assert_eq!(json["ambiguity_count"], 1);
    assert_eq!(
        json["summary"],
        json!({
            "criteria_converted": 3,
            "validates_resolved": 2,
            "tasks_converted": 5,
            "features_processed": 1,
        })
    );
    assert_eq!(
        json["diagnostics"],
        json!([{ "Warning": { "message": "test warning" } }])
    );
}
