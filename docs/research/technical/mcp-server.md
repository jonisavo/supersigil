# MCP Server Research

*April 2026*

## The Real Question

The CLI with `--format json` already functions as an MCP server in practice:
structured input (command + flags), structured output (JSON), stateless
calls. Agent skills already guide agents to the right CLI commands. So what
would an actual MCP server add?

### What MCP Buys

1. **Discoverability.** Agents see `supersigil_verify` in their tool list
   without needing a skill to tell them it exists. However, MCP config
   locations vary by client: Claude Code uses project `.mcp.json`, VS Code
   uses `.vscode/mcp.json`, Codex CLI uses `codex mcp add` /
   `~/.codex/config.toml`, Cursor has its own pattern. Distribution would
   need per-client config snippets or documentation, not a single file.

2. **No shell-out overhead.** In-process tool call vs. spawning a subprocess,
   parsing stdout, handling exit codes. Marginal for single calls, noticeable
   in tight verify-fix-verify loops.

3. **Ecosystem presence.** Being in the MCP registry (`registry.modelcontextprotocol.io`)
   is a distribution channel. 50+ official servers, 150+ community ones.

4. **Token efficiency.** MCP tool schemas are self-describing. Agents don't
   need skill text explaining `--format json` flags; the tool parameters
   handle it.

### What MCP Doesn't Buy (That Skills Already Provide)

1. **Workflow guidance.** Skills teach agents *when* and *why* to call
   supersigil commands, not just *how*. An MCP tool description can't encode
   "check status before coding" or "run verify after every spec edit."

2. **Progressive complexity.** Skills reveal commands as they become relevant.
   MCP dumps the entire tool surface at once (consuming 550-1,400 tokens per
   tool in the context window).

3. **Cross-tool orchestration.** Skills compose supersigil calls with git,
   cargo, and editor commands. MCP servers are isolated.

### Verdict

An MCP server is a modest incremental improvement over CLI + skills, not a
transformative feature. It's worth doing eventually (the implementation is
thin), but it's not the highest-leverage item on the roadmap. Skills are the
more powerful abstraction for agent integration.

The strongest argument for MCP is discoverability in agent-agnostic contexts
-- teams not using Claude Code (and thus not using supersigil skills) would
still get tool access via `.mcp.json`.

## Implementation Notes (When Ready)

### Rust SDK

Use `rmcp` (official MCP Rust SDK, 4.7M+ downloads). It provides:
- `#[tool]` macro for JSON schema generation from parameter structs
- `#[tool_router]` macro for request dispatch
- `ServerHandler` trait for server lifecycle
- stdio transport (what Claude Code uses)
- tokio async runtime (already in the workspace)

New crate: `crates/supersigil-mcp`, depending on core/verify/evidence crates.

### Tool Surface

Keep it narrow (5-8 tools). Best practice from Block's playbook: design
around what the agent wants to *achieve*, not CLI command parity.

| MCP Tool | Maps to CLI | Notes |
|----------|------------|-------|
| `supersigil_verify` | `verify --format json` | The core tool. Returns structured findings. |
| `supersigil_get_context` | `context <id> --format json` | Document + relationships + criteria. |
| `supersigil_get_plan` | `plan [prefix] --format json` | Outstanding work queue. |
| `supersigil_list_affected` | `affected --since <ref> --format json` | Change-impact analysis. |
| `supersigil_list_documents` | `ls --format json` | Document inventory. |
| `supersigil_get_status` | `status [id] --format json` | Coverage overview. |

**Do NOT expose** as tools: `init`, `new`, `import`, `skills install`,
`explore`, `render`. These are interactive or side-effectful.

### Resources (Optional)

| URI | Content |
|-----|---------|
| `supersigil://schema` | Component/type definitions |
| `supersigil://document/{id}` | Document context (resource template) |

### Distribution

Consider generating MCP config during `supersigil init`. Since config
locations differ per client (`.mcp.json` for Claude Code, `.vscode/mcp.json`
for VS Code, etc.), this would either need to detect the client or generate
multiple files. A simpler approach: document the config snippet in the
`init` success output and on the website, letting users add it to their
client's config location.

### Existing MCP Servers in the Space

Only three spec-related MCP servers exist, all focused on *generating* specs
via prompts, not *verifying* them:

- `mcp-server-spec-driven-development` (formulahendry) -- 3 sequential
  prompts, no tools, very lightweight
- `spec-workflow-mcp` (Pimzino) -- more sophisticated, has a web dashboard,
  but still no deterministic verification
- Azure DevOps MCP Server -- natural language requirements tracking

A supersigil MCP server would be the first to offer machine-verifiable
specification conformance as an MCP tool.
