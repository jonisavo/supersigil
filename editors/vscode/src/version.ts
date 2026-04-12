/**
 * Compare two semver strings numerically.
 * Returns true if `a` is strictly newer than `b`.
 */
export function isNewerVersion(a: string, b: string): boolean {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  if (pa.some(isNaN) || pb.some(isNaN)) return false;
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const na = pa[i] ?? 0;
    const nb = pb[i] ?? 0;
    if (na > nb) return true;
    if (na < nb) return false;
  }
  return false;
}

export type MismatchResult =
  | { kind: "match" }
  | { kind: "skip" }
  | { kind: "mismatch"; serverVersion: string; extensionVersion: string; serverNewer: boolean };

/**
 * Determine whether a version mismatch exists between the LSP server
 * and the extension. Returns a discriminated union describing the
 * outcome.
 *
 * - `skip`: server didn't report a version — nothing to compare.
 * - `match`: versions are identical.
 * - `mismatch`: versions differ; `serverNewer` indicates direction.
 */
export function checkVersionMismatch(
  serverVersion: string | undefined,
  extensionVersion: string | undefined,
): MismatchResult {
  if (!serverVersion || !extensionVersion) {
    return { kind: "skip" };
  }
  if (serverVersion === extensionVersion) {
    return { kind: "match" };
  }
  return {
    kind: "mismatch",
    serverVersion,
    extensionVersion,
    serverNewer: isNewerVersion(serverVersion, extensionVersion),
  };
}
