---
title: "Supersigil — Design Document"
version: "0.1.0"
status: draft
authors: []
---

# Supersigil

Supersigil is a CLI tool and verification framework for spec-driven software
development with AI agents. It provides an artifact layer — a structured,
verifiable graph of specifications, properties, and test mappings — stored as
MDX files that serve simultaneously as human documentation, agent context, and
machine-verifiable contracts.

## Motivation

Modern AI coding agents can plan and implement features, but they lack a
durable contract layer. Specifications drift from code. Properties described
in prose have no verified link to tests. Agents hallucinate references. And
existing spec-driven tools (like Amazon Kiro) hardcode folder structures,
use opaque formats, and lock you into a single IDE workflow.

Supersigil addresses this with three principles:

- **Everything-as-code.** Specs are MDX files in your repository. They render
  as documentation (Astro, Docusaurus, any MDX-aware site), function as agent
  context, and are verified by CI. No separate system of record.

- **Verifiable by default.** Cross-references between documents are typed and
  checked. Property-to-test mappings are discovered and reported. Staleness,
  orphans, and coverage gaps surface as warnings and errors.

- **Workflow-agnostic.** Supersigil does not prescribe an order of operations.
  Write requirements first, or design first, or start with a property because
  you know the invariant you care about. The tool tells you what's missing —
  it doesn't tell you what order to fill it in.

## Core Concepts

### Documents

A supersigil document is an MDX file with YAML front matter namespaced under
`supersigil:`. The front matter carries identity and metadata. The body
carries prose and structured MDX components.

```mdx
---
supersigil:
  id: auth/req/login
  type: requirement
  status: approved
title: "User Login"
sidebar:
  order: 1
---

# User Login

As a user, I want to log in with email and password so that I can
access my account.
```

The `supersigil:` namespace ensures coexistence with other tools. Astro,
Starlight, Docusaurus, or any other system can use their own front matter
keys. Supersigil reads only its namespace and ignores everything else.

### Components

MDX components are the semantic building blocks. They carry structured data
inside otherwise freeform prose. Supersigil's verification engine operates
on the component graph — not on document types.

The built-in components are:

| Component             | Purpose                                            | Key Attributes          |
|-----------------------|----------------------------------------------------|-------------------------|
| `<AcceptanceCriteria>`| Groups criteria within a document                  | —                       |
| `<Criterion>`         | A single testable acceptance criterion             | `id` (required)         |
| `<Validates>`         | Declares that this document validates criteria      | `refs` (required)       |
| `<VerifiedBy>`        | Maps this document to tests                        | `strategy`, `tag`, `paths` |
| `<Implements>`        | Declares that this document implements a spec       | `refs` (required)       |
| `<Illustrates>`       | Links examples to criteria without satisfying coverage | `refs` (required)    |
| `<Task>`              | An implementation task within a tasks document     | `id` (required), `status`, `implements`, `depends` |
| `<DependsOn>`         | Declares ordering dependencies between documents   | `refs` (required)       |
| `<TrackedFiles>`      | Declares which source files this spec is concerned with | `paths` (required)  |

Components are the unit of semantics. Document types are classification tags.
This distinction is fundamental: supersigil's verification logic never
branches on document type — it follows component relationships.

### The Component Graph

Documents relate to each other through component refs. The relationships
are unidirectional:

```
┌─────────────┐   <Validates>   ┌───────────────┐   <VerifiedBy>   ┌───────┐
│  Criterion   │◄───────────────│   Property     │────────────────►│ Tests  │
│  (in req)    │                │   document     │                 │        │
└─────────────┘                └───────────────┘                 └───────┘
       ▲ ▲
       │  └─ <Illustrates> (does not satisfy coverage)
       │                  ┌───────────────┐
       │                  │   Example      │
       │                  │   document     │
       │                  └───────────────┘
       │ <Implements>
┌──────┴──────┐   <DependsOn>   ┌──────────────┐
│   Design     │───────────────►│  Other doc    │
│   document   │                │  (any type)   │
└─────────────┘                └──────────────┘

Tasks document (contains <Task> components with depends chains):

┌──────────────────────────────────────────────┐
│  Tasks document                              │
│  ┌──────┐  depends  ┌──────┐  depends  ┌──────┐
│  │Task A│──────────►│Task B│──────────►│Task C│
│  └──────┘           └──┬───┘           └──────┘
│                        │ implements
│                        ▼
│                   Criterion (in req)         │
└──────────────────────────────────────────────┘

Any document can also declare:

┌─────────────┐   <TrackedFiles>   ┌──────────┐
│  Spec        │──────────────────►│  Source   │
│  document    │                   │  files    │
└─────────────┘                   └──────────┘
```

Requirements do not declare which properties validate them. Designs do not
declare which properties they relate to. The pointing-toward direction is
always from the more concrete artifact (property, design) toward the more
abstract one (requirement, criterion). Supersigil computes reverse mappings
automatically.

This prevents synchronization drift: adding a new property never requires
editing the requirement it validates.

### IDs and References

Every supersigil document has a stable identity declared in front matter:

```yaml
supersigil:
  id: git-worktrees/prop/resolve-idempotence
```

IDs are freeform strings. Supersigil does not enforce a structure, but it
can validate against a configurable pattern and emit warnings when IDs
diverge from convention.

The recommended convention is `{feature}/{type-hint}/{name}`, for example
`git-worktrees/req/worktree-module` or `auth/prop/session-expiry`. The
type hint in the ID is a human convenience — supersigil does not parse or
interpret it.

References between documents use the full ID. To reference a specific
criterion within a document, use a fragment: `auth/req/login#valid-creds`.
The fragment must match the `id` attribute of a `<Criterion>` component
in the target document.

```mdx
<Validates refs="auth/req/login#valid-creds, auth/req/login#invalid-password" />
```

IDs are declared in front matter, not derived from file paths. This means
files can be moved or reorganized without breaking references. Supersigil
lint can optionally warn when an ID and its file path diverge significantly.

## Document Format

### Front Matter

The `supersigil:` key is the only namespace supersigil reads. All fields:

| Field    | Required | Description                                                     |
|----------|----------|-----------------------------------------------------------------|
| `id`     | Yes      | Stable unique identifier for this document.                     |
| `type`   | No       | Classification tag (e.g., `requirement`, `property`, `design`). |
| `status` | No       | Lifecycle status. Valid values configured per type.              |

Additional keys outside the `supersigil:` namespace are ignored by
supersigil and available for other tools (Astro, Jira integrations,
custom metadata).

### Front Matter Parsing

Front matter is recognized only when:

- The file begins with `---` on its own line (the first line of the file).
- A closing `---` appears on its own line before the end of the file.
- The content between the delimiters is valid YAML.

If a file starts with `---` but has no closing delimiter, supersigil
emits a parse error. BOM (byte order mark) at the start of a file is
stripped before front matter detection. Both LF and CRLF line endings
are supported.

### Attribute Grammar

MDX components accept attributes. Supersigil uses a restricted attribute
grammar — it does not evaluate JavaScript expressions. All attribute
values must be **string literals**.

**String attributes:**

```mdx
<Criterion id="valid-creds" />
<VerifiedBy strategy="file-glob" tag="prop:login" />
```

**List attributes:**

Attributes that accept multiple values (like `refs` and `paths`) use a
comma-separated string in the MDX source. The parser stores these as
raw strings. Downstream consumers (graph building, verification) split
on `,`, trim whitespace from each item, and reject empty items — using
the component definitions in config to determine which attributes are
list-typed.

```mdx
<Validates refs="auth/req/login#valid-creds, auth/req/login#invalid-password" />

<VerifiedBy
  strategy="file-glob"
  paths="tests/auth/login_test.rs, tests/auth/token_test.rs"
/>
```

Commas inside IDs or paths are not permitted. IDs must not contain commas
(enforced by lint). If a file path contains a comma (rare but possible),
use a glob pattern that matches it instead.

**Expression attributes are rejected:**

```mdx
{/* This is a lint error in supersigil v1 */}
<Validates refs={["auth/req/login#valid-creds"]} />
```

If supersigil encounters a JSX expression attribute (`{...}` syntax), it
emits a lint error with a fix suggestion showing the equivalent string
attribute. This makes the parser trivial and eliminates ambiguity about
expression evaluation.

### Component: `<AcceptanceCriteria>`

A wrapper that groups `<Criterion>` components. Has no attributes of its
own. Exists for document structure and rendering purposes.

```mdx
<AcceptanceCriteria>
  <Criterion id="valid-creds">
    WHEN a user submits valid email and password,
    THE SYSTEM SHALL return a session token.
  </Criterion>
  <Criterion id="invalid-password">
    WHEN a user submits a valid email with an incorrect password,
    THE SYSTEM SHALL return a 401 error.
  </Criterion>
</AcceptanceCriteria>
```

### Component: `<Criterion>`

A single testable acceptance criterion. Must appear as a child of
`<AcceptanceCriteria>` (enforced by lint, not by the parser).

| Attribute | Required | Description                        |
|-----------|----------|------------------------------------|
| `id`      | Yes      | Unique within the document. Used as a fragment target in refs. |

The body of a `<Criterion>` is the criterion text. Supersigil stores it
for display and agent context but does not parse its prose content. The
EARS notation (WHEN...THE SYSTEM SHALL...) is recommended but not enforced.

### Component: `<Validates>`

Declares that the current document validates one or more criteria. This is
the primary link in the verification chain.

| Attribute | Required | Description                                      |
|-----------|----------|--------------------------------------------------|
| `refs`    | Yes      | Comma-separated document IDs, optionally with `#fragment` targeting a `<Criterion>`. |

Can be self-closing or wrapping. If wrapping, the body is treated as
a human-readable rationale (ignored by the verification engine, rendered
in documentation).

```mdx
{/* Self-closing — common case */}
<Validates refs="auth/req/login#valid-creds" />

{/* Multiple refs */}
<Validates refs="auth/req/login#valid-creds, auth/req/login#invalid-password" />

{/* With rationale — rendered in docs */}
<Validates refs="auth/req/login#valid-creds">
  This property validates the happy path by checking token structure
  and expiry across randomized credential inputs.
</Validates>
```

When a ref contains a fragment (`#valid-creds`), supersigil verifies that
the target document contains a `<Criterion id="valid-creds">`. When no
fragment is present, the ref points at the document as a whole.

### Component: `<VerifiedBy>`

Maps this document to tests that verify its claims.

| Attribute  | Required | Description                                                     |
|------------|----------|-----------------------------------------------------------------|
| `strategy` | Yes      | `"file-glob"` or `"tag"`.                                       |
| `paths`    | If file-glob | Comma-separated file paths (relative to project root) or globs. |
| `tag`      | If tag   | A string to search for in test files. Defaults to the last segment of the document ID. |

**`file-glob` strategy (v1):**

The simplest approach. Declares which test files contain the relevant tests.
Supersigil verifies the files exist. This is language-agnostic.

```mdx
<VerifiedBy
  strategy="file-glob"
  paths="tests/worktree/resolve_test.rs, tests/worktree/create_test.rs"
/>
```

**`tag` strategy (v1.x):**

Supersigil scans test files for the hardcoded tag format:
`supersigil: {tag}`. The format is not configurable — a single universal
convention keeps the ecosystem consistent.

```mdx
<VerifiedBy strategy="tag" tag="prop:resolve-idempotence" />
```

In the test file:

```rust
// supersigil: prop:resolve-idempotence
#[test]
fn resolve_or_create_is_idempotent() {
    // ...
}
```

Or in Python:

```python
# supersigil: prop:resolve-idempotence
def test_resolve_or_create_is_idempotent():
    ...
```

The comment-based annotation is the universal fallback. For supported
ecosystems, language-native plugins (see Test Discovery) provide more
precise test discovery via AST analysis.

### Component: `<Implements>`

Declares that the current document implements (provides a technical design
for) referenced specifications.

| Attribute | Required | Description                                      |
|-----------|----------|--------------------------------------------------|
| `refs`    | Yes      | Comma-separated document IDs, optionally with fragments. |

```mdx
<Implements refs="auth/req/login, auth/req/session-management" />
```

`<Implements>` does not participate in the verification chain (it does not
bridge to tests). It provides traceability: "this design addresses these
requirements." Supersigil checks that the refs resolve but does not require
coverage of individual criteria.

### Component: `<Illustrates>`

Declares that this document illustrates (provides examples for) referenced
criteria. This is the link between example scenarios and the specs they
demonstrate.

| Attribute | Required | Description                                      |
|-----------|----------|--------------------------------------------------|
| `refs`    | Yes      | Comma-separated document IDs, optionally with fragments. |

```mdx
<Illustrates refs="auth/req/login#valid-creds, auth/req/login#rate-limit" />
```

`<Illustrates>` participates in ref resolution (refs must resolve) but
**does not satisfy coverage**. A criterion referenced only by
`<Illustrates>` is still reported as uncovered by `uncovered_criterion`.
This prevents examples from inflating verification confidence — an
example documents behavior, it doesn't verify it.

`supersigil context` displays illustrations as a separate section
("Illustrated by: ..."), keeping them visible without conflating them
with validating properties.

### Component: `<Task>`

An implementation task within a tasks document. Tasks are the actionable
units of work that agents and humans execute. They live inside a document
with `type: tasks` — one document per feature, containing all its tasks.

| Attribute    | Required | Description                                      |
|--------------|----------|--------------------------------------------------|
| `id`         | Yes      | Unique within the document. Used as a fragment target in refs. |
| `status`     | No       | Task status: `draft`, `ready`, `in-progress`, `done`. |
| `implements` | No       | Comma-separated criterion refs this task addresses. |
| `depends`    | No       | Comma-separated sibling task IDs (within the same document) that must complete first. |

```mdx
<Task id="type-alignment" status="done">
  Align session token types between auth service and API gateway.
</Task>

<Task id="adapter-code" status="in-progress" implements="auth/req/login#valid-creds" depends="type-alignment">
  Implement the login handler using the new token types.
</Task>

<Task id="switch-over" depends="adapter-code">
  Swap the old handler for the new one behind the feature flag.
</Task>

<Task id="cleanup" depends="switch-over">
  Remove the old handler, feature flag, and legacy types.
</Task>
```

Tasks can nest. A parent task contains sub-tasks:

```mdx
<Task id="update-data-model" depends="type-alignment">
  Update the data model for the new auth flow.

  <Task id="add-field" status="done">
    Add the session_token field to the User struct.
  </Task>

  <Task id="write-migration" depends="add-field">
    Write the database migration for the new field.
  </Task>
</Task>
```

A parent task is implicitly complete when all its children are done.
Sub-task `depends` references are sibling-scoped (within the same
parent). All task IDs are unique within the document regardless of
nesting depth, so fragment refs stay flat: `auth/tasks/login#write-migration`.

Supersigil verifies:

- Task `depends` references resolve to sibling tasks (within the same
  parent, or within the document for top-level tasks).
- Task `implements` references resolve to existing criteria.
- The dependency graph within a document is a DAG (cycles are a hard error).
- `supersigil context` and `supersigil plan` present tasks in
  topological (dependency) order.

### Component: `<DependsOn>`

Declares ordering dependencies between documents. This is for
document-level ordering (e.g., one design document depends on another).
For task-level ordering within a tasks document, use the `depends`
attribute on `<Task>`.

| Attribute | Required | Description                                      |
|-----------|----------|--------------------------------------------------|
| `refs`    | Yes      | Comma-separated document IDs that this document depends on. |

```mdx
<DependsOn refs="auth/design/token-format" />
```

`<DependsOn>` participates in ref resolution and cycle detection.

### Component: `<TrackedFiles>`

Declares which source files this spec document is concerned with. This
enables **code-to-doc routing**: given a set of changed files (from
`git diff`), supersigil can report which spec documents are potentially
affected and may need review.

| Attribute | Required | Description                                      |
|-----------|----------|--------------------------------------------------|
| `paths`   | Yes      | Comma-separated file paths or globs, relative to the project root. |

```mdx
<TrackedFiles paths="src/auth/**/*.rs, src/session/**/*.rs" />
```

`<TrackedFiles>` does not assert correctness — it is a routing signal.
When source files matching the glob change, supersigil flags the document
as potentially stale. The developer or agent then decides whether the
spec needs updating.

**Path resolution:** All paths are relative to the project root (the
directory containing `supersigil.toml`). Globs follow the same syntax
as the `paths` and `tests` config fields.

**Rename/delete handling:** If a tracked glob matches zero files,
supersigil emits a warning: "TrackedFiles glob `src/auth/**/*.rs` in
document `auth/req/login` matches no files." This catches stale globs
after file renames or deletions without making it a hard error (the
glob may be intentionally broad for files not yet created).

## Configuration

Supersigil is configured via `supersigil.toml` at the project root. The
config uses a "simple case first" pattern: top-level keys for single-project
setups, an explicit `projects` table for monorepos. This mirrors how
`tsconfig.json` uses top-level `compilerOptions` vs. `references`, and how
Cargo distinguishes `[package]` from `[workspace]`.

### Minimal Configuration

```toml
paths = ["specs/**/*.mdx"]
```

That's it. A one-liner. Everything else — `tests`, document types,
components, test discovery — has sensible defaults and is only declared
when you need to override.

### Single-Project With Overrides

```toml
paths = ["specs/**/*.mdx"]
tests = ["tests/**/*"]

# Optional: validate IDs against a pattern (warning, not error)
id_pattern = '^[a-z0-9-]+(/[a-z0-9-]+)*$'

[documents.types.requirement]
status = ["draft", "review", "approved", "implemented"]
required_components = ["TrackedFiles"]

[documents.types.property]
status = ["draft", "specified", "verified"]

[documents.types.design]
status = ["draft", "review", "approved"]

[documents.types.tasks]
status = ["draft", "ready", "in-progress", "done"]
```

### Multi-Project (Monorepo / Workspace)

When a project grows beyond a single set of paths, use the `projects`
table. Top-level `paths`, `tests`, and `projects` are mutually exclusive
— if `paths` or `tests` appear alongside `projects`, supersigil exits
with an error.

```toml
[projects.backend]
paths = ["services/api/specs/**/*.mdx"]
tests = ["services/api/tests/**/*"]

[projects.frontend]
paths = ["apps/web/specs/**/*.mdx"]
tests = ["apps/web/tests/**/*"]

# Shared config still lives at the top level
id_pattern = '^[a-z0-9-]+(/[a-z0-9-]+)*$'

[documents.types.requirement]
status = ["draft", "review", "approved", "implemented"]

[documents.types.property]
status = ["draft", "specified", "verified"]

[documents.types.design]
status = ["draft", "review", "approved"]

[documents.types.tasks]
status = ["draft", "ready", "in-progress", "done"]
```

### Full Configuration With All Options

```toml
paths = ["specs/**/*.mdx"]
tests = ["tests/**/*"]
id_pattern = '^[a-z0-9-]+(/[a-z0-9-]+)*$'

# Document type definitions. These configure valid statuses and
# required components per type. Types are classification tags — they
# do not affect verification logic beyond these declarations.
[documents.types.requirement]
status = ["draft", "review", "approved", "implemented"]
required_components = ["TrackedFiles"]

[documents.types.property]
status = ["draft", "specified", "verified"]

[documents.types.design]
status = ["draft", "review", "approved"]

[documents.types.tasks]
status = ["draft", "ready", "in-progress", "done"]

# Component definitions. These configure the verification rules.
# The built-in components ship as defaults; only declare overrides
# or custom components here.
# Each attribute has a `required` boolean and an optional `list` boolean.
# Attributes with `list = true` are split on commas by downstream consumers.
# Attributes without `list` (or `list = false`) are treated as single strings.
[components.AcceptanceCriteria]
attributes = {}

[components.Criterion]
attributes.id = { required = true }
referenceable = true

[components.Validates]
attributes.refs = { required = true, list = true }
target_component = "Criterion"

[components.VerifiedBy]
attributes.strategy = { required = true }
attributes.tag = { required = false }
attributes.paths = { required = false, list = true }

[components.Implements]
attributes.refs = { required = true, list = true }

[components.Illustrates]
attributes.refs = { required = true, list = true }

[components.Task]
attributes.id = { required = true }
attributes.status = { required = false }
attributes.implements = { required = false, list = true }
attributes.depends = { required = false, list = true }
referenceable = true

[components.TrackedFiles]
attributes.paths = { required = true, list = true }

[components.DependsOn]
attributes.refs = { required = true, list = true }

# Verification strictness — default severity for all configurable rules.
# Values: "off", "warning", "error"
# Built-in default: "error" (strict by default, relax as needed).
# Hard errors (broken_ref, duplicate_id, missing_required_attribute,
# expression_attribute, dependency_cycle) are always fatal and not configurable.
[verify]
strictness = "error"

# Per-rule severity overrides: "off", "warning", or "error"
# Precedence: draft gating > per-rule override > global strictness > built-in default
# Unknown rule keys are config errors (catches typos).
[verify.rules]
# uncovered_criterion = "error"        # built-in default
# unverified_validation = "error"      # built-in default
# missing_test_files = "error"         # built-in default
# zero_tag_matches = "warning"         # built-in default
# status_inconsistency = "warning"     # built-in default
# missing_required_component = "warning" # built-in default
# stale_tracked_files = "warning"      # built-in default
# empty_tracked_glob = "warning"       # built-in default
# orphan_test_tag = "warning"          # built-in default
# invalid_id_pattern = "warning"       # built-in default
# isolated_document = "off"            # built-in default

# Ecosystem plugins for language-native test discovery.
# Built-in: "rust" (uses syn to parse #[test] items).
# Default when [ecosystem] is omitted: plugins = ["rust"].
# Set plugins = [] to explicitly disable all plugins.
# Future: Extism WASM plugins for other languages.
[ecosystem]
plugins = ["rust"]

# Test results consumption (for pass/fail reporting)
[test_results]
formats = ["junit-xml"]
paths = ["target/test-results/**/*.xml", "test-reports/**/*.xml"]
```

### Test Discovery

Supersigil uses a hardcoded tag format for comment-based test discovery:
`supersigil: {tag}`. This is not configurable — a single universal
format keeps the ecosystem consistent and avoids per-project bikeshedding.

In test files, annotate tests with a comment containing the tag:

```rust
// supersigil: prop:resolve-idempotence
#[test]
fn resolve_or_create_is_idempotent() { ... }
```

```python
# supersigil: prop:resolve-idempotence
def test_resolve_or_create_is_idempotent():
    ...
```

Supersigil scans for this format across all common comment styles
(`//`, `#`, `///`, `--`, `/* */`). The tag is matched literally.

**Ecosystem plugins** provide language-native test discovery that is
more precise than comment scanning. The built-in Rust plugin uses `syn`
to parse test items and find supersigil annotations via attributes or
doc comments. It also understands proptest — extracting case counts,
shrunk counterexamples from failure output, and regression file
locations. This surfaces the difference between "2 unit tests passing"
and "1 property-based test, 1000 cases, no counterexamples" in
verification reports. Future plugins (via Extism WASM) can provide
the same for Python (hypothesis), TypeScript, Go, and other ecosystems.

```toml
[ecosystem]
plugins = ["rust"]                         # built-in, compiled in
# plugins = ["rust", "./plugins/py.wasm"]  # future: Extism WASM
```

The plugin interface is designed in Rust and will be exposed through
Extism when stabilized:

```rust
pub trait EcosystemPlugin {
    /// Discover tests that verify a given property tag.
    fn discover_tests(&self, tag: &str, test_paths: &[PathBuf]) -> Vec<DiscoveredTest>;
}

pub struct DiscoveredTest {
    pub file: PathBuf,
    pub line: usize,
    pub name: String,
    /// Test framework metadata: "unit", "proptest", "integration", etc.
    pub kind: Option<String>,
    /// For property-based tests: number of cases, counterexamples, etc.
    pub metadata: HashMap<String, String>,
}
```

The comment-based scanner serves as the universal fallback for languages
without a dedicated plugin.

### Component Configuration

Components are declared in config with their attribute requirements and
relationship rules. The built-in components (`Criterion`, `Validates`,
`VerifiedBy`, `Implements`, `TrackedFiles`) ship as defaults but can be
overridden or extended.

Document types can declare `required_components` — components that must
be present in documents of that type:

```toml
[documents.types.requirement]
status = ["draft", "review", "approved", "implemented"]
required_components = ["TrackedFiles"]

[documents.types.property]
status = ["draft", "specified", "verified"]
# Properties implicitly need Validates and VerifiedBy, but those
# are enforced by the unverified_validation rule, not by this field.
```

The `missing_required_component` rule fires when a document declares a
type but is missing a required component. This is a configurable rule
(default: warning), and is suppressed to info for draft documents.

Users can define entirely custom components:

```toml
[components.Risk]
attributes.severity = { required = true }
attributes.mitigation = { required = false }

[components.Blocks]
attributes.refs = { required = true, list = true }
```

Custom components participate in ref resolution (if they have `refs`
attributes) but have no built-in verification behavior beyond structural
checks. Custom verification can be added via hooks.

### Hooks

For verification logic beyond what the built-in rules provide, supersigil
supports external process hooks:

```toml
[hooks]
post_verify = ["./scripts/check-coverage-threshold.sh"]
post_lint = ["./scripts/custom-checks.sh"]
export = ["./scripts/sync-to-linear.sh"]
timeout_seconds = 30  # default: 30
```

**Execution contract:**

- Hook groups run in a fixed lifecycle order: `post_lint` → `post_verify`
  → `export`. Within each group, hooks run sequentially in declaration
  order.
- Each hook receives the verification report as JSON on stdin.
- A hook's stdout is captured as additional findings (JSON array of
  `{level: "error"|"warning"|"info", message: "..."}` objects).
- A hook's stderr is captured for diagnostics but does not affect the
  report.
- A non-zero exit code from a hook adds an error to the report:
  "hook `./scripts/check-coverage-threshold.sh` failed (exit code 1)."
- Each hook has a configurable timeout (default 30 seconds). If a hook
  exceeds its timeout, it is killed and an error is added.
- Captured stdout and stderr are truncated to 64 KB per hook to keep
  reports stable and prevent runaway output.

## Verification

`supersigil verify` is the primary command. Findings are split into two
categories:

**Hard errors** are structural integrity failures. They are always fatal,
never configurable, and prevent verification from completing — the same
way a syntax error prevents compilation.

| Hard Error | Description |
|---|---|
| `broken_ref` | A `refs` attribute points to a document ID or `#fragment` that doesn't exist. |
| `duplicate_id` | Two or more documents share the same `supersigil.id`. |
| `missing_required_attribute` | A component is missing a required attribute (e.g., `<Criterion>` without `id`). |
| `expression_attribute` | An attribute uses JSX expression syntax (`{...}`) instead of a string literal. |
| `unknown_component` | An MDX component name isn't in the built-in or configured component set. Commonly indicates agent hallucination. |
| `dependency_cycle` | A `<Task>` `depends` graph or `<DependsOn>` ref graph contains a cycle. |

**Configurable rules** are verification findings with a built-in default
severity. Each rule's severity can be overridden via `[verify.rules]` in
config. The global `strictness` setting shifts all defaults. All levels
use the same values: `"off"`, `"warning"`, `"error"`.

Precedence (highest to lowest):

1. **Draft gating** — if the document has `status: draft`, all
   configurable rules are suppressed to `info` regardless of any
   other setting. Hard errors are never suppressed.
2. **Per-rule override** — `[verify.rules]` in config.
3. **Global strictness** — `[verify] strictness`.
4. **Built-in default** — the value in the table below.

| Rule | Description | Built-in Default |
|---|---|---|
| `uncovered_criterion` | A `<Criterion>` exists that no `<Validates>` in any document references. | error |
| `unverified_validation` | A document has `<Validates>` but no `<VerifiedBy>`. It makes a verification claim with no test evidence. | error |
| `missing_test_files` | A `<VerifiedBy strategy="file-glob">` references files that don't exist on disk. | error |
| `zero_tag_matches` | A `<VerifiedBy strategy="tag">` tag matches zero tests in the scanned files. | warning |
| `status_inconsistency` | A document's `status` conflicts with its verification state (e.g., `verified` with no tests). | warning |
| `missing_required_component` | A document type declares `required_components` and this document is missing one (e.g., requirement without `<TrackedFiles>`). | warning |
| `stale_tracked_files` | Source files matching a `<TrackedFiles>` glob have changed since a given git ref (only active with `--since`). | warning |
| `empty_tracked_glob` | A `<TrackedFiles>` glob matches zero files on disk. | warning |
| `orphan_test_tag` | A test file contains a `supersigil:` tag that doesn't correspond to any document's `<VerifiedBy>` tag. | warning |
| `invalid_id_pattern` | A document's ID doesn't match the configured `id_pattern`. | warning |
| `isolated_document` | A document has `supersigil:` front matter but no incoming or outgoing component refs — disconnected from the graph. | off |

### Status-Gated Rule Application

As defined in the precedence model above, documents with `status: draft`
have all configurable rules suppressed to `info` level. This is the
highest-priority rule in the precedence chain — a per-rule override of
`"error"` does **not** override draft suppression. Draft documents still
show what *would* be an error or warning, but don't fail the build.

```
  auth/prop/token-generation (draft)
    ℹ uncovered_criterion: would be error if not draft
    ℹ unverified_validation: would be error if not draft
```

Hard errors (`broken_ref`, `duplicate_id`, `missing_required_attribute`,
`expression_attribute`, `dependency_cycle`) are **never** suppressed by
draft status. A broken ref is broken regardless of lifecycle stage.

This makes `status: draft` genuinely useful: it's the mechanism that lets
you work iteratively without the tool blocking you, while still surfacing
what needs to be done before the document is promoted. When you change
status to anything other than `draft`, the full rule set applies.

### `lint` vs `verify`

`supersigil lint` and `supersigil verify` share the same hard errors
but differ in scope and speed:

**`lint`** is per-file. It parses each document independently and checks
structural correctness: valid front matter, known components, required
attributes, string-only attributes, `<Criterion>` nesting. Unknown
component names and missing required attributes are hard errors caught
at lint time. It does **not** build the cross-document graph. This makes
it fast — suitable for editor integration, pre-commit hooks, and agent
authoring feedback.

**`verify`** is whole-graph. It builds the full document index, resolves
all refs, checks coverage, test mappings, dependency ordering, status
consistency, and orphan detection. This is the CI command.

The practical implication: an agent authoring a new document runs `lint`
for immediate feedback (syntax errors, unknown components, missing
attributes). The CI pipeline runs `verify` for the complete picture.

### Ref Resolution

Every `refs` attribute in every component is checked:

- The target document ID must exist in the project (or in any project in
  the workspace — see Cross-Project Refs below).
- If a fragment is present (`#criterion-id`), the target document must
  contain a component with a matching `id` attribute. If the referring
  component has a `target_component` configured (e.g., `Validates` targets
  `Criterion`), the matched component must be of that type.

This is a hard error — not configurable.

### Cross-Project Refs

In multi-project workspaces, refs resolve across all projects by default.
A spec in `backend` can reference a criterion in `core`. The global
document index is built across all projects; `verify --project backend`
scopes its *findings* to backend documents but resolves refs globally.

To isolate a project (make cross-project refs errors), set `isolated`:

```toml
[projects.backend]
paths = ["services/api/specs/**/*.mdx"]
isolated = true  # refs must resolve within this project only
```

### Coverage

For every `<Criterion>` in the project, supersigil checks whether at least
one `<Validates>` in any document references it.

Rule: `uncovered_criterion`. Built-in default: **error.** If you wrote a
`<Criterion>`, you intended it to be validated. (Suppressed to info for
draft documents.)

### Dependency Ordering

The `<Task>` `depends` graph (within each tasks document) and the
`<DependsOn>` ref graph (between documents) are both checked for cycles.
A cycle is a hard error — it makes topological ordering impossible
and indicates a structural problem in the implementation plan.

Task `depends` references are also checked for resolution: each must
match a sibling task ID within the same parent (or within the document
for top-level tasks). Unresolved depends references are hard errors.

If the graphs are acyclic, supersigil computes a topological order
used by `supersigil context` and `supersigil plan` to present tasks
in implementation sequence.

### Test Mapping

For every `<VerifiedBy>` component, supersigil checks that the declared
tests exist. The verification output distinguishes confidence tiers to
prevent false confidence:

- **`file-glob` strategy:** The declared paths/globs are checked for
  existence. The output labels these as "N files linked (existence only)"
  to signal that supersigil has not verified that relevant tests are
  *inside* those files.
  Rule: `missing_test_files`. Built-in default: **error.**

- **`tag` strategy:** Test files are scanned for the hardcoded tag
  format (see Test Discovery). The output labels these as
  "N tests matched (tag: ...)" — a stronger signal because supersigil
  found annotated test sites.
  Rule: `zero_tag_matches`. Built-in default: **warning.**

For documents that have `<Validates>` but no `<VerifiedBy>`, the document
makes a verification claim it hasn't backed up with tests.
Rule: `unverified_validation`. Built-in default: **error.** (Suppressed
to info for draft documents — you can write the property first and add
test mappings later.)

### Test Results (Optional)

If test result files (JUnit XML) are available, supersigil matches test
names against property tags and reports pass/fail status per property.
This is optional — `supersigil verify` works without test results, it
just can't report pass/fail. The output labels these as
"N tests matched, M passing" — the highest confidence tier.

### Tracked Files Staleness

For every `<TrackedFiles>` component, supersigil can check whether the
tracked source files have changed relative to the spec document. This
is the code-to-doc routing check.

- `supersigil affected --since <ref>` computes which spec documents
  have `<TrackedFiles>` globs matching files changed since the given
  git ref. Output is a list of `{id, path, matched_globs, changed_files}`
  objects, where `path` is the spec document's file path relative to
  the project root. The diff is computed as `ref..HEAD` (direct, not merge-base)
  and includes uncommitted and staged changes by default. Use
  `--committed-only` to exclude the working tree and index, or
  `--merge-base` to diff against `$(git merge-base ref HEAD)` instead
  of `ref` directly (appropriate for long-running branches where `ref`
  has advanced since the branch point).

- During `supersigil verify`, if `--since <ref>` is provided, documents
  whose tracked files changed are flagged as potentially stale. The same
  diff semantics apply (`ref..HEAD`, includes uncommitted/staged unless
  `--committed-only`; use `--merge-base` for branch-relative diffs).
  Rule: `stale_tracked_files`. Built-in default: **warning.**

- If a `<TrackedFiles>` glob matches zero files on disk, supersigil
  emits a finding.
  Rule: `empty_tracked_glob`. Built-in default: **warning.**

### Status Consistency

If a document has `status: verified` but its `<VerifiedBy>` has no
matched tests, that's a finding. If a document has `status: implemented`
but not all of its criteria have validating properties, that's a finding.

Status does not enforce state transitions — supersigil reports
inconsistencies between what the status *claims* and what verification
*observes*.

Rule: `status_inconsistency`. Built-in default: **warning.**

### Orphan Detection

- Test files with supersigil tags that don't correspond to any document's
  `<VerifiedBy>` tag are reported as orphans.
  Rule: `orphan_test_tag`. Built-in default: **warning.**

- Documents with no incoming or outgoing component refs are reported as
  isolated — they're in the spec system but disconnected from the graph.
  Rule: `isolated_document`. Built-in default: **off.**

### Output

The verification report can be output as:

- **Terminal** (default): Human-readable summary with colors and symbols.
- **JSON**: Machine-readable for CI integration and hooks.
- **Markdown**: Committable report, renderable in documentation.

Exit codes: 0 for clean, 1 for errors, 2 for warnings-only. When
`strictness = "error"` (or when `[verify.rules]` promotes findings to
errors), those findings produce exit code 1.

### CLI Output Conventions

All terminal output follows standard CLI best practices:

- Color via ANSI escapes, respecting `NO_COLOR` and `FORCE_COLOR`
  environment variables (see https://no-color.org).
- Color is auto-disabled when stdout is not a TTY (piped or redirected).
- `--color always|never|auto` flag for explicit control.
- Stderr for diagnostics and progress; stdout for data (so
  `supersigil ls | grep draft` works).
- Unicode symbols (✓, ✗, ⚠, ℹ) in terminal mode, ASCII fallback when
  not a TTY.

## Agent Integration

Supersigil is designed to be consumed by AI coding agents. Key commands:

### `supersigil context <id>`

Outputs a focused, structured view of a document and its relationships.
An agent implementing a feature includes this in its context window.
Tasks (from linked tasks documents) are presented in topological
(dependency) order, giving agents a ready-made implementation sequence.

```
$ supersigil context auth/req/login

# Requirement: User Login
ID: auth/req/login
Status: approved

## Criteria:
- valid-creds: WHEN a user submits valid email and password,
  THE SYSTEM SHALL return a session token.
  → Validated by: auth/prop/token-generation (verified, 2 test files)

- invalid-password: WHEN a user submits an incorrect password,
  THE SYSTEM SHALL return a 401 error.
  → Validated by: auth/prop/error-responses (specified, 0 test files)

## Implemented by:
- auth/design/login-flow (approved)

## Illustrated by:
- auth/example/login-happy-path
- auth/example/login-rate-limited

## Tasks (from auth/tasks/login, in dependency order):
1. type-alignment (done) ✓
2. adapter-code (in-progress) — depends on: type-alignment
   implements: #valid-creds
3. switch-over (ready) — depends on: adapter-code
4. cleanup (draft) — depends on: switch-over

## No validating property:
- rate-limit: WHEN a user exceeds 5 failed attempts...
```

### `supersigil plan <id>`

Outputs the outstanding work for a requirement or feature: uncovered
criteria, pending tasks in dependency order, and current test status.
This is the natural entry point for an agent starting a session — a
focused "what work remains?" view.

```
$ supersigil plan auth/req/login

# Plan: auth/req/login (User Login)

## Outstanding criteria:
- invalid-password: no validating property
- rate-limit: no validating property

## Pending tasks (from auth/tasks/login, in dependency order):
1. error-responses (ready)
   implements: #invalid-password
   depends on: type-alignment (done) ✓

2. rate-limiter (draft)
   implements: #rate-limit
   depends on: error-responses (ready)

## Illustrated by:
- auth/example/login-happy-path
- auth/example/login-rate-limited

## Completed:
- type-alignment (done) ✓
  auth/prop/token-generation → 2 tests passing ✓
```

The input can be:

- A **requirement ID** — shows tasks implementing its criteria.
- A **feature prefix** (e.g., `auth/`) — shows everything under that
  namespace.
- **No argument** — project-wide plan, all outstanding work.

With `--format json`, the output is machine-consumable for agent
orchestration: an array of `{criterion, status, tasks, illustrations}`
objects.

### `supersigil affected --since <ref>`

Reports which spec documents have `<TrackedFiles>` globs matching files
changed since the given git ref. This is the code-to-doc routing signal.

```
$ supersigil affected --since main

  3 documents affected by changes since main:

  auth/req/login
    matched glob: src/auth/**/*.rs
    changed files: src/auth/handler.rs, src/auth/password.rs

  auth/prop/token-generation
    matched glob: src/auth/**/*.rs
    changed files: src/auth/handler.rs

  auth/design/login-flow
    matched glob: src/auth/**/*.rs, src/session/**/*.rs
    changed files: src/auth/handler.rs
```

With `--format json`, the output is a JSON array of
`{id, path, matched_globs, changed_files}` objects (`path` is the spec
document's file path relative to the project root) — suitable for
CI scripts that gate merges on spec review.

### `supersigil schema`

Outputs the valid components, their attributes, and the current document
types as JSON or YAML. An agent includes this in its context to produce
syntactically valid MDX without hallucinating component names or attributes.

```
$ supersigil schema --format yaml

components:
  AcceptanceCriteria:
    attributes: {}
  Criterion:
    attributes:
      id: { required: true }
    referenceable: true
  Validates:
    attributes:
      refs: { required: true, list: true }
    target_component: Criterion
  VerifiedBy:
    attributes:
      strategy: { required: true }
      tag: { required: false }
      paths: { required: false, list: true }
  Implements:
    attributes:
      refs: { required: true, list: true }
  Illustrates:
    attributes:
      refs: { required: true, list: true }
  Task:
    attributes:
      id: { required: true }
      status: { required: false }
      implements: { required: false, list: true }
      depends: { required: false, list: true }
    referenceable: true
  TrackedFiles:
    attributes:
      paths: { required: true, list: true }
  DependsOn:
    attributes:
      refs: { required: true, list: true }
document_types:
  requirement:
    status: [draft, review, approved, implemented]
  property:
    status: [draft, specified, verified]
  design:
    status: [draft, review, approved]
  tasks:
    status: [draft, ready, in-progress, done]
```

### Agent Feedback Loop

When an agent writes or modifies spec documents, `supersigil verify` in
CI (or as a pre-commit hook) provides immediate feedback:

- Hallucinated IDs → "ref `auth/prop/nonexistent` does not resolve"
- Invalid components → "unknown component `<Verifies>`, did you mean `<Validates>`?"
- Missing attributes → "`<Criterion>` requires `id` attribute"
- Broken fragments → "document `auth/req/login` has no criterion `#typo`"

This correction signal is what turns supersigil from a documentation tool
into an engineering tool. The specs are not just described — they are
enforced.

## Implementation

### Language and Distribution

Supersigil is implemented in Rust and distributed as a single static
binary. The `markdown` crate (markdown-rs) provides MDX parsing with
full AST access, including `MdxJsxFlowElement` and `MdxJsxAttribute`
nodes for component extraction.

Front matter is parsed separately (extract the YAML block before MDX
parsing, deserialize with `serde_yaml`).

### Crate Architecture

```
supersigil/
├── crates/
│   ├── supersigil-core/       # Document model, component graph, config
│   │   ├── src/
│   │   │   ├── config.rs      # supersigil.toml parsing
│   │   │   ├── document.rs    # SpecDocument, front matter, component extraction
│   │   │   ├── graph.rs       # Ref resolution, reverse mappings
│   │   │   ├── components.rs  # Built-in component definitions
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── supersigil-parser/     # MDX parsing, front matter extraction
│   │   ├── src/
│   │   │   ├── mdx.rs         # markdown-rs integration, AST walking
│   │   │   ├── frontmatter.rs # YAML extraction and deserialization
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── supersigil-verify/     # Verification engine
│   │   ├── src/
│   │   │   ├── coverage.rs    # Criterion coverage checking
│   │   │   ├── refs.rs        # Ref resolution and orphan detection
│   │   │   ├── tests.rs       # Test discovery and mapping
│   │   │   ├── tracked.rs     # TrackedFiles staleness and affected command
│   │   │   ├── status.rs      # Status consistency checking
│   │   │   ├── report.rs      # Output formatting (terminal, JSON, markdown)
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   └── supersigil-cli/        # CLI entry point
│       ├── src/
│       │   └── main.rs        # Argument parsing, subcommand dispatch
│       └── Cargo.toml
├── supersigil.toml            # Dogfooding: supersigil's own config
└── Cargo.toml                 # Workspace
```

The `core`, `parser`, and `verify` crates are designed as libraries,
not just internal modules for the CLI. Other Rust applications (such as
Superpilot-desktop) can depend on them directly for embedded spec
verification, graph queries, and incremental re-parsing. The CLI crate
is a thin layer over the libraries — it handles argument parsing,
terminal output, and exit codes, but the verification logic is entirely
in the library crates. This separation is also required for the "watch
mode" open question, which needs incremental verification (re-verify
only documents whose files or dependencies changed).

### Key Dependencies

| Crate          | Purpose                                          |
|----------------|--------------------------------------------------|
| `markdown`     | MDX parsing, AST generation (`to_mdast` with MDX constructs enabled) |
| `serde`, `serde_yaml` | Front matter deserialization                |
| `toml`         | Config file parsing                              |
| `glob`         | File path matching                               |
| `clap`         | CLI argument parsing                             |
| `serde_json`   | JSON output for schema, reports, hook IPC         |
| `git2`         | Git diff for `affected` and `verify --since`      |

### Parser Pipeline

The parser processes each MDX file in three stages:

**Stage 1: Front Matter Extraction.** Read the file. Strip a leading BOM
(U+FEFF) if present. Normalize line endings to LF. Check that the first
line is exactly `---`. If not, return `ParseResult::NotSupersigil(path)`.
Scan for the closing `---` on its own line. If the opening delimiter is
present but no closing delimiter is found, emit a parse error. Extract
the YAML between the delimiters. If the YAML does not contain a
`supersigil:` key, return `ParseResult::NotSupersigil(path)`.
Deserialize the `supersigil:` key into a typed struct. Preserve all
other keys as opaque metadata in `SpecDocument.extra`.

**Stage 2: MDX AST Generation.** Pass the body (everything after front
matter) to `markdown::to_mdast` with MDX constructs enabled. This
produces a tree of `mdast::Node` variants.

**Stage 3: Component Extraction.** Walk the AST, collecting
`MdxJsxFlowElement` nodes. For each, extract the component name (from
`element.name`) and attributes (from `element.attributes`). Attributes
are parsed according to the restricted attribute grammar: string literals
are stored as raw strings; expression attributes (`{...}` syntax) produce
a lint error. The parser does not split list-type attributes — all
attribute values are stored as raw strings. List splitting (for `refs`,
`paths`, etc.) is deferred to downstream consumers using component
definitions from config, which declare `list = true` on list-typed
attributes.
Collect children recursively for nesting (e.g., `<Criterion>` inside
`<AcceptanceCriteria>`). Body text is the concatenation of all
non-component text nodes within a component; child components are
extracted separately into `children`.

The output is a `ParseResult`:

```rust
/// The parser returns a ParseResult to distinguish supersigil documents
/// from non-supersigil files (e.g., README.mdx, index pages) that may
/// be caught by glob patterns.
pub enum ParseResult {
    /// A valid supersigil document with front matter and components.
    Document(SpecDocument),
    /// A file without supersigil front matter — not a spec document.
    NotSupersigil(PathBuf),
}

pub struct SpecDocument {
    /// Source file path (relative to project root).
    pub path: PathBuf,
    /// Parsed front matter (supersigil: namespace).
    pub frontmatter: Frontmatter,
    /// All YAML front matter keys outside the supersigil: namespace,
    /// preserved as opaque data for documentation toolchains.
    pub extra: HashMap<String, serde_yaml::Value>,
    /// Extracted components with their attributes and positions.
    pub components: Vec<ExtractedComponent>,
}

pub struct Frontmatter {
    pub id: String,
    pub doc_type: Option<String>,
    pub status: Option<String>,
}

pub struct ExtractedComponent {
    pub name: String,
    /// All attribute values are raw strings as extracted from MDX.
    /// List splitting (for refs, paths, etc.) is deferred to downstream
    /// consumers using component definitions from config.
    pub attributes: HashMap<String, String>,
    pub children: Vec<ExtractedComponent>,
    /// Concatenation of all non-component text nodes. None if self-closing
    /// or if the component contains only child components with no text.
    pub body_text: Option<String>,
    pub position: SourcePosition,
}
```

### Verification Pipeline

`supersigil verify` runs the following stages in order:

1. **Load config** from `supersigil.toml`.
2. **Discover files** by expanding project path globs.
3. **Parse all documents** into `ParseResult` values. Filter out
   `NotSupersigil` results (non-spec files caught by globs). Collect
   `SpecDocument` structs from `Document` results.
4. **Build the graph**: index documents by ID, build a map of all
   referenceable components (Criterion id → document ID + component).
   In multi-project workspaces, build a global index for ref resolution.
5. **Resolve refs**: for every `refs` attribute, check the target exists.
   For isolated projects, restrict resolution to the project's own docs.
6. **Check dependency ordering**: verify `<Task>` `depends` graphs
   (within each tasks document) and `<DependsOn>` ref graphs are DAGs.
   Compute topological order for `context` and `plan` output.
7. **Check coverage**: for every `<Criterion>`, check if any `<Validates>`
   refs point to it.
8. **Check test mappings**: for every `<VerifiedBy>`, check file existence
   (file-glob) or scan for tags (tag strategy). Label confidence tiers.
9. **Check tracked files**: for every `<TrackedFiles>`, verify globs match
   at least one file. If `--since <ref>` is provided, check for changes.
10. **Check status consistency**: compare declared statuses against actual
    verification state.
11. **Detect orphans**: scan test files for supersigil tags not referenced
    by any document.
12. **Apply severity**: resolve each finding's effective severity using
    the four-level precedence (draft gating > per-rule > global > built-in).
13. **Run hooks**: if configured, pass the report to external processes.
    Enforce timeout, capture stdout/stderr (truncated to 64 KB).
14. **Output report** in the requested format.

## CLI Commands

```
supersigil init                    # Create supersigil.toml with defaults
supersigil verify                  # Run all verification checks
supersigil verify --project backend  # Verify a single project (multi-project only)
supersigil verify --format json    # Output as JSON
supersigil verify --test-results target/results.xml  # Include pass/fail
supersigil verify --since main     # Flag docs with tracked files changed since ref
supersigil lint                    # Structural checks only (fast)
supersigil ls                      # List all documents (alias: list)
supersigil ls --type requirement   # Filter by document type
supersigil ls --status draft       # Filter by status
supersigil ls --project backend    # Filter by project (multi-project only)
supersigil status                  # Summary: coverage, staleness, counts
supersigil status <id>             # Status of a specific document
supersigil context <id>            # Agent-friendly view of a document
supersigil plan <id>               # Outstanding work: uncovered criteria, pending tasks
supersigil plan <id> --format json # Machine-readable for agent orchestration
supersigil plan                    # Project-wide plan (all outstanding work)
supersigil schema                  # Output component/type definitions
supersigil graph                   # Output the document graph (mermaid)
supersigil graph --format dot      # Output as graphviz dot
supersigil affected --since main   # List docs whose tracked files changed
supersigil affected --since HEAD~1 --format json  # Machine-readable for CI
supersigil affected --since main --committed-only  # Exclude uncommitted/staged
supersigil affected --since main --merge-base  # Diff against branch point
supersigil new <type> <id>         # Scaffold a new document
supersigil import --from kiro      # Import .kiro/specs/ to supersigil format
supersigil import --from kiro --dry-run  # Preview import without writing files
```

## Example: Complete Feature Spec

To illustrate how documents relate in practice, here is a small but
complete example — a login feature with one requirement, one property,
and one design document.

### `specs/auth/req/login.mdx`

```mdx
---
supersigil:
  id: auth/req/login
  type: requirement
  status: approved
title: "User Login"
---

# User Login

As a user, I want to log in with my email and password so that I can
access my account securely.

<TrackedFiles paths="src/auth/**/*.rs" />

<AcceptanceCriteria>
  <Criterion id="valid-creds">
    WHEN a user submits a valid email and password pair,
    THE SYSTEM SHALL return a 200 response with a session token.
  </Criterion>
  <Criterion id="invalid-password">
    WHEN a user submits a valid email with an incorrect password,
    THE SYSTEM SHALL return a 401 response with an error message.
  </Criterion>
  <Criterion id="rate-limit">
    WHEN a user exceeds 5 failed login attempts within 15 minutes,
    THE SYSTEM SHALL block further attempts for that email and return 429.
  </Criterion>
</AcceptanceCriteria>
```

### `specs/auth/prop/token-generation.mdx`

```mdx
---
supersigil:
  id: auth/prop/token-generation
  type: property
  status: verified
---

# Session token generation is correct

For any valid email and password pair that matches a registered user,
the login endpoint returns a JWT containing the user's ID and an expiry
timestamp no more than 24 hours in the future.

<Validates refs="auth/req/login#valid-creds" />

<VerifiedBy
  strategy="file-glob"
  paths="tests/auth/login_test.rs, tests/auth/token_test.rs"
/>
```

### `specs/auth/design/login-flow.mdx`

```mdx
---
supersigil:
  id: auth/design/login-flow
  type: design
  status: approved
---

# Login Flow — Technical Design

<Implements refs="auth/req/login" />

## Architecture

The login flow uses a three-layer architecture: HTTP handler → auth
service → user repository. Password verification uses bcrypt with a
cost factor of 12. Session tokens are JWTs signed with RS256.

## Sequence

(mermaid diagram here)

## Design Decisions

1. **JWT over opaque tokens**: JWTs allow stateless verification at
   API gateway level, reducing database load for authenticated requests.

2. **bcrypt over argon2**: The deployment environment has limited memory,
   making argon2's memory-hard guarantees less effective. bcrypt with
   cost 12 provides adequate security for the threat model.
```

### Verification Output

Running `supersigil verify` against these three documents produces:

```
supersigil verify

  Parsed 3 spec documents (1 requirement, 1 property, 1 design)

  Refs:
    ✓ 2 cross-references resolved

  Coverage:
    auth/req/login ............................ 1/3 criteria covered
      ✗ criterion "invalid-password" has no validating property
      ✗ criterion "rate-limit" has no validating property

  Test mapping:
    auth/prop/token-generation ................ 2 files linked (existence only)
      ✓ tests/auth/login_test.rs
      ✓ tests/auth/token_test.rs

  Status:
    ✓ all statuses consistent

  Result: 2 errors, 0 warnings
```

## Design Decisions Log

### Components carry semantics, not document types

Document types (`requirement`, `property`, `design`) are classification
tags for humans and documentation tooling. Supersigil's verification
engine operates on the component graph: `<Criterion>`, `<Validates>`,
`<VerifiedBy>`, `<Implements>`. This means a single document can contain
any combination of components, and custom workflows can use document
types that supersigil has no built-in knowledge of.

**Implication:** A user who calls their documents "user stories" instead
of "requirements" simply uses `type: user-story` in front matter. As long
as those documents contain `<Criterion>` components, supersigil's coverage
checking works identically.

### Unidirectional references

Properties point at requirements (via `<Validates>`), not the reverse.
Designs point at requirements (via `<Implements>`), not the reverse.
Supersigil computes reverse mappings from the forward refs.

**Rationale:** Bidirectional references create a synchronization burden.
Adding a new property should never require editing the requirement it
validates. Unidirectional refs make the more abstract artifact (the
requirement) stable while the more concrete artifacts (properties,
designs) evolve around it.

### MDX for structured components in prose

MDX provides actual AST nodes for components (`MdxJsxFlowElement`),
eliminating the need for convention-based parsing of plain markdown.
Components degrade gracefully in non-MDX renderers (the content inside
them is still visible as text). And the ecosystem (Astro, Docusaurus,
Next.js) renders them as actual UI components.

**Tradeoff:** MDX parsing is more complex than plain markdown. The
`markdown` crate (markdown-rs) provides this in Rust, but the MDX
support adds weight. If a future user needs plain-markdown support,
a fallback parser could extract structured data from HTML comments
or fenced code blocks, but this is not planned for v1.

### String-only attributes with comma-separated lists

Supersigil rejects JSX expression attributes (`refs={[...]}`). All
attribute values must be plain string literals. Multi-value attributes
use comma-separated strings (`refs="a, b, c"`).

**Rationale:** JSX expression attributes require either evaluating
JavaScript (heavyweight, unsafe, non-deterministic) or parsing a subset
of JS (underspecified, brittle). Comma-separated strings are trivially
parseable, unambiguous, and work identically across every MDX parser.
The tradeoff is that commas are prohibited in IDs and paths — a
restriction that is enforced by lint and has no practical cost.

**Parsing rules:** The parser stores all attribute values as raw strings.
List splitting is performed by downstream consumers (graph building,
verification) using the component definitions in config, which declare
each attribute as string-typed or list-typed via `list = true`. For
list-typed attributes: split on `,`, trim whitespace from each item,
reject empty items (e.g., trailing comma produces an error). No escaping
mechanism — commas are simply not valid in IDs or file paths.

### TrackedFiles for code-to-doc routing

Supersigil's core verification checks the doc graph (refs, coverage,
test mappings). But the most common real-world problem is spec drift:
code changes but the spec that describes it is not updated. `<TrackedFiles>`
addresses this by declaring which source files a spec is concerned with.

**Design choice:** `<TrackedFiles>` is a routing signal, not a correctness
assertion. It says "if these files change, this spec should be reviewed" —
it does not say "this spec is correct if and only if these files have a
certain shape." This keeps the semantics simple and avoids false precision.

**Alternative considered:** Deriving file associations from test mappings
(if a test file changes, the property it verifies is potentially stale).
Rejected because tests and specs can address different aspects of the same
code, and test file changes don't always indicate spec drift.

### Tasks as components within documents, not individual documents

Task tracking is a common need in spec-driven development — every Kiro
spec has a `tasks.md` with ordered implementation steps. Supersigil
models tasks as `<Task>` components within a `type: tasks` document,
following the same pattern as `<Criterion>` components within a
requirement document. One tasks document per feature, containing all
its tasks.

`<Task>` components have `id`, `status`, `implements` (linking to
criteria), and `depends` (declaring ordering within the document).
Tasks can nest — a parent task contains sub-tasks, mirroring Kiro's
"2. Update data model → 2.1 Add field" structure without exploding
the document count.

**Rationale:** Making each task a separate document would produce 10–20
documents per feature, which is unwieldy. The component model keeps
task granularity inside the document boundary while still making tasks
individually referenceable via fragment syntax
(`auth/tasks/login#adapter-code`). Task *ordering* is verified by
supersigil (cycle detection, topological sort). Task *execution* is
the agent's responsibility — it reads the plan, picks up the next
task, edits the `status` attribute in the MDX, and commits.

**Two levels of dependency:** `depends` on `<Task>` handles ordering
within a tasks document (the common case). `<DependsOn>` at the
document level handles ordering between documents (rarer, e.g., one
design document depends on another).

### Configurable strictness for CI enforcement

Findings are split into two categories. **Hard errors** (broken refs,
duplicate IDs, missing required attributes, expression attributes,
dependency cycles) are structural integrity failures — always fatal,
never configurable.
**Configurable rules** have a built-in default severity that can be
overridden.

Three configurable levels of precedence, all using the same vocabulary
(`"off"`, `"warning"`, `"error"`), plus draft gating at the top:

1. **Draft gating** — `status: draft` suppresses to info (highest priority).
2. **Per-rule overrides** (`[verify.rules]`) — explicit per-rule config.
3. **Global strictness** (`[verify] strictness`) — sets the default.
4. **Built-in defaults** — lowest priority.

Unknown rule keys in `[verify.rules]` are config errors to catch typos.

**Rationale:** Hard errors are not negotiable — suppressing them invites
real problems. Making them unconfigurable eliminates the temptation.
For everything else, one vocabulary across all scopes means zero
translation overhead.

### Status-gating: draft documents are not blocked

Documents with `status: draft` have all configurable rules suppressed to
`info` level. Findings still appear in the output (as "would be error if
not draft") but don't fail the build. Hard errors are never suppressed.

**Rationale:** This solves the tension between strict defaults and
iterative authoring. `status: draft` is the mechanism that makes
strictness humane — you write the spec incrementally, and supersigil
tells you what's missing without blocking you. When you promote the
status, the full rule set applies. Status stops being just a label and
becomes the gate that controls rule application.

### Freeform IDs with optional validation

IDs are declared in front matter and are freeform strings. This is
resistant to AI agent hallucination (agents can use any string) while
remaining correctable (supersigil verify catches broken refs). An
optional `id_pattern` in config lets teams enforce conventions via
warnings.

**Alternative considered:** Deriving IDs from file paths. Rejected
because it couples identity to filesystem layout, making reorganization
a breaking change.

### Test discovery: hardcoded format, ecosystem plugins for depth

The v1 test mapping strategy is explicit file globs: the `<VerifiedBy>`
component declares which files contain relevant tests. This is
language-agnostic and requires no pattern matching.

Tag scanning uses a hardcoded format (`supersigil: {tag}`) that is not
configurable. A single universal convention avoids per-project
bikeshedding and makes tags greppable across any codebase.

For language-native test discovery (AST-level, not comment-level),
supersigil uses ecosystem plugins. The built-in Rust plugin uses `syn`
to find annotated test items and understands proptest (case counts,
counterexamples, regression files). Future plugins (via Extism WASM)
extend this to other languages. The plugin interface is designed in
Rust first and will be exposed through Extism when stabilized.

Test *execution* and pass/fail reporting is handled by consuming
existing test result formats (JUnit XML), not by running tests.
Supersigil is a verification tool, not a test runner.

### Advisory status, not enforced state machines

Statuses (`draft`, `approved`, `verified`, etc.) are informational.
Supersigil reports inconsistencies (e.g., `status: verified` with no
tests) but does not prevent status transitions. Enforcing state machines
in a CLI tool creates friction without proportional value.

### Rust with single-binary distribution

Supersigil is implemented in Rust for single-binary distribution, fast
filesystem traversal, and native MDX parsing via the `markdown` crate.
Pluggability is handled via external process hooks (stdin/stdout JSON),
avoiding the need for a plugin runtime.

## Migration from Kiro

`supersigil import --from kiro` reads `.kiro/specs/` directories and
converts Kiro's three-file spec format (requirements.md, design.md,
tasks.md) into supersigil MDX documents. This is a v1 feature — without
it, adoption from existing Kiro workflows requires manual rewriting.

**Conversion strategy:**

- EARS notation (`WHEN...THE SYSTEM SHALL...`) in requirements.md is
  mapped to `<Criterion>` components. Each acceptance criterion gets an
  auto-generated `id` derived from the section heading.
- `Validates: Requirements X.Y` strings in design.md properties are
  mapped to `<Validates refs="...">` with the corresponding criterion IDs.
- Mermaid diagrams, code blocks, and prose are preserved as-is.
- tasks.md is imported as a single `type: tasks` document containing
  `<Task>` components. Each top-level task becomes a `<Task>` with an
  auto-generated `id`. Sub-tasks (e.g., "2.1 Add field" under
  "2. Update data model") become nested `<Task>` components within
  their parent. Ordered task lists produce `depends` attributes
  pointing at the preceding sibling task. Kiro `Validates:` metadata
  on tasks maps to `implements` attributes where the criterion can be
  resolved; unresolvable references are left as TODO markers.

**Confidence and reviewability:**

Not all mappings are unambiguous. The importer adds `<!-- TODO(supersigil-import): ... -->`
comments at locations where conversion was uncertain — for example, when
a `Validates: Requirements 1.2` reference could not be unambiguously
resolved to a specific criterion. These markers make imported documents
trustworthy for immediate use while flagging what needs human review.

**CLI flags:**

- `--dry-run`: Preview the import without writing files. Outputs a
  conversion report: what would be created, what was mapped, what was
  ambiguous.
- `--output-dir <path>`: Where to write the converted documents
  (default: `specs/`).
- `--prefix <id-prefix>`: Prefix for generated document IDs (e.g.,
  `my-feature/` produces `my-feature/req/...`).

## Open Questions

- **LSP support**: A language server providing autocomplete on refs,
  go-to-definition, and diagnostics would significantly improve the
  authoring experience. This is a v2 concern but the parser architecture
  should be designed to support incremental re-parsing.

- **Watch mode**: `supersigil verify --watch` for continuous feedback
  during authoring. Requires file watching and incremental verification
  (re-verify only documents whose files or dependencies changed).

- **WASM plugins**: For verification rules that need more than
  stdin/stdout hooks, WASM plugins (via Extism or similar) could
  provide sandboxed, cross-language extensibility. Not planned for v1.

- **Spec generation**: Should `supersigil new` be purely structural
  (template files) or optionally agent-powered (call an LLM to scaffold
  from a prompt)? Likely both, with a `--scaffold` flag.

- **Executable examples**: Example scenarios embedded in spec documents
  (golden outputs, fixture data, input/output pairs) could be extracted
  and run as tests, analogous to `cargo test --doc`. This would require
  a new component (e.g., `<Example>`) with language and runner metadata,
  and an ecosystem plugin interface for execution. The verification
  chain would close fully: criterion → property → test *and* criterion →
  example → execution. Design questions include how to handle
  non-deterministic output, environment setup, and whether examples
  should be inline (in code blocks inside the component) or external
  (referenced by path).

## Implementation Specs

The implementation is split into five specs, ordered by dependency chain.
Each spec is testable in isolation — the graph can be verified without a
CLI, and CLI formatting can be tested without real verification results.

### Spec 1: Parser + Config (`supersigil-parser`, `supersigil-core/config`)

The foundation. Turns raw inputs into structured data.

- **MDX parsing**: Front matter extraction (BOM stripping, CRLF
  normalization, delimiter detection, YAML deserialization of the
  `supersigil:` namespace). MDX AST generation via `markdown-rs`.
  Component extraction from `MdxJsxFlowElement` nodes.
- **Attribute grammar**: String literal attributes only. All attribute
  values stored as raw strings by the parser. List splitting deferred
  to downstream consumers using component definitions from config.
  Expression attribute (`{...}`) detection and lint error emission.
- **Component extraction**: Recursive child collection (e.g., `<Criterion>`
  inside `<AcceptanceCriteria>`). Body text capture. Source position
  tracking.
- **Config parsing**: `supersigil.toml` deserialization. Single-project
  vs. multi-project mutual exclusivity. Document type definitions with
  status lists and `required_components`. Component definitions with
  attribute requirements. Verification rule severity overrides. Ecosystem
  plugin declarations. Hook configuration.
- **Output**: `ParseResult` values (`Document(SpecDocument)` or
  `NotSupersigil(path)`) and `Config` struct. No cross-document logic.

**Crates**: `supersigil-parser`, config module of `supersigil-core`.

### Spec 2: Document Graph (`supersigil-core/graph`)

Builds the cross-document data structure from parsed documents.

- **Indexing**: Document index by ID. Referenceable component index
  (Criterion id → document ID + component). Duplicate ID detection.
- **Ref resolution**: For every `refs` attribute, resolve target document
  and optional `#fragment`. Fragment type checking against
  `target_component` config. Cross-project resolution in multi-project
  workspaces. Isolated project scoping.
- **Cycle detection**: DAG validation for `<Task>` `depends` graphs
  (within each tasks document) and `<DependsOn>` ref graphs (between
  documents).
- **Topological sort**: Compute implementation order from dependency
  graphs. Used by `context` and `plan` output.
- **Reverse mappings**: Compute which documents validate, implement, or
  illustrate a given criterion or document.
- **Query logic**: `context` output generation (document + relationships +
  tasks in dependency order). `plan` output generation (outstanding
  criteria, pending tasks, completed work). Both as structured data,
  not formatted strings.

**Crates**: graph module of `supersigil-core`.

### Spec 3: Verification Engine (`supersigil-verify`)

Consumes the document graph and produces findings.

- **Dependency**: Spec 3 depends on Spec 2 (document graph). The
  verification engine consumes the graph's document index, ref resolution
  results, and TrackedFiles index. The `affected` command in particular
  needs the TrackedFiles component index from the graph to match globs
  against git diff output.
- **Coverage checking**: `uncovered_criterion` — every `<Criterion>` must
  have at least one `<Validates>` pointing at it.
- **Test mapping**: `missing_test_files` (file-glob existence),
  `zero_tag_matches` (tag scanning across comment styles),
  `unverified_validation` (Validates without VerifiedBy).
- **Tracked files**: `empty_tracked_glob` (glob matches zero files),
  `stale_tracked_files` (files changed since git ref via `git2`).
  `affected` command logic (TrackedFiles globs × git diff).
- **Status consistency**: `status_inconsistency` — declared status vs.
  observed verification state.
- **Structural rules**: `missing_required_component`,
  `invalid_id_pattern`, `isolated_document`, `orphan_test_tag`.
- **Severity resolution**: Four-level precedence (draft gating > per-rule
  override > global strictness > built-in default). Hard errors always
  fatal.
- **Hooks**: External process execution with stdin JSON, stdout capture,
  stderr capture, timeout enforcement, 64 KB truncation.
- **Report generation**: Structured report as data. Terminal, JSON, and
  Markdown formatters.
- **Test results**: Optional JUnit XML consumption for pass/fail per
  property.

**Crates**: `supersigil-verify`.

### Spec 4: CLI (`supersigil-cli`)

Thin layer over the libraries. Argument parsing and output formatting.

- **Argument parsing**: `clap` subcommands for `init`, `verify`, `lint`,
  `ls`, `status`, `context`, `plan`, `schema`, `graph`, `affected`,
  `new`, `import`.
- **Subcommand dispatch**: Wire CLI args to library calls. Pass config
  overrides (e.g., `--project`, `--format`, `--since`).
- **Output formatting**: Terminal output with ANSI colors. `NO_COLOR` and
  `FORCE_COLOR` environment variable support. `--color always|never|auto`.
  TTY detection for auto mode. Unicode symbols (✓, ✗, ⚠, ℹ) with ASCII
  fallback when not a TTY.
- **Exit codes**: 0 clean, 1 errors, 2 warnings-only.
- **Stderr/stdout discipline**: Stderr for diagnostics and progress,
  stdout for data.

**Crates**: `supersigil-cli`.

### Spec 5: Kiro Import (`supersigil import --from kiro`)

Migration tool for existing `.kiro/specs/` directories.

- **Discovery**: Find `.kiro/specs/*/` directories containing
  `requirements.md`, `design.md`, `tasks.md`.
- **Requirements conversion**: Parse EARS notation
  (`WHEN...THE SYSTEM SHALL...`) into `<Criterion>` components with
  auto-generated IDs. Wrap in `<AcceptanceCriteria>`.
- **Design conversion**: Parse `Validates: Requirements X.Y` strings into
  `<Validates refs="...">`. Preserve prose, mermaid diagrams, code blocks.
  Emit `<Implements>` for the parent requirement.
- **Tasks conversion**: Parse ordered task lists into `<Task>` components.
  Nested sub-tasks (e.g., "2.1 Add field") become nested `<Task>`.
  Sequential ordering produces `depends` attributes. `Validates:` metadata
  maps to `implements` where resolvable.
- **Ambiguity markers**: `<!-- TODO(supersigil-import): ... -->` at
  uncertain conversion points.
- **CLI flags**: `--dry-run`, `--output-dir`, `--prefix`.

**Crates**: `supersigil-import`. Separate crate — it has a unique
dependency (Kiro's markdown format parsing) that nothing else in
supersigil needs, and it's the kind of module you ship in v1 then
rarely touch again. Keeping it out of `supersigil-cli` avoids bloating
the CLI crate with migration-specific parsing logic.
