---
supersigil:
  id: version-mismatch/design
  type: design
  status: approved
title: "Version Mismatch Detection"
---

```supersigil-xml
<Implements refs="version-mismatch/req" />
<TrackedFiles paths="crates/supersigil-lsp/src/state.rs, editors/vscode/src/extension.ts" />
```

## Overview

Two changes: the LSP server populates `ServerInfo` in its
`InitializeResult`, and the VS Code extension reads it after client
startup to compare versions. On mismatch, a once-per-session warning
dialog appears.

## Server Side

In `crates/supersigil-lsp/src/state.rs`, the `initialize` handler
currently returns `InitializeResult { capabilities, ..Default::default() }`.
Change this to populate the `server_info` field:

```rust
let result = InitializeResult {
    capabilities,
    server_info: Some(ServerInfo {
        name: "supersigil-lsp".to_owned(),
        version: Some(env!("CARGO_PKG_VERSION").to_owned()),
    }),
};
```

`env!("CARGO_PKG_VERSION")` is resolved at compile time from
`crates/supersigil-lsp/Cargo.toml` — no runtime cost, no new
dependencies.

## Extension Side

In `editors/vscode/src/extension.ts`, after `client.start()` resolves
in `startClientForFolder`, check the server version:

1. Read server version from `client.initializeResult?.serverInfo?.version`.
   If absent, skip silently.

2. Read extension version from
   `vscode.extensions.getExtension("supersigil.supersigil")!.packageJSON.version`.

3. If they differ and a module-level `mismatchShown` flag is `false`:
   - Set `mismatchShown = true`.
   - Log `[extension] Version mismatch: server v{server}, extension v{ext}`
     to the output channel.
   - If the server version is newer than the extension version: show
     an information message with an "Update Extension" button that runs
     `workbench.extensions.action.showExtensionsWithIds` filtered to
     `["supersigil.supersigil"]`.
   - Otherwise: show a plain information message with no action buttons.

The `mismatchShown` flag follows the same pattern as the existing
`notFoundShown` flag — a module-level boolean that prevents repeat
dialogs across multi-root workspace folder starts.

## Version Comparison

Mismatch detection uses simple string equality (`serverVersion !==
extensionVersion`). Both versions follow semver and are always
published in lockstep. No need for range-based comparison.

For the "server newer" check (to decide whether to show the "Update
Extension" button), split both versions on `.` and compare
major/minor/patch segments numerically. Lexicographic string comparison
is **not** safe for semver (`"0.10.0" < "0.9.0"` lexicographically).
Prerelease and build metadata are not used; if present, treat as
not-newer and show the plain warning.

## Testing Strategy

The server-side change is a one-line addition to `InitializeResult`
construction — verified by the existing LSP integration tests that
check initialization. Add a test asserting `server_info` is present
with the expected name and a non-empty version.

The extension-side logic is UI-facing (dialog, output channel) and
depends on `client.initializeResult` which is only available after a
real LSP handshake. Manual verification: start the extension with a
mismatched binary and confirm the dialog appears.

```supersigil-xml
<Decision id="string-equality" standalone="Version comparison strategy">
  Use string equality for mismatch detection rather than semver range
  checking.

  <Rationale>
    All crates and the extension are published at the same version on
    every release. There are no independent version tracks to compare
    ranges against. String equality is the simplest correct check.
  </Rationale>

  <Alternative id="semver-range" status="rejected">
    Use semver-aware comparison with compatibility ranges. Rejected
    because there is no independent versioning — all components share
    a single version number, so range compatibility is meaningless.
  </Alternative>
</Decision>
```
