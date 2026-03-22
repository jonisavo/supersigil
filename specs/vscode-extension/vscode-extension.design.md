---
supersigil:
  id: vscode-extension/design
  type: design
  status: draft
title: "VS Code Extension"
---

```supersigil-xml
<Implements refs="vscode-extension/req" />
<DependsOn refs="lsp/design" />
<TrackedFiles paths="editors/vscode/src/**/*.ts, editors/vscode/package.json" />
```

## Overview

A thin VS Code extension that launches `supersigil-lsp` over stdio and
wires it into the editor via `vscode-languageclient`. The extension adds
binary discovery, command palette entries, and a status bar indicator on
top of the LSP client.

## Architecture

```
VS Code workspace
  │
  ├─ workspaceContains:**/supersigil.toml  →  activate()
  │
  └─ extension.ts
       │
       ├─ resolveServerBinary()
       │    ├─ 1. Check supersigil.lsp.serverPath setting
       │    ├─ 2. Search $PATH for supersigil-lsp
       │    └─ 3. Show not-found notification, stop
       │
       ├─ LanguageClient (vscode-languageclient)
       │    ├─ serverOptions: { command: path, transport: stdio }
       │    ├─ clientOptions: { documentSelector: [{ scheme: file, language: markdown, mdx }] }
       │    └─ client.start()
       │
       ├─ Commands
       │    ├─ supersigil.verify     →  client.sendRequest(executeCommand)
       │    └─ supersigil.restartServer  →  client.restart()
       │
       └─ StatusBarItem
            ├─ Running:  "Supersigil"
            ├─ Error:    "⚠ Supersigil"
            └─ Click:    supersigil.restartServer
```

All logic lives in a single `src/extension.ts` file. The extension
exports `activate()` and `deactivate()` functions as required by the
VS Code extension host.

## Project Layout

```
editors/vscode/
├── package.json          # Extension manifest
├── tsconfig.json         # TypeScript config (noEmit for type checking)
├── esbuild.mjs           # Build script
├── .vscodeignore         # Excludes src/, node_modules from .vsix
├── src/
│   └── extension.ts      # Entry point
└── dist/                 # Build output (gitignored)
    └── extension.js      # Bundled extension
```

## Extension Manifest

`package.json` declares:

- `activationEvents`: `["workspaceContains:**/supersigil.toml"]`
- `contributes.commands`: verify and restart server
- `contributes.configuration`: `supersigil.lsp.serverPath` setting
- `main`: `./dist/extension.js` (bundled output)
- `engines.vscode`: `^1.74.0` (minimum for documentSelector-based
  activation without explicit `onLanguage` events)

## Binary Resolution

`resolveServerBinary()` returns a path or `undefined`:

1. Read `supersigil.lsp.serverPath` from workspace configuration. If set
   and the file exists and is executable, return it. If set but invalid,
   show an error notification and return `undefined`.
2. Search `$PATH` for `supersigil-lsp` using Node's `child_process.execSync('which supersigil-lsp')` (Unix) or `where.exe` (Windows). If
   found, return the resolved path.
3. Show an informational notification: "Supersigil LSP server not found.
   Install with `cargo install supersigil-lsp` or configure
   `supersigil.lsp.serverPath`." Include an "Open Settings" action button.
   Track that the notification was shown to avoid repeating it in the same
   session. Return `undefined`.

When `undefined` is returned, the extension activates but does not start
the language client. The status bar shows the error state. The user can
install the binary and use "Restart Server" to connect.

## Language Client Configuration

```typescript
const serverOptions: ServerOptions = {
  command: serverPath,
  transport: TransportKind.stdio,
};

const clientOptions: LanguageClientOptions = {
  documentSelector: [
    { scheme: 'file', language: 'markdown' },
    { scheme: 'file', language: 'mdx' },
  ],
  outputChannel: vscode.window.createOutputChannel('Supersigil LSP'),
};
```

The `outputChannel` captures server stderr for debugging. The
`vscode-languageclient` library handles capability negotiation, request
routing, and crash recovery.

## Status Bar

The status bar item is created in `activate()` and disposed in
`deactivate()`. It tracks the language client state:

- **Running**: text `"Supersigil"`, no background color
- **Error/Stopped**: text `"$(warning) Supersigil"`, warning background
  color via `statusBarItem.backgroundColor`
- **Click**: runs `supersigil.restartServer`

The item is aligned to the right side of the status bar with a low
priority to avoid crowding important items.

## Commands

**supersigil.verify**: Sends a `workspace/executeCommand` request to the
LSP server with command name `supersigil.verify`. The LSP server handles
this by running the verify pipeline and publishing diagnostics. No
arguments are passed (the server uses its configured tier).

**supersigil.restartServer**: Calls `client.restart()` on the language
client. If the client does not exist (binary not found on first
activation), it retries binary resolution and creates a new client.

## Build Tooling

**esbuild.mjs**: Bundles `src/extension.ts` into `dist/extension.js`.

```javascript
// Key configuration
{
  entryPoints: ['src/extension.ts'],
  bundle: true,
  format: 'cjs',
  platform: 'node',
  target: 'node18',
  outfile: 'dist/extension.js',
  external: ['vscode'],
  minify: production,
  sourcemap: !production,
}
```

The `vscode` module is external because it is provided by the extension
host at runtime.

**pnpm scripts**:

- `build`: esbuild production bundle
- `watch`: esbuild watch mode (development)
- `check-types`: `tsc --noEmit` (type checking only, esbuild strips types)
- `package`: `pnpm run build && vsce package --no-dependencies`
- `lint`: `tsc --noEmit` (alias for check-types in CI)

**Dependencies**:

- `vscode-languageclient` (runtime)
- `@types/vscode`, `esbuild`, `@vscode/vsce`, `typescript` (dev)

## Error Handling

- **Binary not found**: Notification with install instructions, extension
  activates in degraded mode (status bar shows warning, restart command
  available).
- **Binary not executable**: Error notification with the configured path,
  suggesting the user check the setting.
- **Server crash**: `vscode-languageclient` auto-restarts up to the
  configured limit (default 3). After exhausting restarts, status bar
  shows warning state. User can manually restart.
- **Server stderr output**: Captured in the "Supersigil LSP" output
  channel for debugging.

## Testing Strategy

- **Manual smoke test**: Install locally via `pnpm package` → install
  `.vsix`, open a Supersigil project, verify diagnostics, completions,
  go-to-definition, and hover all work through the LSP.
- **Unit testable structure**: Binary resolution and status bar state
  logic are pure functions that can be tested without the VS Code API.
  This enables future automated tests without complex mocking.
- **Future e2e tests**: The extension is structured so that
  `@vscode/test-electron` can launch a VS Code instance with a test
  workspace and use `vscode.executeCompletionItemProvider` /
  `vscode.executeDefinitionProvider` to assert LSP features work
  end-to-end. The single-file structure and clean `activate()`/
  `deactivate()` boundary make this straightforward to add.
- **LSP feature correctness**: Tested in the `supersigil-lsp` Rust crate.
  The extension is responsible only for wiring, not for language features.

## Decisions

```supersigil-xml
<Decision id="decision-1">
  Require the user to install `supersigil-lsp` separately rather than
  bundling the binary inside the `.vsix` package.

  <References refs="vscode-extension/req#req-1-1, vscode-extension/req#req-1-2, vscode-extension/req#req-1-3" />

  <Rationale>
    The LSP binary is a platform-specific Rust executable (~10 MB). Bundling
    it would require building and packaging separate `.vsix` files per
    platform (linux-x64, darwin-arm64, win32-x64, etc.) or shipping a
    universal fat package. This adds significant CI complexity for a project
    that already distributes the binary via `cargo install`. The extension
    compensates with a helpful not-found notification and a setting override
    for custom paths.
  </Rationale>

  <Alternative id="bundled-binary" status="rejected">
    Bundle platform-specific binaries in the `.vsix` using VS Code's
    platform-specific packaging. Zero setup for users, but requires
    cross-compilation CI for every release and dramatically increases
    package size. Appropriate for marketplace-scale distribution but
    premature at this stage.
  </Alternative>
</Decision>

<Decision id="decision-2">
  Keep the extension as a thin LSP client with light extras (commands,
  status bar), not a feature-rich editor integration.

  <References refs="vscode-extension/req#req-4-1, vscode-extension/req#req-4-2, vscode-extension/req#req-5-1" />

  <Rationale>
    All language intelligence (diagnostics, completions, hover,
    go-to-definition) is implemented in the LSP server and works with any
    LSP-capable editor. Putting features in the extension rather than the
    server would create VS Code-only functionality that other editors
    cannot access. The extension should only contain what cannot be done
    via LSP: binary discovery, VS Code-specific UI (status bar, command
    palette), and client lifecycle management.
  </Rationale>

  <Alternative id="rich-extension" status="rejected">
    Add VS Code-specific features like a spec graph tree view, custom
    syntax highlighting for Supersigil components, or code snippets.
    These create editor lock-in and duplicate work that should live in
    the LSP server (e.g., completions with snippets already provide
    component scaffolding). Can be revisited for features that are truly
    VS Code-specific and cannot be expressed via LSP.
  </Alternative>
</Decision>

<Decision id="decision-3">
  Build the VS Code extension first, deferring IntelliJ, Neovim, and
  Zed extensions.

  <Rationale>
    VS Code has the largest market share among spec authors and the
    simplest extension model for LSP clients. The `vscode-languageclient`
    library handles most of the complexity. Building one extension end-to-end
    validates the LSP server's real-world behavior before investing in
    additional editor integrations. IntelliJ's LSP client API (added in
    2023.2) is more constrained and would require more investigation.
  </Rationale>

  <Alternative id="all-editors" status="rejected">
    Build extensions for all target editors simultaneously. Spreads effort
    thin and risks discovering LSP server issues late across multiple
    codebases. Better to validate with one editor first.
  </Alternative>
</Decision>

<Decision id="decision-4">
  Use `vscode-languageclient` library rather than the raw VS Code LSP API.

  <References refs="vscode-extension/req#req-3-1, vscode-extension/req#req-3-3" />

  <Rationale>
    `vscode-languageclient` handles server process spawning, stdio
    transport, JSON-RPC framing, capability negotiation, and crash
    recovery with configurable restart limits. The extension code is ~200
    lines instead of ~500+. The library is maintained by Microsoft and
    used by nearly every LSP extension in the marketplace.
  </Rationale>

  <Alternative id="raw-lsp-api" status="rejected">
    Manage the child process, stdio streams, and JSON-RPC protocol
    manually. Full control but significantly more code to write and
    maintain. No benefit at this scope.
  </Alternative>
</Decision>
```
