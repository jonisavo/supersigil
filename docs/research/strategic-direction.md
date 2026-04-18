# Strategic Direction

*April 2026*

## Current Position

Supersigil is at v0.2.0 (as of April 2026) with ~270 commits, 3 releases,
and effectively zero public visibility. The technology
is differentiated -- no other tool combines deterministic verification,
specs-in-the-repo, criterion-level traceability, ecosystem plugins, and LSP
integration. The challenge is polish and distribution.

## Core Insights

**Skills are the moat.** Supersigil's agent skills are a stronger integration
mechanism than most teams realize. They teach agents *when* and *why* to use
supersigil commands, not just *how*. This is more valuable than raw tool
availability (MCP) because agents need workflow guidance, not just API access.

**Living specs vs. stable specs is a false dichotomy.** Supersigil shouldn't
position itself as the "stable spec" tool in opposition to "living spec"
tools like Intent/Augment. The real differentiator is *verified* specs.
Living specs without verification silently drift. Supersigil's graph makes
living specs *auditable*: `supersigil affected` detects which documents'
TrackedFiles match changed source files (plus one-hop transitive refs),
agent skills encourage updating specs during implementation, and
`supersigil verify` confirms the updates are structurally consistent.

Today `affected` covers TrackedFiles-matched changes but not evidence-only
changes (test files) or semantic drift. Extending `affected` to cover
evidence is a natural next step (see "Extending affected" below). The goal
is to let teams move along the spectrum — from stable to living — with
growing confidence as the detection surface expands.

## Priority Sequencing

### Tier 1: Polish (Immediate)

The things that determine whether someone who tries supersigil keeps using
it. See [polish-audit.md](polish-audit.md) for full details.

**High impact, low effort:**
- Verify progress feedback and timing
- Error message improvements (config not found context, cycle paths,
  "did you mean?" suggestions)
- Init skills installation feedback (list what was installed)
- New command: derive title from slug, validate ID against pattern
- Clean verify message showing what was checked

**Medium effort:**
- Verify summary breakdown by rule type
- Troubleshooting / FAQ documentation page
- Config generation with commented-out examples

### Tier 2: Developer Experience Features (Next Quarter)

**Watch mode** -- transforms the authoring experience. Users currently
switch between editor and terminal to run verify manually. Watch mode makes
the verify loop continuous. See [technical/watch-mode.md](technical/watch-mode.md).

**`--fix` flag** -- reduces friction for common structural issues. The LSP
code actions already implement most fixes; the `--fix` flag applies them in
batch. Missing attributes, ID ordering, broken refs with obvious corrections.

**GitHub Action** (`supersigil/setup-action`) -- makes CI adoption
frictionless. Install binary, run verify, format as GitHub annotations,
set check status. Currently users must write their own workflow file
(`.github/workflows/verify.yml` exists as an example but isn't reusable).

**Structured CI output** -- SARIF format for GitHub Security tab, GitLab
Code Quality format. Transforms verify findings from "CI log output" into
"integrated code review annotations."

### Tier 3: Ecosystem Expansion (Following Quarter)

**Python plugin** -- largest test ecosystem after JS. Strong demand in
ML/data/science contexts with regulatory requirements.
See [technical/ecosystem-plugins.md](technical/ecosystem-plugins.md).

**Go plugin** -- cloud-native infrastructure companies increasingly need
compliance traceability.

**JUnit XML ingestion** -- universal bridge for languages without dedicated
plugins. Immediate support for any language with a JUnit-compatible reporter.

**Editor expansion** -- Neovim (nvim-lspconfig snippets, Treesitter grammar)
and Zed (native LSP extension). Low effort since the LSP server is complete.

### Tier 4: MCP Server (When Needed)

The CLI with `--format json` plus agent skills already provides most of what
an MCP server would offer. The incremental value is discoverability in
non-Claude-Code contexts and avoiding shell-out overhead.
See [technical/mcp-server.md](technical/mcp-server.md).

Worth doing when:
- Teams are actively requesting it
- The tool surface is stable (avoid versioning churn)
- There's time to do it well (good tool descriptions, token-conscious
  responses, proper resource templates)

Not urgent because skills handle the workflow guidance that MCP cannot.

### Living Spec Support (Cross-Cutting)

Not a tier but a thread running through all tiers. The goal is to make
supersigil equally useful for teams that treat specs as stable references
and teams that update specs continuously alongside code.

**What already exists:**
- `supersigil affected --since <ref>` detects which specs are stale after
  code changes. This is the foundation.
- Agent skills (ss-feature-development) already encourage updating `<Task
  status="...">` and spec docs when implementation reveals changes.
- `status: draft` gating lets teams work iteratively without breaking CI.

**What could be added:**
- **Affected-doc overlap as a first-class signal.** When `affected` shows a
  spec is impacted but `verify` only reports it as advisory context, there is
  still room for richer review guidance. A stronger affected-doc workflow that
  summarizes what changed and why the doc is in scope would help.
- **Spec update hints in verify output.** When verification finds affected
  specs, suggest which sections likely need updating based on which tracked
  files changed.
- **Agent skill for spec maintenance.** A dedicated skill (not just guidance
  in ss-feature-development) that agents invoke after implementation to review
  and update affected specs. The skill would run `affected`, read the changed
  specs, and update criteria/tasks/status to match the new reality.
- **Spec changelog tracking.** When specs change, record what changed and
  why (in git commit messages or a structured field). This gives living specs
  an audit trail that stable specs get for free by not changing.

### Extending `affected` (Cross-Cutting)

Today `affected --since <ref>` detects documents whose TrackedFiles globs
match changed files, plus one-hop transitive references. This covers
implementation-file impact but misses several important change categories:

**Evidence-file changes.** If a test file changes (new test added, existing
test modified, `#[verifies]` annotation updated), the documents whose
criteria that test covers should be flagged. This requires mapping changed
files against VerifiedBy globs and ecosystem plugin discovery inputs.

Important: this is a *different signal* from TrackedFiles overlap.
TrackedFiles overlap means "implementation may have drifted from the
spec." Evidence changes mean "the verification surface changed and should
be reviewed." These should be reported as distinct categories — e.g.
`evidence_changed` vs `stale` — not folded into the same semantics.
Conflating them would muddy what `affected` means and make output harder
to act on. A changed test doesn't imply the owning spec is stale; it
implies the evidence supporting its criteria changed.

**Spec-document changes.** If a spec file itself changes, all documents
that reference it (Implements, DependsOn, References) may need review.
Today `affected` only looks at TrackedFiles, not spec-to-spec relationships
triggered by spec file changes.

**Multi-hop transitive detection.** Currently stops at one hop. If A's
TrackedFiles changed, and B references A, B is flagged — but C which
references B is not. Full transitive closure would catch deeper ripple
effects, though the value diminishes with depth.

**Concrete steps:**
1. Add evidence-file detection to `affected` as a distinct signal
   (`evidence_changed`). Map changed files against VerifiedBy file-glob
   patterns and plugin discovery inputs. Report separately from
   TrackedFiles overlap.
2. Add spec-file detection. If a `.md` file in configured paths changed,
   flag it and its one-hop references.
3. Consider configurable transitive depth (default 1, optional full
   closure).

This directly enables the living-spec workflow: agents run
`affected --since HEAD~1` after implementation, see which specs need
attention (distinguishing "implementation drifted" from "evidence changed"),
and update accordingly. It also informs watch mode's invalidation logic —
each signal category maps to a different re-verification scope.

### Tier 5: Longer-Term Vision

**Convention-based mapping** -- zero-annotation test-to-criterion mapping
via naming conventions. Powerful for disciplined teams but needs careful
design to avoid false positives.

**Reverse traceability** -- generate specs from existing test suites. The
`ss-retroactive-specification` skill is a start; a `supersigil analyze` command
could provide structured input for the skill.

**WASM plugins** -- custom verification rules and evidence discovery via
Extism. Enables organization-specific rules and community plugins.

**Test body rendering** -- show test source alongside criteria in rendered
specs. Tests become living examples. Valuable for onboarding, audits, and
AI agent context.

**Traceability matrix export** -- CSV/PDF for regulatory audits. Bridges
the gap between supersigil's repo-native approach and compliance processes
that expect traditional document formats.

## Gaps Not Covered Elsewhere

**Dogfooding.** Supersigil verifies its own specs in CI. Specific learnings
from self-hosting should be captured as they surface — what rules fire most,
what workflow friction exists, what the eval scenarios reveal about agent
usability.

**Community infrastructure.** No Discord/Matrix, no GitHub Discussions tab,
no CONTRIBUTING.md. AGENTS.md and CLAUDE.md exist but are agent-facing, not
contributor-facing. If visibility efforts succeed, new users have nowhere to
ask questions or report friction. Setting up Discussions before a launch
post is necessary.

**Sustainability.** Open source (MIT/Apache-2.0) with no revenue model.
Not urgent at v0.2 but worth thinking about before v1: sponsorships,
paid enterprise features (SAML, audit export, SLA), consulting/training,
or grant funding.

## Growth Strategy

### Wedge Markets

1. **Kiro users.** Supersigil imports Kiro format. Position as the
   verification layer Kiro doesn't provide. Publish an integration guide.

2. **Spec Kit users.** ~85k+ stars, no verification. Supersigil as the CI
   enforcement layer.

3. **Small regulated teams.** Enterprise RM tools cost thousands per seat.
   Supersigil provides traceability at zero cost for teams that need it for
   SOC 2, ISO 26262, IEC 62304 compliance but can't justify Jama/DOORS.

4. **Rust ecosystem.** No competing tool exists. The `#[verifies]` attribute
   is novel. Rust teams that care about testing discipline are a natural fit.

### Visibility

The tool has zero public presence. Actions needed:

- **Hacker News launch post** -- the competitive landscape analysis provides
  strong framing. "The only tool that verifies your specs stay true."
- **Blog posts** -- "Why we built supersigil," "Spec-driven development with
  AI agents," "Requirements traceability without enterprise tooling."
- **Conference talks** -- RustConf, local meetups, AI/developer tooling
  conferences.
- **Documentation quality** -- the website is solid; filling the gaps
  (troubleshooting, migration guide, custom components) would make it
  comprehensive.

### The Narrative

The strongest framing for supersigil:

> 96% of developers don't fully trust AI-generated code, yet only 48%
> verify it. Supersigil closes that gap. Write your requirements as Markdown.
> Link your tests to criteria. Let CI tell you when specs and code diverge.
> No SaaS, no separate system of record, no thousand-dollar seats. Just
> specs as code, verified by default.

This hits the verification gap (Sonar research), the anti-SaaS positioning
(developer preference for repo-native tools), and the AI-age relevance
(specs that agents can read and verify against).
