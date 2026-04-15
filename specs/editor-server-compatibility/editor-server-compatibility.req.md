---
supersigil:
  id: editor-server-compatibility/req
  type: requirements
  status: implemented
title: "Editor/Server Compatibility"
---

## Introduction

Selective releases break the old assumption that the editor package version and
the `supersigil-lsp` package version always move together. Exact package-version
matching is therefore no longer the right compatibility check. What the editors
actually need is one small, explicit contract that says whether they can safely
talk to the running server.

This spec defines that contract for both VS Code and IntelliJ. The goal is not
negotiation or range handling. The goal is a simple runtime answer: compatible
or incompatible.

This replaces the old VS Code-only version-mismatch warning behavior.

### Scope

- **In scope:** one shared compatibility-info query exposed by the
  `supersigil-lsp` binary, one editor-supported compatibility constant per
  editor, startup-time compatibility checks in VS Code and IntelliJ before an
  LSP session is started, and hard-stop behavior on incompatibility.
- **Out of scope:** automatic updates, semver range negotiation, partial
  compatibility modes, and package-version lockstep between editors and crates.

## Definitions

- **Compatibility_Version**: A small shared version token that represents the
  editor/server protocol contract. It is separate from package versions.
- **Compatibility_Info_Query**: The `supersigil-lsp --compatibility-info`
  process invocation that returns compatibility metadata as JSON and exits.
- **Compatible_Session**: An editor/server session where the editor-supported
  Compatibility_Version matches the server-reported Compatibility_Version.
- **Incompatible_Session**: An editor/server session where the versions differ,
  the version is missing, or the compatibility-info query fails.

## Requirement 1: Server Compatibility Reporting

As an editor client, I need the server to report one compatibility version in a
shared format, so that both editors can make the same startup decision.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    THE `supersigil-lsp` binary SHALL expose compatibility metadata via
    `supersigil-lsp --compatibility-info`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/main.rs" />
  </Criterion>
  <Criterion id="req-1-2">
    THE compatibility-info response SHALL include one Compatibility_Version
    separate from the server package version.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/main.rs" />
  </Criterion>
  <Criterion id="req-1-3">
    THE compatibility-info surface SHALL remain reachable from both VS Code and
    IntelliJ before an LSP session is started.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/main.rs, editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilLspServerSupportProvider.kt, editors/vscode/src/extension.ts" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Editor Startup Check

As a user, I want each editor to verify compatibility at startup, so that an
unsupported server is rejected before it causes subtle behavior differences.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    BEFORE starting a VS Code LanguageClient, THE extension SHALL run the
    compatibility-info query against the resolved `supersigil-lsp` binary and
    compare the reported Compatibility_Version to the extension's supported
    Compatibility_Version.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/extension.ts, editors/vscode/src/version.ts" />
  </Criterion>
  <Criterion id="req-2-2">
    BEFORE ensuring an IntelliJ LSP server is started, THE plugin SHALL run the
    compatibility-info query against the resolved `supersigil-lsp` binary and
    compare the reported Compatibility_Version to the plugin's supported
    Compatibility_Version.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilLspServerSupportProvider.kt, editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilProjectUtil.kt" />
  </Criterion>
  <Criterion id="req-2-3">
    IF the compatibility-info query fails or the server does not report a
    Compatibility_Version, THE editor SHALL treat the session as incompatible.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/extension.ts, editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilLspServerSupportProvider.kt" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Incompatibility Handling

As a user, I want incompatible editor/server sessions to fail loudly and early,
so that I know I need to update one side instead of working against a broken
pairing.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    WHEN an Incompatible_Session is detected, THE editor SHALL refuse to start
    that session rather than continuing with degraded behavior.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/extension.ts, editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilLspServerSupportProvider.kt" />
  </Criterion>
  <Criterion id="req-3-2">
    WHEN VS Code detects an Incompatible_Session, THE extension SHALL show an
    actionable error message that includes the supported and reported
    Compatibility_Versions and offers a path to update the extension.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/extension.ts" />
  </Criterion>
  <Criterion id="req-3-3">
    WHEN IntelliJ detects an Incompatible_Session, THE plugin SHALL show an
    actionable notification that includes the supported and reported
    Compatibility_Versions and offers a path to update either the plugin or the
    server installation.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilNotifications.kt" />
  </Criterion>
  <Criterion id="req-3-4">
    BOTH editors SHALL log the supported and reported Compatibility_Versions
    when incompatibility is detected.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/extension.ts, editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilLspServerSupportProvider.kt" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 4: Simple Compatibility Contract

As a maintainer, I want the compatibility contract to stay simple, so that it
does not grow into a second release-management system.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-4-1">
    THE Compatibility_Version SHALL be one shared constant, such as a single
    integer or short string, rather than a semver range or negotiated matrix.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/main.rs, editors/vscode/src/version.ts, editors/intellij/src/main/kotlin/org/supersigil/intellij/CompatibilityCheck.kt" />
  </Criterion>
  <Criterion id="req-4-2">
    Changing an editor package version or the server package version SHALL NOT
    by itself require changing the Compatibility_Version.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/main.rs, editors/vscode/src/version.ts, editors/intellij/src/main/kotlin/org/supersigil/intellij/CompatibilityCheck.kt" />
  </Criterion>
  <Criterion id="req-4-3">
    Changes that break the custom request/response contract or other
    editor-visible protocol behavior SHALL require bumping the
    Compatibility_Version in the server and both editors before release.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/main.rs, editors/vscode/src/version.ts, editors/intellij/src/main/kotlin/org/supersigil/intellij/CompatibilityCheck.kt" />
  </Criterion>
</AcceptanceCriteria>
```
