---
supersigil:
  id: intellij-plugin/tasks
  type: tasks
  status: done
title: "IntelliJ Plugin"
---

```supersigil-xml
<DependsOn refs="intellij-plugin/design" />
```

## Overview

Implementation sequence for the IntelliJ plugin. Starts with adapting
the cloned JetBrains plugin template, then adds the LSP integration,
settings, Spec Explorer, verify action, and TextMate grammar. Each
task is independently verifiable.

```supersigil-xml
<Task
  id="task-1"
  status="done"
  implements="intellij-plugin/req#req-3-2"
>
  Adapt the cloned JetBrains plugin template in `editors/intellij/`.
  Remove the nested `.git/` directory. Rename the package from
  `com.github.jonisavo.supersigilintellij` to `org.supersigil.intellij`.
  Update `gradle.properties`: set `platformVersion` to 2025.3,
  `pluginSinceBuild` to `253`, `pluginGroup` to `org.supersigil.intellij`.
  Update `plugin.xml` with correct plugin ID (`org.supersigil.intellij`),
  name (`Supersigil`), and vendor. Remove template boilerplate classes
  (`My*`). Add plugin dependencies: `com.intellij.modules.lsp`,
  `org.jetbrains.plugins.textmate`. Verify `./gradlew build` succeeds
  with an empty plugin.
</Task>

<Task
  id="task-2"
  status="done"
  depends="task-1"
  implements="intellij-plugin/req#req-6-1, intellij-plugin/req#req-6-2"
>
  Implement `SupersigilSettings` as a `PersistentStateComponent` with
  a `serverPath: String?` field. Implement
  `SupersigilSettingsConfigurable` with a settings UI under
  Settings > Tools > Supersigil containing a text field with file
  chooser for the server binary path.
</Task>

<Task
  id="task-3"
  status="done"
  depends="task-1"
  implements="intellij-plugin/req#req-1-1, intellij-plugin/req#req-1-2, intellij-plugin/req#req-1-3"
>
  Implement binary resolution logic: check `SupersigilSettings` path
  first, then `PATH` lookup via `PathEnvironmentVariableUtil`, then
  `~/.cargo/bin/supersigil-lsp` and `~/.local/bin/supersigil-lsp`.
  Show a notification balloon with install instructions and a link to
  Settings when the binary is not found. Extract the resolution logic
  into a pure function. Write unit tests first (TDD): configured path
  found, configured path missing, PATH hit, fallback hit, nothing
  found.
</Task>

<Task
  id="task-4"
  status="done"
  depends="task-3"
  implements="intellij-plugin/req#req-2-1, intellij-plugin/req#req-2-2, intellij-plugin/req#req-2-3, intellij-plugin/req#req-3-1, intellij-plugin/req#req-3-3, intellij-plugin/req#req-3-4"
>
  Implement `SupersigilLspServerSupportProvider` and
  `SupersigilLspServerDescriptor`. The support provider checks for
  `supersigil.toml` in the project root on `fileOpened()`. The
  descriptor configures `createCommandLine()` with the resolved binary
  path and stdio transport, and `isSupportedFile()` for `.md` and
  `.mdx` files. Register the support provider in `plugin.xml` via
  `com.intellij.platform.lsp.serverSupportProvider`. Verify that
  opening a `.md` file in a supersigil project starts the LSP server
  and provides diagnostics.
</Task>

<Task
  id="task-5"
  status="done"
  depends="task-4"
  implements="intellij-plugin/req#req-4-1, intellij-plugin/req#req-4-2, intellij-plugin/req#req-4-3, intellij-plugin/req#req-4-4, intellij-plugin/req#req-4-5, intellij-plugin/req#req-4-6"
>
  Implement `SpecExplorerToolWindowFactory` and
  `SpecExplorerTreeModel`. The factory checks `isApplicableAsync()`
  for `supersigil.toml`. The tree model groups documents by project
  and ID prefix, maps doc types to icons, and maps statuses to colors.
  Send `supersigil/documentList` to the LSP server and listen for
  `supersigil/documentsChanged` notifications to refresh. Click on a
  document node opens the file. Register the tool window in
  `plugin.xml`. Write unit tests first for `SpecExplorerTreeModel`
  (TDD): single-project grouping, multi-project grouping, ungrouped
  documents, icon mapping per doc_type, status color mapping.
</Task>

<Task
  id="task-6"
  status="done"
  depends="task-4"
  implements="intellij-plugin/req#req-5-1, intellij-plugin/req#req-5-2, intellij-plugin/req#req-5-3"
>
  Implement `VerifyAction` as an `AnAction`. Register it in the Tools
  menu and in the Spec Explorer tool window toolbar. The action sends
  `workspace/executeCommand` with `supersigil.verify` to the LSP
  server. Disable the action when no LSP server is running.
</Task>

<Task
  id="task-7"
  status="done"
  depends="task-1"
  implements="intellij-plugin/req#req-7-1"
>
  Implement `SupersigilCodeFenceLanguageProvider` mapping
  `supersigil-xml` to XML. Register it in `plugin.xml` via
  `org.intellij.markdown.fenceLanguageProvider`. Add
  `org.intellij.plugins.markdown` as a plugin dependency. Verify
  that `supersigil-xml` fenced code blocks in markdown files get XML
  highlighting via IntelliJ's built-in code fence injection.
</Task>

<Task
  id="task-8"
  status="ready"
  depends="task-4, task-5, task-6, task-7"
>
  End-to-end smoke test: build the plugin with `./gradlew build`, run
  `./gradlew runIde` to launch a sandboxed IntelliJ instance, open a
  supersigil project, and verify: LSP diagnostics appear, completions
  work, hover shows component info, go-to-definition navigates to
  targets, Spec Explorer populates, verify action triggers
  verification, and `supersigil-xml` blocks have XML highlighting.
  Run `./gradlew verifyPlugin` for binary compatibility checks.
</Task>
```
