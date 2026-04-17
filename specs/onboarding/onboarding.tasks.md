---
supersigil:
  id: onboarding/tasks
  type: tasks
  status: done
title: Onboarding improvements
---

```supersigil-xml
<DependsOn refs="onboarding/design" />
```

## Overview

Three independent tracks that can be worked in parallel. Within each track,
tasks are dependency-ordered.

The original onboarding work is complete. The next pass aligns Windows-facing
copy and manual checks with native Windows release artifacts.

## Track 1: Landing Page Install Widget

```supersigil-xml
<Task id="task-1-1" status="done" implements="onboarding/req#req-1-1, onboarding/req#req-1-2, onboarding/req#req-1-3, onboarding/req#req-1-4">
  Create InstallWidget.astro component with four tabs (Homebrew, AUR, Cargo, GitHub Releases), OS auto-detection via navigator.userAgentData/navigator.platform, tab switching, and per-tab copy-to-clipboard. SSR default is Cargo. Style with existing landing-tokens.css patterns.
</Task>
```

```supersigil-xml
<Task id="task-1-2" status="done" depends="task-1-1" implements="onboarding/req#req-1-1, onboarding/req#req-1-4">
  Replace the hardcoded install box in Hero.astro with the new InstallWidget component. Remove the old install-box markup and the copy-button script block.
</Task>
```

```supersigil-xml
<Task id="task-1-3" status="done" depends="task-1-1" implements="onboarding/req#req-1-5">
  Replace the static code block in CtaSection.astro with the InstallWidget component. Remove the old cta-install markup.
</Task>
```

```supersigil-xml
<Task id="task-1-4" status="done" depends="task-1-2, task-1-3">
  Manual testing: verify OS detection, tab switching, copy button, responsive layout, light/dark themes in both Hero and CTA sections.
</Task>
```

```supersigil-xml
<Task id="task-1-5" status="done" depends="task-1-1" implements="onboarding/req#req-1-6">
  Update the InstallWidget copy so the Windows tab is described as the native
  download path for `supersigil.exe` and `supersigil-lsp.exe`, while still
  linking to GitHub Releases.
</Task>
```

## Track 2: VS Code Extension Empty State

```supersigil-xml
<Task id="task-2-1" status="done" implements="onboarding/req#req-2-1">
  Add supersigil.binaryNotFound context key. Set it to true in startAllClients when resolveServerBinary returns undefined but supersigil roots exist. Set to false when the binary is found.
</Task>
```

```supersigil-xml
<Task id="task-2-2" status="done" depends="task-2-1" implements="onboarding/req#req-2-1, onboarding/req#req-2-3">
  Add a new viewsWelcome entry in package.json gated on supersigil.binaryNotFound. Include links to the install guide, Open Terminal, Configure Path, and Retry.
</Task>
```

```supersigil-xml
<Task id="task-2-3" status="done" depends="task-2-1" implements="onboarding/req#req-2-3, onboarding/req#req-2-4">
  Register supersigil.openInstallTerminal (opens a new terminal) and supersigil.retryBinaryResolution (re-runs resolveServerBinary, starts clients if found, clears binaryNotFound context, shows notification if still not found) commands.
</Task>
```

```supersigil-xml
<Task id="task-2-4" status="done" depends="task-2-1" implements="onboarding/req#req-2-2">
  Make the existing showInformationMessage in resolveServerBinary platform-aware: Homebrew on darwin, package manager mention on linux, GitHub Releases on win32. Add a Retry action alongside the existing Open Settings action.
</Task>
```

```supersigil-xml
<Task id="task-2-5" status="done" depends="task-2-2, task-2-3, task-2-4">
  Manual testing: install VS Code extension without supersigil-lsp on PATH. Verify welcome view appears with correct actions. Install binary, click Retry, verify LSP starts and welcome view disappears.
</Task>
```

```supersigil-xml
<Task id="task-2-6" status="done" depends="task-2-4" implements="onboarding/req#req-2-2">
  Update the VS Code empty-state and notification copy so Windows users are
  directed to the native GitHub Releases download path and reminded to add the
  unpacked binary directory to `PATH` or configure `supersigil.lsp.serverPath`.
</Task>
```

## Track 3: IntelliJ Plugin Empty State

```supersigil-xml
<Task id="task-3-1" status="done" implements="onboarding/req#req-3-1, onboarding/req#req-3-2, onboarding/req#req-3-3">
  Update notifyBinaryNotFound in SupersigilNotifications.kt: detect OS via System.getProperty("os.name"), show platform-appropriate install command, change to STICKY_BALLOON, add Open Terminal action (activates Terminal tool window) and Installation Guide action (opens supersigil.org/getting-started in browser).
</Task>
```

```supersigil-xml
<Task id="task-3-2" status="done" implements="onboarding/req#req-3-1">
  Update SpecExplorerToolWindowFactory: when resolveServerBinary returns null, set actionable empty text with clickable "Installation guide" and "Open Settings" links using StatusText API instead of plain "Waiting for language server..." text.
</Task>
```

```supersigil-xml
<Task id="task-3-3" status="done" depends="task-3-1, task-3-2">
  Manual testing: install IntelliJ plugin without supersigil-lsp. Verify notification content, persistence, and actions. Verify Spec Explorer empty text links work.
</Task>
```

```supersigil-xml
<Task id="task-3-4" status="done" depends="task-3-1" implements="onboarding/req#req-3-2">
  Update the IntelliJ install guidance so Windows users are directed to the
  native GitHub Releases download path and reminded to add the unpacked binary
  directory to `PATH` or configure the plugin's server path.
</Task>
```
