---
supersigil:
  id: intellij-plugin/design
  type: design
  status: approved
title: "IntelliJ Plugin"
---

```supersigil-xml
<Implements refs="intellij-plugin/req" />
<DependsOn refs="lsp-server/design" />
<TrackedFiles paths="editors/intellij/src/**/*.kt, editors/intellij/src/main/resources/META-INF/plugin.xml" />
```

## Overview

A thin IntelliJ IDEA plugin that launches `supersigil-lsp` over stdio
using the platform's built-in LSP client API. The plugin adds binary
discovery, a Spec Explorer tool window, a verify action, and Markdown
code fence language injection on top of the LSP client.

## Architecture

```
IntelliJ project
  │
  ├─ LspServerSupportProvider  →  fileOpened()
  │    └─ checks for supersigil.toml in project root
  │
  ├─ LspServerDescriptor
  │    ├─ createCommandLine()  →  resolved supersigil-lsp binary
  │    ├─ isSupportedFile()    →  *.md, *.mdx within project
  │    └─ stdio transport
  │
  ├─ SpecExplorerToolWindowFactory
  │    ├─ isApplicableAsync()  →  checks for supersigil.toml
  │    ├─ Tree (JBTreeTable or SimpleTree)
  │    │    ├─ project nodes   (multi-project only)
  │    │    ├─ group nodes     (ID prefix)
  │    │    └─ document nodes  (leaf, click opens file)
  │    └─ toolbar: verify button
  │
  ├─ VerifyAction (AnAction)
  │    └─ workspace/executeCommand → supersigil.verify
  │
  ├─ SupersigilSettings (PersistentStateComponent)
  │    └─ serverPath: String?
  │
  └─ CodeFenceLanguageProvider
       └─ maps "supersigil-xml" → XML
```

All language intelligence (diagnostics, completions, hover,
go-to-definition, find references, rename, document symbols, code
actions, code lens) is handled by the LSP server via standard LSP
protocol. The plugin contains no language analysis logic.

## Project Layout

```
editors/intellij/
├── build.gradle.kts
├── gradle.properties
├── settings.gradle.kts
├── gradle/
│   ├── wrapper/
│   └── libs.versions.toml
├── src/main/
│   ├── kotlin/org/supersigil/intellij/
│   │   ├── SupersigilLspServerSupportProvider.kt
│   │   ├── SupersigilLspServerDescriptor.kt
│   │   ├── SupersigilSettings.kt
│   │   ├── SupersigilSettingsConfigurable.kt
│   │   ├── SupersigilCodeFenceLanguageProvider.kt
│   │   ├── SpecExplorerToolWindowFactory.kt
│   │   ├── SpecExplorerTreeModel.kt
│   │   └── VerifyAction.kt
│   └── resources/
│       ├── META-INF/plugin.xml
│       ├── syntaxes/
│       │   └── supersigil-xml-injection.json
│       └── icons/
│           └── supersigil.svg
└── src/test/
    └── kotlin/org/supersigil/intellij/
        └── SpecExplorerTreeModelTest.kt
```

## Build Configuration

- **Build tool**: IntelliJ Platform Gradle Plugin 2.x
- **Language**: Kotlin 2.x (required for 2025.1+ targets)
- **Minimum target**: IntelliJ 2025.3 (`pluginSinceBuild = 253`)
- **Package**: `org.supersigil.intellij`
- **Plugin dependencies** (in `plugin.xml`):
  - `com.intellij.modules.platform` — core platform
  - `com.intellij.modules.lsp` — built-in LSP client
  - `org.intellij.plugins.markdown` — Markdown code fence injection

The `supersigil-lsp` binary is not bundled in the plugin distribution.

## LSP Integration

`SupersigilLspServerSupportProvider` implements
`LspServerSupportProvider`. When a file is opened, it checks whether
the project contains a `supersigil.toml` at the project base path. If
found, it creates a `SupersigilLspServerDescriptor`.

`SupersigilLspServerDescriptor` extends `LspServerDescriptor`:

- `createCommandLine()` returns a `GeneralCommandLine` pointing to the
  resolved `supersigil-lsp` binary.
- `isSupportedFile(VirtualFile)` returns true for `.md` and `.mdx`
  files within the project root.

The platform's LSP client handles stdio transport, JSON-RPC framing,
capability negotiation, and crash recovery. File change notifications
(`workspace/didChangeWatchedFiles`) are sent automatically by the
platform.

### Graceful Degradation

Code lens support was added to the IntelliJ LSP client in 2026.1. On
2025.3, the platform does not advertise the `codeLens` capability, so
the server does not send lenses. No conditional code is needed in the
plugin; when users upgrade to 2026.1+, code lenses appear
automatically via standard capability negotiation.

## Binary Resolution

`resolveBinary()` returns a path or null:

1. Read `serverPath` from `SupersigilSettings`. If set and the file
   exists, return it. If set but invalid, show an error notification.
2. Search `$PATH` for `supersigil-lsp` using
   `PathEnvironmentVariableUtil.findInPath()`.
3. Check `~/.cargo/bin/supersigil-lsp` and
   `~/.local/bin/supersigil-lsp`.
4. Show a notification balloon with install instructions and a link
   to Settings > Tools > Supersigil. Return null.

When null is returned, the LSP server is not started. The user can
install the binary and reopen a file to trigger a new resolution
attempt.

## Spec Explorer Tool Window

A `ToolWindowFactory` registered via `com.intellij.toolWindow` in
`plugin.xml`.

### Activation

`isApplicableAsync(Project)` checks for `supersigil.toml` in the
project base directory. The tool window is hidden in non-supersigil
projects.

### Data Flow

The tree sends `supersigil/documentList` custom requests to the LSP
server via the `LspServerDescriptor`'s underlying JSON-RPC connection.
It listens for `supersigil/documentsChanged` notifications to trigger
a refresh. Both use the same LSP server process.

### Tree Model

`SpecExplorerTreeModel` is a pure data transformation: it takes
`DocumentEntry[]` (the LSP response) and produces a tree structure.
This is the same grouping logic used in the VS Code extension's
`SpecExplorerProvider`:

- **Project node**: shown only in multi-project configs.
- **Group node**: documents sharing an ID prefix before `/`.
  Shows document count.
- **Document node**: leaf item. Shows doc_type and status as
  description text. Click opens the file via
  `FileEditorManager.openFile()`.
- Documents with no `/` in their ID appear ungrouped at the top level.

### Icons

| doc_type | Icon |
|---|---|
| `requirements` | `AllIcons.Actions.Checked` |
| `design` | `AllIcons.Actions.Edit` |
| `tasks` | `AllIcons.Nodes.Tag` |
| `adr` / `decision` | `AllIcons.Nodes.Annotationtype` |
| `documentation` | `AllIcons.FileTypes.Text` |
| other | `AllIcons.FileTypes.Any_type` |

### Status Colors

| Status | Color |
|---|---|
| `approved`, `implemented`, `done`, `accepted` | Green |
| `superseded` | Gray |
| `draft`, no status | Default |

### Toolbar

The tool window toolbar includes a verify button that triggers
`VerifyAction`.

## Verify Action

An `AnAction` subclass registered in the Tools menu and in the Spec
Explorer toolbar.

The action sends `workspace/executeCommand` with command
`supersigil.verify` to the LSP server. The `update()` method disables
the action when no LSP server is running.

No default keyboard shortcut. Users can bind one via Settings > Keymap.

## Settings

`SupersigilSettings` implements
`PersistentStateComponent<SupersigilSettings.State>` with
application-level scope.

State:
- `serverPath: String?` — absolute path to the `supersigil-lsp` binary.
  Null means auto-resolve.

`SupersigilSettingsConfigurable` implements `Configurable` and provides
the settings UI under Settings > Tools > Supersigil. A single text
field with a file chooser button for the server path.

## Syntax Highlighting

IntelliJ's Markdown plugin uses its own PSI-based parser, not
TextMate grammars. TextMate injection grammars (as used by the VS Code
extension) do not work in IntelliJ's Markdown editor.

Instead, `SupersigilCodeFenceLanguageProvider` implements the
Markdown plugin's `CodeFenceLanguageProvider` interface. It maps the
`supersigil-xml` info string to XML, allowing the Markdown plugin's
built-in `CodeFenceInjector` to inject XML language support into
those fenced code blocks.

Requires a plugin dependency on `org.intellij.plugins.markdown`
(bundled by default in all IntelliJ distributions).

## Error Handling

- **Binary not found**: Notification balloon with install instructions
  and a link to Settings. Plugin remains functional but LSP features
  are unavailable.
- **Binary not executable**: Error notification showing the configured
  path.
- **Server crash**: The platform's LSP client handles crash recovery
  and restart automatically.
- **Custom request failure**: If `supersigil/documentList` fails, the
  Spec Explorer shows an empty tree. Errors are logged.

## Testing Strategy

- **`SpecExplorerTreeModel`**: Unit tests with no IntelliJ platform
  dependency. Test grouping by project, prefix, and ungrouped
  documents. Test icon and status mapping.
- **Binary resolution**: Unit-testable by extracting the resolution
  logic into a pure function that takes candidate paths and a
  file-exists predicate.
- **LSP integration**: Manual testing for v1.
- **Plugin verification**: `./gradlew runPluginVerifier` validates
  binary compatibility with target platform versions.

## Decisions

```supersigil-xml
<Decision id="decision-1">
  Use IntelliJ's built-in LSP client API rather than LSP4IJ or manual
  PSI implementation.

  <References refs="intellij-plugin/req#req-3-1" />

  <Rationale>
    The built-in LSP client (com.intellij.modules.lsp) covers all
    standard LSP features needed by supersigil. It is maintained by
    JetBrains, requires no external dependencies, and integrates
    natively with IntelliJ's inspections panel, structure view, and
    breadcrumbs. Since the 2025.3 unified distribution, it is available
    to all users for free.
  </Rationale>

  <Alternative id="lsp4ij" status="rejected">
    Use Red Hat's LSP4IJ plugin as the LSP client. Broader spec
    coverage and works on Android Studio, but adds a runtime dependency
    users must install, could conflict with the built-in LSP client,
    and provides less native-feeling integration. The built-in client
    has caught up significantly in 2025.3 and 2026.1.
  </Alternative>

  <Alternative id="hybrid-psi" status="rejected">
    Use the built-in LSP client for standard features but implement
    missing features (e.g. code lens on pre-2026.1) using PSI APIs.
    Significant extra complexity with two communication channels to
    the server. The graceful degradation approach (features appear
    when the platform supports them) is simpler and sufficient.
  </Alternative>
</Decision>

<Decision id="decision-2">
  Do not bundle the supersigil-lsp binary in the plugin distribution.

  <References refs="intellij-plugin/req#req-1-1, intellij-plugin/req#req-1-2, intellij-plugin/req#req-1-3" />

  <Rationale>
    The LSP binary is a platform-specific Rust executable. Bundling it
    would require building and packaging separate plugin archives per
    platform, adding significant CI complexity. The project already
    distributes the binary via cargo install. The plugin compensates
    with auto-resolution from PATH and common install locations, plus
    a helpful not-found notification.
  </Rationale>
</Decision>

<Decision id="decision-3">
  Target IntelliJ 2025.3 as minimum, with code lens appearing
  automatically on 2026.1+.

  <References refs="intellij-plugin/req#req-3-2, intellij-plugin/req#req-3-4" />

  <Rationale>
    2025.3 is the first unified distribution where the LSP client API
    is free for all users. Targeting it maximizes reach. Code lens
    support was added in 2026.1 but is a non-essential feature (the
    same information is available via hover and diagnostics). Standard
    LSP capability negotiation means code lenses appear automatically
    on 2026.1+ without any plugin-side conditional code.
  </Rationale>
</Decision>
```
