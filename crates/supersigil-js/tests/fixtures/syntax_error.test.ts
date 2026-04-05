// A test file with intentional syntax errors for fault-tolerance testing.
import { verifies } from "@supersigil/vitest";
import { describe, it } from "vitest";

describe("broken test", () => {
  it("has a syntax error", verifies("auth/req#req-1"), () => {
    const x = {{{;
  });
});
