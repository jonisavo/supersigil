---
supersigil:
  id: req/echo
  type: requirement
  status: draft
---

## Echo command spec

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="echo-works">
    The echo command prints its argument to stdout.
  </Criterion>
</AcceptanceCriteria>
<Example id="echo-test" runner="shell">
  <Expected status="0" />
</Example>
<VerifiedBy strategy="example" refs="echo-test" />
```

### Example: echo-test

```sh supersigil-ref=echo-test
echo hello
```

```txt supersigil-ref=echo-test#expected
hello
```
