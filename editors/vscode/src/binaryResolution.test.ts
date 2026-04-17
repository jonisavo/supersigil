import { describe, expect, it } from "vitest";
import { verifies } from "@supersigil/vitest";
import {
  defaultServerBinaryCandidates,
  pathLookupCommand,
  serverBinaryName,
} from "./binaryResolution";

describe("serverBinaryName", () => {
  it(
    "uses a native exe name on Windows",
    verifies("vscode-extension/req#req-1-2", "vscode-extension/req#req-1-4"),
    () => {
      expect(serverBinaryName("win32")).toBe("supersigil-lsp.exe");
    },
  );

  it("keeps the Unix binary name on macOS and Linux", () => {
    expect(serverBinaryName("darwin")).toBe("supersigil-lsp");
    expect(serverBinaryName("linux")).toBe("supersigil-lsp");
  });
});

describe("pathLookupCommand", () => {
  it(
    "uses where.exe with the native Windows executable name",
    verifies("vscode-extension/req#req-1-4"),
    () => {
      expect(pathLookupCommand("win32")).toBe("where.exe supersigil-lsp.exe");
    },
  );

  it("uses which on Unix-like hosts", () => {
    expect(pathLookupCommand("linux")).toBe("which supersigil-lsp");
    expect(pathLookupCommand("darwin")).toBe("which supersigil-lsp");
  });
});

describe("defaultServerBinaryCandidates", () => {
  it(
    "prefers the native cargo bin path on Windows",
    verifies("vscode-extension/req#req-1-2", "vscode-extension/req#req-1-4"),
    () => {
      expect(
        defaultServerBinaryCandidates("C:\\Users\\example-user", "win32"),
      ).toEqual([
        "C:\\Users\\example-user\\.cargo\\bin\\supersigil-lsp.exe",
      ]);
    },
  );

  it(
    "keeps Unix fallback candidates unchanged",
    verifies("vscode-extension/req#req-1-2"),
    () => {
      expect(defaultServerBinaryCandidates("/home/example-user", "linux")).toEqual([
        "/home/example-user/.cargo/bin/supersigil-lsp",
        "/home/example-user/.local/bin/supersigil-lsp",
      ]);
    },
  );
});
