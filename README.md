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

Supersigil is being rebuilt. The new tool records what agent sessions did
to a repository, links every changed span to the recorded edits and the
rationale behind them, keeps what was tried and reverted, and lets you
review a change in the terminal with that history beside the diff. It stays
honest about what the evidence does and does not show. The design is in
[`docs/research/multiverse-review-design.md`](docs/research/multiverse-review-design.md).

The previous Supersigil, a spec-driven development tool with a
verification engine, an LSP server, and editor extensions, lives at the
`archive/spec-driven` tag and in the v0.14.0 releases on crates.io,
Homebrew, AUR, and the VS Code and JetBrains marketplaces. Its docs site is
not maintained.

## Status

Only shell completions exist in this tree today. The first arc lands in
three steps: the local record of agent sessions, the review model with
attribution, and a terminal review.

## Development

```sh
mise setup     # once
mise qa        # format, lint, build, test
```

## License

MIT OR Apache-2.0.
