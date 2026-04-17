---
supersigil:
  id: intellij-plugin/req
  type: requirements
  status: implemented
title: "IntelliJ Plugin"
---

## Introduction

An IntelliJ IDEA plugin that connects to the `supersigil-lsp` language
server, surfacing diagnostics, completions, hover, navigation, and
other language features for Markdown spec files.

Scope: the IntelliJ plugin itself — binary discovery, LSP client
lifecycle, Spec Explorer tool window, verify action, settings page,
and TextMate grammar registration. The LSP server features are already
specified in `lsp-server/req` and are out of scope here.

Since the IntelliJ unified distribution (2025.3) ships the LSP client
API to all users regardless of subscription, the plugin targets all
IntelliJ IDEA users.

```supersigil-xml
<References refs="lsp-server/req, lsp-server/design, vscode-extension/req" />
```

## Definitions

- **Server_Binary**: The `supersigil-lsp` executable that the plugin
  launches as a child process over stdio.
- **Binary_Resolution**: The process for locating the Server_Binary:
  setting override, `$PATH` lookup for the host-appropriate executable
  name, common install location fallbacks (`~/.cargo/bin/supersigil-lsp`,
  `~/.local/bin/supersigil-lsp`,
  `%USERPROFILE%\.cargo\bin\supersigil-lsp.exe`), then not-found
  notification.
- **Spec Explorer**: A sidebar tool window showing spec documents
  grouped by feature area for navigation and status visibility.

## Requirement 1: Binary Discovery

As a spec author, I want the plugin to find and launch the LSP server
automatically, so that I get language intelligence without manual
configuration.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    WHEN the `supersigil.lsp.serverPath` setting is configured, THE
    plugin SHALL use that path to launch the Server_Binary. IF the path
    does not exist or is not executable, THE plugin SHALL show an error
    notification.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/test/kotlin/org/supersigil/intellij/BinaryResolutionTest.kt, editors/intellij/src/main/kotlin/org/supersigil/intellij/BinaryResolution.kt" />
  </Criterion>
  <Criterion id="req-1-2">
    WHEN `supersigil.lsp.serverPath` is not set, THE plugin SHALL
    search `$PATH` for the host-appropriate executable name
    (`supersigil-lsp` on Unix-like hosts, `supersigil-lsp.exe` on Windows),
    then check common install locations (`~/.cargo/bin/supersigil-lsp`,
    `~/.local/bin/supersigil-lsp`,
    `%USERPROFILE%\.cargo\bin\supersigil-lsp.exe`), and use the first match
    found.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/test/kotlin/org/supersigil/intellij/BinaryResolutionTest.kt, editors/intellij/src/main/kotlin/org/supersigil/intellij/BinaryResolution.kt" />
  </Criterion>
  <Criterion id="req-1-3">
    WHEN the Server_Binary cannot be found by either method, THE plugin
    SHALL show a notification balloon with install instructions and a
    link to open the Settings page.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilNotifications.kt, editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilLspServerSupportProvider.kt" />
  </Criterion>
  <Criterion id="req-1-4">
    ON Windows, Binary_Resolution SHALL remain native to the IDE host
    environment and SHALL NOT require WSL or bash-based shims to locate the
    Server_Binary.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/test/kotlin/org/supersigil/intellij/BinaryResolutionTest.kt, editors/intellij/src/main/kotlin/org/supersigil/intellij/BinaryResolution.kt" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Plugin Activation

As a spec author, I want the plugin to activate its LSP features only
in Supersigil projects, so that it does not consume resources in
unrelated workspaces.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    THE plugin SHALL start an LSP server instance when a supported file
    is opened in a project that contains a `supersigil.toml` file.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilLspServerSupportProvider.kt" />
  </Criterion>
  <Criterion id="req-2-2">
    THE plugin SHALL handle `.md` and `.mdx` files within the project
    root as supported files for LSP features.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilLspServerDescriptor.kt" />
  </Criterion>
  <Criterion id="req-2-3">
    THE plugin SHALL NOT start an LSP server in projects that do not
    contain a `supersigil.toml` file.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilLspServerSupportProvider.kt" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: LSP Client Lifecycle

As a spec author, I want the LSP connection to be managed by the
platform's built-in LSP client, so that I get reliable language
intelligence with standard IntelliJ behavior.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    THE plugin SHALL use IntelliJ's built-in LSP client API
    (`LspServerSupportProvider` / `LspServerDescriptor`) to manage the
    server lifecycle. The server SHALL communicate via stdio.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilLspServerSupportProvider.kt, editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilLspServerDescriptor.kt" />
  </Criterion>
  <Criterion id="req-3-2">
    THE plugin SHALL target IntelliJ 2025.3 as the minimum supported
    platform version.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/gradle.properties" />
  </Criterion>
  <Criterion id="req-3-3">
    LSP features available in the target platform version SHALL work
    without additional plugin-side code: diagnostics, completions,
    hover, go-to-definition, find references, rename, document symbols,
    and code actions.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilLspServerDescriptor.kt" />
  </Criterion>
  <Criterion id="req-3-4">
    Code lens support SHALL become available automatically when the user
    runs on IntelliJ 2026.1 or later, via standard LSP capability
    negotiation. No plugin-side conditional code is needed.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilLspServerDescriptor.kt" />
  </Criterion>
  <Criterion id="req-3-5">
    ON Windows, THE plugin SHALL be able to start the resolved native
    `supersigil-lsp.exe` process through IntelliJ's built-in LSP client over
    stdio, without WSL or Unix-only helper processes.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilLspServerSupportProvider.kt, editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilLspServerDescriptor.kt, crates/supersigil-lsp/src/main.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 4: Spec Explorer Tool Window

As a spec author, I want a sidebar tool window that shows spec
documents grouped by feature area, so that I can navigate to spec
files and see project status at a glance.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-4-1">
    THE plugin SHALL register a Spec Explorer tool window that displays
    documents from the LSP server's `supersigil/documentList` custom
    request, grouped by project (if multi-project) and ID prefix.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SpecExplorerToolWindowFactory.kt, editors/intellij/src/test/kotlin/org/supersigil/intellij/SpecExplorerTreeModelTest.kt" />
  </Criterion>
  <Criterion id="req-4-2">
    Documents sharing an ID prefix before the first `/` SHALL appear
    under a collapsible group node. Documents with no `/` in their ID
    SHALL appear ungrouped at the top level.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/test/kotlin/org/supersigil/intellij/SpecExplorerTreeModelTest.kt, editors/intellij/src/main/kotlin/org/supersigil/intellij/SpecExplorerTreeModel.kt" />
  </Criterion>
  <Criterion id="req-4-3">
    WHEN the user clicks a document node, THE plugin SHALL open the
    corresponding file in the editor.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SpecExplorerToolWindowFactory.kt" />
  </Criterion>
  <Criterion id="req-4-4">
    Each document node SHALL display an icon based on document type and
    a description showing `doc_type` and status.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/test/kotlin/org/supersigil/intellij/SpecExplorerTreeModelTest.kt, editors/intellij/src/main/kotlin/org/supersigil/intellij/SpecExplorerTreeModel.kt" />
  </Criterion>
  <Criterion id="req-4-5">
    WHEN the plugin receives a `supersigil/documentsChanged`
    notification from the LSP server, THE tree SHALL refresh by
    re-fetching the document list.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SpecExplorerToolWindowFactory.kt" />
  </Criterion>
  <Criterion id="req-4-6">
    THE tool window SHALL only be available when the project contains a
    `supersigil.toml` file.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SpecExplorerToolWindowFactory.kt" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 5: Verify Action

As a spec author, I want to trigger verification from the IDE, so
that I can check spec health without switching to the terminal.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-5-1">
    THE plugin SHALL register a "Supersigil: Verify" action accessible
    from the Tools menu.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/resources/META-INF/plugin.xml" />
  </Criterion>
  <Criterion id="req-5-2">
    THE Spec Explorer tool window toolbar SHALL include a verify button
    that triggers the same action.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SpecExplorerToolWindowFactory.kt" />
  </Criterion>
  <Criterion id="req-5-3">
    THE verify action SHALL send a `workspace/executeCommand` request
    with command `supersigil.verify` to the LSP server. THE action
    SHALL be disabled when the LSP server is not running.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/VerifyAction.kt" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 6: Settings

As a spec author, I want to configure the LSP server binary path, so
that I can use a custom installation or development build.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-6-1">
    THE plugin SHALL provide a settings page under Settings > Tools >
    Supersigil with a field for the `supersigil-lsp` binary path.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilSettingsConfigurable.kt, editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilSettings.kt" />
  </Criterion>
  <Criterion id="req-6-2">
    WHEN the setting is empty, THE plugin SHALL auto-resolve the binary
    using Binary_Resolution.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/test/kotlin/org/supersigil/intellij/BinaryResolutionTest.kt, editors/intellij/src/main/kotlin/org/supersigil/intellij/BinaryResolution.kt" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 7: Syntax Highlighting

As a spec author, I want XML highlighting in `supersigil-xml` fenced
code blocks, so that component markup is readable.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-7-1">
    THE plugin SHALL register a `CodeFenceLanguageProvider` that maps
    the `supersigil-xml` fenced code block language identifier to XML,
    enabling syntax highlighting via IntelliJ's built-in Markdown
    language injection.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilCodeFenceLanguageProvider.kt" />
  </Criterion>
</AcceptanceCriteria>
```
~~~~
