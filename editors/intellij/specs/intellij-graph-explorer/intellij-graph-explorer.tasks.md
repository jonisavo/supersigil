---
supersigil:
  id: intellij-graph-explorer/tasks
  type: tasks
  status: done
title: "IntelliJ Graph Explorer"
---

```supersigil-xml
<DependsOn refs="intellij-graph-explorer/design" />
```

## Overview

Historical implementation plan for the original IntelliJ graph
explorer integration. The current runtime rollout is tracked by
`graph-explorer-runtime/tasks`.

Implementation sequence for the IntelliJ Graph Explorer tool window.
Starts with the shared infrastructure (navigation extraction, build
pipeline), then the JCEF panel and resource serving, bridge script,
data flow, live updates, and theme integration. Each task is
independently verifiable.

```supersigil-xml
<Task
  id="task-1"
  status="done"
  implements="intellij-graph-explorer/req#req-4-1, intellij-graph-explorer/req#req-4-2"
>
  Extract shared navigation utilities from
  SupersigilPreviewExtensionProvider into NavigationUtil.kt. Move
  openFile, openCriterion, splitAction, and findCriterionLine to the
  new utility object. Update SupersigilPreviewExtensionProvider to
  delegate to NavigationUtil. Move the existing splitAction and
  findCriterionLine tests from SupersigilPreviewExtensionTest to a new
  NavigationUtilTest. Verify existing tests still pass.
</Task>

<Task
  id="task-2"
  status="done"
  implements="intellij-graph-explorer/req#req-7-1, intellij-graph-explorer/req#req-7-2"
>
  Add the website build:explorer-iife script. Create
  website/build-explorer-iife.mjs that bundles
  src/components/explore/graph-explorer.js as an IIFE with globalName
  SupersigilExplorer, outputting to website/dist/explorer-iife/explorer.js.
  Add the build:explorer-iife script to website/package.json. Add
  buildExplorerKit and copyExplorerAssets Gradle tasks to
  build.gradle.kts following the existing buildPreviewKit pattern.
  Add processResources dependency on copyExplorerAssets. Create
  src/main/resources/supersigil-explorer/.gitignore to ignore copied
  assets. Verify ./gradlew processResources copies all expected files
  into supersigil-explorer/.
</Task>

<Task
  id="task-3"
  status="done"
  depends="task-2"
  implements="intellij-graph-explorer/req#req-7-3"
>
  Implement ExplorerResourceRequestHandler: a CefRequestHandler
  registered on JBCefClient that intercepts requests matching the
  https://supersigil-explorer/ prefix and serves classpath resources
  from supersigil-explorer/ via a CefResourceHandler. Support MIME
  types for .js (application/javascript) and .css (text/css). Reject
  requests outside the known resource set. Write unit tests for the
  URL-to-resource-path mapping and MIME type resolution.
</Task>

<Task
  id="task-4"
  status="done"
  depends="task-2, task-3"
  implements="intellij-graph-explorer/req#req-1-1, intellij-graph-explorer/req#req-1-2, intellij-graph-explorer/req#req-1-3"
>
  Implement GraphExplorerToolWindowFactory. Register the tool window
  in plugin.xml with isApplicableAsync checking for supersigil.toml
  and JBCefApp.isSupported(). Create a JBCefBrowser, register the
  ExplorerResourceRequestHandler, generate the HTML shell string, and
  load it via loadHTML(html, "https://supersigil-explorer/"). Add the
  browser component to a SimpleToolWindowPanel. Add toolbar with
  refresh and verify buttons. Verify the tool window appears and loads
  the HTML shell (explorer won't render yet without the bridge).
</Task>

<Task
  id="task-5"
  status="done"
  depends="task-1, task-4"
  implements="intellij-graph-explorer/req#req-4-3, intellij-graph-explorer/req#req-5-1, intellij-graph-explorer/req#req-5-2, intellij-graph-explorer/req#req-5-3"
>
  Write the explorer-bridge.js resource. On DOMContentLoaded, check
  for __supersigilQuery availability. Implement
  window.__supersigilReceiveData(json) that calls
  SupersigilExplorer.mount() with graph data, render data, and a link
  resolver. Implement the link resolver: evidence links use
  #supersigil-action:open-file:path:line, document/criterion links use
  hash-based routing. Add a click handler to intercept evidence links
  and route through __supersigilAction. Add a MutationObserver to
  inject "Open File" buttons in the detail panel header, routing
  clicks through __supersigilAction. Implement state preservation:
  capture hash before unmount, restore after re-mount.
</Task>

<Task
  id="task-6"
  status="done"
  depends="task-4, task-5"
  implements="intellij-graph-explorer/req#req-5-1"
>
  Wire JBCefJSQuery bridge in GraphExplorerToolWindowFactory. Create
  dataQuery and actionQuery JBCefJSQuery instances registered with
  toolWindow.disposable. Register a CefLoadHandlerAdapter that injects
  __supersigilQuery, __supersigilAction, and the theme class via
  executeJavaScript after each page load. Wire the dataQuery handler
  to fetch document components from the LSP server (reusing the
  existing fetchDocumentComponents pattern from the preview extension).
  Wire the actionQuery handler to delegate to NavigationUtil.
</Task>

<Task
  id="task-7"
  status="done"
  depends="task-6"
  implements="intellij-graph-explorer/req#req-2-1, intellij-graph-explorer/req#req-2-2, intellij-graph-explorer/req#req-2-3"
>
  Implement the data flow: graph data fetch and push to JCEF browser.
  On tool window open (after LSP server is running), fetch graph data
  via workspace/executeCommand("supersigil.graphData"). Fetch document
  components for each document with bounded concurrency capped at 10
  pooled worker tasks, tolerating individual failures. Assemble
  the payload and push to the browser via executeJavaScript calling
  window.__supersigilReceiveData(json). Add retry-with-alarm polling
  (same pattern as SpecExplorerToolWindowFactory) for when the LSP
  server is not yet running. Extract the concurrent fetch logic into
  a testable function and write unit tests with mock responses.
</Task>

<Task
  id="task-8"
  status="done"
  depends="task-7"
  implements="intellij-graph-explorer/req#req-3-1, intellij-graph-explorer/req#req-3-2"
>
  Implement live updates. Subscribe to documentsChanged via
  SupersigilLspServerDescriptor.addDocumentsChangedListener() with
  toolWindow.disposable. On notification: if the tool window is
  visible, re-fetch and push data debounced at 200ms via a refresh
  Alarm. If hidden, set a staleWhileHidden flag. Add a content
  manager listener to detect visibility changes and refresh on
  becoming visible. The bridge script's state preservation (hash
  capture/restore) handles the re-render.
</Task>

<Task
  id="task-9"
  status="done"
  depends="task-4"
  implements="intellij-graph-explorer/req#req-6-1, intellij-graph-explorer/req#req-6-2"
>
  Write the intellij-theme-adapter.css resource. Define design token
  CSS variables for light theme (default) and dark theme (html.dark
  selector). Sample colors from IntelliJ's default light and
  Darcula/New UI dark themes. Set font-body and font-heading to system
  sans-serif, font-mono to "JetBrains Mono", monospace. The JVM-side
  theme class injection (JBColor.isBright()) is wired in task-6.
</Task>

<Task
  id="task-10"
  status="done"
  depends="task-7, task-8, task-9"
  implements="intellij-graph-explorer/req#req-4-4"
>
  End-to-end smoke test: build the plugin with ./gradlew build, run
  ./gradlew runIde to launch a sandboxed IntelliJ instance, open a
  supersigil project, and verify: Graph Explorer tool window appears,
  graph renders with document nodes and edges, clicking a node shows
  the detail panel with verification status, "Open File" button opens
  the spec file, evidence links navigate to source files, live updates
  refresh the graph when specs change, light and dark themes display
  correct colors. Run ./gradlew verifyPlugin for binary
  compatibility checks.
</Task>
```
