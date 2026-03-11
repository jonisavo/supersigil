// Unit tests for ComponentDefs: defaults, merge, is_known, get
// Task 4.1: TDD — tests written before implementation
// Requirements: 7.1, 7.3, 14.4, 14.5

use std::collections::HashMap;

use supersigil_core::{AttributeDef, ComponentDef, ComponentDefError, ComponentDefs};

// ---------------------------------------------------------------------------
// Helper: build an AttributeDef concisely
// ---------------------------------------------------------------------------

fn attr(required: bool, list: bool) -> AttributeDef {
    AttributeDef { required, list }
}

// ---------------------------------------------------------------------------
// ComponentDefs::defaults() — 8 built-in components (Req 7.3, 14.4)
// ---------------------------------------------------------------------------

const BUILTIN_NAMES: [&str; 8] = [
    "AcceptanceCriteria",
    "Criterion",
    "References",
    "VerifiedBy",
    "Implements",
    "Task",
    "TrackedFiles",
    "DependsOn",
];

#[test]
fn defaults_returns_exactly_eight_components() {
    let defs = ComponentDefs::defaults();
    assert_eq!(defs.len(), 8);
    for name in &BUILTIN_NAMES {
        assert!(defs.is_known(name), "missing built-in: {name}");
    }
}

#[test]
fn acceptance_criteria_has_no_attributes() {
    let defs = ComponentDefs::defaults();
    let ac = defs.get("AcceptanceCriteria").unwrap();
    assert!(ac.attributes.is_empty());
    assert!(!ac.referenceable);
    assert_eq!(ac.target_component, None);
}

#[test]
fn criterion_has_required_id_and_is_referenceable() {
    let defs = ComponentDefs::defaults();
    let c = defs.get("Criterion").unwrap();
    assert_eq!(c.attributes.len(), 1);
    assert_eq!(c.attributes["id"], attr(true, false));
    assert!(c.referenceable);
    assert_eq!(c.target_component, None);
}

#[test]
fn references_has_required_list_refs() {
    let defs = ComponentDefs::defaults();
    let v = defs.get("References").unwrap();
    assert_eq!(v.attributes.len(), 1);
    assert_eq!(v.attributes["refs"], attr(true, true));
    assert!(!v.referenceable);
    assert_eq!(v.target_component, None);
}

#[test]
fn verified_by_attributes() {
    let defs = ComponentDefs::defaults();
    let vb = defs.get("VerifiedBy").unwrap();
    assert_eq!(vb.attributes.len(), 3);
    assert_eq!(vb.attributes["strategy"], attr(true, false));
    assert_eq!(vb.attributes["tag"], attr(false, false));
    assert_eq!(vb.attributes["paths"], attr(false, true));
    assert!(!vb.referenceable);
    assert_eq!(vb.target_component, None);
}

#[test]
fn implements_has_required_list_refs() {
    let defs = ComponentDefs::defaults();
    let i = defs.get("Implements").unwrap();
    assert_eq!(i.attributes.len(), 1);
    assert_eq!(i.attributes["refs"], attr(true, true));
    assert!(!i.referenceable);
    assert_eq!(i.target_component, None);
}

#[test]
fn task_attributes() {
    let defs = ComponentDefs::defaults();
    let t = defs.get("Task").unwrap();
    assert_eq!(t.attributes.len(), 4);
    assert_eq!(t.attributes["id"], attr(true, false));
    assert_eq!(t.attributes["status"], attr(false, false));
    assert_eq!(t.attributes["implements"], attr(false, true));
    assert_eq!(t.attributes["depends"], attr(false, true));
    assert!(t.referenceable);
    assert_eq!(t.target_component, None);
}

#[test]
fn tracked_files_has_required_list_paths() {
    let defs = ComponentDefs::defaults();
    let tf = defs.get("TrackedFiles").unwrap();
    assert_eq!(tf.attributes.len(), 1);
    assert_eq!(tf.attributes["paths"], attr(true, true));
    assert!(!tf.referenceable);
    assert_eq!(tf.target_component, None);
}

#[test]
fn depends_on_has_required_list_refs() {
    let defs = ComponentDefs::defaults();
    let d = defs.get("DependsOn").unwrap();
    assert_eq!(d.attributes.len(), 1);
    assert_eq!(d.attributes["refs"], attr(true, true));
    assert!(!d.referenceable);
    assert_eq!(d.target_component, None);
}

// ---------------------------------------------------------------------------
// List-typed attributes (Req 7.1, 7.3)
// ---------------------------------------------------------------------------

#[test]
fn list_typed_refs_on_references_implements_depends_on() {
    let defs = ComponentDefs::defaults();
    for name in ["References", "Implements", "DependsOn"] {
        let def = defs.get(name).unwrap();
        assert!(
            def.attributes["refs"].list,
            "{name} should have list-typed refs"
        );
    }
}

#[test]
fn list_typed_paths_on_verified_by_and_tracked_files() {
    let defs = ComponentDefs::defaults();
    assert!(defs.get("VerifiedBy").unwrap().attributes["paths"].list);
    assert!(defs.get("TrackedFiles").unwrap().attributes["paths"].list);
}

#[test]
fn list_typed_implements_and_depends_on_task() {
    let defs = ComponentDefs::defaults();
    let task = defs.get("Task").unwrap();
    assert!(task.attributes["implements"].list);
    assert!(task.attributes["depends"].list);
}

// ---------------------------------------------------------------------------
// ComponentDefs::is_known() and get()
// ---------------------------------------------------------------------------

#[test]
fn is_known_returns_true_for_builtins() {
    let defs = ComponentDefs::defaults();
    for name in &BUILTIN_NAMES {
        assert!(defs.is_known(name));
    }
}

#[test]
fn is_known_returns_false_for_unknown() {
    let defs = ComponentDefs::defaults();
    assert!(!defs.is_known("Nonexistent"));
    assert!(!defs.is_known(""));
    assert!(!defs.is_known("references")); // case-sensitive
}

#[test]
fn get_returns_none_for_unknown() {
    let defs = ComponentDefs::defaults();
    assert!(defs.get("Nonexistent").is_none());
}

// ---------------------------------------------------------------------------
// ComponentDefs::merge() (Req 14.5)
// ---------------------------------------------------------------------------

#[test]
fn merge_user_override_replaces_builtin() {
    let defaults = ComponentDefs::defaults();
    let mut user = HashMap::new();
    // Override Criterion with a different schema
    user.insert(
        "Criterion".to_string(),
        ComponentDef {
            attributes: HashMap::from([("label".to_string(), attr(true, false))]),
            referenceable: false,
            verifiable: false,
            target_component: None,
            description: None,
            examples: Vec::new(),
        },
    );

    let merged = ComponentDefs::merge(defaults, user).unwrap();
    let criterion = merged.get("Criterion").unwrap();
    // Should have the user's schema, not the built-in
    assert_eq!(criterion.attributes.len(), 1);
    assert!(criterion.attributes.contains_key("label"));
    assert!(!criterion.attributes.contains_key("id"));
    assert!(!criterion.referenceable);
}

#[test]
fn merge_new_user_component_added() {
    let defaults = ComponentDefs::defaults();
    let mut user = HashMap::new();
    user.insert(
        "CustomWidget".to_string(),
        ComponentDef {
            attributes: HashMap::from([("color".to_string(), attr(false, false))]),
            referenceable: false,
            verifiable: false,
            target_component: None,
            description: None,
            examples: Vec::new(),
        },
    );

    let merged = ComponentDefs::merge(defaults, user).unwrap();
    assert!(merged.is_known("CustomWidget"));
    let cw = merged.get("CustomWidget").unwrap();
    assert_eq!(cw.attributes.len(), 1);
    assert!(cw.attributes.contains_key("color"));
}

#[test]
fn merge_unmentioned_builtins_preserved() {
    let defaults = ComponentDefs::defaults();
    let mut user = HashMap::new();
    // Only override one component
    user.insert(
        "Criterion".to_string(),
        ComponentDef {
            attributes: HashMap::new(),
            referenceable: false,
            verifiable: false,
            target_component: None,
            description: None,
            examples: Vec::new(),
        },
    );

    let merged = ComponentDefs::merge(defaults, user).unwrap();
    // All 8 built-ins should still be present (Criterion overridden + 7 unchanged)
    // plus no new ones since we only overrode
    assert_eq!(merged.len(), 8);
    for name in &BUILTIN_NAMES {
        assert!(merged.is_known(name), "built-in {name} should be preserved");
    }
}

#[test]
fn merge_override_plus_new_component() {
    let defaults = ComponentDefs::defaults();
    let mut user = HashMap::new();
    user.insert(
        "Criterion".to_string(),
        ComponentDef {
            attributes: HashMap::new(),
            referenceable: false,
            verifiable: false,
            target_component: None,
            description: None,
            examples: Vec::new(),
        },
    );
    user.insert(
        "NewComp".to_string(),
        ComponentDef {
            attributes: HashMap::from([("x".to_string(), attr(true, true))]),
            referenceable: true,
            verifiable: false,
            target_component: Some("Criterion".to_string()),
            description: None,
            examples: Vec::new(),
        },
    );

    let merged = ComponentDefs::merge(defaults, user).unwrap();
    assert_eq!(merged.len(), 9); // 8 built-in + 1 new
    assert!(merged.is_known("NewComp"));
    assert!(merged.is_known("Criterion"));
    // Criterion should be the overridden version
    assert!(merged.get("Criterion").unwrap().attributes.is_empty());
}

#[test]
fn merge_empty_user_defs_preserves_all_defaults() {
    let defaults = ComponentDefs::defaults();
    let merged = ComponentDefs::merge(defaults, HashMap::new()).unwrap();
    assert_eq!(merged.len(), 8);
    // Verify a sample built-in is intact
    let criterion = merged.get("Criterion").unwrap();
    assert!(criterion.referenceable);
    assert_eq!(criterion.attributes["id"], attr(true, false));
}

// ---------------------------------------------------------------------------
// Description and examples on built-in components (Task 2)
// ---------------------------------------------------------------------------

#[test]
fn all_builtins_have_descriptions() {
    let defs = ComponentDefs::defaults();
    for name in &BUILTIN_NAMES {
        let def = defs.get(name).unwrap();
        assert!(
            def.description.is_some(),
            "{name} should have a description"
        );
    }
}

#[test]
fn all_builtins_have_examples() {
    let defs = ComponentDefs::defaults();
    for name in &BUILTIN_NAMES {
        let def = defs.get(name).unwrap();
        assert!(
            !def.examples.is_empty(),
            "{name} should have at least one example"
        );
    }
}

#[test]
fn list_attribute_examples_use_string_literal_syntax() {
    let defs = ComponentDefs::defaults();

    let verified_by = defs.get("VerifiedBy").unwrap();
    assert_eq!(
        verified_by.examples[1],
        "<VerifiedBy strategy=\"file-glob\" paths=\"path/to/test-file.rs\" />"
    );

    let tracked_files = defs.get("TrackedFiles").unwrap();
    assert_eq!(
        tracked_files.examples[0],
        "<TrackedFiles paths=\"src/auth/**/*.rs, tests/auth/**/*.rs\" />"
    );
}

// ---------------------------------------------------------------------------
// Verifiable field (Task 1: verifiable targets)
// ---------------------------------------------------------------------------

#[test]
fn criterion_is_verifiable() {
    let defs = ComponentDefs::defaults();
    let c = defs.get("Criterion").unwrap();
    assert!(c.verifiable, "Criterion should be verifiable");
}

#[test]
fn non_criterion_components_are_not_verifiable() {
    let defs = ComponentDefs::defaults();
    for name in &BUILTIN_NAMES {
        if *name == "Criterion" {
            continue;
        }
        let def = defs.get(name).unwrap();
        assert!(!def.verifiable, "{name} should NOT be verifiable");
    }
}

#[test]
fn verifiable_requires_referenceable() {
    let user = HashMap::from([(
        "BadVerifiable".to_string(),
        ComponentDef {
            attributes: HashMap::from([("id".to_string(), attr(true, false))]),
            referenceable: false,
            verifiable: true,
            target_component: None,
            description: None,
            examples: Vec::new(),
        },
    )]);

    let errs = ComponentDefs::merge(ComponentDefs::defaults(), user).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, ComponentDefError::VerifiableNotReferenceable { .. })),
        "expected VerifiableNotReferenceable error, got: {errs:?}"
    );
}

#[test]
fn verifiable_requires_id_attribute() {
    let user = HashMap::from([(
        "BadVerifiable".to_string(),
        ComponentDef {
            attributes: HashMap::new(),
            referenceable: true,
            verifiable: true,
            target_component: None,
            description: None,
            examples: Vec::new(),
        },
    )]);

    let errs = ComponentDefs::merge(ComponentDefs::defaults(), user).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, ComponentDefError::VerifiableMissingId { .. })),
        "expected VerifiableMissingId error, got: {errs:?}"
    );
}

#[test]
fn verifiable_not_referenceable_and_missing_id_produces_two_errors() {
    let user = HashMap::from([(
        "DoublyBad".to_string(),
        ComponentDef {
            attributes: HashMap::new(),
            referenceable: false,
            verifiable: true,
            target_component: None,
            description: None,
            examples: Vec::new(),
        },
    )]);

    let errs = ComponentDefs::merge(ComponentDefs::defaults(), user).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, ComponentDefError::VerifiableNotReferenceable { .. })),
        "expected VerifiableNotReferenceable error, got: {errs:?}"
    );
    assert!(
        errs.iter()
            .any(|e| matches!(e, ComponentDefError::VerifiableMissingId { .. })),
        "expected VerifiableMissingId error, got: {errs:?}"
    );
    assert_eq!(
        errs.len(),
        2,
        "expected exactly 2 errors for dual failure, got: {errs:?}"
    );
}
