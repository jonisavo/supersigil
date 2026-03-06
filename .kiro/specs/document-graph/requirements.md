# Requirements Document

## Introduction

The Document Graph module builds the cross-document data structure from parsed supersigil documents. It consumes `SpecDocument` values produced by the parser (Spec 1) and `Config` structs from the config loader, and produces an indexed, validated graph of document relationships. The graph supports ref resolution, cycle detection, topological sorting, reverse mapping computation, and structured query output for the `context` and `plan` commands. This module lives in `supersigil-core` as the `graph` module.

## Glossary

- **Graph_Builder**: The subsystem that constructs the `DocumentGraph` from a collection of `SpecDocument` values and a `Config`.
- **Document_Index**: A map from document ID (string) to `SpecDocument`, enabling O(1) lookup by ID.
- **Component_Index**: A map keyed by `(document_id, component_id)` pairs, mapping each referenceable component (e.g., `Criterion`, `Task`) to the owning document ID and the component itself. The key is a compound key because component IDs (like Criterion `id`) are only unique within a document, not globally.
- **Ref**: A string in a `refs` attribute, consisting of a document ID and an optional `#fragment` suffix targeting a referenceable component.
- **Fragment**: The portion of a ref after `#`, which must match the `id` attribute of a referenceable component in the target document.
- **Reverse_Mapping**: A computed index from a target document or criterion to the set of documents that reference it via `Validates`, `Implements`, or `Illustrates`.
- **Topological_Order**: A linear ordering of nodes in a DAG such that for every directed edge (u, v), u appears before v.
- **Context_Output**: A structured data representation of a document, its relationships, and its tasks in dependency order, used by the `context` command.
- **Plan_Output**: A structured data representation of outstanding criteria, pending tasks, and completed work for a document or feature prefix, used by the `plan` command.
- **Project_Scope**: The set of documents belonging to a single project in a multi-project workspace, as determined by the `Config`.
- **Isolated_Project**: A project configured with `isolated = true`, restricting ref resolution to documents within that project only.
- **Linked_Tasks_Document**: A tasks document is linked to a target requirement document when any of its `Task` components have `implements` refs pointing to criteria in the target document.
- **TrackedFiles_Index**: A map from document ID to the list of `TrackedFiles` path globs declared in that document, enabling downstream consumers (e.g., the verification engine) to match changed files against spec documents.


## Requirements

### Requirement 1: Document Indexing

**User Story:** As a verification engine consumer, I want all parsed documents indexed by their unique ID, so that any document can be looked up in constant time during ref resolution and query generation.

#### Acceptance Criteria

1. WHEN a collection of `SpecDocument` values is provided, THE Graph_Builder SHALL construct a Document_Index mapping each document's `frontmatter.id` to the corresponding `SpecDocument`.
2. WHEN two or more documents share the same `frontmatter.id`, THE Graph_Builder SHALL return a `duplicate_id` hard error identifying all documents with the conflicting ID and their file paths.
3. THE Document_Index SHALL support O(1) lookup of a `SpecDocument` by its string ID.
4. WHEN a multi-project `Config` is provided, THE Graph_Builder SHALL build a single global Document_Index spanning all projects.

### Requirement 2: Referenceable Component Indexing

**User Story:** As a verification engine consumer, I want all referenceable components (e.g., `Criterion`, `Task`) indexed by their `id` attribute, so that fragment refs can be resolved to the correct component within the correct document.

#### Acceptance Criteria

1. WHEN a `SpecDocument` contains components whose definitions in `ComponentDefs` have `referenceable = true`, THE Graph_Builder SHALL index each such component by its `id` attribute, mapping it to the owning document ID and the component.
2. WHEN two referenceable components within the same document share the same `id` attribute, THE Graph_Builder SHALL return a `duplicate_component_id` hard error identifying the conflicting components and their source positions.
3. THE Component_Index SHALL support lookup by a (document_id, fragment) pair, returning the matching referenceable component.
4. WHEN a referenceable component is nested inside another component (e.g., `Criterion` inside `AcceptanceCriteria`), THE Graph_Builder SHALL index the nested component using the parent document's ID.


### Requirement 3: Ref Resolution

**User Story:** As a verification engine consumer, I want every `refs` attribute in every component resolved to its target document and optional fragment, so that broken references are detected as hard errors.

#### Acceptance Criteria

1. WHEN a component has a `refs` attribute (as determined by `ComponentDefs`), THE Graph_Builder SHALL split the attribute value using `split_list_attribute` and resolve each ref individually.
2. WHEN a ref contains no `#fragment`, THE Graph_Builder SHALL verify that a document with the given ID exists in the Document_Index.
3. WHEN a ref contains a `#fragment`, THE Graph_Builder SHALL verify that the target document exists and contains a referenceable component whose `id` matches the fragment.
4. WHEN a ref's `#fragment` resolves to a referenceable component and the referring component's definition specifies a `target_component`, THE Graph_Builder SHALL verify that the resolved component's name matches the `target_component` value.
5. IF a ref's document ID does not exist in the Document_Index, THEN THE Graph_Builder SHALL return a `broken_ref` hard error identifying the referring document, component, and the unresolved ref string.
6. IF a ref's `#fragment` does not match any referenceable component in the target document, THEN THE Graph_Builder SHALL return a `broken_ref` hard error identifying the referring document, component, and the unresolved fragment.
7. IF a ref's fragment resolves to a component whose name does not match the expected `target_component`, THEN THE Graph_Builder SHALL return a `broken_ref` hard error identifying the type mismatch.

### Requirement 4: Cross-Project Ref Resolution

**User Story:** As a monorepo user, I want refs to resolve across all projects by default, so that specs in one project can reference criteria in another project.

#### Acceptance Criteria

1. WHEN a multi-project `Config` is provided without `isolated = true` on any project, THE Graph_Builder SHALL resolve refs against the global Document_Index spanning all projects.
2. WHEN a project is configured with `isolated = true`, THE Graph_Builder SHALL restrict ref resolution for documents in that project to only documents within the same project.
3. IF a document in an Isolated_Project contains a ref that resolves to a document in a different project, THEN THE Graph_Builder SHALL return a `broken_ref` hard error identifying the cross-project reference violation.
4. WHEN verifying a single project via a project scope filter, THE Graph_Builder SHALL still resolve refs against the global Document_Index for non-isolated projects.


### Requirement 5: Task Dependency Cycle Detection

**User Story:** As a spec author, I want the graph to detect cycles in `Task` `depends` chains within each tasks document, so that circular dependencies are caught as hard errors before topological sorting.

#### Acceptance Criteria

1. WHEN a tasks document contains `Task` components with `depends` attributes, THE Graph_Builder SHALL construct a directed graph of task dependencies within that document.
2. WHEN the task dependency graph within a document is acyclic, THE Graph_Builder SHALL report no cycle errors for that document.
3. IF the task dependency graph within a document contains a cycle, THEN THE Graph_Builder SHALL return a `dependency_cycle` hard error identifying the cycle participants. Self-references (a task listing itself in `depends`) are treated as trivial cycles.
4. WHEN a `Task` component's `depends` attribute references a task ID that does not exist as a sibling (within the same parent for nested tasks, or within the document for top-level tasks), THE Graph_Builder SHALL return a `broken_ref` hard error for the unresolved depends reference.
5. WHEN `Task` components are nested, THE Graph_Builder SHALL scope `depends` resolution to sibling tasks within the same parent.

### Requirement 6: Document Dependency Cycle Detection

**User Story:** As a spec author, I want the graph to detect cycles in `DependsOn` ref chains between documents, so that circular document-level dependencies are caught as hard errors.

#### Acceptance Criteria

1. WHEN documents contain `DependsOn` components, THE Graph_Builder SHALL construct a directed graph of document-level dependencies from the resolved refs.
2. WHEN the document dependency graph is acyclic, THE Graph_Builder SHALL report no cycle errors.
3. IF the document dependency graph contains a cycle, THEN THE Graph_Builder SHALL return a `dependency_cycle` hard error identifying the cycle participants by their document IDs. Self-references (a document listing itself in `DependsOn`) are treated as trivial cycles.

### Requirement 7: Topological Sort

**User Story:** As a query consumer, I want tasks and document dependencies computed in topological order, so that the `context` and `plan` commands can present work items in implementation sequence.

#### Acceptance Criteria

1. WHEN the task dependency graph within a document is a valid DAG, THE Graph_Builder SHALL compute a Topological_Order of the tasks.
2. WHEN the document dependency graph is a valid DAG, THE Graph_Builder SHALL compute a Topological_Order of the documents.
3. THE Topological_Order SHALL guarantee that for every dependency edge (A depends on B), B appears before A in the ordering.
4. WHEN multiple valid topological orderings exist, THE Graph_Builder SHALL produce a deterministic ordering (stable across identical inputs). The tiebreaker for nodes with no ordering constraint between them SHALL be declaration order in the source document (for tasks) or alphabetical by document ID (for documents).


### Requirement 8: Reverse Mappings

**User Story:** As a query consumer, I want to know which documents validate, implement, or illustrate a given criterion or document, so that the `context` and `plan` commands can display incoming relationships without requiring bidirectional refs in source documents.

#### Acceptance Criteria

1. WHEN documents contain `Validates` components with resolved refs, THE Graph_Builder SHALL compute a reverse mapping from each target criterion (document_id, fragment) to the set of validating document IDs.
2. WHEN documents contain `Implements` components with resolved refs, THE Graph_Builder SHALL compute a reverse mapping from each target document ID to the set of implementing document IDs. Fragment portions of `Implements` refs are resolved for validation (Requirement 3) but discarded in the reverse mapping — Implements mappings are always document-level, consistent with the design principle that Implements provides traceability without requiring criterion-level coverage.
3. WHEN documents contain `Illustrates` components with resolved refs, THE Graph_Builder SHALL compute a reverse mapping from each target criterion or document to the set of illustrating document IDs.
4. THE Reverse_Mapping SHALL be queryable by target document ID or by (document_id, fragment) pair.
5. WHEN no documents reference a given target, THE Reverse_Mapping SHALL return an empty set for that target.
6. WHEN a `refs` attribute contains duplicate ref strings (e.g., the same ref listed twice), THE Graph_Builder SHALL deduplicate them — each unique ref contributes once to the reverse mapping.

### Requirement 9: Context Query

**User Story:** As an AI agent or developer, I want to retrieve a structured view of a document and all its relationships, so that I have complete context for implementing or reviewing a feature.

#### Acceptance Criteria

1. WHEN a valid document ID is provided, THE Graph_Builder SHALL produce a Context_Output containing the target document's frontmatter, criteria, and body content.
2. THE Context_Output SHALL include the Reverse_Mapping of which documents validate each criterion in the target document, along with each validating document's status.
3. THE Context_Output SHALL include the Reverse_Mapping of which documents implement the target document.
4. THE Context_Output SHALL include the Reverse_Mapping of which documents illustrate the target document or its criteria.
5. WHEN tasks documents contain `Task` components whose `implements` attribute references criteria in the target document, THE Context_Output SHALL include those tasks in Topological_Order.
6. IF the provided document ID does not exist in the Document_Index, THEN THE Graph_Builder SHALL return an error indicating the document was not found.
7. THE Context_Output SHALL be a structured data type, not a formatted string.


### Requirement 10: Plan Query

**User Story:** As an AI agent or developer, I want to retrieve a structured view of outstanding work for a requirement or feature, so that I know what criteria are uncovered, what tasks are pending, and what work is complete.

#### Acceptance Criteria

1. WHEN a valid document ID is provided, THE Graph_Builder SHALL produce a Plan_Output containing outstanding criteria (those with no validating document in the Reverse_Mapping AND no completed Task — status `done` — that implements them via resolved `implements` refs).
2. THE Plan_Output SHALL include pending tasks (status not `done`) from Linked_Tasks_Documents, presented in Topological_Order.
3. THE Plan_Output SHALL include completed tasks (status `done`) with the set of criteria they implement (from resolved `implements` refs).
4. THE Plan_Output SHALL include illustrating documents from the Reverse_Mapping.
5. WHEN a feature prefix string is provided (e.g., `auth/`), THE Graph_Builder SHALL produce a Plan_Output aggregating all documents whose IDs start with that prefix. Outstanding criteria SHALL be grouped by source requirement document. Tasks SHALL be listed per tasks document in Topological_Order. Illustrating documents SHALL be attributed to the criteria or documents they reference.
6. WHEN no argument is provided, THE Graph_Builder SHALL produce a project-wide Plan_Output covering all documents.
7. IF the provided document ID does not exist in the Document_Index and the string does not match any document ID prefix, THEN THE Graph_Builder SHALL return an error indicating no matching documents were found.
8. THE Plan_Output SHALL be a structured data type, not a formatted string.

### Requirement 11: Task Implements Resolution

**User Story:** As a query consumer, I want `Task` `implements` attributes resolved to their target criteria, so that the plan and context queries can associate tasks with the criteria they address.

#### Acceptance Criteria

1. WHEN a `Task` component has an `implements` attribute, THE Graph_Builder SHALL split the attribute value and resolve each ref to a target criterion using the same resolution logic as Requirement 3. Each ref MUST include a `#fragment` targeting a `Criterion` component; a ref without a fragment is a `broken_ref` hard error.
2. IF a `Task` `implements` ref does not resolve to an existing criterion, THEN THE Graph_Builder SHALL return a `broken_ref` hard error.
3. THE Graph_Builder SHALL make resolved task-to-criterion mappings available for Context_Output and Plan_Output generation.

### Requirement 12: TrackedFiles Indexing

**User Story:** As a verification engine consumer, I want all `TrackedFiles` components indexed by their owning document, so that downstream consumers (e.g., the `affected` command and `stale_tracked_files` rule in Spec 3) can match changed files against spec documents without re-parsing.

#### Acceptance Criteria

1. WHEN a `SpecDocument` contains `TrackedFiles` components, THE Graph_Builder SHALL index each component's `paths` attribute values by the owning document ID in the TrackedFiles_Index.
2. THE TrackedFiles_Index SHALL support lookup by document ID, returning all path globs declared in that document's `TrackedFiles` components.
3. THE TrackedFiles_Index SHALL support iteration over all entries, enabling downstream consumers to match a set of changed file paths against all tracked globs across all documents.
4. WHEN a document contains multiple `TrackedFiles` components, THE Graph_Builder SHALL aggregate all path globs from all such components under the same document ID.

### Requirement 13: Graph Construction Error Aggregation

**User Story:** As a verification engine consumer, I want all graph construction errors collected and returned together, so that a single build pass reports all problems rather than stopping at the first error.

#### Acceptance Criteria

1. WHEN multiple errors occur during graph construction (duplicate IDs, duplicate component IDs, broken refs, dependency cycles), THE Graph_Builder SHALL collect all errors and return them as a single collection.
2. THE Graph_Builder SHALL continue processing independent checks even after encountering errors in earlier checks, collecting as many errors as possible in a single pass.
3. WHEN no errors occur during graph construction, THE Graph_Builder SHALL return the completed `DocumentGraph` with all indexes, reverse mappings, topological orderings, and TrackedFiles_Index populated.
