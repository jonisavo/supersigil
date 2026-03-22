---
supersigil:
  id: demo/req
  type: requirements
  status: draft
title: "Demo Requirements"
---

## Overview

A demo spec for testing executable examples.

```supersigil-xml
<References refs="demo/helper" />

<AcceptanceCriteria>
<Criterion id="demo-1">Demo criterion for testing</Criterion>
</AcceptanceCriteria>

<Example id="echo-test" lang="sh" runner="sh" verifies="demo/req#demo-1">
  <Expected status="0" />
</Example>

<Example id="rust-test" runner="cargo-test" verifies="demo/req#demo-1">
  <Expected status="0" contains="rust-example-pass" />
</Example>
```

```sh supersigil-ref=echo-test
echo hello
```

```text supersigil-ref=echo-test#expected
hello
```

```rust supersigil-ref=rust-test
#[test]
fn rust_test() {
    println!("rust-example-pass");
}
```
