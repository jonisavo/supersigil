# Test Tagging

Use tag-based `VerifiedBy` when the test suite can carry explicit Supersigil evidence.

## Choose a Strategy

- Prefer `strategy="tag"` when you can edit the relevant tests.
- Use `strategy="file-glob"` when file existence is the best available evidence or the tests live in generated/external locations where tags are awkward.

## Ecosystem Evidence Helpers

Prefer ecosystem-native helpers over manual comment tags when the
project's language has one. These integrate with the verification
engine automatically.

### JavaScript / TypeScript (Vitest)

Install `@supersigil/vitest` and use the `verifies()` helper in tests:

```typescript
import { verifies } from '@supersigil/vitest'
import { describe, it, expect } from 'vitest'

it('logs in with valid credentials', verifies('auth/req#req-1-1'), () => {
  // test body
})
```

The discovery engine reads Vitest test metadata via AST parsing.
No manual comments needed.

### Rust

Use the `#[verifies(...)]` attribute macro from `supersigil-rust`:

```rust
use supersigil_rust::verifies;

#[verifies("auth/req#req-1-1")]
#[test]
fn logs_in_with_valid_credentials() {
    // test body
}
```

The discovery engine reads these attributes via `syn` parsing.

### Authoring Guardrail: ESLint Plugin

`@supersigil/eslint-plugin` provides a `valid-criterion-ref` rule that
validates criterion refs at lint time. It catches typos and unknown
refs before `supersigil verify` runs. This is an authoring aid, not
an evidence discovery mechanism.

## Manual Comment Tags (Universal Fallback)

Use manual comment tags when no ecosystem helper exists for the
project's language, or when the test framework does not support
metadata.

### Exact Tag Format

The scanner looks for the literal form:

```text
supersigil: auth-login
```

The tag itself should be stable, short, and specific to the behavior or property being verified.

### Comment Style Examples

Use comment styles the scanner recognizes today:

```rust
// supersigil: auth-login
#[test]
fn logs_in_with_valid_credentials() {}
```

```rust
/// supersigil: auth-login
fn login_property_test() {}
```

```python
# supersigil: auth-login
def test_login_success():
    ...
```

```sql
-- supersigil: auth-login
select 1;
```

```c
/* supersigil: auth-login */
```

### Match the Spec

When using tag-based verification, the spec should carry the same tag:

```mdx
<VerifiedBy strategy="tag" tag="auth-login" />
```

When using file-glob verification, keep the paths concrete:

```mdx
<VerifiedBy strategy="file-glob" paths="tests/auth/login_test.rs" />
```

Do not leave empty `tag=""` or `paths=""` placeholders in the document.
