---
supersigil:
  id: failures/req
  type: requirements
  status: active
title: "Failing Examples Fixture"
---

## Overview

A fixture spec whose executable examples are designed to fail, for checking
verify output.

<AcceptanceCriteria>
<Criterion id="never-passes">This criterion is never satisfied</Criterion>
<Criterion id="http-contract">HTTP contract verification</Criterion>
</AcceptanceCriteria>

### Shell — wrong output

<Example id="sh-wrong-output" lang="sh" runner="sh" verifies="failures/req#never-passes">

```sh
echo "actual output"
```

<Expected status="0">

```
expected output
```

</Expected>
</Example>

### Shell — non-zero exit

<Example id="sh-bad-exit" lang="sh" runner="sh" verifies="failures/req#never-passes">

```sh
echo "oops" >&2
exit 42
```

<Expected status="0">

```
this is never reached
```

</Expected>
</Example>

### Cargo test — assertion failure

<Example id="rust-assertion" runner="cargo-test" verifies="failures/req#never-passes">

```rust
#[test]
fn failing_test() {
    assert_eq!(1 + 1, 3, "math is broken");
}
```

<Expected status="0" />
</Example>

### HTTP — connection refused

<Example id="http-refused" lang="http" runner="http" verifies="failures/req#never-passes">

```http
GET http://127.0.0.1:1/does-not-exist
Accept: application/json
```

<Expected status="200">

```
{"ok": true}
```

</Expected>
</Example>

### HTTP — wrong status code

Expects 200 but the server returns 404. Start `serve.py` first.

<Example id="http-wrong-status" lang="http" runner="http" env="BASE_URL=http://127.0.0.1:9876" verifies="failures/req#http-contract">

```http
GET /not-found
Accept: application/json
```

<Expected status="200">

```json
{"status": "ok", "count": 42}
```

</Expected>
</Example>

### HTTP — wrong body

Expects a different JSON body than what the server returns.

<Example id="http-wrong-body" lang="http" runner="http" env="BASE_URL=http://127.0.0.1:9876" verifies="failures/req#http-contract">

```http
GET /ok
Accept: application/json
```

<Expected status="200">

```json
{"status": "ok", "count": 99}
```

</Expected>
</Example>

### HTTP — wrong body (contains check)

The response body doesn't contain the expected substring.

<Example id="http-missing-substring" lang="http" runner="http" env="BASE_URL=http://127.0.0.1:9876" verifies="failures/req#http-contract">

```http
GET /ok
Accept: application/json
```

<Expected status="200" contains="not-in-response" />
</Example>

### HTTP — unexpected server error

Expects a 201 but gets a 500.

<Example id="http-server-error" lang="http" runner="http" env="BASE_URL=http://127.0.0.1:9876" verifies="failures/req#http-contract">

```http
GET /server-error
```

<Expected status="201">

```
created successfully
```

</Expected>
</Example>
