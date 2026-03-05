// Snapshot tests for the Kiro import pipeline.
//
// Full-pipeline snapshots feed real `.kiro/specs/` directories through
// `plan_kiro_import` and snapshot each generated MDX document using `insta`.
// Synthetic snapshots cover edge cases with hand-crafted minimal inputs.

mod common;

use common::{config_for, workspace_root, write_kiro_spec};
use supersigil_import::plan_kiro_import;

// ---------------------------------------------------------------------------
// 21.2: Full-pipeline snapshot tests for real Kiro specs
// ---------------------------------------------------------------------------

/// Copy a single real Kiro spec feature into an isolated temp dir and run the
/// import pipeline, returning the plan. This isolates each test from other
/// features that live in the same `.kiro/specs/` directory.
fn plan_single_real_feature(feature_name: &str) -> supersigil_import::ImportPlan {
    let real_specs = workspace_root().join(".kiro").join("specs");
    let real_feature = real_specs.join(feature_name);

    let tmp = tempfile::tempdir().unwrap();
    let specs_dir = tmp.path().join("specs");
    let feature_dir = specs_dir.join(feature_name);
    std::fs::create_dir_all(&feature_dir).unwrap();

    // Copy whichever files exist
    for filename in ["requirements.md", "design.md", "tasks.md"] {
        let src = real_feature.join(filename);
        if src.exists() {
            std::fs::copy(&src, feature_dir.join(filename)).unwrap();
        }
    }

    let config = config_for(&specs_dir, &tmp.path().join("out"));
    plan_kiro_import(&config).expect("plan_kiro_import should succeed for real spec")
}

/// Snapshot a single document from a plan, identified by a suffix in its
/// `document_id` (e.g., `"req/parser-and-config"`).
fn snapshot_doc<'a>(plan: &'a supersigil_import::ImportPlan, id_suffix: &str) -> &'a str {
    let doc = plan
        .documents
        .iter()
        .find(|d| d.document_id.ends_with(id_suffix))
        .unwrap_or_else(|| {
            let ids: Vec<_> = plan.documents.iter().map(|d| &d.document_id).collect();
            panic!("no document with id ending in '{id_suffix}'; available: {ids:?}");
        });
    &doc.content
}

#[test]
fn snapshot_parser_and_config_req() {
    let plan = plan_single_real_feature("parser-and-config");
    let content = snapshot_doc(&plan, "req/parser-and-config");
    insta::assert_snapshot!("parser_and_config__req", content);
}

#[test]
fn snapshot_parser_and_config_design() {
    let plan = plan_single_real_feature("parser-and-config");
    let content = snapshot_doc(&plan, "design/parser-and-config");
    insta::assert_snapshot!("parser_and_config__design", content);
}

#[test]
fn snapshot_parser_and_config_tasks() {
    let plan = plan_single_real_feature("parser-and-config");
    let content = snapshot_doc(&plan, "tasks/parser-and-config");
    insta::assert_snapshot!("parser_and_config__tasks", content);
}

#[test]
fn snapshot_document_graph_req() {
    let plan = plan_single_real_feature("document-graph");
    let content = snapshot_doc(&plan, "req/document-graph");
    insta::assert_snapshot!("document_graph__req", content);
}

#[test]
fn snapshot_document_graph_design() {
    let plan = plan_single_real_feature("document-graph");
    let content = snapshot_doc(&plan, "design/document-graph");
    insta::assert_snapshot!("document_graph__design", content);
}

#[test]
fn snapshot_document_graph_tasks() {
    let plan = plan_single_real_feature("document-graph");
    let content = snapshot_doc(&plan, "tasks/document-graph");
    insta::assert_snapshot!("document_graph__tasks", content);
}

// ---------------------------------------------------------------------------
// 21.3: Synthetic snapshot tests for edge cases
// ---------------------------------------------------------------------------

/// Design-only feature (no requirements or tasks) → design MDX should contain
/// an ambiguity marker for missing requirements and no `<Implements>`.
#[test]
fn snapshot_edge_design_only() {
    let tmp = tempfile::tempdir().unwrap();
    let specs_dir = tmp.path().join("specs");

    let design_md = "\
# Design Document: Standalone Design

## Overview

This feature has a design but no requirements or tasks.

## Architecture

The system uses a simple pipeline.

```mermaid
graph TD
    A[Input] --> B[Output]
```

## Correctness Properties

### Property 1: Basic invariant

The output is always non-empty.

**Validates: Requirements 1.1**
";

    write_kiro_spec(&specs_dir, "design-only", None, Some(design_md), None);

    let config = config_for(&specs_dir, &tmp.path().join("out"));
    let plan = plan_kiro_import(&config).unwrap();
    let content = snapshot_doc(&plan, "design/design-only");
    insta::assert_snapshot!("edge__design_only", content);
}

/// Tasks with `N/A` metadata → tasks MDX with `TaskRefs::None` handling
/// (no `implements` attribute, no ambiguity marker for the N/A itself).
#[test]
fn snapshot_edge_tasks_na_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let specs_dir = tmp.path().join("specs");

    let tasks_md = "\
# Implementation Plan: NA Metadata

## Tasks

- [x] 1. Set up project scaffolding
  - Create directory structure
  - _Requirements: N/A_

  - [x] 1.1 Initialize repository
    - Run git init
    - _Requirements: N/A (infrastructure)_

- [ ] 2. Implement core logic
  - Write the main algorithm
  - _Requirements: 1.1, 1.2_
";

    let req_md = "\
# Requirements Document: NA Metadata

### Requirement 1: Core Logic

**User Story:** As a user, I want core logic.

#### Acceptance Criteria

1. THE System SHALL process input.
2. THE System SHALL produce output.
";

    write_kiro_spec(
        &specs_dir,
        "na-metadata",
        Some(req_md),
        None,
        Some(tasks_md),
    );

    let config = config_for(&specs_dir, &tmp.path().join("out"));
    let plan = plan_kiro_import(&config).unwrap();
    let content = snapshot_doc(&plan, "tasks/na-metadata");
    insta::assert_snapshot!("edge__tasks_na_metadata", content);
}

/// Tasks with optional markers (`[x]* 2.1 ...`) → tasks MDX should include
/// the task with an ambiguity marker noting the optional status.
#[test]
fn snapshot_edge_tasks_optional_marker() {
    let tmp = tempfile::tempdir().unwrap();
    let specs_dir = tmp.path().join("specs");

    let tasks_md = "\
# Implementation Plan: Optional Tasks

## Tasks

- [x] 1. Required task
  - Do the required work

  - [x] 1.1 Required sub-task
    - Sub-task description

  - [x]* 1.2 Optional sub-task
    - This sub-task is optional

- [ ]* 2. Optional top-level task
  - This entire task is optional
";

    write_kiro_spec(&specs_dir, "optional-tasks", None, None, Some(tasks_md));

    let config = config_for(&specs_dir, &tmp.path().join("out"));
    let plan = plan_kiro_import(&config).unwrap();
    let content = snapshot_doc(&plan, "tasks/optional-tasks");
    insta::assert_snapshot!("edge__tasks_optional_marker", content);
}

/// Non-requirement Validates target (`Design Decision 5`) → design MDX should
/// contain an ambiguity marker noting the non-requirement target.
#[test]
fn snapshot_edge_non_requirement_validates() {
    let tmp = tempfile::tempdir().unwrap();
    let specs_dir = tmp.path().join("specs");

    let design_md = "\
# Design: Mixed Validates

## Overview

A design with both requirement and non-requirement Validates targets.

## Correctness Properties

### Property 1: Standard property

This property validates against requirements.

**Validates: Requirements 1.1, 1.2**

### Property 2: Non-standard property

This property validates against a design decision.

**Validates: Design Decision 5**

### Property 3: Another standard property

This one is normal.

**Validates: Requirements 2.1**
";

    let req_md = "\
# Requirements Document: Mixed Validates

### Requirement 1: First Requirement

**User Story:** As a user, I want the first feature.

#### Acceptance Criteria

1. THE System SHALL do thing one.
2. THE System SHALL do thing two.

### Requirement 2: Second Requirement

**User Story:** As a user, I want the second feature.

#### Acceptance Criteria

1. THE System SHALL do thing three.
";

    write_kiro_spec(
        &specs_dir,
        "mixed-validates",
        Some(req_md),
        Some(design_md),
        None,
    );

    let config = config_for(&specs_dir, &tmp.path().join("out"));
    let plan = plan_kiro_import(&config).unwrap();
    let content = snapshot_doc(&plan, "design/mixed-validates");
    insta::assert_snapshot!("edge__non_requirement_validates", content);
}
