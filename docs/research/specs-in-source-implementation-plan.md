# Specs in Source: Implementation Plan

*April 2026*

This plan turns the source-embedded specs research into an implementation
roadmap. It assumes the decision from
[specs-in-source-decision.md](specs-in-source-decision.md): Markdown documents
remain authoritative, and source comments enrich those documents with
verifiable implementation metadata.

The goal is not to move all specs into source. The goal is to make source files
contribute precise, local graph atoms while Markdown keeps requirements,
architecture, task planning, and prose-heavy rationale.

Only the first implementation arc is binding. Later phases are ordered roadmap
investments that should be re-evaluated after the first arc proves or disproves
the model.

## Guiding Decisions

- **Markdown stays authoritative.** User-facing requirements, design intent,
  tasks, ADRs, and review gates stay in Markdown. Source directives may add
  implementation-close metadata, but source is not the primary place to approve
  product requirements.
- **Source criteria are implementation-level.** A later `criterion` directive
  relaxes specify-before-implement only for source-local implementation
  details. Requirement criteria remain Markdown-first.
- **Structured comments provide the syntax.** The directive shape is
  `//supersigil:<directive> ...` or the host language's equivalent comment
  carrier. The grammar is shared; comment extraction and code association are
  language-specific where the host syntax requires it.
- **The graph remains the artifact.** The broad `build_graph(Vec<SpecDocument>)`
  shape can stay, but the meaning of a `SpecDocument` changes once it can be a
  merge of Markdown and source-derived components. Component provenance,
  source-span diagnostics, and criterion-level indexes are real model changes.
- **Ownership uses a hybrid model.** `tracked-by <doc>` establishes the
  file-local default owner for later enrichment directives in that file.
  Directives that naturally name an owner, such as
  `criterion <doc#id> "description"`, use that explicit owner. Directives that
  target another document, such as `implements <doc#criterion>`, inherit the
  file owner unless they use an explicit owner override chosen in the Phase 0
  ADR. Diagnostics should show whether ownership was inherited or explicit so
  refactors do not silently hide ownership changes.
- **Legacy evidence mechanisms are temporary migration scaffolding.** Rust
  `#[verifies]` and Vitest `verifies()` can run in dual mode while Supersigil
  migrates itself. Because Supersigil is its own only user today, the plan does
  not promise indefinite compatibility.
- **Discoverability is part of correctness.** Source comments are weaker than
  compile-time Rust proc-macro validation until the LSP provides diagnostics,
  completion, and navigation. Source `verifies` must remain experimental or
  gated until that safety exists.
- **Every implementation phase starts with tests.** Parser fixtures, source
  extraction fixtures, graph/enrichment tests, LSP diagnostics tests, and
  verification snapshots should be written before the implementation they
  constrain.
- **Full source documents are a parking lot.** Source-defined documents,
  transitive coverage, and source documents as independent graph nodes require a
  separate ADR after enrichment is proven.

## Current Code Seams

The current architecture has three important seams:

- Markdown files are discovered and parsed by the CLI loader into
  `SpecDocument` values before `build_graph`.
- `DocumentGraph` consumes `Vec<SpecDocument>` and is largely input-source
  agnostic.
- `EcosystemPlugin` runs after the graph exists and currently emits evidence
  records only. It is the right seam for `verifies` evidence, but not for
  source-derived `Criterion`, `Implements`, or `TrackedFiles` components.

That means `//supersigil:verifies` can be introduced through the existing
evidence path, while `implements`, `criterion`, and `tracked-by` need a
pre-graph enrichment phase used by every graph-building surface.

## Binding Scope: First Implementation Arc

The first arc proves the universal comment mechanism without committing to
pre-graph source enrichment:

1. Phase 0: source enrichment ADR and specs.
2. Phase 1: directive parser and shared model.
3. Phase 2: source discovery and project-scoping rules.
4. Phase 3: Rust and JS/TS comment extraction and code association.
5. Phase 4: experimental `//supersigil:verifies` evidence in dual mode.
6. Phase 5a: in-buffer LSP safety for directive syntax and refs.
7. Phase 5b: workspace LSP parity for the same discovery and extraction
   semantics as the CLI.

This arc stops before source-derived `Criterion`, `Implements`, or
`TrackedFiles` components mutate the graph input.

## Phase 0: Source Enrichment ADR and Spec Baseline

**Purpose:** Capture the model before implementation.

Write Supersigil docs for the feature:

- ADR: Markdown-authoritative source enrichment.
- Requirements: directive parsing, source discovery, provenance, ownership,
  graph enrichment, evidence integration, project scoping, and editor safety.
- Design: loader, parser, evidence, graph, LSP, and migration boundaries.
- Tasks: dependency-ordered implementation tasks for the first buildable arc.

Required decisions:

- The exact syntax for explicit owner overrides under the hybrid ownership
  model.
- How source-derived components record file/span provenance separately from the
  owning Markdown document path.
- How source-derived components are ordered inside the merged document.
- How duplicate IDs are reported.
- How duplicate relationship declarations are deduplicated, merged, or
  reported.
- How old and new `verifies` evidence deduplicates in dual mode:
  - Same test and same criterion from legacy plus source directive: one logical
    evidence record with merged provenance.
  - Same test and different criteria from legacy plus source directive: both
    explicit claims are kept unless a later strict-migration mode chooses to
    warn.
  - Broken source directive refs: LSP diagnostic and `supersigil verify`
    finding; no compile-time guarantee is implied.
- How CLI commands and the LSP use the same enrichment input set and semantics,
  even though the CLI is batch-oriented and the LSP is incremental.
- How project scoping works when source files match multiple project globs,
  source directives target documents in another project, or generated/ignored
  files contain directives.
- Whether criterion-level `implements` is supported in the first enrichment
  pass or deferred until query/index support lands.

Exit criteria:

- New docs are draft or review-ready.
- `supersigil verify` is clean enough for honest iteration.
- The first implementation arc is scoped to a small, testable slice.

## Phase 1: Directive Grammar and Model

**Purpose:** Define the language-independent directive contract without AST or
graph integration.

Build a shared directive model, likely in a new internal crate such as
`supersigil-directives`.

Deliverables:

- Parser for directive text after the comment marker.
- Directive records for:
  - `verifies <doc#criterion>`
  - `tracked-by <doc>`
  - `implements <doc>` and `implements <doc#criterion>`
  - later `criterion <doc#id> "description"`
- Directive metadata for inherited owner, explicit owner, target refs, and
  source provenance.
- Argument spans for diagnostics and LSP targeting.
- Escaping and quote handling for directive text.
- Validation helpers for malformed refs, missing arguments, unknown directives,
  and unsupported directive shapes.
- A normalized in-memory model that legacy Rust and JS evidence extractors can
  route through, so downstream deduplication sees one evidence shape.

Exit criteria:

- Unit tests cover valid directives, malformed directives, quoting, spacing,
  span calculation, owner metadata, and unknown directive diagnostics.
- No tree-sitter, evidence, graph, or LSP integration is required yet.

## Phase 2: Source Discovery and Project Scoping

**Purpose:** Make source enrichment scan a deterministic, configured file set.

This must exist before `tracked-by` can work, because `tracked-by` cannot
bootstrap discovery of files that were never scanned.

Deliverables:

- Config surface for source directive discovery, including include globs,
  excludes, language selection, ignore behavior, and project scoping.
- Deterministic source file ordering.
- Shared discovery code usable by CLI loaders and the LSP.
- Explicit behavior for files matched by multiple projects.
- Explicit behavior for a directive in one project targeting a document owned
  by another project.
- Explicit behavior for generated files, ignored files, project `paths`,
  project `tests`, and `[test_discovery]` ignores.
- Feature flag or config gate while the feature is experimental.

Exit criteria:

- CLI and LSP can resolve the same source-discovery input set.
- Multi-project behavior is documented and tested.
- Ignored and generated files do not silently enter the graph.

## Phase 3: Comment Extraction and Code Association

**Purpose:** Extract structured comments and associate them with useful host
language context.

Split this into two layers:

- Comment extraction: tree-sitter finds comment nodes and ignores strings.
- Semantic association: host-language code determines what the directive is
  attached to.

Initial language scope:

- Rust.
- JavaScript / TypeScript.

Deliverables:

- Tree-sitter-backed comment extraction for the initial languages.
- Directive-to-code association for nearby functions, modules, declarations,
  and test calls.
- Concrete association rules for Rust attributes and doc comments, including
  cases such as a directive before `#[test]`, `#[tokio::test]`, or stacked
  attributes.
- Concrete association rules for JS/TS comments, `describe` nesting, exported
  functions, and TSX/JSX comment carriers.
- Parse-failure diagnostics when tree-sitter cannot parse a scanned file well
  enough to trust associations. Files should not be skipped silently.
- Floating-directive diagnostics when a directive cannot be attached to a
  supported code node.
- Source ranges for the directive marker, directive name, and arguments.
- Rust test identity extraction for `#[test]`, `#[tokio::test]`, proptest
  forms, and other supported shapes already recognized by the Rust plugin.
- JS/TS test identity extraction that preserves `describe` nesting and existing
  Vitest/Jest naming behavior.

Exit criteria:

- Fixture tests prove directives are extracted only from comments.
- Fixture tests cover parse failures and floating directives.
- Directive records include enough context to produce useful evidence,
  diagnostics, and navigation.
- Existing `#[verifies]` and Vitest `verifies()` extraction can be normalized
  into the same downstream evidence model.

## Phase 4: Experimental Source `verifies` Evidence

**Purpose:** Introduce the lowest-risk directive through the existing
post-graph evidence pipeline without making comments the default path before
LSP safety exists.

`//supersigil:verifies` links tests to existing criteria. It does not mutate
documents, so it can be implemented before pre-graph enrichment. Until Phase 5
lands, it should be off by default, internal-only, or guarded by an
experimental config flag.

Deliverables:

- Convert `verifies` directives into `VerificationEvidenceRecord` values.
- Add source-directive evidence kind and provenance.
- Normalize legacy Rust and JS evidence into the same in-memory evidence shape
  where practical.
- Implement the Phase 0 dual-mode deduplication rules.
- Report unresolved source-directive refs with source locations.
- Update JSON/Markdown reporting, export surfaces, explorer data, and any
  evidence-kind displays.
- Keep Rust proc macro and Vitest helper support only as migration scaffolding.

Exit criteria:

- `supersigil verify` accepts gated `//supersigil:verifies` evidence.
- Mixed old/new annotations are deterministic.
- A small crate or package can dogfood the new directive without losing
  coverage.
- The directive is not the default migration recommendation until Phase 5a
  exists.

## Phase 5: LSP Safety

**Purpose:** Restore the safety lost when moving from Rust compile-time
validation to comments.

### Phase 5a: In-Buffer Directive Safety

Deliverables:

- Diagnostics for malformed source directives in the open buffer.
- Diagnostics for broken directive refs when the target document set is
  available.
- Completion for document and criterion refs inside directive comments.
- Go-to-definition from directive refs to Markdown spec targets.

Exit criteria:

- A developer editing source comments gets immediate feedback on broken refs in
  normal editor usage.
- Source `verifies` can become the preferred internal migration path only after
  this phase lands.

### Phase 5b: Workspace LSP Parity

Deliverables:

- LSP workspace indexing that uses the same source discovery and extraction
  semantics as the CLI.
- Incremental invalidation that keeps the LSP's input set equivalent to the
  CLI's batch input set.
- Agreement tests or fixtures comparing CLI and LSP diagnostics for the same
  source files.

Exit criteria:

- CLI and LSP diagnostics agree for the same file set.
- Source directives feel like first-class metadata, not unchecked comments.

## First Arc Exit Criteria

- The source directive grammar is stable enough for internal use.
- Rust and JS/TS `verifies` comments work behind the chosen gate.
- LSP safety exists before broad internal migration.
- The old Rust macro and Vitest helper can either remain temporarily or be
  removed after internal migration.
- No pre-graph enrichment feature is started until Phase 0 ownership and
  project-scoping decisions are recorded.

## Roadmap Investments

The following phases are ordered, but not committed. Each should be re-scoped
after the first arc.

## Phase 6: Pre-Graph Enrichment Plumbing

**Purpose:** Add the loader-wide phase required for source-derived components.

This is the key model change. It must run anywhere a graph is built, not only
inside `verify`.

Affected surfaces:

- `verify`
- `ls`
- `context`
- `plan`
- `status`
- `affected`
- `graph`
- `export`
- `explore`
- LSP indexing, diagnostics, and rebuilds

Deliverables:

- Shared enrichment pipeline after Markdown parsing and before `build_graph`.
- Component-level provenance so source-derived components point to source files
  instead of the owning Markdown document path.
- Merge rules for source-derived components.
- Deterministic ordering across Markdown and source-derived components.
- Config/project scoping applied consistently.
- Diagnostics converted into parse, graph, or verification findings using the
  same severity model as comparable Markdown mistakes.

Exit criteria:

- Every graph-building command sees the same enriched document inputs.
- The LSP uses the same enrichment input set and semantics through its
  incremental path.
- Graph errors from source-derived components point to source files.
- Existing Markdown-only projects behave unchanged when source directives are
  disabled.

## Phase 7: Enrichment V1: `tracked-by` and Document-Level `implements`

**Purpose:** Start graph enrichment with low-risk structural metadata.

Start with:

- `tracked-by <doc>`
- document-level `implements <doc>`

Defer criterion-level `implements <doc#criterion>` unless the graph/query
indexes are extended at the same time.

Deliverables:

- `tracked-by` contributes tracked-file relationships for the owning document.
- Document-level `implements` contributes ordinary `Implements` edges.
- The hybrid ownership model is visible in diagnostics and reports.
- Duplicate relationship declarations are deduplicated or reported according to
  Phase 0 merge rules.
- Duplicate IDs remain separate from duplicate relationship handling.

Exit criteria:

- `affected` can see source-declared tracked-file relationships.
- `context`, `graph`, and explorer views surface source-derived edges.
- Design docs can begin shedding some `TrackedFiles` and coarse `Implements`
  declarations without losing graph behavior.

## Phase 8: Criterion-Level Implements Indexing

**Purpose:** Make `implements <doc#criterion>` meaningful across graph queries.

Current graph validation can resolve fragments, but some reverse mappings and
query outputs are document-level. Criterion-level implementation directives
need richer surfacing before migration.

Deliverables:

- Preserve and expose criterion-level `Implements` relationships.
- Reverse indexes for "where is this criterion implemented?"
- Query output showing source locations for implementing code.
- Context output showing source locations for implementing code.
- Explorer/graph data support where useful.
- Clear behavior for requirement coverage: direct evidence remains the default;
  transitive coverage remains deferred unless separately designed.

Exit criteria:

- `implements <doc#criterion>` is visible as criterion-level traceability, not
  only as a document-level edge.
- No coverage semantics change accidentally.

## Phase 9: Inline Criteria

**Purpose:** Allow source to contribute implementation-level criteria to an
existing Markdown document.

Directive:

```text
supersigil:criterion <doc#id> "description"
```

Deliverables:

- Source-derived `Criterion` components.
- Duplicate ID detection across Markdown and source.
- Deterministic component ordering.
- A source-aware sequential-ID rule that avoids noisy findings for intentional
  source-local criteria while preserving useful ordering checks in Markdown.
- Coverage checks and `plan` output treating inline criteria like ordinary
  criteria.
- Flat criteria only in the first pass; nested `VerifiedBy` or rich prose
  remains deferred.

Exit criteria:

- Inline criteria appear in `context`, `plan`, `status`, `graph`, `export`,
  and rendered/explorer views.
- Coverage and broken-ref checks behave the same as for Markdown criteria.
- Long-form criteria still clearly belong in Markdown.

## Phase 10: Convention-Based Evidence Mapping

**Purpose:** Use inline/source-local criteria as anchors for inferred test
coverage before broad dogfooding depends on manual annotations alone.

Deliverables:

- Config-gated convention matching.
- Module-scoped matching using source directive locations.
- Confidence scoring.
- Distinct inferred evidence kind and provenance.
- Clear report output showing inferred vs explicit evidence.

Exit criteria:

- Explicit evidence remains the strongest signal.
- Inferred evidence is visible, auditable, and easy to disable.
- False positives are limited by module scope and confidence thresholds.

## Phase 11: Dogfooding and Migration

**Purpose:** Prove the model in Supersigil itself before recommending it.

Recommended order:

1. Dogfood `supersigil-directives` itself or another small Rust crate in dual
   mode.
2. Migrate selected `supersigil-rust` evidence to source directives.
3. Migrate one JS/TS package in dual mode.
4. Move selected source-close `tracked-by` and `implements` links out of
   Markdown design docs.
5. Add inline criteria only for implementation-close surfaces.
6. Remove the Rust macro and Vitest helper after internal migration if no
   public-API decision keeps them.

Good candidates:

- `rust-plugin`
- `js-plugin`
- `ref-discovery`
- selected `ecosystem-plugins` implementation slices

Defer:

- `workspace-projects` until multi-project source discovery is proven.
- ADRs and task documents indefinitely.
- Cross-cutting requirements that do not have one clear code anchor.

Exit criteria:

- Verify output is equivalent or more useful after migration.
- Agent workflow is measurably simpler.
- No legacy annotation mechanism remains merely for unexamined compatibility.

## Phase 12: Lean Design Docs and Skill Updates

**Purpose:** Use source enrichment to reduce design-doc drift.

Deliverables:

- Update design docs to keep architecture and rationale while moving
  implementation-close metadata near source.
- Update authoring guidance so agents know when metadata belongs in Markdown
  and when it belongs in source.
- Update `ss-feature-specification` and `ss-feature-development` skills.
- Add examples showing the four-tier model:
  - requirements in Markdown
  - lean design in Markdown
  - implementation criteria in source
  - evidence in tests

Exit criteria:

- Design docs are shorter and less likely to duplicate implementation details.
- Agent skills use source directives without skipping Markdown review gates.

## Phase 13: Rich LSP Discoverability

**Purpose:** Make source directives comfortable to navigate at project scale.

Deliverables:

- Hover showing criterion text, status, coverage, and owning document.
- Code lens for source directives where editor support allows.
- Workspace symbol support for documents, criteria, and source directives.
- Optional inlay hints or file decorations after hover/code lens are proven.
- Source-aware find references and rename behavior where feasible.
- IntelliJ support through the existing `supersigil-lsp` shell where possible;
  native plugin work is limited to file-type activation, capability gaps, or
  platform-specific UX that LSP cannot cover.

Exit criteria:

- Source directives are discoverable from both local code context and global
  project search.
- VS Code, Neovim, and IntelliJ use the shared LSP semantics where the editor
  allows it.

## Phase 14: Broader Ecosystem Support

**Purpose:** Extend structured source directives beyond Rust and JS/TS.

Likely order:

1. Python.
2. Go.
3. Java/Kotlin.
4. C/C++.
5. JUnit XML ingestion as a complement, not the primary source-discovery
   mechanism.

Exit criteria:

- New languages reuse the same directive parser.
- Language work mostly adds tree-sitter grammar support and semantic
  association rules.
- The directive syntax remains universal.

## Parking Lot: Optional Full Source Documents

**Purpose:** Explore source-defined documents only after enrichment is proven.

This track requires a separate ADR because it reopens the identity and
source-of-truth problems that enrichment avoids.

Possible scope:

- `document` directive for source-defined implementation documents.
- Explicit document IDs; path-derived IDs only as defaults.
- Optional transitive coverage model.
- Source docs as separate graph nodes rather than enrichment of Markdown docs.

Exit criteria:

- There is a proven use case that enrichment cannot handle.
- The feature remains opt-in and does not weaken Markdown-authoritative
  workflows.

## Phase 15: Productization

**Purpose:** Make source directives usable by people outside Supersigil after
internal dogfooding proves the model.

Deliverables:

- Website guide for source directives.
- Migration guide from `#[verifies]` and Vitest `verifies()` if those
  mechanisms still exist at that point.
- CI examples and GitHub Action guidance.
- Troubleshooting docs for broken refs, source discovery, project scoping, and
  LSP setup.
- Regulated-team examples showing implementation-to-requirement tracing.
- Comparison pages for Kiro, Spec Kit, Allure, and traditional requirements
  tools.

Exit criteria:

- A new project can adopt source directives from documentation alone.
- Existing projects can migrate incrementally.
- The feature strengthens Supersigil's "verified specs as code" positioning.

## Core Risks

- **Provenance risk:** source-derived components need source file/span data or
  diagnostics will point to the wrong file.
- **Ownership risk:** directives such as `implements` and `criterion` need an
  unambiguous owning document, and inherited ownership must be visible during
  refactors.
- **Project-scoping risk:** overlapping project globs can make one directive
  appear to belong to multiple projects unless the discovery model is explicit.
- **Discovery risk:** source enrichment needs configured source scanning before
  `tracked-by` can be useful.
- **Enrichment parity risk:** CLI commands and the LSP must use the same input
  set and semantics even though their execution models differ.
- **Graph semantics risk:** criterion-level `implements` needs query/index
  support before it is useful.
- **Migration risk:** comments without LSP safety would be weaker than existing
  Rust proc-macro validation, so source `verifies` must stay gated until LSP
  safety lands.
- **Parser risk:** tree-sitter parse failures and host-language edge cases must
  produce visible diagnostics instead of silently dropping directives.

## Non-Goals for the First Arc

- Full source-defined documents.
- Transitive requirement coverage.
- Maintaining legacy `#[verifies]` or Vitest `verifies()` indefinitely.
- Moving ADRs, tasks, or prose-heavy requirements into source.
- Supporting every language at once.
