---
supersigil:
  id: onboarding/design
  type: design
  status: approved
title: Onboarding improvements
---

```supersigil-xml
<Implements refs="onboarding/req" />
```

```supersigil-xml
<TrackedFiles paths="website/src/components/landing/Hero.astro, website/src/components/landing/CtaSection.astro, website/src/components/landing/InstallWidget.astro, editors/vscode/src/extension.ts, editors/vscode/package.json, editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilNotifications.kt, editors/intellij/src/main/kotlin/org/supersigil/intellij/SpecExplorerToolWindowFactory.kt" />
```

## Overview

Three independent changes that share a theme: showing users the right install method for their platform at the moment they need it.

1. **Landing page**: Replace the single `cargo install` box with a tabbed widget that auto-selects based on the visitor's OS.
2. **VS Code extension**: Add a `viewsWelcome` entry and a Retry command for the "binary not found" state.
3. **IntelliJ plugin**: Enrich the existing balloon notification with platform-appropriate commands, an Open Terminal action, and clearer guidance.

## Part 1: Landing Page Install Widget

### Current state

Both `Hero.astro` and `CtaSection.astro` hardcode a single install command:

```
$ cargo install supersigil
```

The getting-started page already has Homebrew / Cargo / AUR tabs, but the landing page does not surface them.

### Design

Extract a new `InstallWidget.astro` component used by both Hero and CtaSection. The widget:

- Renders a row of tab buttons: **Homebrew**, **AUR**, **Cargo**, **GitHub Releases**.
- Uses `navigator.userAgentData?.platform` (with `navigator.platform` and `navigator.userAgent` fallbacks) to detect the OS at page load and auto-select:
  - **macOS** → Homebrew
  - **Linux** → AUR
  - **Windows** → Cargo
  - **Unknown** → Cargo
- Clicking a tab switches the displayed command. Only one command is visible at a time.
- Each command has a copy-to-clipboard button (reusing the existing `install-copy` pattern).
- The GitHub Releases tab shows a link instead of a shell command, opening the
  native Windows release download page in a new tab.

#### Tab content

| Tab              | Content                                              |
|------------------|------------------------------------------------------|
| Homebrew         | `brew install jonisavo/supersigil/supersigil`        |
| AUR              | `yay -S supersigil-bin`                              |
| Cargo            | `cargo install supersigil`                           |
| GitHub Releases  | Link to the Windows release page for `supersigil.exe` and `supersigil-lsp.exe` |

#### Styling

The widget reuses existing landing page tokens (`--bg-surface`, `--border`, `--font-mono`, etc.). Tabs are styled as small pill buttons above the install box, with the active tab highlighted using `--gold`. The overall footprint stays compact — no larger than the current install box plus one row of tabs.

#### CTA section

The CTA section replaces its static `<code>` block with the same `<InstallWidget />` component. This keeps both locations in sync and removes duplication.

### Server-side rendering note

OS detection requires client-side JS. The component renders with a default selection (Cargo) and the `<script>` block updates it on load. This avoids a flash — the box is always visible, just potentially showing the wrong default for one frame.

## Part 2: VS Code Extension Empty State

### Current state

When `resolveServerBinary()` returns `undefined`, the extension:
1. Shows a one-time `showInformationMessage` notification mentioning `cargo install supersigil-lsp` and offering "Open Settings".
2. Sets `supersigil.noRoots` context (but this fires when no `supersigil.toml` is found, not when the binary is missing — these are different states).
3. The Spec Explorer sidebar shows "No supersigil project found" only when `supersigil.noRoots` is true.

There is no welcome view for the "binary not found" state.

### Design

#### New context key: `supersigil.binaryNotFound`

Set to `true` when `resolveServerBinary()` returns `undefined` and at least one supersigil root exists. Set to `false` once the binary is found. This distinguishes "no project" from "project exists but binary missing".

#### New viewsWelcome entry

Add a second `viewsWelcome` in `package.json`:

```json
{
  "view": "supersigil.specExplorer",
  "contents": "Supersigil LSP server not found.\n\nInstall it to enable spec previews, diagnostics, and navigation.\n\n[Install Instructions](https://supersigil.org/getting-started/)\n[Open Terminal](command:supersigil.openInstallTerminal)\n[Configure Path](command:workbench.action.openSettings?%5B%22supersigil.lsp.serverPath%22%5D)\n[Retry](command:supersigil.retryBinaryResolution)",
  "when": "supersigil.binaryNotFound"
}
```

This shows when there's a supersigil project but no binary.

#### New command

- **`supersigil.retryBinaryResolution`**: Re-runs `resolveServerBinary()`. If found, starts all clients, clears the `binaryNotFound` context, and refreshes the sidebar. If still not found, shows a brief notification.

The "Open Terminal" action uses VS Code's built-in `workbench.action.terminal.new` command.

#### Platform-appropriate notification

Replace the current generic `showInformationMessage` with one that checks `process.platform`:
- **darwin**: "Install with `brew install jonisavo/supersigil/supersigil`"
- **linux**: "Install with your package manager or `cargo install supersigil-lsp`"
- **win32**: "Download the native Windows archive from GitHub Releases, add its unpacked directory to `PATH`, or install with `cargo install supersigil-lsp`"

The notification keeps the "Open Settings" action and adds a "Retry" action.

#### Lifecycle changes

In `startAllClients`:
1. Call `resolveServerBinary()`.
2. If `undefined` and supersigil roots exist, set `supersigil.binaryNotFound` to `true`.
3. If found, set `supersigil.binaryNotFound` to `false`.

The `retryBinaryResolution` command calls `startAllClients` which handles both paths.

## Part 3: IntelliJ Plugin Empty State

### Current state

`notifyBinaryNotFound()` shows a balloon notification:
- If configured path: ERROR with the path.
- If not configured: WARNING with `cargo install supersigil-lsp` and an "Open Settings" action.

The Spec Explorer tool window shows "Waiting for language server..." in its empty text, with no actionable guidance.

### Design

#### Enhanced notification

Enrich `notifyBinaryNotFound()` for the unconfigured case:

- Detect OS using `System.getProperty("os.name")`:
  - **macOS**: "Install with `brew install jonisavo/supersigil/supersigil`"
  - **Linux**: "Install with your package manager or `cargo install supersigil-lsp`"
- **Windows**: "Download the native Windows archive from GitHub Releases, add its unpacked directory to `PATH`, or install with `cargo install supersigil-lsp`"
- Change notification type from BALLOON to STICKY_BALLOON so it persists until dismissed.
- Add actions:
  - **"Open Terminal"**: Opens the IntelliJ terminal tool window via `ToolWindowManager.getInstance(project).getToolWindow("Terminal")?.activate(null)`.
  - **"Open Settings"**: Existing action, kept as-is.
  - **"Installation Guide"**: Opens `https://supersigil.org/getting-started/` in the browser via `BrowserUtil.browse()`.

#### Tool window empty text

Update the "Waiting for language server..." empty text in `SpecExplorerToolWindowFactory` to be more actionable when the binary is not found. When `resolveServerBinary()` returns null:

```
tree.emptyText.setText("Supersigil LSP server not found")
tree.emptyText.appendLine("Install it to get started", SimpleTextAttributes.GRAYED_ATTRIBUTES)
tree.emptyText.appendLine("Installation guide", linkAttributes) { BrowserUtil.browse("https://supersigil.org/getting-started/") }
tree.emptyText.appendLine("Open Settings", linkAttributes) { ShowSettingsUtil... }
```

This uses IntelliJ's `StatusText` API which supports clickable links in empty states.

## Testing Strategy

### Landing page
- Automated regression test: cover install-tab OS mapping in `website/src/components/landing/install-widget.test.js`.
- Manual browser testing: verify OS auto-detection, tab switching, copy button, responsive layout.
- Check both light and dark themes.
- Check the CTA section matches the hero widget behavior.

### VS Code extension
- Unit test: `resolveServerBinary()` already has implicit coverage via the existing integration. The new context key and retry command are thin wrappers.
- Manual testing: install extension without `supersigil-lsp` on PATH, verify the welcome view appears with correct actions. Install binary, click Retry, verify LSP starts.

### IntelliJ plugin
- Manual testing: install plugin without `supersigil-lsp`, verify notification content and actions.
- Test empty text links in the Spec Explorer tool window.

## Alternatives Considered

### Landing page: Auto-download script (`curl | sh`)

Rejected. Supersigil doesn't have a universal install script, and adding one is out of scope. Tabs with existing package manager commands are lower effort and more transparent.

### VS Code: File watcher for binary appearance

Rejected per user feedback. A Retry button is sufficient and avoids the complexity of watching filesystem paths that may not exist yet.

### IntelliJ: Custom tool window panel for empty state

Rejected. IntelliJ's `StatusText` API on the existing tree view provides clickable links without needing a separate panel or card-based UI. This keeps the implementation simple and consistent with IntelliJ conventions.
