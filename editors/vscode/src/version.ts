import { spawnSync, type SpawnSyncReturns } from "child_process";

// Keep this aligned with the server's compatibility constant. Bump it only for
// editor-visible protocol changes; package-version bumps alone do not require it.
export const SUPPORTED_COMPATIBILITY_VERSION = 1;
export const COMPATIBILITY_INFO_TIMEOUT_MS = 1000;

export interface CompatibilityInfo {
  compatibilityVersion: number;
  serverVersion: string;
}

export type CompatibilityResult =
  | {
      kind: "compatible";
      supportedVersion: number;
      reportedVersion: number;
      serverVersion: string;
    }
  | {
      kind: "incompatible";
      reason: "mismatch" | "query-failed" | "invalid-response";
      supportedVersion: number;
      reportedVersion: number | null;
      serverVersion: string | null;
    };

type CompatibilityRunner = (
  command: string,
  args: string[],
  options: { encoding: "utf8"; timeout: number; killSignal: "SIGKILL" },
) => Pick<SpawnSyncReturns<string>, "error" | "status" | "stdout" | "stderr">;

export function parseCompatibilityInfo(
  stdout: string,
): CompatibilityInfo | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(stdout);
  } catch {
    return null;
  }

  if (!parsed || typeof parsed !== "object") {
    return null;
  }

  const compatibilityVersion = (parsed as Record<string, unknown>).compatibility_version;
  const serverVersion = (parsed as Record<string, unknown>).server_version;
  if (
    typeof compatibilityVersion !== "number" ||
    !Number.isInteger(compatibilityVersion) ||
    typeof serverVersion !== "string" ||
    serverVersion.length === 0
  ) {
    return null;
  }

  return {
    compatibilityVersion,
    serverVersion,
  };
}

export function checkCompatibilityInfo(
  info: CompatibilityInfo | null,
  supportedVersion = SUPPORTED_COMPATIBILITY_VERSION,
): CompatibilityResult {
  if (!info) {
    return {
      kind: "incompatible",
      reason: "invalid-response",
      supportedVersion,
      reportedVersion: null,
      serverVersion: null,
    };
  }

  if (info.compatibilityVersion !== supportedVersion) {
    return {
      kind: "incompatible",
      reason: "mismatch",
      supportedVersion,
      reportedVersion: info.compatibilityVersion,
      serverVersion: info.serverVersion,
    };
  }

  return {
    kind: "compatible",
    supportedVersion,
    reportedVersion: info.compatibilityVersion,
    serverVersion: info.serverVersion,
  };
}

export function queryCompatibilityInfo(
  serverPath: string,
  runner: CompatibilityRunner = spawnSync,
): CompatibilityResult {
  const result = runner(serverPath, ["--compatibility-info"], {
    encoding: "utf8",
    timeout: COMPATIBILITY_INFO_TIMEOUT_MS,
    killSignal: "SIGKILL",
  });

  if (result.error || result.status !== 0) {
    return {
      kind: "incompatible",
      reason: "query-failed",
      supportedVersion: SUPPORTED_COMPATIBILITY_VERSION,
      reportedVersion: null,
      serverVersion: null,
    };
  }

  return checkCompatibilityInfo(parseCompatibilityInfo(result.stdout));
}
