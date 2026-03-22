---
supersigil:
  id: lsp/adr
  type: adr
  status: accepted
title: "LSP Architecture Decisions"
---

```supersigil-xml
<References refs="lsp/req, lsp/design" />
```

## Context

Supersigil needs a language server to provide diagnostics, go-to-definition,
autocomplete, and hover for Markdown spec files. Key architectural choices
affect crate boundaries, runtime characteristics, framework selection, and
how the server integrates with Markdown editing without interfering with
general content.

```supersigil-xml
<Decision id="decision-1">
  Implement the LSP server as a separate `supersigil-lsp` crate with its
  own binary, not as a subcommand of the CLI.

  <References refs="lsp/req#req-5-1" />

  <Rationale>
    LSP servers are long-running, stateful, and async. The CLI is run-once
    and batch. Coupling them in one binary forces the CLI to carry async
    runtime and LSP framework dependencies it does not need. A separate
    binary keeps the CLI lean, lets editors launch the LSP process
    independently, and allows independent release cadence.
  </Rationale>

  <Alternative id="cli-subcommand" status="rejected">
    Add `supersigil lsp` as a CLI subcommand. Simpler distribution (one
    binary) but couples fundamentally different lifecycle models. The CLI
    binary would grow significantly in size and dependency surface for a
    feature most CLI users never invoke.
  </Alternative>
</Decision>

<Decision id="decision-2">
  Use `async-lsp` as the LSP framework.

  <References refs="lsp/req#req-5-1, lsp/req#req-5-3" />

  <Rationale>
    `async-lsp` processes notifications sequentially and requests
    concurrently, matching the LSP specification. Notification handlers get
    `&amp;mut self`, eliminating `Arc&lt;RwLock&lt;&gt;&gt;` boilerplate for state updates.
    Tower middleware provides composable lifecycle management. The smaller
    ecosystem compared to `tower-lsp-server` is acceptable given the
    straightforward scope of four features on a single file type.
  </Rationale>

  <Alternative id="tower-lsp-server" status="rejected">
    Actively maintained community fork of `tower-lsp`. Largest ecosystem
    and most examples, but inherits a notification ordering bug where
    `didChange` can race with `completion` requests. All handlers receive
    `&amp;self`, requiring interior mutability for every state mutation.
  </Alternative>

  <Alternative id="lsp-server-crate" status="rejected">
    Minimal scaffold from rust-analyzer. Correct notification ordering and
    full control, but requires significant manual dispatch boilerplate and
    no async runtime. More code to maintain for no benefit at this scope.
  </Alternative>
</Decision>

<Decision id="decision-3">
  Use hybrid re-indexing: single-file re-parse on `didChange`, full
  `DocumentGraph` rebuild on `didSave`.

  <References refs="lsp/req#req-5-3, lsp/req#req-5-4, lsp/req#req-1-1, lsp/req#req-1-2" />

  <Rationale>
    Re-parsing one file on each keystroke gives instant local feedback
    (parse errors, component validation) without touching other files or
    the graph. Rebuilding the graph on save ensures cross-document analysis
    (broken refs, cycles, coverage) is consistent with what is on disk.
    This mirrors the per-file / cross-crate split used by rust-analyzer.
  </Rationale>

  <Alternative id="save-only" status="rejected">
    Re-parse only on save. Simpler, but loses live-as-you-type diagnostics
    and makes the authoring experience feel sluggish.
  </Alternative>

  <Alternative id="full-rebuild-on-change" status="rejected">
    Rebuild the graph on every change. Correct, but rebuilds all files on
    each keystroke. Wasteful and degrades responsiveness in large repos.
  </Alternative>
</Decision>

<Decision id="decision-4">
  Register for both `markdown` and `mdx` language IDs, using fence-aware
  context detection to scope features to `supersigil-xml` fenced blocks.

  <References refs="lsp/req#req-7-1, lsp/req#req-7-2, lsp/req#req-7-3" />

  <Rationale>
    Spec files are standard Markdown with `supersigil-xml` fences. Registering
    for both `markdown` and `mdx` lets the server work regardless of how
    editors classify the files. Fence-aware context detection
    (`is_in_supersigil_fence`) ensures that completions, hover, and definition
    only trigger inside fenced blocks and frontmatter, so the server does not
    interfere with general Markdown editing or other language servers. Modern
    editors (VS Code, Neovim, Zed) support multiple language servers per file
    type and merge their contributions cleanly.
  </Rationale>

  <Alternative id="custom-language-id" status="rejected">
    Register for a custom `supersigil` language ID. Cleanly avoids
    multi-server merging, but users lose all general Markdown tooling on spec
    files unless they configure dual associations manually.
  </Alternative>
</Decision>

<Decision id="decision-5">
  Offer two configurable diagnostics tiers (`lint` and `verify`) with
  `verify` as the default.

  <References refs="lsp/req#req-1-3, lsp/req#req-5-7, lsp/req#req-6-1" />

  <Rationale>
    The verification pipeline (with real evidence from VerifiedBy tags
    and file globs) runs in milliseconds, making it viable as a default
    on-save action. A `lint` tier gives users a faster option that skips
    evidence discovery. A `full` tier for executable examples was
    considered but deferred — example execution takes seconds and the
    LSP does not yet integrate the example executor.
  </Rationale>

  <Alternative id="single-tier" status="rejected">
    Always run verification. Users with complex repos or slow file
    systems cannot opt into a lighter mode.
  </Alternative>

  <Alternative id="three-tiers" status="rejected">
    A third `full` tier for example execution was originally planned but
    dropped because the example executor is not yet wired into the LSP
    and adding an unused tier created confusion.
  </Alternative>
</Decision>
```

## Consequences

- A new `supersigil-lsp` binary must be distributed alongside the CLI.
  Editor extensions need to locate and launch this binary.
- The `async-lsp` dependency introduces `tokio` into the LSP crate (but
  not the CLI or other crates).
- Prerequisite changes to `supersigil-parser` (in-memory parsing API) and
  `supersigil-core` (config field) are needed before LSP work can start.
- The hybrid re-indexing strategy means cross-document diagnostics may be
  stale between saves. Users see per-file issues immediately but must save
  to get updated ref resolution and coverage analysis.
- Registering for both `markdown` and `mdx` means the server may run
  alongside other language servers. Fence-aware context detection ensures
  Supersigil features only activate inside `supersigil-xml` fences,
  avoiding interference with general Markdown editing.
