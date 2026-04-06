---
supersigil:
  id: vscode-extension/tasks
  type: tasks
  status: done
title: "VS Code Extension"
---

```supersigil-xml
<DependsOn refs="vscode-extension/design" />
```

## Overview

Implementation sequence for the VS Code extension. Starts with project
scaffolding and build tooling, then the core extension logic, then
packaging. Each task is independently verifiable.

```supersigil-xml
<Task
  id="task-1"
  status="done"
  implements="vscode-extension/req#req-6-1, vscode-extension/req#req-6-3"
>
  Scaffold `editors/vscode/` with `package.json`, `tsconfig.json`,
  `esbuild.mjs`, and `.vscodeignore`. Set up pnpm workspace, install
  dependencies (`vscode-languageclient`, `@types/vscode`, `esbuild`,
  `@vscode/vsce`, `typescript`). Verify `pnpm build` produces
  `dist/extension.js` from an empty `src/extension.ts` stub.
</Task>

<Task
  id="task-2"
  status="done"
  depends="task-1"
  implements="vscode-extension/req#req-1-1, vscode-extension/req#req-1-2, vscode-extension/req#req-1-3"
>
  Implement `resolveServerBinary()` in `src/extension.ts`: read
  `supersigil.lsp.serverPath` setting, fall back to `$PATH` lookup,
  show not-found notification with "Open Settings" action. Track
  notification shown state to avoid repeating per session.
</Task>

<Task
  id="task-3"
  status="done"
  depends="task-1"
  implements="vscode-extension/req#req-2-1, vscode-extension/req#req-2-2, vscode-extension/req#req-3-1, vscode-extension/req#req-3-2, vscode-extension/req#req-3-3, vscode-extension/req#req-3-4"
>
  Implement `activate()` and `deactivate()` in `src/extension.ts`.
  Create `LanguageClient` with stdio transport and `markdown` and `mdx`
  document selector. Register activation event in `package.json`
  (`workspaceContains:**/supersigil.toml`). Wire auto-restart on crash
  with max 3 retries. Stop client on deactivate.
</Task>

<Task
  id="task-4"
  status="done"
  depends="task-3"
  implements="vscode-extension/req#req-4-1, vscode-extension/req#req-4-2"
>
  Register command palette commands in `package.json` and implement
  handlers in `extension.ts`. "Supersigil: Verify" sends
  `workspace/executeCommand` with `supersigil.verify`. "Supersigil:
  Restart Server" calls `client.restart()`, retrying binary resolution
  if client was not created.
</Task>

<Task
  id="task-5"
  status="done"
  depends="task-3"
  implements="vscode-extension/req#req-5-1, vscode-extension/req#req-5-2, vscode-extension/req#req-5-3, vscode-extension/req#req-5-4"
>
  Add status bar item: show "Supersigil" when running, warning indicator
  on crash/stop, click triggers restart command. Hide when extension is
  not active. Track language client state changes to update the item.
</Task>

<Task
  id="task-6"
  status="done"
  depends="task-4, task-5"
  implements="vscode-extension/req#req-6-2"
>
  Configure `.vscodeignore` to exclude `src/`, `node_modules/`,
  `tsconfig.json`, `esbuild.mjs`. Verify `pnpm package` produces a
  `.vsix` containing only `dist/extension.js`, `package.json`, and any
  icon or readme. Smoke test: install the `.vsix` in VS Code, open a
  Supersigil project, verify diagnostics, completions, go-to-definition,
  and hover work through the LSP.
</Task>
```
