import { afterEach, describe, expect, it } from "vitest";
import { verifies } from "@supersigil/vitest";
import { closeSync, mkdtempSync, openSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const tempDirs = [];
const releaseCommandModule = new URL("../release-command.mjs", import.meta.url).href;

afterEach(() => {
  while (tempDirs.length > 0) {
    rmSync(tempDirs.pop(), { recursive: true, force: true });
  }
});

function createTempDir() {
  const dir = mkdtempSync(join(tmpdir(), "release-command-test-"));
  tempDirs.push(dir);
  return dir;
}

describe("release command execution", () => {
  it(
    "streams large child output without ENOBUFS",
    verifies("release-targets/req#req-3-1", "release-targets/req#req-3-2"),
    async () => {
      const { runStreaming } = await import(releaseCommandModule);
      const tempDir = createTempDir();
      const stdoutFd = openSync(join(tempDir, "stdout.log"), "w");
      const stderrFd = openSync(join(tempDir, "stderr.log"), "w");

      try {
        const result = runStreaming(process.execPath, [
          "-e",
          "process.stdout.write('x'.repeat(2 * 1024 * 1024))",
        ], {
          stdio: ["ignore", stdoutFd, stderrFd],
        });

        expect(result.status).toBe(0);
        expect(result.error).toBeUndefined();
      } finally {
        closeSync(stdoutFd);
        closeSync(stderrFd);
      }
    },
  );

  it(
    "captures stdout for commands that need output parsing",
    verifies("release-targets/req#req-3-5"),
    async () => {
      const { runCaptured } = await import(releaseCommandModule);
      const result = runCaptured(process.execPath, ["-e", "process.stdout.write('hello')"]);

      expect(result.status).toBe(0);
      expect(result.stdout).toBe("hello");
    },
  );
});
