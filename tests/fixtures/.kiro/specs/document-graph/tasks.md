# Implementation Plan: Document Graph

## Overview

Build the `supersigil-core::graph` module following TDD: for each pipeline stage, write property tests first (red), implement to make them pass (green), then refactor. The build pipeline is sequential — indexing → component indexing → ref resolution → task implements → cycle detection → topo sort → reverse mappings → tracked files → query layer. Each task starts with the property test, then the implementation.

## Tasks

- [x] 1. Set up graph module skeleton and error types
  - [x] 1.1 Create `crates/supersigil-core/src/graph/` directory with `mod.rs`, `error.rs`, `index.rs`, `resolve.rs`, `cycle.rs`, `topo.rs`, `reverse.rs`, `query.rs`, and `tests/` subdirectory with `mod.rs` and `generators.rs`
    - Define `GraphError` enum in `error.rs` and `QueryError` enum in `query.rs` (both with `thiserror` derives) per the design
    - Define `ResolvedRef`, `DocumentGraph` struct (with empty fields), `ContextOutput`, `CriterionContext`, `DocRef`, `TaskContext`, `PlanOutput`, `OutstandingCriterion`, `PlanTask`, `IllustrationRef`, `PlanQuery` types in `mod.rs` / `query.rs`
    - Stub `build_graph` function signature: `pub fn build_graph(documents: Vec<SpecDocument>, config: &Config) -> Result<DocumentGraph, Vec<GraphError>>`
    - Stub `DocumentGraph` read-only accessor methods (return `None`/empty defaults). These stubs are replaced with real implementations as each pipeline stage is completed: `document`/`documents` in task 3.3, `component` in task 4.3, `resolved_refs` in task 6.5, `task_implements` in task 7.2, `task_order`/`doc_order` in task 10.3, `validates`/`implements`/`illustrates` in task 12.3, `tracked_files`/`all_tracked_files` in task 13.3, `doc_project` in task 3.3.
    - Register `mod graph;` in `crates/supersigil-core/src/lib.rs` and add public re-exports
    - _Requirements: 13.3_

  - [x] 1.2 Create proptest generators in `tests/generators.rs`
    - `arb_frontmatter()` — generates `Frontmatter` with random IDs (alphanumeric + `/` + `-`), optional `doc_type`, optional `status`
    - `arb_extracted_component(name, referenceable_id)` — generates `ExtractedComponent` with given name and optional `id` attribute
    - `arb_spec_document(components)` — generates `SpecDocument` with given components and random frontmatter
    - `arb_document_set(n)` — generates `n` documents with guaranteed unique IDs
    - `arb_config()` — generates `Config` with optional multi-project setup and `ComponentDefs`
    - `arb_dag(n)` — generates a random DAG with `n` nodes for dependency testing
    - _Requirements: all (generators support all property tests)_

- [x] 2. Checkpoint — Verify module compiles
  - Ensure `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features`, and `cargo nextest run` all pass. Ask the user if questions arise.

- [x] 3. Document indexing (index.rs)
  - [x] 3.1 Write property test for document index round-trip
    - **Property 1: Document index round-trip**
    - Generate a collection of `SpecDocument` values with unique IDs, build the graph, look up each document by ID, assert it matches the original
    - **Validates: Requirements 1.1, 1.4**

  - [x] 3.2 Write property test for duplicate document ID detection
    - **Property 2: Duplicate document ID detection**
    - Generate documents where two or more share the same `frontmatter.id`, assert `build_graph` returns `DuplicateId` error with the conflicting ID and all file paths
    - **Validates: Requirements 1.2**

  - [x] 3.3 Implement document indexing in `index.rs`
    - Build `HashMap<String, SpecDocument>` from input documents keyed by `frontmatter.id`
    - Detect duplicate IDs during insertion, emit `GraphError::DuplicateId` with all conflicting paths
    - Build `doc_project` map from `Config` project membership
    - Replace the `document`, `documents`, and `doc_project` accessor stubs with real implementations
    - Wire into `build_graph` as pipeline stage 1
    - _Requirements: 1.1, 1.2, 1.3, 1.4_

- [x] 4. Referenceable component indexing (index.rs)
  - [x] 4.1 Write property test for component index round-trip
    - **Property 3: Referenceable component index round-trip**
    - Generate documents with referenceable components (including nested `Criterion` inside `AcceptanceCriteria`), build the graph, look up each by `(doc_id, component_id)`, assert match
    - **Validates: Requirements 2.1, 2.3, 2.4**

  - [x] 4.2 Write property test for duplicate component ID detection
    - **Property 4: Duplicate component ID detection**
    - Generate a document with two referenceable components sharing the same `id`, assert `DuplicateComponentId` error with positions
    - **Validates: Requirements 2.2**

  - [x] 4.3 Implement component indexing in `index.rs`
    - Iterate all documents and their components recursively
    - For each component whose `ComponentDef` has `referenceable = true`, extract `id` attribute and index as `(doc_id, component_id) → (doc_id, component)`
    - Detect duplicate component IDs within the same document, emit `GraphError::DuplicateComponentId`
    - Replace the `component` accessor stub with the real implementation
    - Wire into `build_graph` as pipeline stage 2
    - _Requirements: 2.1, 2.2, 2.3, 2.4_

- [x] 5. Checkpoint — Indexing complete
  - Ensure all tests pass, ask the user if questions arise.

- [x] 6. Ref resolution (resolve.rs)
  - [x] 6.1 Write property test for valid ref resolution
    - **Property 5: Valid refs resolve successfully**
    - Generate documents with valid refs (doc-only and doc#fragment), assert `ResolvedRef` produced with correct target doc ID and fragment, including `target_component` type matching
    - **Validates: Requirements 3.2, 3.3, 3.4**

  - [x] 6.2 Write property test for invalid ref detection
    - **Property 6: Invalid refs produce broken_ref errors**
    - Generate documents with refs pointing to nonexistent doc IDs, nonexistent fragments, or wrong `target_component` types, assert `BrokenRef` errors
    - **Validates: Requirements 3.5, 3.6, 3.7**

  - [x] 6.3 Write property test for non-isolated cross-project refs
    - **Property 7: Non-isolated cross-project refs resolve globally**
    - Generate multi-project config without `isolated = true`, create cross-project refs, assert they resolve successfully
    - **Validates: Requirements 4.1, 4.4**

  - [x] 6.4 Write property test for isolated project ref restriction
    - **Property 8: Isolated project refs are restricted**
    - Generate a project with `isolated = true`, create a ref to a document in another project, assert `BrokenRef` error
    - **Validates: Requirements 4.2, 4.3**

  - [x] 6.5 Implement ref resolution in `resolve.rs`
    - For each component with a `refs` attribute (determined by `ComponentDefs`), split using `split_list_attribute`
    - Parse each ref into `(doc_id, Option<fragment>)` using `parse_ref` helper
    - Verify doc_id exists in document index; if fragment present, verify `(doc_id, fragment)` in component index
    - If `ComponentDef` has `target_component`, verify resolved component name matches
    - For isolated projects, restrict resolution to same-project documents
    - Convert `ListSplitError` to `BrokenRef`
    - Collect all `BrokenRef` errors, store successful `ResolvedRef` values in `resolved_refs` map keyed by `(source_doc_id, Vec<usize>)` where the `Vec<usize>` is the component index path (single-element for top-level components, multi-element for nested components per design.md)
    - Replace the `resolved_refs` accessor stub with the real implementation
    - Wire into `build_graph` as pipeline stage 3
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6, 3.7, 4.1, 4.2, 4.3, 4.4_

- [x] 7. Task implements resolution (resolve.rs)
  - [x] 7.1 Write property test for task implements resolution
    - **Property 20: Task implements resolution**
    - Generate `Task` components with `implements` attributes: valid refs with `#fragment` targeting `Criterion`, refs without fragments, refs to nonexistent criteria. Assert valid refs resolve, missing-fragment refs produce `BrokenRef`, nonexistent refs produce `BrokenRef`
    - **Validates: Requirements 11.1, 11.2**

  - [x] 7.2 Implement task implements resolution in `resolve.rs`
    - For each `Task` component with an `implements` attribute, split and resolve each ref
    - Each ref MUST include a `#fragment` targeting a `Criterion`; ref without fragment is `BrokenRef`
    - Store resolved mappings in `task_implements: HashMap<(String, String), Vec<(String, String)>>`
    - Replace the `task_implements` accessor stub with the real implementation
    - Wire into `build_graph` as pipeline stage 4
    - _Requirements: 11.1, 11.2, 11.3_

- [x] 8. Checkpoint — Resolution complete
  - Ensure all tests pass, ask the user if questions arise.

- [x] 9. Cycle detection (cycle.rs)
  - [x] 9.1 Write property test for acyclic task graphs
    - **Property 9: Acyclic task graphs produce no cycle errors**
    - Generate tasks documents where `depends` edges form a DAG (using `arb_dag`), assert no `TaskDependencyCycle` errors
    - **Validates: Requirements 5.2**

  - [x] 9.2 Write property test for cyclic task graphs
    - **Property 10: Cyclic task graphs produce cycle errors**
    - Generate tasks documents with cycles in `depends` (including self-references), assert `TaskDependencyCycle` error with cycle participants
    - **Validates: Requirements 5.3**

  - [x] 9.3 Write property test for task depends scoping
    - **Property 11: Task depends scoping and resolution**
    - Generate nested tasks with `depends` referencing non-sibling or nonexistent task IDs, assert `BrokenRef` errors. Valid sibling refs should resolve.
    - **Validates: Requirements 5.4, 5.5**

  - [x] 9.4 Write property test for acyclic document dependency graphs
    - **Property 12: Acyclic document dependency graphs produce no cycle errors**
    - Generate documents with `DependsOn` refs forming a DAG, assert no `DocumentDependencyCycle` errors
    - **Validates: Requirements 6.2**

  - [x] 9.5 Write property test for cyclic document dependency graphs
    - **Property 13: Cyclic document dependency graphs produce cycle errors**
    - Generate documents with `DependsOn` refs forming cycles (including self-references), assert `DocumentDependencyCycle` error with cycle participants
    - **Validates: Requirements 6.3**

  - [x] 9.6 Implement cycle detection in `cycle.rs`
    - Implement DFS-based cycle detection with White/Gray/Black coloring
    - For task dependencies: build per-document directed graph from `Task` `depends` attributes, scope resolution to siblings (same parent for nested, same document for top-level)
    - For document dependencies: build directed graph from `DependsOn` resolved refs
    - Collect all cycles (continue DFS after finding a cycle), deduplicate equivalent cycles by normalizing to lexicographically smallest start node
    - Emit `TaskDependencyCycle` per document, `DocumentDependencyCycle` for document graph
    - Emit `BrokenRef` for `depends` refs to nonexistent sibling tasks
    - Wire into `build_graph` as pipeline stages 5 (task cycles) and 6 (document cycles)
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5, 6.1, 6.2, 6.3_

- [x] 10. Topological sort (topo.rs)
  - [x] 10.1 Write property test for topological order invariant
    - **Property 14: Topological order invariant**
    - Generate valid DAGs (task and document), compute topo order, assert for every edge (A depends on B) that B appears before A
    - **Validates: Requirements 7.1, 7.2, 7.3**

  - [x] 10.2 Write property test for topological sort determinism
    - **Property 15: Topological sort determinism**
    - Generate valid DAGs, sort twice on identical input, assert identical output. Verify tiebreaker: declaration order for tasks, alphabetical for documents
    - **Validates: Requirements 7.4**

  - [x] 10.3 Implement topological sort in `topo.rs`
    - Implement Kahn's algorithm with tiebreaking
    - Task tiebreaker: declaration order (index in source document's component list)
    - Document tiebreaker: alphabetical by document ID
    - Use `BTreeSet` as the priority queue for deterministic ordering
    - Compute `task_topo_orders` per tasks document and `doc_topo_order` for document graph
    - Replace the `task_order` and `doc_order` accessor stubs with real implementations
    - Wire into `build_graph` as pipeline stage 7
    - _Requirements: 7.1, 7.2, 7.3, 7.4_

- [x] 11. Checkpoint — Graph structure complete
  - Ensure all tests pass, ask the user if questions arise.

- [x] 12. Reverse mappings (reverse.rs)
  - [x] 12.1 Write property test for reverse mapping completeness
    - **Property 16: Reverse mapping completeness**
    - Generate documents with `Validates`, `Implements`, `Illustrates` components with resolved refs. Assert each target has the source doc ID in its reverse set. Assert `Implements` discards fragments (document-level only). Assert duplicate refs contribute only once.
    - **Validates: Requirements 8.1, 8.2, 8.3, 8.6**

  - [x] 12.2 Write property test for reverse mapping queryability
    - **Property 23: Reverse mapping queryability**
    - Build a graph, query `validates`, `implements`, `illustrates` by target doc ID and `(doc_id, fragment)`. Assert correct results. Assert unreferenced targets return empty set.
    - **Validates: Requirements 8.4, 8.5**

  - [x] 12.3 Implement reverse mappings in `reverse.rs`
    - Iterate all resolved refs, build three reverse indexes: `validates_reverse`, `implements_reverse`, `illustrates_reverse`
    - `Validates`: key by `(target_doc_id, Option<fragment>)` → `BTreeSet<source_doc_id>`
    - `Implements`: key by `target_doc_id` → `BTreeSet<source_doc_id>` (discard fragments)
    - `Illustrates`: key by `(target_doc_id, Option<fragment>)` → `BTreeSet<source_doc_id>`
    - Deduplicate refs within same attribute
    - Implement accessor methods that return empty `BTreeSet` for unreferenced targets
    - Wire into `build_graph` as pipeline stage 8
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5, 8.6_

- [x] 13. TrackedFiles indexing (index.rs)
  - [x] 13.1 Write property test for TrackedFiles index completeness
    - **Property 21: TrackedFiles index completeness**
    - Generate documents with one or more `TrackedFiles` components, build graph, assert all path globs aggregated under the document ID and retrievable
    - **Validates: Requirements 12.1, 12.2, 12.4**

  - [x] 13.2 Write property test for TrackedFiles index iteration
    - **Property 27: TrackedFiles index iteration**
    - Build a graph with multiple documents having `TrackedFiles`, iterate `all_tracked_files()`, assert every `(doc_id, globs)` pair is yielded
    - **Validates: Requirements 12.3**

  - [x] 13.3 Implement TrackedFiles indexing
    - Iterate all documents, find `TrackedFiles` components, split `paths` attribute, aggregate under document ID
    - Implement `tracked_files(doc_id)` and `all_tracked_files()` accessors
    - Wire into `build_graph` as pipeline stage 9
    - _Requirements: 12.1, 12.2, 12.3, 12.4_

- [x] 14. Error aggregation
  - [x] 14.1 Write property test for error aggregation
    - **Property 22: Error aggregation**
    - Generate input with multiple independent errors (duplicate IDs, broken refs, cycles), assert `build_graph` returns all errors in a single `Vec<GraphError>`. Generate error-free input, assert `Ok(DocumentGraph)`.
    - **Validates: Requirements 13.1, 13.2, 13.3**

  - [x] 14.2 Verify error aggregation across all pipeline stages
    - Ensure the shared error vector is threaded through all stages
    - Ensure each stage continues with best-effort data after errors
    - Ensure `build_graph` returns `Err(errors)` if any errors collected, `Ok(graph)` otherwise
    - _Requirements: 13.1, 13.2, 13.3_

- [x] 15. Checkpoint — Core graph construction complete
  - Ensure all tests pass, ask the user if questions arise.

- [x] 16. Context query (query.rs)
  - [x] 16.1 Write property test for context output completeness
    - **Property 17: Context output completeness**
    - Build a graph with documents, criteria, reverse mappings, and linked tasks. Call `context(id)`, assert output contains: target document, criteria with validation/illustration status, implementing documents, tasks in topological order.
    - **Validates: Requirements 9.1, 9.2, 9.3, 9.4, 9.5**

  - [x] 16.2 Write property test for context query error
    - **Property 24: Context query error for nonexistent document**
    - Call `context(id)` with a nonexistent ID, assert `QueryError::DocumentNotFound`
    - **Validates: Requirements 9.6**

  - [x] 16.3 Write property test for task-to-criterion mappings in context output
    - **Property 28a: Task-to-criterion mappings in ContextOutput**
    - Build a graph with tasks that have resolved `implements` refs. Assert `ContextOutput` includes the resolved criterion refs in each task's `implements` field.
    - **Validates: Requirements 11.3 (context half)**

  - [x] 16.4 Implement context query in `query.rs`
    - Look up document by ID, return `QueryError::DocumentNotFound` if missing
    - Extract criteria from document components
    - For each criterion, look up `validates_reverse` and `illustrates_reverse`, include validating doc status
    - Look up `implements_reverse` for the document
    - Find linked tasks documents (tasks whose `implements` refs point to criteria in this document), collect tasks in topological order
    - Assemble and return `ContextOutput`
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5, 9.6, 9.7_

- [x] 17. Plan query (query.rs)
  - [x] 17.1 Write property test for plan output correctness
    - **Property 18: Plan output correctness**
    - Build a graph, call `plan(PlanQuery::Document(id))`, assert: outstanding criteria (no validating doc), pending tasks (status ≠ done) in topo order, completed tasks with implements refs, illustrating documents.
    - **Validates: Requirements 10.1, 10.2, 10.3, 10.4**

  - [x] 17.2 Write property test for plan prefix aggregation
    - **Property 19: Plan prefix aggregation**
    - Build a graph with documents sharing a prefix (e.g., `auth/`), call `plan(PlanQuery::Prefix("auth/"))`, assert aggregation: outstanding criteria grouped by source doc, tasks per tasks doc in topo order.
    - **Validates: Requirements 10.5**

  - [x] 17.3 Write property test for project-wide plan
    - **Property 26: Project-wide plan**
    - Build a graph, call `plan(PlanQuery::All)`, assert output covers all documents: outstanding criteria from all requirement docs, pending and completed tasks from all tasks docs.
    - **Validates: Requirements 10.6**

  - [x] 17.4 Write property test for plan query error
    - **Property 25: Plan query error for nonexistent target**
    - Call `PlanQuery::parse` with a string that matches no exact ID and no prefix, assert `QueryError::NoMatchingDocuments`
    - **Validates: Requirements 10.7**

  - [x] 17.5 Write property test for task-to-criterion mappings in plan output
    - **Property 28b: Task-to-criterion mappings in PlanOutput**
    - Build a graph with tasks that have resolved `implements` refs. Assert `PlanOutput` includes the resolved criterion refs in each task's `implements` field.
    - **Validates: Requirements 11.3 (plan half)**

  - [x] 17.6 Implement plan query in `query.rs`
    - Implement `PlanQuery::parse` with disambiguation: empty → `All`, exact match → `Document`, prefix match → `Prefix`, else → error
    - For `Document`: find outstanding criteria (no validators in reverse map), pending tasks (status ≠ done) in topo order, completed tasks, illustrating docs
    - For `Prefix`: aggregate all matching documents, group outstanding criteria by source doc, list tasks per tasks doc
    - For `All`: same as prefix but covering all documents
    - Assemble and return `PlanOutput`
    - _Requirements: 10.1, 10.2, 10.3, 10.4, 10.5, 10.6, 10.7, 10.8_

- [x] 18. Unit tests (tests/unit.rs)
  - [x] 18.1 Write unit tests for concrete examples from the supersigil design document
    - The `auth/req/login` example: build a small graph with requirement, tasks, and validation documents, assert context and plan outputs match expected structure
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5, 9.7, 10.1, 10.2, 10.3, 10.4, 10.8_

  - [x] 18.2 Write unit tests for edge cases
    - Empty document collection → `Ok(DocumentGraph)` with empty indexes
    - Document with no components → indexed, no component index entries, no reverse mappings
    - Self-referencing `depends` → `TaskDependencyCycle` with single-node cycle
    - Single-node document cycle (DependsOn self) → `DocumentDependencyCycle`
    - Context/plan for documents with no criteria, no tasks, no reverse mappings → valid empty-ish outputs
    - _Requirements: 5.3, 6.3, 9.1, 9.6, 9.7, 10.1, 10.7, 10.8, 13.1, 13.2, 13.3_

  - [x] 18.3 Write integration unit tests for cross-phase error aggregation
    - Document with both broken refs and cycles should report both error types in a single `Vec<GraphError>`
    - Document with duplicate component IDs and broken refs should report both
    - _Requirements: 13.1, 13.2, 13.3_

- [x] 19. Final checkpoint — All tests pass
  - Run `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features`, `cargo nextest run`
  - Ensure zero warnings and zero errors
  - Ask the user if questions arise.

## Notes

- Each property test sub-task references its property number from the design document and the requirements it validates
- The TDD flow is: write property test (red) → implement code (green) → refactor → run `cargo fmt`/`clippy`/`nextest`
- Accessor stubs created in task 1.1 are replaced with real implementations in the corresponding pipeline stage tasks (see 1.1 for the full mapping)
- Property 28 is split into 28a (ContextOutput, task 16.3) and 28b (PlanOutput, task 17.5) so each test goes green in the same task as its implementation
- Checkpoints are placed after each major pipeline stage group to catch regressions early
- `proptest` is already a dev-dependency of `supersigil-core`
- All code goes in `crates/supersigil-core/src/graph/` with tests in `crates/supersigil-core/src/graph/tests/`
