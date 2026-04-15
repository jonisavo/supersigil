import { describe, expect, it } from "vitest";
import { verifies } from "@supersigil/vitest";
import {
  COMPATIBILITY_INFO_TIMEOUT_MS,
  SUPPORTED_COMPATIBILITY_VERSION,
  checkCompatibilityInfo,
  parseCompatibilityInfo,
  queryCompatibilityInfo,
} from "./version";

describe("parseCompatibilityInfo", () => {
  it(
    "parses compatibility info JSON from the server preflight",
    verifies(
      "editor-server-compatibility/req#req-1-1",
      "editor-server-compatibility/req#req-1-2",
    ),
    () => {
      expect(
        parseCompatibilityInfo(
          '{"compatibility_version":1,"server_version":"0.10.0"}',
        ),
      ).toEqual({
        compatibilityVersion: 1,
        serverVersion: "0.10.0",
      });
    },
  );

  it(
    "returns null when the compatibility version is missing",
    verifies("editor-server-compatibility/req#req-2-3"),
    () => {
      expect(
        parseCompatibilityInfo('{"server_version":"0.10.0"}'),
      ).toBeNull();
    },
  );
});

describe("checkCompatibilityInfo", () => {
  it(
    "accepts a matching compatibility version",
    verifies(
      "editor-server-compatibility/req#req-2-1",
      "editor-server-compatibility/req#req-4-1",
    ),
    () => {
      expect(
        checkCompatibilityInfo({
          compatibilityVersion: SUPPORTED_COMPATIBILITY_VERSION,
          serverVersion: "0.10.0",
        }),
      ).toEqual({
        kind: "compatible",
        supportedVersion: SUPPORTED_COMPATIBILITY_VERSION,
        reportedVersion: SUPPORTED_COMPATIBILITY_VERSION,
        serverVersion: "0.10.0",
      });
    },
  );

  it(
    "rejects a mismatched compatibility version",
    verifies(
      "editor-server-compatibility/req#req-2-1",
      "editor-server-compatibility/req#req-3-1",
      "editor-server-compatibility/req#req-3-2",
      "editor-server-compatibility/req#req-3-4",
    ),
    () => {
      expect(
        checkCompatibilityInfo({
          compatibilityVersion: SUPPORTED_COMPATIBILITY_VERSION + 1,
          serverVersion: "0.11.0",
        }),
      ).toEqual({
        kind: "incompatible",
        reason: "mismatch",
        supportedVersion: SUPPORTED_COMPATIBILITY_VERSION,
        reportedVersion: SUPPORTED_COMPATIBILITY_VERSION + 1,
        serverVersion: "0.11.0",
      });
    },
  );

  it(
    "treats a missing compatibility payload as incompatible",
    verifies("editor-server-compatibility/req#req-2-3"),
    () => {
      expect(checkCompatibilityInfo(null)).toEqual({
        kind: "incompatible",
        reason: "invalid-response",
        supportedVersion: SUPPORTED_COMPATIBILITY_VERSION,
        reportedVersion: null,
        serverVersion: null,
      });
    },
  );
});

describe("queryCompatibilityInfo", () => {
  it("runs the preflight query with a timeout", () => {
    queryCompatibilityInfo("/tmp/supersigil-lsp", (_command, _args, options) => {
      expect(options).toMatchObject({
        encoding: "utf8",
        timeout: COMPATIBILITY_INFO_TIMEOUT_MS,
        killSignal: "SIGKILL",
      });

      return {
        status: 1,
        stdout: "",
        stderr: "boom",
      };
    });
  });

  it(
    "treats a failed preflight command as incompatible",
    verifies("editor-server-compatibility/req#req-2-3"),
    () => {
      const result = queryCompatibilityInfo(
        "/tmp/supersigil-lsp",
        () => ({
          status: 1,
          stdout: "",
          stderr: "boom",
        }),
      );

      expect(result).toEqual({
        kind: "incompatible",
        reason: "query-failed",
        supportedVersion: SUPPORTED_COMPATIBILITY_VERSION,
        reportedVersion: null,
        serverVersion: null,
      });
    },
  );
});
