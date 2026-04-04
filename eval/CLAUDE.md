
Default to using pnpm and vitest.

- Use `pnpm install` to install dependencies
- Use `pnpm test` or `vitest run` to run tests

## Testing

Use `vitest` to run tests.

```ts#index.test.ts
import { test, expect } from "vitest";

test("hello world", () => {
  expect(1).toBe(1);
});
```
