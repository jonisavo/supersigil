import { describe, it, expect } from "vitest";
import { verifies } from "@supersigil/vitest";
import { isNewerVersion, checkVersionMismatch } from "./version";

describe("isNewerVersion", () => {
  it("returns true when first version is newer", verifies("version-mismatch/req#req-3-3"), () => {
    expect(isNewerVersion("0.7.0", "0.6.0")).toBe(true);
    expect(isNewerVersion("1.0.0", "0.9.0")).toBe(true);
    expect(isNewerVersion("0.6.1", "0.6.0")).toBe(true);
  });

  it("handles double-digit segments correctly", verifies("version-mismatch/req#req-3-3"), () => {
    expect(isNewerVersion("0.10.0", "0.9.0")).toBe(true);
    expect(isNewerVersion("0.9.0", "0.10.0")).toBe(false);
  });

  it("returns false when versions are equal", () => {
    expect(isNewerVersion("0.6.0", "0.6.0")).toBe(false);
  });

  it("returns false when first version is older", () => {
    expect(isNewerVersion("0.5.0", "0.6.0")).toBe(false);
  });

  it("returns false for non-numeric segments", () => {
    expect(isNewerVersion("0.6.0-beta", "0.5.0")).toBe(false);
  });

  it("handles versions with different segment counts", () => {
    expect(isNewerVersion("1.0.0.1", "1.0.0")).toBe(true);
    expect(isNewerVersion("1.0.0", "1.0.0.1")).toBe(false);
  });
});

describe("checkVersionMismatch", () => {
  it("returns skip when server version is undefined", verifies("version-mismatch/req#req-2-2"), () => {
    expect(checkVersionMismatch(undefined, "0.6.0")).toEqual({ kind: "skip" });
  });

  it("returns skip when extension version is undefined", verifies("version-mismatch/req#req-2-2"), () => {
    expect(checkVersionMismatch("0.6.0", undefined)).toEqual({ kind: "skip" });
  });

  it("returns skip when both are undefined", verifies("version-mismatch/req#req-2-2"), () => {
    expect(checkVersionMismatch(undefined, undefined)).toEqual({ kind: "skip" });
  });

  it("returns match when versions are identical", verifies("version-mismatch/req#req-2-1"), () => {
    expect(checkVersionMismatch("0.6.0", "0.6.0")).toEqual({ kind: "match" });
  });

  it("returns mismatch with serverNewer=true when server is ahead", verifies("version-mismatch/req#req-2-1", "version-mismatch/req#req-3-3"), () => {
    const result = checkVersionMismatch("0.7.0", "0.6.0");
    expect(result).toEqual({
      kind: "mismatch",
      serverVersion: "0.7.0",
      extensionVersion: "0.6.0",
      serverNewer: true,
    });
  });

  it("returns mismatch with serverNewer=false when extension is ahead", verifies("version-mismatch/req#req-2-1"), () => {
    const result = checkVersionMismatch("0.5.0", "0.6.0");
    expect(result).toEqual({
      kind: "mismatch",
      serverVersion: "0.5.0",
      extensionVersion: "0.6.0",
      serverNewer: false,
    });
  });
});
