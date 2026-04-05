---
supersigil:
  id: executable-examples/req
  type: requirements
  status: implemented
title: Executable Examples
---

## Introduction

Specification documents can embed runnable code samples that execute as
part of `supersigil verify`. Each example targets a runner, captures
its output, and optionally compares it against an inline golden
expectation. Passing examples with `verifies` contribute verification
evidence to the spec graph, closing the chain from criterion to live
execution.

Supersigil orchestrates example execution but delegates compilation,
interpretation, and service lifecycle to external tools.

### Scope

**In scope:** `<Example>` and `<Expected>` components, runner dispatch,
output matching (JSON with wildcards, text, regex, snapshot), evidence
integration, the `supersigil examples` discovery command, and
configuration surface.

**Out of scope:** service fixture management (start/stop/health-check),
execution sandboxing beyond temp directories, and IDE/editor
integration.

### Cross-cutting notes

The two-phase verify pipeline described in Requirement 4 implies a
future amendment to `verification-engine/req` (currently assumes the
`ArtifactGraph` is fully built before `verify()` is called and that
coverage runs as a single-pass rule). Both specs are draft; the
verification-engine contract will be updated when this feature is
implemented.

The code block extraction and snapshot rewrite capabilities described
in Requirements 1 and 3 imply a future amendment to
`parser-pipeline/req`. The current parser data model
(`ExtractedComponent.body_text`) does not preserve fenced code block
content or byte spans needed for rewriting. The parser amendment is
part of this feature's implementation scope.

## Definitions

- **Example**: An `<Example>` component in a spec document with code
  content (inline text or linked via `supersigil-ref`) and an optional
  `<Expected>` child (at most one).
- **Runner**: An execution strategy for an example's code block. Built-in
  runners may use native implementations; user-defined runners are shell
  command templates.
- **Golden Output**: The content of an `<Expected>` block that the
  example's actual output is compared against.
- **Matching Mode**: The comparison strategy for golden output: `json`,
  `text`, `regex`, or `snapshot`.
- **Evidence**: A `VerificationEvidenceRecord` produced at runtime by a
  passing example whose `verifies` attribute targets criteria. Evidence
  is distinct from the authored `verifies` refs themselves, which are
  static graph metadata resolved at parse time.

## Requirement 1: Example and Expected Components

As a spec author, I want to embed runnable code samples with optional
expected output directly in my spec documents, so that examples are
co-located with the criteria they verify.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    THE parser SHALL recognize `Example` as a built-in component with
    required attributes `id` and `runner`, and optional attributes
    `lang`, `verifies`, `references`, `timeout`, `env`, and `setup`.
    IF `lang` is omitted and the code source is an external code fence
    linked via `supersigil-ref`, the executor SHALL derive it from the
    fence language. IF `lang` is omitted and the code source is inline
    text, it is a lint error.
  </Criterion>

  <Criterion id="req-1-2">
    THE parser SHALL recognize `Expected` as a built-in component with
    optional attributes `status`, `format`, and `contains`, and SHALL
    require that `Expected` appears only as a direct child of `Example`.
    An `Example` SHALL have at most one `Expected` child; the fragment
    ID `expected` is reserved for this implicit reference target.
  </Criterion>

  <Criterion id="req-1-3">
    THE `Example` component SHALL be referenceable by `id`, so that
    other documents can reference specific examples. `Example` SHALL NOT
    be marked verifiable — it is an evidence source, not a verification
    target.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/src/component_defs.rs" />
  </Criterion>

  <Criterion id="req-1-4">
    THE `verifies` attribute on `Example` SHALL contain refs that target
    verifiable components (criteria). These refs SHALL be validated at
    graph-build time using the same resolution rules as other
    cross-document refs. At runtime, passing examples with `verifies`
    SHALL produce verification evidence records (see req-4-2).
  </Criterion>

  <Criterion id="req-1-5">
    WHEN `references` is set on an `Example`, the graph SHALL create
    informational `references` edges only, with no verification
    semantics.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/src/graph/resolve.rs, crates/supersigil-core/src/graph/reverse.rs, crates/supersigil-core/src/graph/tests/unit.rs" />
  </Criterion>

  <Criterion id="req-1-6">
    IF `Expected` is absent from an `Example`, THEN verification SHALL
    treat exit code 0 as pass and non-zero as fail, with no output
    matching.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/examples/runner.rs" />
  </Criterion>

  <Criterion id="req-1-7">
    IF `format` is omitted from `Expected`, it SHALL default to `text`.
    The `status`, `contains`, and body-format checks on `Expected` SHALL
    be conjunctive: all specified checks must pass for the example to
    pass. An `Expected` with only attributes and no body SHALL be valid,
    applying only the attribute-based checks.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/examples/matcher.rs" />
  </Criterion>

  <Criterion id="req-1-8">
    THE parser SHALL extract code content from `Example` and `Expected`
    components and preserve it in the data model. Code content is either
    inline text within the XML element or a Markdown code fence linked
    via `supersigil-ref` in the fence info string. The `supersigil-ref`
    value is whitespace-delimited, uses `#` as the fragment separator,
    and resolves document-locally. A `supersigil-ref` targeting no
    component in the document SHALL be a lint error. For `Expected` with
    `format="snapshot"`, the parser SHALL also preserve byte spans
    sufficient to rewrite the code content in the source file.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/examples/executor.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Runner Dispatch

As a spec author, I want supersigil to dispatch example execution to
runners, so that I can use any language or test framework without
supersigil needing to understand them.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    FOR subprocess-based runners, the example executor SHALL write the
    example code block to a temp file in a fresh temp directory, invoke
    the runner command as a subprocess, and capture stdout, stderr, and
    exit code. Output matching SHALL operate on stdout only; stderr
    SHALL be captured for diagnostic reporting on failure.
  </Criterion>

  <Criterion id="req-2-2">
    THE runner SHALL be resolved from built-in runner definitions or
    from `[examples.runners]` in `supersigil.toml`, with user-defined
    runners taking precedence over built-ins of the same name.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/examples/runner.rs" />
  </Criterion>

  <Criterion id="req-2-3">
    THE built-in runner set SHALL include `cargo-test`, `http`, and
    `sh`. Built-in runners MAY use native implementations rather than
    subprocess dispatch. User-defined runners are always shell command
    templates. Runner-specific contracts are design-level concerns.
  </Criterion>

  <Criterion id="req-2-4">
    WHEN `timeout` is set on an `Example` (or falls back to the global
    `[examples].timeout` default), the executor SHALL enforce the
    timeout and report the example as failed with a timeout diagnostic.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/examples/executor.rs" />
  </Criterion>

  <Criterion id="req-2-5">
    WHEN `env` is set on an `Example`, the executor SHALL make those
    key-value pairs available to the runner.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/examples/runner.rs" />
  </Criterion>

  <Criterion id="req-2-6">
    WHEN `setup` is set on an `Example`, the executor SHALL run the
    setup script in the temp directory before invoking the runner, and
    SHALL fail the example if the setup script exits non-zero.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Output Matching

As a spec author, I want to declare expected output inline in my specs
and have supersigil compare it against actual runner output, so that
my specs prove themselves.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    WHEN `format="json"` on `Expected`, the matcher SHALL perform deep
    JSON comparison against stdout, treating wildcard placeholders
    `&lt;any-string&gt;`, `&lt;any-number&gt;`, `&lt;any-uuid&gt;`, and `&lt;any-iso8601&gt;`
    as matching any value of the corresponding shape.
  </Criterion>

  <Criterion id="req-3-2">
    WHEN `format="text"` on `Expected`, the matcher SHALL compare
    trimmed stdout against the trimmed expected text exactly.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/examples/matcher.rs" />
  </Criterion>

  <Criterion id="req-3-3">
    WHEN `format="regex"` on `Expected`, the matcher SHALL compile the
    expected text as a regex and match it against the full stdout.
  </Criterion>

  <Criterion id="req-3-4">
    WHEN `format="snapshot"` on `Expected`, the matcher SHALL compare
    stdout against the inline expected text, and `--update-snapshots`
    on `supersigil verify` SHALL rewrite the `Expected` code content
    in the source file to the actual output.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-cli/src/commands/verify.rs" />
  </Criterion>

  <Criterion id="req-3-5">
    WHEN `status` is set on `Expected`, the matcher SHALL compare the
    runner exit code (or HTTP status code for the `http` runner) against
    the expected status, and SHALL fail the example if they differ.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/examples/matcher.rs" />
  </Criterion>

  <Criterion id="req-3-6">
    WHEN `contains` is set on `Expected`, the matcher SHALL check that
    stdout contains the specified substring.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/examples/matcher.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 4: Verify Integration

As a project maintainer, I want examples to run as part of
`supersigil verify`, so that spec drift is caught in the same CI step
as structural verification.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-4-1">
    THE `supersigil verify` command SHALL run in two phases: first
    structural verification (graph rules, lint, plugin evidence), then
    example execution. Example results SHALL be included in the final
    `VerificationReport`.
  </Criterion>

  <Criterion id="req-4-2">
    WHEN an example with `verifies` passes, the executor SHALL produce
    a `VerificationEvidenceRecord` with evidence kind `example` that is
    merged into the `ArtifactGraph` after example execution completes.
    Coverage evaluation SHALL run after this merge so that example
    evidence can satisfy criterion coverage.
  </Criterion>

  <Criterion id="req-4-3">
    WHEN an example with `verifies` fails, the verification report
    SHALL include a finding with the example id, runner, actual vs
    expected output diff, and the criteria it was meant to verify.
  </Criterion>

  <Criterion id="req-4-4">
    IF structural verification produces errors, THEN example execution
    SHALL be skipped entirely. Coverage SHALL still be evaluated against
    the pre-example `ArtifactGraph`, and the report SHALL note that
    examples were not run.
  </Criterion>

  <Criterion id="req-4-5">
    THE `supersigil verify` command SHALL support `--skip-examples` to
    run structural verification without example execution, and
    `--update-snapshots` to update inline snapshot expectations.
  </Criterion>

  <Criterion id="req-4-6">
    Post-verify hooks SHALL run after structural, example, and coverage
    findings are assembled into an interim report. Hooks SHALL receive
    the interim report JSON on stdin and their emitted findings SHALL be
    merged to produce the final report, preserving the existing hook
    contract from `verification-engine/req` (req-7-3).
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-cli/src/commands/verify.rs" />
  </Criterion>

  <Criterion id="req-4-7">
    WHEN `supersigil verify` writes terminal output to a TTY while
    examples are executing, it SHALL render live per-example progress
    with a spinner for running examples and an aggregate completion
    count. WHEN terminal output is not a TTY, it SHALL emit append-only
    progress lines instead of live redraws.
  </Criterion>

  <Criterion id="req-4-8">
    Example findings SHALL participate in the existing severity
    resolution chain: draft gating, per-rule overrides, global
    strictness, and built-in defaults. WHEN draft gating or
    configuration downgrades findings from error or warning to info,
    the terminal output SHALL include a hint stating the number of
    downgraded findings and the reason, so that users are not surprised
    by a clean exit status despite visible example failures.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 5: Examples Discovery Command

As a spec author, I want a `supersigil examples` command that lists
examples with context-aware scoping, so that I can discover which
examples exist and what they verify.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-5-1">
    THE `supersigil examples` command SHALL list all `Example` components
    with their id, lang, runner, verifies/references targets, and
    containing document.
  </Criterion>

  <Criterion id="req-5-2">
    WHEN invoked without `--all`, the command SHALL scope results to
    examples in documents related to the current working directory via
    `TrackedFiles`, following the same context-scoping logic as
    `supersigil refs`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-cli/src/commands/examples.rs" />
  </Criterion>

  <Criterion id="req-5-3">
    THE command SHALL support `--format terminal` (default) and
    `--format json` output modes.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 6: Configuration

As a project maintainer, I want to configure example execution defaults
and custom runners in `supersigil.toml`, so that the feature adapts to
my project's tooling.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-6-1">
    THE config model SHALL expose `[examples]` with optional `timeout`
    (default 30s) and `parallelism` (default: half of available CPU
    threads, minimum 1) settings.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/src/config.rs" />
  </Criterion>

  <Criterion id="req-6-2">
    THE config model SHALL expose `[examples.runners.&lt;name&gt;]` with a
    `command` template string, allowing user-defined runners that
    override or extend the built-in set.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/examples/runner.rs" />
  </Criterion>

  <Criterion id="req-6-3">
    RUNNER command templates SHALL support `{file}`, `{dir}`, `{lang}`,
    and `{name}` placeholders interpolated by the executor. `{lang}`
    SHALL use the explicit `lang` attribute when present and the
    derived code-block language otherwise.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/examples/runner.rs" />
  </Criterion>

  <Criterion id="req-6-4">
    THE executor SHALL run independent examples concurrently up to the
    configured `parallelism` limit. Report output order SHALL be stable
    (sorted by document id then example id) regardless of execution
    order. Snapshot rewrites SHALL be serialized to avoid concurrent
    writes to the same source file.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/examples/executor.rs" />
  </Criterion>

  <Criterion id="req-6-5">
    THE `supersigil verify` command SHALL accept `-j, --parallelism &lt;N&gt;`
    to override the configured example parallelism for that invocation.
    The precedence SHALL be: CLI flag &gt; `supersigil.toml` config &gt;
    default. WHEN the flag is omitted, the config or default value
    SHALL apply.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-cli/src/commands.rs, crates/supersigil-cli/src/commands/verify.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Self-Verification

> **Note:** These examples are in the current MDX authoring syntax. They
> will be converted to the Markdown + `supersigil-xml` format
> (see `document-format/adr`) as part of the format migration.

These examples exercise the feature against a fixture project in
`tests/fixtures/example-project`.

```sh supersigil-ref=fixture-lint
cd fixture
cargo run -q -p supersigil-cli -- lint
```

```supersigil-xml
<Example
  id="fixture-lint"
  lang="sh"
  runner="sh"
  setup="tests/fixtures/example-project/setup-workspace-links.sh"
  verifies="executable-examples/req#req-1-1, executable-examples/req#req-1-2"
>
<Expected status="0" contains="no errors" />
</Example>
```

```sh supersigil-ref=fixture-examples-json
cd fixture
cargo run -q -p supersigil-cli -- examples --format json
```

```json supersigil-ref=fixture-examples-json#expected
[
  {
    "doc_id": "demo/req",
    "example_id": "echo-test",
    "lang": "sh",
    "runner": "sh",
    "has_expected": true,
    "verifies": ["demo/req#demo-1"]
  },
  {
    "doc_id": "demo/req",
    "example_id": "rust-test",
    "lang": "rust",
    "runner": "cargo-test",
    "has_expected": true,
    "verifies": ["demo/req#demo-1"]
  }
]
```

```supersigil-xml
<Example
  id="fixture-examples-json"
  lang="sh"
  runner="sh"
  setup="tests/fixtures/example-project/setup-workspace-links.sh"
  verifies="executable-examples/req#req-5-1, executable-examples/req#req-5-3"
>
<Expected status="0" format="json" />
</Example>
```

```rust supersigil-ref=fixture-cargo-test-examples-json
use std::process::Command;

#[test]
fn fixture_examples_json_from_rust() {
    let output = Command::new("cargo")
        .args(["run", "-q", "-p", "supersigil-cli", "--", "examples", "--format", "json"])
        .current_dir("fixture")
        .output()
        .expect("run supersigil examples");

    assert!(
        output.status.success(),
        "examples command failed: {:?}",
        output.status,
    );

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    println!("{stdout}");
    assert!(
        stdout.contains("\"example_id\": \"echo-test\"")
            && stdout.contains("\"example_id\": \"rust-test\"")
            && stdout.contains("\"lang\": \"rust\""),
        "stdout did not include fixture example metadata: {stdout}",
    );
}
```

```supersigil-xml
<Example
  id="fixture-cargo-test-examples-json"
  lang="rust"
  runner="cargo-test"
  timeout="120"
  setup="tests/fixtures/example-project/setup-workspace-links.sh"
  verifies="executable-examples/req#req-2-3, executable-examples/req#req-2-6, executable-examples/req#req-5-1, executable-examples/req#req-5-3"
>
<Expected status="0" contains="rust-test" />
</Example>
```

```rust supersigil-ref=fixture-cargo-test-verify
use std::process::Command;

#[test]
fn fixture_verify_from_rust() {
    let output = Command::new("cargo")
        .args(["run", "-q", "-p", "supersigil-cli", "--", "verify", "--format", "json"])
        .current_dir("fixture")
        .output()
        .expect("run supersigil verify");

    assert!(
        output.status.success(),
        "verify command failed: {:?}",
        output.status,
    );

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    println!("{stdout}");
    assert!(
        stdout.contains("\"evidence_summary\""),
        "stdout did not include evidence summary: {stdout}",
    );
}
```

```supersigil-xml
<Example
  id="fixture-cargo-test-verify"
  lang="rust"
  runner="cargo-test"
  timeout="120"
  setup="tests/fixtures/example-project/setup-workspace-links.sh"
  verifies="executable-examples/req#req-2-3, executable-examples/req#req-2-6, executable-examples/req#req-4-1"
>
<Expected status="0" contains="evidence_summary" />
</Example>
```

```sh supersigil-ref=fixture-verify-terminal
cd fixture
cargo run -q -p supersigil-cli -- verify
```

```regex supersigil-ref=fixture-verify-terminal#expected
(?s).*Executing 2 examples:.*demo/req::echo-test.*demo/req::rust-test.*Examples: 2 passed.*
```

```supersigil-xml
<Example
  id="fixture-verify-terminal"
  lang="sh"
  runner="sh"
  setup="tests/fixtures/example-project/setup-workspace-links.sh"
  verifies="executable-examples/req#req-3-3, executable-examples/req#req-4-1"
>
<Expected status="0" format="regex" />
</Example>
```

```sh supersigil-ref=fixture-verify
cd fixture
cargo run -q -p supersigil-cli -- verify --format json
```

```supersigil-xml
<Example
  id="fixture-verify"
  lang="sh"
  runner="sh"
  timeout="120"
  setup="tests/fixtures/example-project/setup-workspace-links.sh"
  verifies="executable-examples/req#req-4-1"
>
<Expected status="0" contains="evidence_summary" />
</Example>
```
