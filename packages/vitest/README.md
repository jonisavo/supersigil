# @supersigil/vitest

Vitest helper for annotating tests with [Supersigil](https://supersigil.org) criterion refs.

## Installation

```sh
npm install -D @supersigil/vitest
```

## Usage

Use the `verifies()` function to link test cases to specification criteria:

```ts
import { verifies } from '@supersigil/vitest'

it('rejects invalid email', verifies('auth/req#req-1-1'), () => {
  expect(validateEmail('bad')).toBe(false)
})
```

Multiple criteria can be referenced:

```ts
it('sends welcome email', verifies('auth/req#req-2-1', 'notifications/req#req-1-1'), () => {
  // ...
})
```

The returned object can be spread into other test options:

```ts
it('completes within timeout', { ...verifies('perf/req#req-1-1'), timeout: 5000 }, () => {
  // ...
})
```

## How it works

`verifies()` returns a Vitest-compatible metadata object (`{ meta: { verifies: [...] } }`) that Supersigil reads during verification to trace test coverage back to specification criteria.

## License

[MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE)
