---
supersigil:
  id: decision-components/design
  type: design
  status: approved
title: "Decision Components"
---

```supersigil-xml
<Implements refs="decision-components/req" />
<TrackedFiles paths="crates/supersigil-core/src/component_defs.rs, crates/supersigil-core/src/graph.rs, crates/supersigil-core/src/graph/reverse.rs, crates/supersigil-core/src/graph/query.rs, crates/supersigil-verify/src/rules/structural.rs, crates/supersigil-verify/src/rules/decision.rs, crates/supersigil-verify/src/report.rs, crates/supersigil-verify/src/lib.rs, crates/supersigil-verify/src/affected.rs, crates/supersigil-core/src/config.rs, crates/supersigil-cli/src/commands/new.rs, crates/supersigil-cli/src/commands.rs" />
```

## Overview

This design adds three built-in components (`Decision`, `Rationale`,
`Alternative`), a new `adr` document type, six verification rules, and
integrations with the `context` and `affected` commands.

The implementation touches four crates:

- **supersigil-core** — component definitions, graph constants, component
  indexing, reverse mappings, context query, tracked-files indexing.
- **supersigil-verify** — six new rules (three structural, three
  decision-quality), rule registration, verify pipeline orchestration.
- **supersigil-cli** — `new` scaffold template for `adr`, built-in doc type
  registration, affected transitive staleness.
- **supersigil-core config** — `KNOWN_RULES` additions.

## Component Definitions

### `Decision`

```rust
// In ComponentDefs::defaults()
defs.insert("Decision".into(), ComponentDef {
    attributes: HashMap::from([
        ("id".into(), AttributeDef { required: true, list: false }),
        ("standalone".into(), AttributeDef { required: false, list: false }),
    ]),
    referenceable: true,
    verifiable: false,
    target_component: None,
    description: Some("A recorded architectural choice...".into()),
    examples: vec![...],
});
```

- Referenceable with a required `id`. Participates in the component index
  at `(doc_id, decision_id)` and can be targeted with fragment syntax
  (`doc-id#decision-id`).
- Not verifiable — decisions are traceability nodes, not coverage targets.
- Optional `standalone` attribute: a non-empty reason string declaring that
  the decision is intentionally unconnected. When present, the
  `orphan_decision` rule skips this decision.
- Children: supports nested `References`, `TrackedFiles`, `DependsOn`,
  `Rationale`, and `Alternative`. The parser already extracts nested
  components recursively, so no parser changes are needed for nesting.

### `Rationale`

```rust
defs.insert("Rationale".into(), ComponentDef {
    attributes: HashMap::new(),
    referenceable: false,
    verifiable: false,
    target_component: None,
    description: Some("Justification for a Decision...".into()),
    examples: vec![...],
});
```

- No attributes. Body text is the content.
- Not referenceable, not verifiable. Serves as a semantic wrapper (same
  pattern as `AcceptanceCriteria`).

### `Alternative`

```rust
defs.insert("Alternative".into(), ComponentDef {
    attributes: HashMap::from([
        ("id".into(), AttributeDef { required: true, list: false }),
        ("status".into(), AttributeDef { required: true, list: false }),
    ]),
    referenceable: true,
    verifiable: false,
    target_component: None,
    description: Some("A considered option...".into()),
    examples: vec![...],
});
```

- Referenceable with a required `id`. Indexed at
  `(doc_id, alternative_id)`.
- Required `status` attribute: `rejected`, `deferred`, or `superseded`.
  Status values are not validated by the component definition system (it
  only checks presence, not value sets). Validation of recognized values
  happens via the `invalid_alternative_status` rule.
- Zero or more `Alternative` components are permitted per `Decision`.
  There is no upper cardinality limit.

### Constants

Add to `crates/supersigil-core/src/graph.rs`:

```rust
pub(crate) const DECISION: &str = "Decision";
pub(crate) const RATIONALE: &str = "Rationale";
pub(crate) const ALTERNATIVE: &str = "Alternative";
```

## Graph Integration

### Component Indexing

No changes needed. `build_component_index` already recursively indexes all
referenceable components. `Decision` and `Alternative` will be indexed
automatically because they have `referenceable: true` and a required `id`.

### Reference Resolution

No changes needed. The resolve pipeline already iterates all extracted
components recursively and resolves `refs` attributes on any component
that has them. `References`, `TrackedFiles`, and `DependsOn` nested
inside `Decision` will be resolved by the existing code.

### Reverse Mappings

No changes needed in `build_reverse_mappings`. The function dispatches on
component names (`References`, `Implements`, `DependsOn`) regardless of
nesting depth. A `<References>` nested inside a `<Decision>` produces the
same reverse-mapping entries as a top-level `<References>`.

### Tracked-Files Indexing

Verify that `build_tracked_files_index` walks nested components
recursively. If it does (matching the pattern of `build_component_index`),
then `<TrackedFiles>` inside a `<Decision>` is indexed for staleness
detection without changes. If it only walks top-level components, extend
it with the same recursive walk pattern.

## Context Output

### Decisions in the Target Document (req-6-1)

Extend `ContextOutput` with a new field:

```rust
pub struct ContextOutput {
    pub document: SpecDocument,
    pub criteria: Vec<TargetContext>,
    pub decisions: Vec<DecisionContext>,  // NEW
    pub implemented_by: Vec<DocRef>,
    pub referenced_by: Vec<String>,
    pub tasks: Vec<TaskInfo>,
}

pub struct DecisionContext {
    pub id: String,
    pub body_text: Option<String>,
    pub rationale_text: Option<String>,
    pub alternatives: Vec<AlternativeContext>,
}

pub struct AlternativeContext {
    pub id: String,
    pub status: String,
    pub body_text: Option<String>,
}
```

In `build_context`, add an `extract_decisions` function that walks the
document's component tree and collects `Decision` components with their
nested `Rationale` and `Alternative` children.

### Decisions Referencing the Target Document (req-6-2)

When building context for document X, find decisions in other documents
that reference X. Use the existing `references_reverse` index to find
source documents, then scan those documents for `Decision` components
whose nested `References` target X.

This is a context-output concern — no new reverse index needed. The scan
is bounded by the set of referencing documents from the existing reverse
map.

Add a `linked_decisions: Vec<LinkedDecision>` field to `ContextOutput`:

```rust
pub struct LinkedDecision {
    pub source_doc_id: String,
    pub decision_id: String,
    pub body_text: Option<String>,
}
```

### Terminal Formatting

```
# design: auth/design
Status: approved

## Decisions:
- unidirectional-refs: References point from concrete to abstract.
  Rationale: Bidirectional refs create synchronization burden.
  Alternatives: bidirectional (rejected), computed-both (rejected)

## Linked decisions (from other documents):
- arch/adr#mdx-choice: MDX for structured components in prose.

## Verification targets:
...
```

### JSON Serialization

`DecisionContext`, `AlternativeContext`, and `LinkedDecision` derive
`Serialize`. The JSON output of `supersigil context --format json`
includes the new fields additively — existing consumers that don't read
the new fields are unaffected.

## Verification Rules

### Structural Rules (4 new)

These follow the parent-constraint patterns established by
`check_expected_placement` and `check_verified_by_placement` in
`crates/supersigil-verify/src/rules/structural.rs`.

**`invalid_rationale_placement`** (default: warning)

Rationale placed outside a Decision. Uses the exact-parent-name pattern:

```rust
fn walk_for_rationale_placement(
    doc_id: &str,
    components: &[ExtractedComponent],
    parent_name: Option<&str>,
    findings: &mut Vec<Finding>,
) {
    for comp in components {
        if comp.name == RATIONALE && parent_name != Some(DECISION) {
            // emit InvalidRationalePlacement finding
        }
        walk_for_rationale_placement(
            doc_id, &comp.children, Some(&comp.name), findings,
        );
    }
}
```

**`invalid_alternative_placement`** (default: warning)

Same pattern: Alternative placed outside a Decision.

**`duplicate_rationale`** (default: warning)

For each Decision, count Rationale children. If count > 1, emit a finding
on each excess Rationale:

```rust
fn walk_for_duplicate_rationale(
    doc_id: &str,
    components: &[ExtractedComponent],
    findings: &mut Vec<Finding>,
) {
    for comp in components {
        if comp.name == DECISION {
            let rationales: Vec<_> = comp.children.iter()
                .filter(|c| c.name == RATIONALE)
                .collect();
            if rationales.len() > 1 {
                for excess in &rationales[1..] {
                    // emit DuplicateRationale finding
                }
            }
        }
        walk_for_duplicate_rationale(doc_id, &comp.children, findings);
    }
}
```

**`invalid_alternative_status`** (default: warning)

An Alternative with a `status` value that is not one of `rejected`,
`deferred`, or `superseded`. Walk Decision components, check each
Alternative child's status attribute:

```rust
const RECOGNIZED_ALTERNATIVE_STATUSES: &[&str] =
    &["rejected", "deferred", "superseded"];

fn check_alternative_status(docs: &[&SpecDocument]) -> Vec<Finding> {
    // For each Alternative inside a Decision:
    //   if status not in RECOGNIZED_ALTERNATIVE_STATUSES, emit finding
}
```

### Decision Quality Rules (3 new)

Create `crates/supersigil-verify/src/rules/decision.rs`.

**`incomplete_decision`** (default: warning)

Decision with no Rationale child:

```rust
pub fn check_incomplete(docs: &[&SpecDocument]) -> Vec<Finding> {
    // For each Decision component in each doc:
    //   if no child has name == RATIONALE, emit finding
}
```

**`orphan_decision`** (default: warning)

Decision with no outward connections and not referenced by anything:

```rust
pub fn check_orphan(
    docs: &[&SpecDocument],
    graph: &DocumentGraph,
) -> Vec<Finding> {
    // For each Decision in each doc:
    //   1. If Decision has a `standalone` attribute, skip it
    //   2. Check children for References, TrackedFiles, DependsOn
    //   3. Check graph references_reverse for (doc_id, Some(decision_id))
    //   4. If both empty, emit finding
}
```

**`missing_decision_coverage`** (default: off)

Design documents with no Decision anywhere referencing them:

```rust
pub fn check_coverage(
    docs: &[&SpecDocument],
    graph: &DocumentGraph,
) -> Vec<Finding> {
    // For each doc where doc_type == "design":
    //   1. Check the document itself for Decision components
    //      (a Decision embedded in the design doc counts as coverage)
    //   2. Collect source docs from references_reverse(doc_id, None)
    //      For each source doc, check if it contains Decision components
    //      whose nested References target this design doc
    //   3. If neither found, emit finding
}
```

### Rule Registration

**`crates/supersigil-verify/src/report.rs`** — Add to `RuleName` enum,
`RuleName::ALL`, `config_key()`, and `default_severity()`:

| Variant | Config key | Default |
|---------|-----------|---------|
| `InvalidRationalePlacement` | `invalid_rationale_placement` | Warning |
| `InvalidAlternativePlacement` | `invalid_alternative_placement` | Warning |
| `DuplicateRationale` | `duplicate_rationale` | Warning |
| `InvalidAlternativeStatus` | `invalid_alternative_status` | Warning |
| `IncompleteDecision` | `incomplete_decision` | Warning |
| `OrphanDecision` | `orphan_decision` | Warning |
| `MissingDecisionCoverage` | `missing_decision_coverage` | Off |

**`crates/supersigil-core/src/config.rs`** — Add all seven config keys to
`KNOWN_RULES`.

**`crates/supersigil-verify/src/lib.rs`** — Call the new check functions
in `verify_structural`, after existing structural checks:

```rust
findings.extend(rules::structural::check_rationale_placement(&docs));
findings.extend(rules::structural::check_alternative_placement(&docs));
findings.extend(rules::structural::check_duplicate_rationale(&docs));
findings.extend(rules::structural::check_alternative_status(&docs));
findings.extend(rules::decision::check_incomplete(&docs));
findings.extend(rules::decision::check_orphan(&docs, graph));
findings.extend(rules::decision::check_coverage(&docs, graph));
```

All seven rules participate in the existing 4-level severity precedence:
draft gating → per-rule override → global strictness → built-in default.

## Affected Integration

### Direct Staleness (req-6-3)

If `build_tracked_files_index` already walks nested components, this
works without changes — `TrackedFiles` inside a `Decision` is indexed
against the parent document, and `affected` checks against that index.

If not, extend the walk to match the recursive pattern of
`build_component_index`.

### Transitive Staleness (req-6-4)

Transitive staleness is a general `affected` enhancement specified in
`verification-engine/req#req-6-4`. The implementation lives in the
affected query, not in this feature's code. Decisions benefit from it
automatically: when a Decision's nested `References` targets an affected
document, the Decision's owning document is transitively flagged.

See the verification-engine design for the implementation approach. The
key addition is a `transitive_from: Option<String>` field on
`AffectedDocument` and one hop of reverse-reference expansion after the
direct affected set is computed.

## ADR Document Type

### Built-in Registration

Add `"adr"` to `BUILTIN_DOC_TYPES` in
`crates/supersigil-cli/src/commands.rs`:

```rust
pub const BUILTIN_DOC_TYPES: &[&str] =
    &["requirements", "design", "tasks", "adr"];
```

Add the type definition with statuses `draft`, `review`, `accepted`,
`superseded`.

### `supersigil new adr` Scaffold

Add an `"adr"` branch to `generate_template` in
`crates/supersigil-cli/src/commands/new.rs`:

```rust
"adr" => {
    let references_line = if req_exists {
        format!(r#"<References refs="{feature}/req" />"#)
    } else {
        String::new()
    };
    // Template with commented-out Decision/Rationale/Alternative
    // as guidance for authors
}
```

The ID convention: `{feature}/adr`, file name `{feature}.adr.mdx`.

## Testing Strategy

### Component Definitions

- Unit tests in `crates/supersigil-core/tests/component_defs_unit_tests.rs`
  verifying that `Decision`, `Rationale`, and `Alternative` are present in
  defaults with correct attribute schemas, referenceability, and
  verifiability.

### Graph Integration

- Property tests in `crates/supersigil-core/src/graph/tests/` verifying:
  - Decision and Alternative are indexed in the component index.
  - References nested inside Decision produce correct reverse mappings.
  - TrackedFiles nested inside Decision are indexed.
  - Fragment refs to `doc#decision-id` and `doc#alternative-id` resolve.

### Verification Rules

- Tests in a new `crates/supersigil-verify/src/rules/decision.rs` test
  module or adjacent test file covering:
  - `invalid_rationale_placement`: Rationale at root, inside non-Decision,
    inside Decision (no finding).
  - `invalid_alternative_placement`: same pattern.
  - `duplicate_rationale`: zero, one (no finding), two Rationale children.
  - `incomplete_decision`: Decision with Rationale (no finding), without.
  - `orphan_decision`: Decision with nested References (no finding),
    Decision referenced by another doc (no finding), isolated Decision.
  - `missing_decision_coverage`: design doc with referencing Decision (no
    finding), without. Off by default — test that it is suppressed unless
    configured.
  - Draft gating suppresses all six to info.

### Context Output

- Tests verifying:
  - `context` output includes decisions from the target document.
  - `context` output includes linked decisions from referencing documents.
  - JSON serialization round-trips the new fields.

### Affected Integration

- Tests verifying:
  - Nested TrackedFiles inside Decision produce affected entries.
  - Transitive staleness adds referencing documents.
  - Transitive entries carry `transitive_from`.
  - Direct and transitive entries are deduplicated.

### CLI Scaffolding

- Test that `supersigil new adr <feature>` produces a lint-clean document
  with `type: adr` and `status: draft`.
- Test that `adr` appears in the known types list.
