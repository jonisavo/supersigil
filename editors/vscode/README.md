# Supersigil for Visual Studio Code

Supersigil treats specification documents as code. This extension connects to
the `supersigil-lsp` language server to give you real-time feedback, navigation,
and rich previews for Supersigil spec files — right inside VS Code.

## Features

### Diagnostics

Get instant feedback on parse errors, broken cross-references, duplicate IDs,
circular dependencies, coverage gaps, and affected-document hints as you edit.

### Navigation and completions

- **Go to definition** — follow `refs`, `implements`, and `depends` links to
  jump between spec documents.
- **Autocomplete** — document IDs, criterion IDs, and component attributes.
- **Hover** — see document metadata and follow links without leaving your file.
- **Document symbols** — outline view of all criteria in a document.

### Code actions

Quick fixes for common issues: add missing attributes, fix duplicate IDs,
resolve broken references, and more.

### Spec Explorer

A sidebar tree view that organizes your specifications by project, ID prefix
group, and document. Icons and colors indicate document type and status at a
glance.

### Markdown preview

`supersigil-xml` fenced code blocks render as rich, interactive components
inside the built-in Markdown preview:

- Criterion ID, description, and verification status
- Collapsible evidence lists with links to source files and line numbers
- Clickable links to navigate between documents and criteria

### Verify command

Run `Supersigil: Verify` from the command palette to trigger cross-document
verification for the active project.

## Requirements

The `supersigil-lsp` binary must be available. The extension looks for it on
`$PATH`, in `~/.cargo/bin`, and in `~/.local/bin`. You can also set a custom
path in settings:

```json
{
  "supersigil.lsp.serverPath": "/path/to/supersigil-lsp"
}
```

Install via Cargo:

```sh
cargo install supersigil-lsp
```

Or download a binary from the
[GitHub releases](https://github.com/jonisavo/supersigil/releases).

## Commands

| Command                          | Description                          |
| -------------------------------- | ------------------------------------ |
| `Supersigil: Verify`             | Run verification                     |
| `Supersigil: Restart Server`     | Restart all LSP server instances     |
| `Supersigil: Show Status`        | Show server health and diagnostics   |
| `Supersigil: Initialize Project` | Run `supersigil init` in a terminal  |
| `Supersigil: Go to Criterion`    | Jump to a specific criterion         |

## Settings

| Setting                       | Default | Description                                            |
| ----------------------------- | ------- | ------------------------------------------------------ |
| `supersigil.lsp.serverPath`   | `null`  | Absolute path to `supersigil-lsp`. Searches `$PATH` when null. |
