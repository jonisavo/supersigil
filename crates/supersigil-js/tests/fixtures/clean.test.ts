// A valid test file with no verifies() annotations.
import { describe, it, expect } from "vitest";

describe("math", () => {
  it("adds numbers", () => {
    expect(1 + 2).toBe(3);
  });
});
