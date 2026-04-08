---
supersigil:
  id: config/req
  type: requirements
  status: implemented
title: "Config"
---

## Introduction

This spec recovers the current configuration behavior implemented in
`supersigil-core`. It covers TOML loading, single-project and multi-project
mode validation, document and component definition types, ecosystem and
test-result configuration, ID-pattern validation, and the shared
list-splitting utility used downstream by graph and verification code.

The config domain owns the typed model for `supersigil.toml`, but not every
runtime invariant is enforced at `load_config` time. Some invariants, such as
`verifiable` component-definition consistency, are validated when
`ComponentDefs` are merged for runtime use.

## Definitions

- **Config_Loader**: The `load_config` function in `supersigil-core`.
- **Single_Project_Mode**: Top-level `paths` plus optional top-level `tests`.
- **Multi_Project_Mode**: Named `[projects.<name>]` entries.
- **Project_Config**: One project entry with `paths`, optional `tests`, and
  optional `isolated`.
- **Component_Defs_Runtime_View**: The merged built-in plus user-defined
  component definitions produced by `ComponentDefs::merge`.
- **Rust_Plugin_Config**: The optional `[ecosystem.rust]` settings block.

## Requirement 1: TOML Loading and Mode Validation

As a repository maintainer, I want `supersigil.toml` loaded into a strict typed
model, so that invalid workspace shapes fail deterministically.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    WHEN a valid `supersigil.toml` file is provided, THE Config_Loader SHALL
    deserialize it into a `Config` value.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/src/config.rs" />
  </Criterion>
  <Criterion id="req-1-2">
    IF the TOML is malformed, or contains unknown keys at any configured level,
    THEN THE Config_Loader SHALL return config errors rather than ignoring the
    invalid input.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/tests/config_unit_tests.rs" />
  </Criterion>
  <Criterion id="req-1-3">
    THE Config_Loader SHALL accept either Single_Project_Mode or
    Multi_Project_Mode. IF `paths` is combined with `projects`, or `tests` is
    combined with `projects`, THEN loading SHALL report mutual-exclusivity
    errors. IF neither discovery mode is configured, THEN loading SHALL report
    a missing-required error.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/tests/config_unit_tests.rs" />
  </Criterion>
  <Criterion id="req-1-4">
    IN Single_Project_Mode, top-level `tests` SHALL default to an empty list
    when omitted. IN Multi_Project_Mode, each Project_Config SHALL require
    `paths`, SHALL default `tests` to an empty list, and SHALL default
    `isolated` to `false`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/tests/config_unit_tests.rs" />
  </Criterion>
  <Criterion id="req-1-5">
    A minimal config containing only `paths = [\"specs/**/*.md\"]` SHALL load
    successfully with defaults for the remaining optional sections.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/tests/config_unit_tests.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Document Types and Component Definitions

As a config consumer, I want document and component schemas expressed as typed
data, so that parsers, graph builders, and verification rules can share one
configuration model.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    THE Config_Loader SHALL deserialize document-type definitions with
    `status`, optional `required_components`, and optional `description`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/tests/component_defs_unit_tests.rs" />
  </Criterion>
  <Criterion id="req-2-2">
    THE config model SHALL deserialize user-defined component definitions with
    attribute schemas, `referenceable`, `verifiable`, `target_component`,
    `description`, and `examples`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/tests/config_unit_tests.rs" />
  </Criterion>
  <Criterion id="req-2-3">
    WHEN callers request the Component_Defs_Runtime_View, built-in components
    SHALL provide the default eight supersigil components, and user definitions
    SHALL override or extend them by name.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/src/component_defs.rs" />
  </Criterion>
  <Criterion id="req-2-4">
    THE Component_Defs_Runtime_View SHALL reject component definitions marked
    `verifiable = true` unless they are also referenceable and require an `id`
    attribute.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/tests/component_defs_unit_tests.rs" />
  </Criterion>
  <Criterion id="req-2-5">
    THE shared `split_list_attribute` utility SHALL split on commas, trim each
    item, and reject empty items produced by empty input, trailing commas,
    consecutive commas, or whitespace-only entries.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/tests/list_split_unit_tests.rs, crates/supersigil-core/tests/list_split_property_tests.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Verification, Ecosystem, Test-Result, and Documentation Settings

As a workspace maintainer, I want top-level configuration for verification and
integration behavior, so that one `Config` value can drive the rest of the
toolchain.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    THE Config_Loader SHALL parse `[verify]` strictness plus per-rule severity
    overrides. IF a configured rule name is unknown, THEN loading SHALL report
    `ConfigError::UnknownRule`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/tests/config_unit_tests.rs" />
  </Criterion>
  <Criterion id="req-3-2">
    THE ecosystem config SHALL default to `plugins = [\"rust\"]`. Explicit
    `plugins = []` SHALL disable all plugins, and unknown plugin names SHALL be
    rejected during config loading.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/tests/config_unit_tests.rs" />
  </Criterion>
  <Criterion id="req-3-3">
    Rust_Plugin_Config SHALL support `validation = \"off\" | \"dev\" | \"all\"`
    plus zero or more `project_scope` entries mapping manifest directory
    prefixes to supersigil project names.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/tests/config_unit_tests.rs" />
  </Criterion>
  <Criterion id="req-3-4">
    THE `test_results` config SHALL store configured formats and paths and
    SHALL default both lists to empty when omitted.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/tests/config_unit_tests.rs" />
  </Criterion>
  <Criterion id="req-3-5">
    THE Config_Loader SHALL parse an optional `[documentation.repository]`
    section with required `provider` (one of: github, gitlab, bitbucket, gitea)
    and `repo` fields, plus optional `host` and `main_branch` fields. Unknown
    provider values SHALL be rejected during config loading.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/tests/config_unit_tests.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 4: ID Pattern and Serialization Stability

As a config consumer, I want config values to survive validation and
round-tripping, so that generated config and serialized outputs remain stable.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-4-1">
    WHEN `id_pattern` is configured, THE Config_Loader SHALL validate that the
    pattern compiles as a regular expression. IF it does not compile, THEN
    loading SHALL report `ConfigError::InvalidIdPattern`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/tests/config_unit_tests.rs" />
  </Criterion>
  <Criterion id="req-4-2">
    `Config` and its nested types SHALL remain serializable and deserializable
    such that valid config values round-trip through TOML without semantic
    loss.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/tests/config_unit_tests.rs" />
  </Criterion>
</AcceptanceCriteria>
```
