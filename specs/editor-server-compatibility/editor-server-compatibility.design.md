---
supersigil:
  id: editor-server-compatibility/design
  type: design
  status: approved
title: "Editor/Server Compatibility"
---

```supersigil-xml
<Implements refs="editor-server-compatibility/req" />
<TrackedFiles paths="crates/supersigil-lsp/src/main.rs, editors/vscode/src/extension.ts, editors/vscode/src/version.ts, editors/vscode/src/version.test.ts, editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilProjectUtil.kt, editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilLspServerSupportProvider.kt, editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilNotifications.kt" />
```

## Overview

Replace exact package-version matching with one explicit compatibility check
shared by both editors.

The design is intentionally small:

1. `supersigil-lsp --compatibility-info` prints one small JSON payload and
   exits.
2. Both editors define one supported Compatibility_Version constant.
3. Before starting an LSP session, each editor runs that preflight query
   against the resolved binary and compares the reported
   Compatibility_Version to its own supported value.
4. If they differ, or the query fails, the editor refuses to start the session
   and shows an actionable error.

Package versions remain useful for display and logging, but they are no longer
the compatibility verdict.

This design replaces the old `editors/vscode/specs/version-mismatch/*`
behavior.

## Architecture

```
editor startup
    │
    ├─ run: supersigil-lsp --compatibility-info
    │        │
    │        └─ binary returns:
    │              {
    │                compatibility_version,
    │                server_version
    │              }
    │
    ├─ compare against editor SUPPORTED_COMPATIBILITY_VERSION
    │
    ├─ match    -> start LSP session
    └─ mismatch -> log, notify user, do not start session
```

## Why a Binary Preflight Query

Use a preflight binary query instead of an in-band LSP compatibility request.

Reason:

- both editors already resolve the `supersigil-lsp` binary path before they try
  to start a server
- a preflight query lets the editor reject incompatibility before any LSP
  session becomes visible or starts serving requests
- this avoids the need for an editor-specific "stop server now" seam after
  startup, which is especially important for IntelliJ's current architecture
- the query stays simple and editor-agnostic

```supersigil-xml
<Decision id="compatibility-via-binary-preflight">
  Expose compatibility metadata through a `supersigil-lsp --compatibility-info`
  preflight query instead of an in-band LSP request.

  <References refs="editor-server-compatibility/req#req-1-1, editor-server-compatibility/req#req-1-3" />

  <Rationale>
    Both editors already know the server binary path before startup. Querying
    the binary directly keeps the check before session creation, which matches
    the hard-stop requirement better than any post-start LSP request.
  </Rationale>

  <Alternative id="execute-command" status="rejected">
    Request compatibility info through `workspace/executeCommand` after startup.
    Rejected because that is too late for a true hard-stop model and requires a
    concrete shutdown seam after the session is already running.
  </Alternative>

  <Alternative id="initialize-result" status="rejected">
    Put the compatibility version in `InitializeResult` and read it directly in
    both editors. Rejected because it is still an in-band, post-start check and
    does not solve the "reject before session use" requirement.
  </Alternative>
</Decision>
```

## Server Side

Add one fast-path CLI branch in `crates/supersigil-lsp/src/main.rs`:

- if invoked as `supersigil-lsp --compatibility-info`, print a small JSON
  object to stdout and exit successfully
- otherwise, continue with normal LSP startup

The JSON shape should be:

```json
{
  "compatibility_version": 1,
  "server_version": "0.10.0"
}
```

The exact Compatibility_Version value should be a compile-time constant in the
server code, separate from `env!("CARGO_PKG_VERSION")`. `server_version` can
still come from `env!("CARGO_PKG_VERSION")`.

## VS Code Integration

After resolving the server binary path, but before creating a `LanguageClient`
in `editors/vscode/src/extension.ts`:

1. Run `supersigil-lsp --compatibility-info` against the resolved binary.
2. Parse the returned `compatibility_version` and `server_version`.
3. Compare the server Compatibility_Version against a local
   `SUPPORTED_COMPATIBILITY_VERSION` constant.
4. On mismatch or query failure:
   - log the supported and reported versions
   - show an error message with an "Update Extension" action
   - do not create or start the client
   - refresh the status bar and related UI state as "not running"

`editors/vscode/src/version.ts` should be repurposed from exact package-version
comparison to a small compatibility helper module. The old "server newer than
extension" branch logic disappears because Compatibility_Version equality is
the only verdict that matters here.

## IntelliJ Integration

After resolving the server binary path, but before calling
`serverStarter.ensureServerStarted(...)`, run the same preflight query from the
IntelliJ plugin.

The startup sequence should:

1. Resolve the configured or discovered `supersigil-lsp` path.
2. Run `supersigil-lsp --compatibility-info`.
3. Compare the reported Compatibility_Version against the plugin's supported
   constant.
4. On mismatch or query failure:
   - log the supported and reported versions
   - show an actionable notification with update/install guidance
   - return without starting the LSP server

This check belongs in the server-start path rather than in a later UI feature.
An incompatible server should be rejected before the plugin begins depending on
it for previews, explorer data, or navigation.

## Incompatible Session Behavior

An incompatible session should fail closed.

That means:

- no background "best effort" mode
- no warning-only path
- no continued operation with partial features

If the versions do not match, or the compatibility-info query fails, the
editor does not start the session and tells the user what to update.

## Compatibility Version Policy

Keep the policy explicit and narrow:

- package version changes alone do not require a Compatibility_Version bump
- compatibility version changes are reserved for editor-visible protocol
  changes, including custom command/response shape changes
- the version should be a simple constant, not a semver range

This keeps release management and compatibility management separate.

## Testing Strategy

Server-side:

- add a test that `supersigil-lsp --compatibility-info` returns the expected
  JSON shape and a non-empty server version

VS Code:

- replace the existing version-mismatch helper tests with compatibility helper
  tests in `editors/vscode/src/version.test.ts`
- cover matching, mismatching, missing-version, and query-failure outcomes
- retire the old `editors/vscode/specs/version-mismatch/*` doc set

IntelliJ:

- add unit coverage for parsing and decision logic around the compatibility
  response
- add one focused test for the incompatibility notification path if the plugin
  test harness can reach that seam cleanly

Manual verification:

- run the editor against a matching server and confirm startup succeeds
- force a mismatched Compatibility_Version and confirm the editor stops the
  session and shows the error

## Alternatives Considered

### Keep exact package-version matching

Rejected. Selective releases mean package versions can legitimately differ even
when the editor and server are still compatible.

### Add semver ranges

Rejected. That creates a larger compatibility policy than the project needs and
turns a simple startup gate into a matrix problem.
