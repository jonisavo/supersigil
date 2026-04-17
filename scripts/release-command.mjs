import { spawnSync } from "node:child_process";
import { buildCommandInvocation } from "./command-invocation.mjs";

export function formatCommandFailure(command, args, result) {
  return [
    `${command} ${args.join(" ")} failed`,
    typeof result.status === "number" ? `exit code: ${result.status}` : "",
    result.signal ? `signal: ${result.signal}` : "",
    result.error?.message ?? "",
    typeof result.stderr === "string" ? result.stderr.trim() : "",
    typeof result.stdout === "string" ? result.stdout.trim() : "",
  ]
    .filter(Boolean)
    .join("\n");
}

export function runCaptured(command, args, { cwd } = {}) {
  const invocation = buildCommandInvocation(command, args);

  return spawnSync(invocation.command, invocation.args, {
    cwd,
    encoding: "utf8",
    stdio: "pipe",
  });
}

export function runStreaming(command, args, { cwd, stdio = "inherit" } = {}) {
  const invocation = buildCommandInvocation(command, args);

  return spawnSync(invocation.command, invocation.args, {
    cwd,
    encoding: "utf8",
    stdio,
  });
}
