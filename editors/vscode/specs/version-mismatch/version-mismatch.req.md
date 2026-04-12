---
supersigil:
  id: version-mismatch/req
  type: requirements
  status: implemented
title: "Version Mismatch Detection"
---

## Introduction

Detect when the VS Code extension version and the LSP server binary
version differ, and warn the user. Today the extension has no awareness
of what version of the server it started; mismatches can cause subtle
issues that are hard to diagnose.

Scope: LSP server reports its version via `ServerInfo`, the extension
compares it to its own `package.json` version after client startup,
and shows a one-time warning dialog when they differ.

Out of scope: automatic server updates, pinned compatibility ranges,
IntelliJ plugin version checking.

## Requirement 1: Server Version Reporting

As an editor extension, I need the LSP server to report its version
during initialization, so that the client can detect mismatches.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    THE LSP server SHALL populate the `ServerInfo` field in its
    `InitializeResult` with name `"supersigil-lsp"` and version set
    to the crate's `CARGO_PKG_VERSION` (compile-time constant).
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Version Comparison

As a spec author, I want the extension to compare its version against
the server's version after startup, so that mismatches are caught.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    AFTER a LanguageClient starts successfully, THE extension SHALL
    read the server version from `client.initializeResult.serverInfo`
    and compare it to the extension's own version from
    `vscode.extensions.getExtension`. Any difference SHALL be treated
    as a mismatch.
  </Criterion>
  <Criterion id="req-2-2">
    IF the server does not provide a version in `ServerInfo`, THE
    extension SHALL skip the comparison silently (no warning, no error).
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Mismatch Warning

As a user, I want to be warned about version mismatches so that I can
take action before encountering confusing issues.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    WHEN a version mismatch is detected, THE extension SHALL show an
    information message stating both versions, e.g. "Supersigil version
    mismatch: server is v0.5.0 but this extension is v0.6.0. This may
    cause unexpected behavior."
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/extension.ts" />
  </Criterion>
  <Criterion id="req-3-2">
    THE mismatch warning SHALL be shown at most once per session,
    regardless of how many workspace folders or client restarts occur.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/extension.ts" />
  </Criterion>
  <Criterion id="req-3-3">
    WHEN the server version is newer than the extension version, THE
    dialog SHALL include an "Update Extension" action that opens the
    Extensions sidebar filtered to the Supersigil extension.
  </Criterion>
  <Criterion id="req-3-4">
    THE extension SHALL log the mismatch to the output channel as
    `[extension] Version mismatch: server v{server}, extension
    v{extension}`.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/extension.ts" />
  </Criterion>
</AcceptanceCriteria>
```
