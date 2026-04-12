//! Serialization tests for import types.

use std::path::PathBuf;

use serde_json::json;
use supersigil_import::{
    AmbiguityBreakdown, AmbiguityKind, Diagnostic, ImportPlan, ImportSummary, PlannedDocument,
};

#[test]
fn import_plan_serializes_to_json() {
    let mut breakdown = AmbiguityBreakdown::default();
    breakdown.record(AmbiguityKind::DuplicateId);

    let plan = ImportPlan {
        documents: vec![PlannedDocument {
            output_path: PathBuf::from("specs/auth/auth.req.md"),
            document_id: "auth/req".into(),
            content: "---\nsupersigil:\n  id: auth/req\n---\n".into(),
        }],
        ambiguity_breakdown: breakdown,
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
    assert_eq!(
        json["ambiguity_breakdown"],
        json!({
            "duplicate_id": 1,
            "unresolved_ref": 0,
            "unparseable_ref": 0,
            "missing_context": 0,
            "unsupported_feature": 0,
        })
    );
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
