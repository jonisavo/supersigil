import { describe, expect, it, vi } from "vitest";
import { verifies } from "@supersigil/vitest";

vi.mock("vscode-languageclient/node", () => ({
  TransportKind: { stdio: 0 },
}));

const { TransportKind } = await import("vscode-languageclient/node");
const { createServerOptions } = await import("./serverOptions");

describe("createServerOptions", () => {
  it(
    "launches the resolved Windows executable directly over stdio",
    verifies("vscode-extension/req#req-3-6"),
    () => {
      expect(
        createServerOptions("C:\\Users\\example-user\\.cargo\\bin\\supersigil-lsp.exe"),
      ).toEqual({
        command: "C:\\Users\\example-user\\.cargo\\bin\\supersigil-lsp.exe",
        transport: TransportKind.stdio,
      });
    },
  );

  it("keeps Unix launch semantics unchanged", () => {
      expect(createServerOptions("/usr/local/bin/supersigil-lsp")).toEqual({
        command: "/usr/local/bin/supersigil-lsp",
        transport: TransportKind.stdio,
      });
  });
});
