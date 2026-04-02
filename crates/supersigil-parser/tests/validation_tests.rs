// Lint-time validation tests (Task 12.1)
// Requirements: 21.1, 21.2, 21.3, 25.1, 25.2

mod common;
use common::dummy_path;

use std::collections::HashMap;
use supersigil_core::{
    AttributeDef, ComponentDef, ComponentDefs, ExtractedComponent, ParseError, SourcePosition,
};
use supersigil_parser::validate_components;

fn make_component(name: &str, attrs: &[(&str, &str)]) -> ExtractedComponent {
    ExtractedComponent {
        name: name.to_string(),
        attributes: attrs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        children: Vec::new(),
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: Vec::new(),
        position: SourcePosition {
            byte_offset: 0,
            line: 1,
            column: 1,
        },
        end_position: SourcePosition {
            byte_offset: 0,
            line: 1,
            column: 1,
        },
    }
}

// ── Unknown PascalCase component produces no errors ──
// Unknown components are now skipped during extraction, so validation
// never sees them.

#[test]
fn unknown_pascal_case_component_no_error() {
    let defs = ComponentDefs::defaults();
    let components = vec![make_component("FooBarBaz", &[])];
    let mut errors = Vec::new();

    validate_components(&components, &defs, &dummy_path(), &mut errors);

    assert!(
        errors.is_empty(),
        "unknown components should produce no errors, got: {errors:?}"
    );
}

// ── Req 25.1: Known component → no error ──

#[test]
fn known_component_no_error() {
    let defs = ComponentDefs::defaults();
    // Criterion is a built-in with required `id`
    let components = vec![make_component("Criterion", &[("id", "c1")])];
    let mut errors = Vec::new();

    validate_components(&components, &defs, &dummy_path(), &mut errors);

    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

// ── Lowercase element name → no errors ──

#[test]
fn lowercase_element_no_errors() {
    let defs = ComponentDefs::defaults();
    // Lowercase names should never produce errors
    let components = vec![make_component("div", &[])];
    let mut errors = Vec::new();

    validate_components(&components, &defs, &dummy_path(), &mut errors);

    assert!(
        errors.is_empty(),
        "lowercase elements should not produce errors, got: {errors:?}"
    );
}

// ── Req 21.1: Missing required attribute → MissingRequiredAttribute error ──

#[test]
fn missing_required_attribute_produces_error() {
    let defs = ComponentDefs::defaults();
    // Criterion requires `id`, but we omit it
    let components = vec![make_component("Criterion", &[])];
    let mut errors = Vec::new();

    validate_components(&components, &defs, &dummy_path(), &mut errors);

    assert_eq!(errors.len(), 1, "expected 1 error, got: {errors:?}");
    assert!(
        matches!(
            &errors[0],
            ParseError::MissingRequiredAttribute { component, attribute, .. }
            if component == "Criterion" && attribute == "id"
        ),
        "expected MissingRequiredAttribute for Criterion.id, got: {:?}",
        errors[0]
    );
}

// ── Req 21.1: Error includes component name, attribute, and position ──

#[test]
fn missing_required_attribute_includes_position() {
    let defs = ComponentDefs::defaults();
    let mut comp = make_component("Criterion", &[]);
    comp.position = SourcePosition {
        byte_offset: 42,
        line: 5,
        column: 3,
    };
    let components = vec![comp];
    let mut errors = Vec::new();

    validate_components(&components, &defs, &dummy_path(), &mut errors);

    assert_eq!(errors.len(), 1);
    match &errors[0] {
        ParseError::MissingRequiredAttribute {
            path,
            component,
            attribute,
            position,
        } => {
            assert_eq!(path, &dummy_path());
            assert_eq!(component, "Criterion");
            assert_eq!(attribute, "id");
            assert_eq!(position.byte_offset, 42);
            assert_eq!(position.line, 5);
            assert_eq!(position.column, 3);
        }
        other => panic!("expected MissingRequiredAttribute, got: {other:?}"),
    }
}

// ── Req 21.2: All required attributes present → no error ──

#[test]
fn all_required_attributes_present_no_error() {
    let defs = ComponentDefs::defaults();
    // VerifiedBy requires `strategy`
    let components = vec![make_component("VerifiedBy", &[("strategy", "unit-test")])];
    let mut errors = Vec::new();

    validate_components(&components, &defs, &dummy_path(), &mut errors);

    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

// ── Req 21.3: Validation uses config component defs when provided ──

#[test]
fn validation_uses_custom_component_defs() {
    // Create custom defs with a "Widget" component requiring "color"
    let user_defs = HashMap::from([(
        "Widget".to_string(),
        ComponentDef {
            attributes: HashMap::from([(
                "color".to_string(),
                AttributeDef {
                    required: true,
                    list: false,
                },
            )]),
            referenceable: false,
            verifiable: false,
            target_component: None,
            description: None,
            examples: Vec::new(),
        },
    )]);
    let defs = ComponentDefs::merge(ComponentDefs::defaults(), user_defs).unwrap();

    // Widget is known, but missing required `color`
    let components = vec![make_component("Widget", &[])];
    let mut errors = Vec::new();

    validate_components(&components, &defs, &dummy_path(), &mut errors);

    assert_eq!(errors.len(), 1, "expected 1 error, got: {errors:?}");
    assert!(
        matches!(
            &errors[0],
            ParseError::MissingRequiredAttribute { component, attribute, .. }
            if component == "Widget" && attribute == "color"
        ),
        "expected MissingRequiredAttribute for Widget.color, got: {:?}",
        errors[0]
    );
}

// ── Req 25.2: Validation uses built-in defaults when no config ──

#[test]
fn validation_uses_builtin_defaults_when_no_config() {
    let defs = ComponentDefs::defaults();
    // "References" is a built-in requiring `refs`
    let components = vec![make_component("References", &[])];
    let mut errors = Vec::new();

    validate_components(&components, &defs, &dummy_path(), &mut errors);

    assert_eq!(errors.len(), 1, "expected 1 error, got: {errors:?}");
    assert!(
        matches!(
            &errors[0],
            ParseError::MissingRequiredAttribute { component, attribute, .. }
            if component == "References" && attribute == "refs"
        ),
        "expected MissingRequiredAttribute for References.refs, got: {:?}",
        errors[0]
    );
}

// ── Multiple errors: unknown component skipped, missing required attr detected ──

#[test]
fn multiple_validation_errors_collected() {
    let defs = ComponentDefs::defaults();
    let components = vec![
        make_component("UnknownThing", &[]), // unknown, skipped by validate
        make_component("Criterion", &[]),    // missing required `id`
    ];
    let mut errors = Vec::new();

    validate_components(&components, &defs, &dummy_path(), &mut errors);

    // Only 1 error: MissingRequiredAttribute for Criterion (UnknownThing is skipped)
    assert_eq!(errors.len(), 1, "expected 1 error, got: {errors:?}");
    assert!(
        matches!(
            &errors[0],
            ParseError::MissingRequiredAttribute { component, attribute, .. }
            if component == "Criterion" && attribute == "id"
        ),
        "expected MissingRequiredAttribute for Criterion.id, got: {:?}",
        errors[0]
    );
}

// ── Nested children are also validated ──

#[test]
fn nested_children_validated() {
    let defs = ComponentDefs::defaults();
    let mut parent = make_component("AcceptanceCriteria", &[]);
    // Child Criterion missing required `id`
    parent.children.push(make_component("Criterion", &[]));
    let components = vec![parent];
    let mut errors = Vec::new();

    validate_components(&components, &defs, &dummy_path(), &mut errors);

    assert_eq!(
        errors.len(),
        1,
        "expected 1 error for nested child, got: {errors:?}"
    );
    assert!(
        matches!(
            &errors[0],
            ParseError::MissingRequiredAttribute { component, attribute, .. }
            if component == "Criterion" && attribute == "id"
        ),
        "expected MissingRequiredAttribute for nested Criterion.id, got: {:?}",
        errors[0]
    );
}
