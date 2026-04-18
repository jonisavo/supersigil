# Competitive Landscape Analysis

*April 2026*

## Summary

Supersigil occupies a unique position: no other tool combines deterministic
spec-to-test verification, specs-in-the-repo, criterion-level traceability,
ecosystem test plugins, and LSP integration. The landscape splits into tools
that *generate* specs (but don't verify them) and tools that *verify code
quality* (but not requirements conformance).

## Direct Competitors

### Kiro (AWS)

IDE and CLI enforcing a three-phase workflow: Requirements (EARS notation),
Design, and Tasks. Specs live in `.kiro/specs/`. Powered by Claude on
Bedrock.

**Key difference:** Kiro creates specs and uses them to guide AI generation,
but has no automated verification of spec-to-code alignment. No CI gate, no
traceability from requirements to tests, no LSP for spec files. It is an IDE
with a workflow, not a verification framework.

**Opportunity:** Supersigil already imports Kiro format
(`supersigil import --from kiro`). The natural positioning: "Kiro creates
your specs. Supersigil verifies they stay true." A dedicated integration
guide would make this concrete.

### GitHub Spec Kit

Open source (MIT), ~85k+ stars (as of April 2026). CLI workflow scaffold: Constitution, Specify,
Plan, Tasks. Works with Copilot, Claude Code, Gemini CLI, Cursor, etc.

**Key difference:** A workflow orchestrator, not a verification system. No CI
gate, no test mapping, no traceability. Uses checklists as "definition of
done" but acknowledges agents frequently ignore them. A third-party
`spec-kit-sync` extension attempts drift detection but is a community add-on.

**Opportunity:** Spec Kit users have no verification layer. Supersigil could
position as the CI enforcement for Spec Kit projects.

### Tessl

Private/closed beta. Aspires to spec-as-source: one-to-one spec-to-code
mappings, generated code marked `// GENERATED FROM SPEC - DO NOT EDIT`.
Offers a Spec Registry with 10,000+ pre-built specs for open source
libraries.

**Key difference:** Fundamentally different model where humans never touch
code. Unproven at scale. No public CI integration or verification pipeline.
The Spec Registry (preventing API hallucinations) is orthogonal to
supersigil's purpose.

### Augment Code Intent

Public beta, $20-200/month. Mac desktop app for multi-agent orchestration
with "living specs" that auto-update as agents implement. Coordinator,
Implementor, and Verifier agents.

**Key difference:** Verification is AI-driven and probabilistic, not
deterministic and structural. Proprietary platform with no CI integration.
"Living specs" auto-update, but without structural verification there's no
guarantee the updates are consistent. Supersigil can support the same living
spec workflow — agent skills encourage spec updates during implementation —
but with deterministic verification that the updates are sound.

### OpenSpec

[github.com/Fission-AI/OpenSpec](https://github.com/Fission-AI/OpenSpec).
Open source (MIT). Proposal-first framework for brownfield changes. Delta-
based markers (ADDED/MODIFIED/REMOVED). Has `openspec validate --strict`
for CI.

**Key difference:** Validates spec document *structure* (syntax, GIVEN/WHEN/
THEN completeness), not spec-to-code traceability. No test mapping, no LSP.
Purpose-built for modifications, narrower scope.

### BMAD Method

Free, open source. 21 specialized AI agent personas (Analyst, Architect,
Scrum Master, etc.) for structured development.

**Key difference:** A process framework, not a verification tool. Creates
documentation artifacts but doesn't check them post-creation.

## Traditional Requirements Management

The enterprise RM market (estimated ~$1.5-2B, varies by source) includes Jama Connect, codeBeamer,
Helix RM, Visure Requirements, SpiraTeam, and IBM DOORS. All are separate
web-based systems of record disconnected from the code repository. They
require manual synchronization, cost thousands per seat per year, and target
compliance-heavy industries (automotive, aerospace, medical).

Supersigil's everything-in-the-repo approach is fundamentally different and
could serve smaller regulated teams at zero cost.

## Adjacent Verification Tools

| Tool | What it does | Gap supersigil fills |
|------|-------------|---------------------|
| Specmatic | OpenAPI/AsyncAPI contract tests in CI | API-domain only, no requirements hierarchy |
| Dokken | Architecture drift detection via questions | Architecture decisions only, no test mapping |
| BDD (Cucumber, SpecFlow, Gauge) | Natural-language scenarios to executable tests | Limited to behavior scenarios, no requirements/design/tasks hierarchy |
| SonarQube | Code quality rules in CI (has MCP server) | Checks quality, not requirements conformance |
| ESLint | Linting rules (has MCP server) | Code patterns, not spec traceability |

## Differentiation Matrix

| Capability | Supersigil | Kiro | Spec Kit | Intent | OpenSpec |
|---|---|---|---|---|---|
| Specs in repo (not SaaS) | Yes | Yes | Yes | No | Yes |
| Typed/verifiable graph | Yes | No | No | No | No |
| Criterion-to-test tracing | Yes | No | No | AI-based | No |
| CI verification gate | Yes | No | No | No | Partial |
| LSP for spec files | Yes | No | No | No | No |
| Ecosystem test plugins | Yes | No | No | No | No |
| Spec drift detection | Yes | No | Third-party | AI-based | Partial |
| Open source | Yes | No | Yes | No | Yes |

## Test Reporting Tools

### Allure Report

Well-established test reporting framework with per-language annotations
(`@allure.link` in Python, `@Link`/`@Issue` in Java/JUnit5). Generates
rich HTML reports from test execution results. Supports traceability to
requirements via link annotations.

**Key difference:** Post-execution only — requires running the test suite
to produce reports. No static analysis, no CI gate on spec conformance, no
edit-time feedback. Allure knows which tests *claim* to verify a requirement
but can't check whether the requirement *has* test coverage without running
the suite. Supersigil's source-level analysis works before execution and
integrates with the LSP for live feedback.

**Relevance:** Allure is well-known in the Java/enterprise world. Teams
familiar with Allure's `@Link` annotations would find supersigil's
`#[verifies]` / `verifies()` pattern familiar. A comparison page could help
Allure users understand supersigil's value-add.

## Rust Ecosystem

There is no Rust-native tool for requirements-to-test traceability. The Rust
verification landscape focuses on formal/mathematical verification (Kani,
Creusot, Verus, Aeneas). The `#[verifies("doc-id#criterion-id")]` attribute
is novel in the Rust ecosystem.

## Market Trends

- **The verification gap**: 96% of developers don't fully trust AI-generated
  code, yet only 48% verify it (Sonar research). Supersigil addresses this
  directly.
- **Growing AI adoption** in requirements engineering (multiple surveys
  report >50% of practitioners using AI) creates demand for machine-readable,
  verifiable spec formats.
- **Living specs vs. stable specs**: Often framed as opposing philosophies,
  but this is a false dichotomy. The real question is whether spec changes
  are *verified*. Intent/Augment auto-updates specs but has no structural
  check that the updates are consistent. Supersigil's verification graph
  makes spec evolution *auditable*: `supersigil affected` detects which
  documents' TrackedFiles match changed source files (plus one-hop transitive
  refs), agent skills encourage updating specs during implementation, and
  `supersigil verify` confirms structural consistency. Today `affected`
  covers implementation-file impact but not evidence-only changes or
  semantic drift — extending it to cover evidence files is a planned
  improvement (see strategic-direction.md). Teams can choose stable specs
  or living specs; the infrastructure supports both, with the detection
  surface growing over time.
- **Multi-agent verification loops**: Teams running multiple AI agents need
  verification gates. Spec-driven gates are recognized as necessary
  infrastructure.
