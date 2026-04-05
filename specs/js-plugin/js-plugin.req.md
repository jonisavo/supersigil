---
supersigil:
  id: js-plugin/req
  type: requirements
  status: implemented
title: "JavaScript/TypeScript Ecosystem Plugin"
---

## Introduction

This spec covers the JavaScript/TypeScript ecosystem plugin for Supersigil,
enabling JS/TS test files to provide verification evidence for spec criteria.

The plugin has three deliverables:

- A Rust crate (`supersigil-js`) that discovers evidence from JS/TS test files
  using oxc-based AST parsing, implementing the `EcosystemPlugin` trait
- An npm package (`@supersigil/vitest`) providing a `verifies()` helper function
  for annotating Vitest tests with criterion refs
- An ESLint plugin (`@supersigil/eslint-plugin`) that statically validates
  criterion ref strings by invoking `supersigil refs`, compatible with
  ESLint v9+ and best-effort with oxlint

Out of scope for this pass:

- Test execution or pass/fail status collection
- Support for test runners other than Vitest (Jest, Playwright, etc.)
- A general-purpose JS SDK for Supersigil spec parsing

## Definitions

- **JS_Evidence_Annotation**: A criterion ref string appearing in a recognized
  annotation pattern within a JS/TS test file. The two recognized forms are the
  `verifies()` call expression and the raw `meta: { verifies: [...] }` object
  in Vitest test options.
- **JS_Record**: One normalized `VerificationEvidenceRecord` produced by the
  JS plugin from a discovered JS_Evidence_Annotation.
- **Criterion_Ref**: A string in the form `{doc_id}#{criterion_id}` that
  identifies a verifiable criterion in the spec graph.

## Requirement 1: AST-Based Evidence Discovery

As a developer using JS/TS tests as verification evidence, I want the plugin to
discover `verifies()` annotations from test source files without requiring a
JS runtime, so that `supersigil verify` works with zero Node.js dependency.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    THE `JsPlugin` SHALL parse JS/TS test files using the oxc parser and SHALL
    extract JS_Evidence_Annotations from the AST.
  </Criterion>
  <Criterion id="req-1-2">
    THE plugin SHALL recognize `verifies()` call expressions where the callee
    is a direct `verifies` identifier, and SHALL extract all string literal
    arguments as Criterion_Refs.
  </Criterion>
  <Criterion id="req-1-3">
    THE plugin SHALL recognize the raw `{ meta: { verifies: [...] } }` object
    literal form in `test` and `it` call second arguments, and SHALL extract
    all string literal elements from the `verifies` array as Criterion_Refs.
  </Criterion>
  <Criterion id="req-1-4">
    THE plugin SHALL recognize spread expressions containing `verifies()` calls
    within test option objects (e.g. `{ ...verifies('ref'), timeout: 5000 }`).
  </Criterion>
  <Criterion id="req-1-5">
    THE plugin SHALL resolve test names from the first string argument of
    `test` and `it` calls, and SHALL track `describe` nesting to produce full
    test names in the form `"describe > test"`.
  </Criterion>
  <Criterion id="req-1-6">
    THE plugin SHALL scan only files matching the configured test patterns
    and SHALL ignore non-matching files.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Evidence Normalization

As the verification pipeline, I want discovered JS/TS evidence normalized into
shared records, so that it can merge with Rust and explicit evidence
consistently.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    FOR EACH discovered JS_Evidence_Annotation, THE plugin SHALL emit one
    JS_Record containing the test name, file path, source location of the
    annotation, test kind, JS verifies provenance, and one or more resolved
    verifiable targets.
  </Criterion>
  <Criterion id="req-2-2">
    WHEN a `verifies()` call or `meta.verifies` array contains multiple
    Criterion_Refs, THE plugin SHALL normalize all of them into the JS_Record
    target set.
  </Criterion>
  <Criterion id="req-2-3">
    WHEN a `verifies()` call contains a non-string-literal argument (variable,
    template literal, or expression), THE plugin SHALL emit a recoverable
    diagnostic warning for that argument and SHALL NOT include it in the
    JS_Record target set.
  </Criterion>
  <Criterion id="req-2-4">
    WHEN the annotation contains a Criterion_Ref that does not match the
    `{doc_id}#{criterion_id}` format, THE plugin SHALL reject that annotation
    with `PluginError::Discovery`.
  </Criterion>
  <Criterion id="req-2-5">
    WHEN all arguments in a `verifies()` call or all elements in a
    `meta.verifies` array are non-string-literals, THE plugin SHALL drop the
    record entirely and SHALL emit only a diagnostic. It SHALL NOT produce a
    JS_Record with an empty target set.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: File Discovery and Filtering

As a workspace maintainer, I want the plugin to discover test files predictably
using configured patterns and standard exclusion mechanisms.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    THE plugin SHALL use configurable glob patterns from `ecosystem.js.test_patterns`
    to discover test files, defaulting to
    `["**/*.test.{ts,tsx,js,jsx}", "**/*.spec.{ts,tsx,js,jsx}"]`.
  </Criterion>
  <Criterion id="req-3-2">
    THE plugin SHALL respect `.gitignore` files when discovering test files,
    so that `node_modules`, `dist`, build output, and other ignored paths are
    excluded automatically.
  </Criterion>
  <Criterion id="req-3-3">
    IF the discovery scope contains no matching test files, THEN the plugin
    SHALL succeed with an empty evidence set.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 4: Discovery Fault Tolerance

As an operator running verification across real JS/TS trees, I want discovery to
continue where possible, so that one bad file does not hide all other evidence.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-4-1">
    IF an individual JS/TS file cannot be parsed (syntax error), THEN the
    plugin SHALL skip that file, SHALL continue processing the rest of the
    discovery scope, and SHALL return a non-fatal plugin diagnostic for that
    file.
  </Criterion>
  <Criterion id="req-4-2">
    IF the discovery scope contains test files but zero JS_Evidence_Annotations,
    THEN `JsPlugin::discover` SHALL succeed with an empty evidence set rather
    than treating this as an error.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 5: Vitest Helper Package

As a JS/TS developer, I want a lightweight helper function for annotating tests,
so that I have a clean API surface that is easy to use and hard to mistype.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-5-1">
    THE `@supersigil/vitest` package SHALL export a `verifies()` function that
    accepts one or more Criterion_Ref strings and returns an object of the
    shape `{ meta: { verifies: string[] } }`. The package targets Vitest >= 4.1
    which introduced declarative `meta` in test options.
  </Criterion>
  <Criterion id="req-5-2">
    THE returned object SHALL be spreadable into a Vitest test options object
    (e.g. `{ ...verifies('ref'), timeout: 5000 }`) and SHALL compose with
    other Vitest test options.
  </Criterion>
  <Criterion id="req-5-3">
    THE `@supersigil/vitest` package SHALL have zero runtime dependencies.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 6: ESLint Plugin for Static Validation

As a developer, I want criterion refs validated in my editor at lint time, so
that bad refs are caught before running `supersigil verify`.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-6-1">
    THE `@supersigil/eslint-plugin` SHALL expose a `valid-criterion-ref` rule
    that validates Criterion_Ref strings in `verifies()` calls and
    `meta.verifies` arrays against the project's spec documents.
  </Criterion>
  <Criterion id="req-6-2">
    THE rule SHALL obtain valid criterion refs by invoking
    `supersigil refs --all --format json` and caching the result for the lint
    session. The `--all` flag ensures the full ref set is returned regardless
    of the working directory. This avoids duplicating spec parsing logic in
    JavaScript.
  </Criterion>
  <Criterion id="req-6-3">
    THE rule SHALL report distinct error messages for malformed refs (missing
    `#`), unknown document IDs, and unknown criterion IDs within a known
    document.
  </Criterion>
  <Criterion id="req-6-4">
    THE rule SHALL be compatible with ESLint v9+ flat config. Compatibility
    with oxlint's JS plugin system is best-effort given its alpha status.
  </Criterion>
  <Criterion id="req-6-5">
    IF the `supersigil` binary is not available or `supersigil refs` fails,
    THE rule SHALL emit a warning and disable itself rather than failing the
    lint run.
  </Criterion>
</AcceptanceCriteria>
```
