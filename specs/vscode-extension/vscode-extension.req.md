---
supersigil:
  id: vscode-extension/req
  type: requirements
  status: draft
title: "VS Code Extension"
---

## Introduction

A VS Code extension that connects to the `supersigil-lsp` language server,
surfacing diagnostics, go-to-definition, autocomplete, and hover for Markdown
spec files directly in the editor.

Scope: the VS Code extension itself — binary discovery, language client
lifecycle, command palette entries, status bar, and extension packaging. The
LSP server features (diagnostics, completions, etc.) are already specified in
`lsp-server/req` and are out of scope here.

```supersigil-xml
<References refs="lsp-server/req, lsp-server/design" />
```

## Definitions

- **Server_Binary**: The `supersigil-lsp` executable that the extension
  launches as a child process over stdio.
- **Binary_Resolution**: The process for locating the Server_Binary:
  setting override, `$PATH` lookup, common install location fallbacks
  (`~/.cargo/bin`, `~/.local/bin`), then not-found notification.

## Requirement 1: Binary Discovery

As a spec author, I want the extension to find and launch the LSP server
automatically, so that I get language intelligence without manual
configuration.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    WHEN the `supersigil.lsp.serverPath` setting is configured, THE extension
    SHALL use that path to launch the Server_Binary. IF the path does not
    exist or is not executable, THE extension SHALL show an error notification.
  </Criterion>
  <Criterion id="req-1-2">
    WHEN `supersigil.lsp.serverPath` is not set, THE extension SHALL search
    `$PATH` for `supersigil-lsp`, then check common install locations
    (`~/.cargo/bin/supersigil-lsp`, `~/.local/bin/supersigil-lsp`), and
    use the first match found.
  </Criterion>
  <Criterion id="req-1-3">
    WHEN the Server_Binary cannot be found by either method, THE extension
    SHALL show an informational notification with install instructions and an
    "Open Settings" action. THE notification SHALL appear at most once per
    session.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Extension Activation

As a spec author, I want the extension to activate only in Supersigil
projects, so that it does not consume resources in unrelated workspaces.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    THE extension SHALL activate when the workspace contains a
    `supersigil.toml` file, using the `workspaceContains` activation event.
  </Criterion>
  <Criterion id="req-2-2">
    THE extension SHALL NOT activate in workspaces that do not contain a
    `supersigil.toml` file.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Language Client Lifecycle

As a spec author, I want the LSP connection to be reliable and recoverable,
so that I do not lose language intelligence during editing sessions.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    ON activation, THE extension SHALL create one `LanguageClient` per
    workspace folder that contains a `supersigil.toml`, each with stdio
    transport targeting the resolved Server_Binary. Each client's
    `documentSelector` SHALL be scoped to its workspace folder's path.
  </Criterion>
  <Criterion id="req-3-2">
    EACH client SHALL register for documents matching
    `{ scheme: 'file', language: 'markdown', pattern: '&lt;folder&gt;/**/*' }`
    and `{ scheme: 'file', language: 'mdx', pattern: '&lt;folder&gt;/**/*' }`.
  </Criterion>
  <Criterion id="req-3-3">
    WHEN the server process crashes, THE extension SHALL attempt to restart
    it automatically, up to a configurable maximum (default: 3 restarts).
  </Criterion>
  <Criterion id="req-3-4">
    ON deactivation, THE extension SHALL stop all language clients and
    terminate all server processes.
  </Criterion>
  <Criterion id="req-3-5">
    WHEN workspace folders are added or removed, THE extension SHALL
    start or stop clients dynamically for folders that contain
    `supersigil.toml`.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 4: Commands

As a spec author, I want to trigger common actions from the command palette,
so that I can verify specs and recover from server issues without leaving
the editor.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-4-1">
    THE extension SHALL register a "Supersigil: Verify" command that sends
    the `supersigil.verify` execute command to the LSP server responsible
    for the active editor's file. In multi-root workspaces, the command
    routes to the correct client by matching the file's workspace folder.
  </Criterion>
  <Criterion id="req-4-2">
    THE extension SHALL register a "Supersigil: Restart Server" command that
    stops and restarts all language clients and server processes.
  </Criterion>
  <Criterion id="req-4-3">
    THE extension SHALL register a "Supersigil: Show Status" command that
    opens a Quick Pick menu showing per-root server status, diagnostic
    file count, and actions (restart server, show output).
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 5: Status Bar

As a spec author, I want visual feedback on the server state, so that I know
whether language intelligence is active.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-5-1">
    THE extension SHALL display a status bar item showing "Supersigil" when
    all server instances are running. In multi-root workspaces, the item
    SHALL show the instance count (e.g. "Supersigil (2)").
  </Criterion>
  <Criterion id="req-5-2">
    WHEN any server instance has stopped or crashed beyond the restart
    limit, THE status bar item SHALL show a warning indicator.
  </Criterion>
  <Criterion id="req-5-3">
    WHEN clicked, THE status bar item SHALL open the "Supersigil: Show
    Status" Quick Pick menu.
  </Criterion>
  <Criterion id="req-5-4">
    THE status bar item SHALL be hidden when the extension is not active.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 6: Extension Packaging

As a developer distributing the extension, I want it to be small and
fast-loading, so that installation and activation are seamless.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-6-1">
    THE extension source SHALL be bundled with esbuild into a single
    JavaScript file for distribution.
  </Criterion>
  <Criterion id="req-6-2">
    THE `.vsix` package SHALL contain only the bundled output, package
    manifest, and any icon or readme — no source files or `node_modules`.
  </Criterion>
  <Criterion id="req-6-3">
    THE extension SHALL be written in TypeScript with type checking
    performed separately from the esbuild bundle step.
  </Criterion>
</AcceptanceCriteria>
```
