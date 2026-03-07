# Test Tagging

Use tag-based `VerifiedBy` when the test suite can carry explicit Supersigil evidence.

## Choose a Strategy

- Prefer `strategy="tag"` when you can edit the relevant tests.
- Use `strategy="file-glob"` when file existence is the best available evidence or the tests live in generated/external locations where tags are awkward.

## Exact Tag Format

The scanner looks for the literal form:

```text
supersigil: auth-login
```

The tag itself should be stable, short, and specific to the behavior or property being verified.

## Comment Style Examples

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

## Match the Spec

When using tag-based verification, the spec should carry the same tag:

```mdx
<VerifiedBy strategy="tag" tag="auth-login" />
```

When using file-glob verification, keep the paths concrete:

```mdx
<VerifiedBy strategy="file-glob" paths="tests/auth/login_test.rs" />
```

Do not leave empty `tag=""` or `paths=""` placeholders in the document.
