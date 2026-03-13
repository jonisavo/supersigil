# Failing Examples Fixture

A supersigil workspace with executable examples designed to fail, for checking
verify output across all runner types.

## Examples

| ID | Runner | Failure type |
|---|---|---|
| `sh-wrong-output` | sh | Body mismatch |
| `sh-bad-exit` | sh | Status + body mismatch |
| `rust-assertion` | cargo-test | Panic exit code 101 |
| `http-refused` | http | Connection refused (no server) |
| `http-wrong-status` | http | 404 vs expected 200 + body |
| `http-wrong-body` | http | JSON body differs (count field) |
| `http-missing-substring` | http | Contains check fails |
| `http-server-error` | http | 500 vs expected 201 + body |

## Usage

```sh
cd tests/fixtures/failing-examples

# Without server (4 non-HTTP examples fail, 4 HTTP get connection refused):
supersigil verify

# With server (all 8 fail with specific match details):
python3 serve.py &
supersigil verify
kill %1
```
