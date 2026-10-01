<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="media/logo.png" />
    <source media="(prefers-color-scheme: light)" srcset="media/logo_light.png" />
    <img alt="Supersigil" src="media/logo.png" width="400" />
  </picture>
</p>

<p align="center">
  Review agent-made changes with the reasoning that produced them.
</p>

---

Supersigil records agent activity and connects changed lines to recorded
edits. It reports missing evidence, competing explanations, and incomplete
searches alongside the change. See the [architecture](docs/architecture.md)
for the evidence model and attribution rules.

The previous Supersigil, a spec-driven development tool with a
verification engine, an LSP server, and editor extensions, lives at the
`archive/spec-driven` tag and in the v0.14.0 releases on crates.io,
Homebrew, AUR, and the VS Code and JetBrains marketplaces. Its docs site is
not maintained.

## Status

The current CLI imports Claude Code sessions, reviews working-tree or commit
changes, and explains individual lines. It prints JSON or a plain terminal
summary. Sourced annotations, human judgments, and an interactive terminal
review remain planned.

```sh
supersigil session sync
supersigil review --format terminal
supersigil review --base main --target HEAD --format json
supersigil why src/lib.rs:12 --format terminal
```

`review` and `why` reconcile relevant records before reading the change.
Untracked files are excluded from a review unless named with
`--include-untracked PATH`. Unattributed lines remain visible.

## Development

```sh
mise setup     # once
mise qa        # format, lint, build, test
```

## License

MIT OR Apache-2.0.
