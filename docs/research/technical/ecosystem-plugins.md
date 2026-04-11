# Ecosystem Plugin Research

*April 2026*

## Current State

Two ecosystem plugins exist:

- **Rust** (`supersigil-rust`): `#[verifies("doc-id#criterion-id")]`
  attribute macro on test functions. Discovery via `syn` AST parsing.
- **JS/TS** (`supersigil-js`): `verifies("doc-id#criterion-id")` function
  call in Vitest tests. Discovery via `oxc` AST parsing.

Both implement the `EcosystemPlugin` trait:
```rust
pub trait EcosystemPlugin {
    fn name(&self) -> &'static str;
    fn plan_discovery_inputs(&self, ...) -> Cow<[PathBuf]>;
    fn discover(&self, files: &[PathBuf], scope: &ProjectScope, documents: &DocumentGraph)
        -> Result<PluginDiscoveryResult, PluginError>;
}
```

## Python Plugin

### Annotation Design

```python
@pytest.mark.verifies("auth/req#login-succeeds")
def test_user_login():
    ...
```

Using `@pytest.mark.verifies` is idiomatic pytest. Markers are statically
parseable as decorator AST nodes. The marker must be registered in
`pyproject.toml` to avoid pytest warnings:

```toml
[tool.pytest.ini_options]
markers = ["verifies(ref): links test to supersigil criterion"]
```

A `supersigil-python` PyPI package could provide marker registration and
runtime helpers (similar to `@supersigil/vitest` for JS).

### Parser Choice

Three options for parsing Python AST in Rust:

| Parser | Crate | Downloads | Pros | Cons |
|--------|-------|-----------|------|------|
| **tree-sitter-python** | `tree-sitter-python` v0.25 | 3.5M+ | Stable API, consistent with Go plugin, error-tolerant, on crates.io | Lower-level than typed AST |
| rustpython-parser | `rustpython-parser` | 450K/mo | Typed Python AST, ergonomic | Superseded by Ruff's parser, maintenance mode |
| ruff_python_parser | git dep only | N/A | Fastest, tracks latest syntax | Unstable API, not on crates.io, Ruff team declined stable Rust API |

**Recommendation: tree-sitter-python.** Stable crate, same infrastructure
as the Go plugin (shared tree-sitter foundation), handles partial/broken
files gracefully.

### AST Pattern

The tree-sitter AST for a decorated test function:
```
(decorated_definition
  (decorator
    (call
      function: (attribute object: (attribute) attribute: (identifier))
      arguments: (argument_list (string))))
  definition: (function_definition name: (identifier)))
```

The plugin would walk `decorated_definition` nodes, check if any decorator
is a call to `*.verifies` or `pytest.mark.verifies`, extract the string
argument, and emit a `VerificationEvidenceRecord` with
`EvidenceKind::PythonDecorator`.

### Test File Discovery

Default patterns: `**/test_*.py`, `**/*_test.py`, `**/tests/**/*.py`.
Configurable via `[ecosystem.python]` in `supersigil.toml`:
```toml
[ecosystem.python]
test_patterns = ["tests/**/*.py", "src/**/test_*.py"]
```

## Go Plugin

### The Challenge

Go has no decorators, annotations, or attributes. Tests are functions named
`Test*` in `*_test.go` files.

### Annotation Design

**Comment directives** (recommended):
```go
//verifies:auth/req#login-succeeds
func TestUserLogin(t *testing.T) {
    ...
}
```

This follows Go's established convention (`//go:generate`, `//go:build`,
`//go:embed`). The `//go:` prefix is reserved for the toolchain, so
`//verifies:` is the right form. Statically parseable, no runtime dependency.

**Runtime complement** (Go 1.25+):
```go
func TestUserLogin(t *testing.T) {
    t.Attr("verifies", "auth/req#login-succeeds")
    // ...
}
```

`t.Attr` emits structured metadata in `go test -json` output. Useful for
result-consumption mode but fragile for static detection (variable naming,
helper functions).

**Recommendation:** Comment directives as the primary mechanism. `t.Attr`
as an optional complement when consuming test results.

### Parser

**tree-sitter-go** v0.25 on crates.io. Comments are "extra" nodes as
siblings to `function_declaration`. The plugin walks backward from Test*
functions to find preceding `//verifies:` comments.

### Test File Discovery

Default pattern: `**/*_test.go`. Go conventions are strict here.

## Shared tree-sitter Foundation

Both Python and Go plugins use tree-sitter. A shared utility layer would:
- Provide tree walking helpers (find nodes by type, extract string literals)
- Manage tree-sitter parser lifecycle (one parser per language, reused)
- Share test file filtering logic (gitignore, glob expansion)

This could live in `supersigil-evidence` or a new `supersigil-treesitter`
internal crate.

## JUnit XML Ingestion

For languages without dedicated plugins, JUnit XML provides a potential path:

```xml
<testcase classname="auth_tests" name="test_login" time="0.07">
  <properties>
    <property name="verifies" value="auth/req#login-succeeds"/>
  </properties>
</testcase>
```

Many test frameworks can emit JUnit-style XML, but per-test `<property>`
elements are not standardized the same way across all reporters. Some
frameworks support them natively (JUnit5, pytest via plugins), others emit
only the basic `<testcase>` structure. This means JUnit XML ingestion would
need verification per framework — treat it as "adapter per reporter" rather
than a universal bridge.

A `supersigil ingest --junit results.xml` command would parse
`<property name="verifies" value="..."/>` entries on `<testcase>` elements
and produce `VerificationEvidenceRecord`s with `EvidenceKind::JunitXml`.

**Tradeoffs:**
- Result consumption requires running tests first. Source-level parsing works
  at edit time and integrates with the LSP.
- Property support varies by framework — each needs checking.
- JUnit XML is a complement to source plugins, not a replacement.

## Priority After Python and Go

| Language | Annotation Mechanism | Parser | Market Signal |
|----------|---------------------|--------|--------------|
| **Java/Kotlin** | `@Verifies("ref")` annotation (JUnit5 meta-annotations) | tree-sitter-java/kotlin | Highest enterprise demand, regulated industries |
| **C#/.NET** | `[Verifies("ref")]` attribute (NUnit/xUnit custom attributes) | tree-sitter-c-sharp | Second-highest enterprise demand |
| **Ruby** | `it "...", verifies: "ref"` (RSpec metadata hash) | tree-sitter-ruby | Smaller market, good ergonomics |
| **Swift** | `@Test(.verifies("ref"))` via TestTrait (evolving API) | tree-sitter-swift | iOS/macOS regulated apps |
| **PHP** | `@verifies ref` docblock annotation | tree-sitter-php | Large market, low traceability demand |

Java/Kotlin should follow Python and Go. The JUnit5 annotation system is the
cleanest fit of any language, and enterprise/regulated software is where
traceability demand is highest.

## Convention-Based Mapping (Future)

Auto-map test names to criteria without annotations:
`test_auth_session_expiry` -> `auth/req#session-expiry`

Implementation:
- Configurable naming convention pattern in `supersigil.toml`
- Fuzzy matcher scoring test names against criterion IDs
- Confidence threshold (only map above ~0.8 similarity)
- Distinct `EvidenceKind::Convention` so users know which mappings are
  inferred vs. explicit
- Opt-in per project (not everyone wants implicit mapping)
