---
supersigil:
  id: onboarding/req
  type: requirements
  status: implemented
title: Onboarding improvements
---

## Introduction

New users encounter two friction points: the landing page offers only `cargo install supersigil` (a Rust-toolchain-only option) and the editor extensions show a generic "not found" notification when the LSP binary is missing. Both miss the chance to surface the right install method for the user's platform and guide them through setup.

### Scope

- **In scope:** Landing page install widget, VS Code extension empty state, IntelliJ plugin empty state.
- **Out of scope:** Getting-started docs page (already has Homebrew/Cargo/AUR tabs), Neovim/other editors, the `supersigil init` flow, npm/npx distribution.

## Definitions

- **Install widget**: The interactive element on the landing page that shows an install command with a copy button.
- **Empty state**: What the editor extension shows when `supersigil-lsp` cannot be found — currently a one-shot notification.

## Requirement 1: Platform-aware install widget on the landing page

As a visitor to the landing page, I want to see install instructions relevant to my operating system, so that I can get started without needing to find the getting-started guide first.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    The landing page hero install widget SHALL offer at least Homebrew, Cargo, AUR, and GitHub Releases as install options.
    <VerifiedBy strategy="file-glob" paths="website/src/components/landing/InstallWidget.astro" />
  </Criterion>
  <Criterion id="req-1-2">
    The install widget SHALL auto-select the most likely option based on the visitor's detected OS (macOS → Homebrew, Linux → AUR, Windows → GitHub Releases, unknown → Cargo).
    <VerifiedBy strategy="file-glob" paths="website/src/components/landing/InstallWidget.astro" />
  </Criterion>
  <Criterion id="req-1-3">
    The visitor SHALL be able to manually switch between install options regardless of detected OS.
    <VerifiedBy strategy="file-glob" paths="website/src/components/landing/InstallWidget.astro" />
  </Criterion>
  <Criterion id="req-1-4">
    Each install option SHALL have a copy-to-clipboard button that copies the corresponding command or URL.
    <VerifiedBy strategy="file-glob" paths="website/src/components/landing/InstallWidget.astro" />
  </Criterion>
  <Criterion id="req-1-5">
    The CTA section at the bottom of the landing page SHALL use the same platform-aware install widget as the hero.
    <VerifiedBy strategy="file-glob" paths="website/src/components/landing/CtaSection.astro" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Actionable empty state in VS Code extension

As a VS Code user who has installed the Supersigil extension but not the LSP binary, I want clear guidance on how to install it, so that I can get the extension working without leaving the editor.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    WHEN supersigil-lsp is not found, the Spec Explorer sidebar SHALL show an inline welcome view with install instructions instead of (or in addition to) the existing notification.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/package.json" />
  </Criterion>
  <Criterion id="req-2-2">
    The empty state SHALL show platform-appropriate install commands (Homebrew on macOS/Linux, cargo install as a fallback, link to GitHub Releases on Windows).
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/extension.ts" />
  </Criterion>
  <Criterion id="req-2-3">
    The empty state SHALL include actions to open the terminal (to run the install command) and to open settings (to configure a custom path).
    <VerifiedBy strategy="file-glob" paths="editors/vscode/package.json" />
  </Criterion>
  <Criterion id="req-2-4">
    The empty state SHALL include a Retry action that re-runs binary resolution and activates the LSP client if found.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/extension.ts" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Actionable empty state in IntelliJ plugin

As an IntelliJ user who has installed the Supersigil plugin but not the LSP binary, I want clear guidance on how to install it, so that I can get the plugin working without leaving the IDE.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    WHEN supersigil-lsp is not found, the plugin SHALL show a persistent, actionable notification with install instructions instead of only the current balloon.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilNotifications.kt" />
  </Criterion>
  <Criterion id="req-3-2">
    The notification SHALL show platform-appropriate install commands (Homebrew on macOS/Linux, cargo install as a fallback, link to GitHub Releases on Windows).
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilNotifications.kt" />
  </Criterion>
  <Criterion id="req-3-3">
    The notification SHALL include actions to open the terminal and to open settings.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilNotifications.kt" />
  </Criterion>
</AcceptanceCriteria>
```
