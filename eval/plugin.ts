import { execFile } from "node:child_process";
import { copyFile, mkdir, readFile, stat, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const isWindows = process.platform === "win32";

const SUPERSIGIL_COMMAND_PATTERNS = [
  /^supersigil\s+verify\b/,
  /^supersigil\s+status\b/,
  /^supersigil\s+plan\b/,
  /^supersigil\s+context\b/,
  /^supersigil\s+refs\b/,
  /^supersigil\s+lint\b/,
  /^supersigil\s+new\b/,
  /^supersigil\s+import\b/,
  /^supersigil\s+graph\b/,
  /^supersigil\s+affected\b/,
  /^supersigil\s+ls\b/,
  /^supersigil\s+schema\b/,
  /^supersigil\s+init\b/,
] as const;

export function extractSupersigilSubcommand(args: string[]): string {
  const first = args[0];
  return first && !first.startsWith("-") ? first : "unknown";
}

export function resolveSupersigilBinaryPath(root: string = repoRoot): string | undefined {
  const wrapperSuffix = isWindows ? ".cmd" : "";
  const wrapper = join(root, "eval", "bin", `supersigil${wrapperSuffix}`);
  if (existsSync(wrapper)) {
    return wrapper;
  }
  return undefined;
}

function getRuntimeDir(workspacePath: string): string {
  return join(dirname(workspacePath), "runtime");
}

async function runAndCapture(
  binary: string,
  args: string[],
  cwd: string,
  destination: string,
): Promise<{ stderrPath?: string }> {
  const stderrPath = buildStderrSidecarPath(destination);

  try {
    const { stdout, stderr } = await execFileAsync(binary, args, {
      cwd,
      env: process.env,
      maxBuffer: 10 * 1024 * 1024,
    });
    await writeFile(destination, stdout);
    if (stderr) {
      await writeFile(stderrPath, stderr);
      return { stderrPath };
    }
    return {};
  } catch (error) {
    const execError = error as {
      stdout?: string | Buffer;
      stderr?: string | Buffer;
      code?: number | string;
      message?: string;
    };
    const stdout = Buffer.isBuffer(execError.stdout)
      ? execError.stdout.toString("utf-8")
      : (execError.stdout ?? "");
    const stderr = Buffer.isBuffer(execError.stderr)
      ? execError.stderr.toString("utf-8")
      : (execError.stderr ?? "");
    const fallbackStdout = !stdout && !stderr && execError.message ? `${execError.message}\n` : stdout;
    await writeFile(destination, fallbackStdout);
    if (stderr) {
      await writeFile(stderrPath, stderr);
      return { stderrPath };
    }
    return {};
  }
}

function buildStderrSidecarPath(destination: string): string {
  const extension = extname(destination);
  if (!extension) {
    return `${destination}.stderr.txt`;
  }
  return `${destination.slice(0, -extension.length)}.stderr.txt`;
}

async function captureSupersigilArtifacts(context: {
  workspacePath: string;
  runDir: string;
  events?: Array<{
    type?: string;
    command?: string;
    exitCode?: number;
    stdout?: string;
    stderr?: string;
  }>;
  resolvedCliPaths?: Record<string, string>;
}): Promise<Array<{ path: string; description?: string }>> {
  const artifacts: Array<{ path: string; description?: string }> = [];
  const runtimeDir = getRuntimeDir(context.workspacePath);
  const binary = context.resolvedCliPaths?.["supersigil"] ?? "supersigil";

  await mkdir(context.runDir, { recursive: true });
  await writeFile(join(context.runDir, "supersigil-binary.txt"), `${binary}\n`);
  artifacts.push({
    path: "supersigil-binary.txt",
    description: "Resolved supersigil CLI path used for artifact capture",
  });

  const configPath = join(context.workspacePath, "supersigil.toml");
  try {
    await stat(configPath);
    await copyFile(configPath, join(context.runDir, "supersigil.toml"));
    artifacts.push({
      path: "supersigil.toml",
      description: "Supersigil project config",
    });
  } catch {
    // Missing config is acceptable for best-effort capture.
  }

  const callsLogPath = join(runtimeDir, "supersigil-calls.log");
  try {
    await copyFile(callsLogPath, join(context.runDir, "supersigil-calls.log"));
    artifacts.push({
      path: "supersigil-calls.log",
      description: "Supersigil CLI invocation log",
    });
  } catch {
    // No calls log yet.
  }

  artifacts.push(...(await captureRecordedVerifyArtifacts(context.runDir, context.events ?? [])));

  const statusCapture = await runAndCapture(
    binary,
    ["status", "--format", "json"],
    context.workspacePath,
    join(context.runDir, "supersigil-status.json"),
  );
  artifacts.push({
    path: "supersigil-status.json",
    description: "Output of supersigil status --format json",
  });
  if (statusCapture.stderrPath) {
    artifacts.push({
      path: "supersigil-status.stderr.txt",
      description: "stderr from supersigil status --format json",
    });
  }

  const verifyCapture = await runAndCapture(
    binary,
    ["verify", "--format", "json"],
    context.workspacePath,
    join(context.runDir, "supersigil-verify.json"),
  );
  artifacts.push({
    path: "supersigil-verify.json",
    description: "Post-run snapshot of supersigil verify --format json",
  });
  if (verifyCapture.stderrPath) {
    artifacts.push({
      path: "supersigil-verify.stderr.txt",
      description: "stderr from the post-run supersigil verify --format json snapshot",
    });
  }

  const lintCapture = await runAndCapture(
    binary,
    ["lint"],
    context.workspacePath,
    join(context.runDir, "supersigil-lint.txt"),
  );
  artifacts.push({
    path: "supersigil-lint.txt",
    description: "Output of supersigil lint",
  });
  if (lintCapture.stderrPath) {
    artifacts.push({
      path: "supersigil-lint.stderr.txt",
      description: "stderr from supersigil lint",
    });
  }

  return artifacts;
}

function recoverStdoutStderr(command: SupersigilCommandRecord): { stdout: string; stderr: string } {
  if (command.stderr.trim() !== "") {
    return { stdout: command.stdout, stderr: command.stderr };
  }

  const leadingJson = extractLeadingJsonValue(command.stdout);
  if (leadingJson === undefined) {
    return { stdout: command.stdout, stderr: command.stderr };
  }

  const trimmed = command.stdout.trimStart();
  const trailing = trimmed.slice(leadingJson.length).trimStart();
  return {
    stdout: leadingJson,
    stderr: trailing,
  };
}

async function captureRecordedVerifyArtifacts(
  runDir: string,
  events: Array<{
    type?: string;
    command?: string;
    exitCode?: number;
    stdout?: string;
    stderr?: string;
  }>,
): Promise<Array<{ path: string; description?: string }>> {
  const artifacts: Array<{ path: string; description?: string }> = [];
  const verifyCommands = collectSupersigilCommands(events).filter(
    (command) => command.subcommand === "verify" && command.args.includes("--format") && command.args.includes("json"),
  );

  for (const [index, command] of verifyCommands.entries()) {
    const occurrence = index + 1;
    const stdoutPath = join(runDir, `supersigil-verify-${occurrence}.json`);
    const stderrPath = join(runDir, `supersigil-verify-${occurrence}.stderr.txt`);
    const { stdout, stderr } = recoverStdoutStderr(command);

    await writeFile(stdoutPath, stdout);
    artifacts.push({
      path: `supersigil-verify-${occurrence}.json`,
      description: `Recovered stdout for verify invocation ${occurrence}`,
    });

    if (stderr.trim() !== "") {
      await writeFile(stderrPath, stderr);
      artifacts.push({
        path: `supersigil-verify-${occurrence}.stderr.txt`,
        description: `Recovered stderr for verify invocation ${occurrence}`,
      });
    }
  }

  return artifacts;
}

function parseCallsLog(content: string): Array<{
  timestamp: string;
  args: string[];
  exitCode: number;
  signal: string | null;
  durationMs: number;
}> {
  return content
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .flatMap((line) => {
      try {
        return [JSON.parse(line)];
      } catch {
        return [];
      }
    });
}

interface SupersigilCommandSelector {
  subcommand: string;
  args_contains?: string[];
  args_not_contains?: string[];
  occurrence?: "first" | "last" | number;
}

interface SupersigilCommandRecord {
  command: string;
  args: string[];
  subcommand: string;
  exitCode: number | undefined;
  stdout: string;
  stderr: string;
}

interface SupersigilSequenceStep extends SupersigilCommandSelector {
  exit_code?: number;
}

function splitShellWords(command: string): string[] {
  const tokens: string[] = [];
  let current = "";
  let quote: "'" | '"' | undefined;
  let escaped = false;

  for (const char of command) {
    if (escaped) {
      current += char;
      escaped = false;
      continue;
    }

    if (char === "\\" && quote !== "'") {
      escaped = true;
      continue;
    }

    if (quote) {
      if (char === quote) {
        quote = undefined;
      } else {
        current += char;
      }
      continue;
    }

    if (char === "'" || char === '"') {
      quote = char;
      continue;
    }

    if (/\s/.test(char)) {
      if (current) {
        tokens.push(current);
        current = "";
      }
      continue;
    }

    current += char;
  }

  if (escaped) {
    current += "\\";
  }

  if (current) {
    tokens.push(current);
  }

  return tokens;
}

function basenameToken(token: string): string {
  const segments = token.split(/[\\/]/);
  return segments[segments.length - 1] ?? token;
}

function isSupersigilBinaryToken(token: string): boolean {
  const normalized = basenameToken(token).toLowerCase();
  return normalized === "supersigil" || normalized === "supersigil.exe";
}

function isShellBinaryToken(token: string): boolean {
  return ["sh", "bash", "zsh"].includes(basenameToken(token).toLowerCase());
}

function extractSupersigilArgsFromCommand(
  command: string,
  depth: number = 0,
): string[] | undefined {
  if (depth > 2) {
    return undefined;
  }

  const tokens = splitShellWords(command);
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    if (!token) {
      continue;
    }

    if (
      isShellBinaryToken(token) &&
      (tokens[index + 1] === "-c" || tokens[index + 1] === "-lc") &&
      tokens[index + 2]
    ) {
      const nested = extractSupersigilArgsFromCommand(tokens[index + 2], depth + 1);
      if (nested) {
        return nested;
      }
    }

    if (isSupersigilBinaryToken(token)) {
      return tokens.slice(index + 1);
    }
  }

  return undefined;
}

function collectSupersigilCommands(
  events: Array<{
    type?: string;
    command?: string;
    exitCode?: number;
    stdout?: string;
    stderr?: string;
  }>,
): SupersigilCommandRecord[] {
  return events.flatMap((event) => {
    if (event.type !== "command_completed" || typeof event.command !== "string") {
      return [];
    }

    const args = extractSupersigilArgsFromCommand(event.command);
    if (!args || args.length === 0) {
      return [];
    }

    return [
      {
        command: event.command,
        args,
        subcommand: extractSupersigilSubcommand(args),
        exitCode: event.exitCode,
        stdout: event.stdout ?? "",
        stderr: event.stderr ?? "",
      },
    ];
  });
}

function matchCommandSelector(
  command: SupersigilCommandRecord,
  selector: SupersigilCommandSelector,
): boolean {
  if (command.subcommand !== selector.subcommand) {
    return false;
  }

  if (selector.args_contains?.some((arg) => !command.args.includes(arg))) {
    return false;
  }

  if (selector.args_not_contains?.some((arg) => command.args.includes(arg))) {
    return false;
  }

  return true;
}

function selectSupersigilCommand(
  commands: SupersigilCommandRecord[],
  selector: SupersigilCommandSelector,
): SupersigilCommandRecord | undefined {
  const matches = commands.filter((command) => matchCommandSelector(command, selector));
  if (matches.length === 0) {
    return undefined;
  }

  if (selector.occurrence === "last") {
    return matches[matches.length - 1];
  }

  if (typeof selector.occurrence === "number" && Number.isInteger(selector.occurrence)) {
    return matches[selector.occurrence - 1];
  }

  return matches[0];
}

function extractLeadingJsonValue(output: string): string | undefined {
  const trimmed = output.trimStart();
  const opener = trimmed[0];
  if (opener !== "{" && opener !== "[") {
    return undefined;
  }

  const stack: string[] = [];
  let inString = false;
  let escaped = false;

  for (let index = 0; index < trimmed.length; index += 1) {
    const char = trimmed[index];

    if (inString) {
      if (escaped) {
        escaped = false;
      } else if (char === "\\") {
        escaped = true;
      } else if (char === "\"") {
        inString = false;
      }
      continue;
    }

    if (char === "\"") {
      inString = true;
      continue;
    }

    if (char === "{") {
      stack.push("}");
      continue;
    }

    if (char === "[") {
      stack.push("]");
      continue;
    }

    if (char === "}" || char === "]") {
      const expected = stack.pop();
      if (expected !== char) {
        return undefined;
      }
      if (stack.length === 0) {
        return trimmed.slice(0, index + 1);
      }
    }
  }

  return undefined;
}

function parseJsonFromCommand(
  command: SupersigilCommandRecord,
  label: string,
):
  | { ok: true; value: unknown }
  | {
      ok: false;
      result: { passed: boolean; details: string };
    } {
  if (command.stdout.trim() === "") {
    return {
      ok: false,
      result: {
        passed: false,
        details: `Selected supersigil ${command.subcommand} command did not produce JSON stdout for ${label}`,
      },
    };
  }

  try {
    return {
      ok: true,
      value: JSON.parse(command.stdout),
    };
  } catch {
    // Some agents merge stderr into stdout. Extract the first complete JSON
    // value from the beginning of the output and ignore trailing prose.
    const leadingJson = extractLeadingJsonValue(command.stdout);
    if (leadingJson !== undefined) {
      try {
        return {
          ok: true,
          value: JSON.parse(leadingJson),
        };
      } catch {
        // fall through to the structured error below
      }
    }

    return {
      ok: false,
      result: {
        passed: false,
        details: `Selected supersigil ${command.subcommand} command produced invalid JSON for ${label}`,
      },
    };
  }
}

function getJsonFieldValue(
  value: unknown,
  field: string,
): { found: boolean; value?: unknown } {
  const segments = field.match(/([^[.\]]+)|\[(\d+)\]/g);
  if (!segments) {
    return { found: false };
  }

  let current: unknown = value;
  for (const segment of segments) {
    if (segment.startsWith("[")) {
      if (!Array.isArray(current)) {
        return { found: false };
      }
      const index = Number(segment.slice(1, -1));
      current = current[index];
      continue;
    }

    if (!current || typeof current !== "object" || !(segment in current)) {
      return { found: false };
    }

    current = (current as Record<string, unknown>)[segment];
  }

  return { found: true, value: current };
}

function targetRef(docId: string, targetId: string): string {
  return `${docId}#${targetId}`;
}

function getTaskRefs(task: unknown): string[] {
  if (!task || typeof task !== "object") {
    return [];
  }

  const implementsValue = (task as { implements?: unknown }).implements;
  if (!Array.isArray(implementsValue)) {
    return [];
  }

  return implementsValue.flatMap((entry) => {
    if (
      Array.isArray(entry) &&
      typeof entry[0] === "string" &&
      typeof entry[1] === "string"
    ) {
      return [targetRef(entry[0], entry[1])];
    }
    return [];
  });
}

function getTaskId(task: unknown): string | undefined {
  if (!task || typeof task !== "object") {
    return undefined;
  }

  const taskId = (task as { task_id?: unknown }).task_id;
  return typeof taskId === "string" ? taskId : undefined;
}

function getTaskDependsOn(task: unknown): string[] {
  if (!task || typeof task !== "object") {
    return [];
  }

  const dependsOn = (task as { depends_on?: unknown }).depends_on;
  if (!Array.isArray(dependsOn)) {
    return [];
  }

  return dependsOn.filter((dependency): dependency is string => typeof dependency === "string");
}

function partitionPlanTargets(plan: unknown): { actionable: string[]; blocked: string[] } {
  if (!plan || typeof plan !== "object") {
    return { actionable: [], blocked: [] };
  }

  const outstandingTargets = Array.isArray((plan as { outstanding_targets?: unknown }).outstanding_targets)
    ? (plan as { outstanding_targets: unknown[] }).outstanding_targets
    : [];
  const pendingTasks = Array.isArray((plan as { pending_tasks?: unknown }).pending_tasks)
    ? (plan as { pending_tasks: unknown[] }).pending_tasks
    : [];
  const completedTasks = Array.isArray((plan as { completed_tasks?: unknown }).completed_tasks)
    ? (plan as { completed_tasks: unknown[] }).completed_tasks
    : [];

  const completedIds = new Set(
    completedTasks.flatMap((task) => {
      const taskId = getTaskId(task);
      return taskId ? [taskId] : [];
    }),
  );
  const pendingIds = new Set(
    pendingTasks.flatMap((task) => {
      const taskId = getTaskId(task);
      return taskId ? [taskId] : [];
    }),
  );

  const unblockedTaskIds = new Set(
    pendingTasks.flatMap((task) => {
      const taskId = getTaskId(task);
      if (!taskId) {
        return [];
      }

      const dependsOn = getTaskDependsOn(task);
      const unblocked = dependsOn.every(
        (dependency) => completedIds.has(dependency) || !pendingIds.has(dependency),
      );
      return unblocked ? [taskId] : [];
    }),
  );

  const actionableRefs = new Set(
    pendingTasks.flatMap((task) => {
      const taskId = getTaskId(task);
      return taskId && unblockedTaskIds.has(taskId) ? getTaskRefs(task) : [];
    }),
  );
  const allPendingRefs = new Set(pendingTasks.flatMap((task) => getTaskRefs(task)));

  const actionable: string[] = [];
  const blocked: string[] = [];

  for (const target of outstandingTargets) {
    if (!target || typeof target !== "object") {
      continue;
    }

    const docId = (target as { doc_id?: unknown }).doc_id;
    const targetId = (target as { target_id?: unknown }).target_id;
    if (typeof docId !== "string" || typeof targetId !== "string") {
      continue;
    }

    const ref = targetRef(docId, targetId);
    if (actionableRefs.has(ref) || !allPendingRefs.has(ref)) {
      actionable.push(ref);
    } else {
      blocked.push(ref);
    }
  }

  return { actionable, blocked };
}

async function evaluateSupersigilExitCode(
  params: unknown,
  context: { events: Array<{ type?: string; command?: string; exitCode?: number }> },
): Promise<{ passed: boolean; details?: string }> {
  const { command, equals } = params as {
    command: SupersigilCommandSelector;
    equals: number;
  };
  const selected = selectSupersigilCommand(collectSupersigilCommands(context.events), command);

  if (!selected) {
    return {
      passed: false,
      details: `No supersigil command matched selector for ${command.subcommand}`,
    };
  }

  return {
    passed: selected.exitCode === equals,
    details:
      selected.exitCode === equals
        ? `Supersigil ${selected.subcommand} exited with expected code ${equals}`
        : `Supersigil ${selected.subcommand} exited with ${String(selected.exitCode)} instead of ${equals}`,
  };
}

async function evaluateSupersigilStatusMetric(
  params: unknown,
  context: {
    events: Array<{
      type?: string;
      command?: string;
      exitCode?: number;
      stdout?: string;
      stderr?: string;
    }>;
  },
): Promise<{ passed: boolean; details?: string }> {
  const { command, field, equals, gte, lte } = params as {
    command: SupersigilCommandSelector;
    field: string;
    equals?: unknown;
    gte?: number;
    lte?: number;
  };
  const selected = selectSupersigilCommand(collectSupersigilCommands(context.events), command);

  if (!selected) {
    return {
      passed: false,
      details: `No supersigil command matched selector for ${command.subcommand}`,
    };
  }

  const parsed = parseJsonFromCommand(selected, "supersigil_status_metric");
  if (!parsed.ok) {
    return parsed.result;
  }

  const fieldValue = getJsonFieldValue(parsed.value, field);
  if (!fieldValue.found) {
    return {
      passed: false,
      details: `Field not found in supersigil status JSON: ${field}`,
    };
  }

  let passed = true;
  const checks: string[] = [];

  if (equals !== undefined) {
    passed = passed && fieldValue.value === equals;
    checks.push(`equals ${String(equals)}`);
  }

  if (gte !== undefined) {
    if (typeof fieldValue.value !== "number") {
      return {
        passed: false,
        details: `Field ${field} is not numeric`,
      };
    }
    passed = passed && fieldValue.value >= gte;
    checks.push(`>= ${gte}`);
  }

  if (lte !== undefined) {
    if (typeof fieldValue.value !== "number") {
      return {
        passed: false,
        details: `Field ${field} is not numeric`,
      };
    }
    passed = passed && fieldValue.value <= lte;
    checks.push(`<= ${lte}`);
  }

  return {
    passed,
    details: passed
      ? `Supersigil status field ${field} matched ${checks.join(" and ")}`
      : `Supersigil status field ${field} was ${String(fieldValue.value)} and did not match ${checks.join(" and ")}`,
  };
}

async function evaluateSupersigilPlanTarget(
  params: unknown,
  context: {
    events: Array<{
      type?: string;
      command?: string;
      exitCode?: number;
      stdout?: string;
      stderr?: string;
    }>;
  },
  kind: "actionable" | "blocked",
): Promise<{ passed: boolean; details?: string }> {
  const { command, target } = params as {
    command: SupersigilCommandSelector;
    target?: string;
  };
  const selected = selectSupersigilCommand(collectSupersigilCommands(context.events), command);

  if (!selected) {
    return {
      passed: false,
      details: `No supersigil command matched selector for ${command.subcommand}`,
    };
  }

  const parsed = parseJsonFromCommand(selected, `supersigil_plan_has_${kind}`);
  if (!parsed.ok) {
    return parsed.result;
  }

  const partitioned = partitionPlanTargets(parsed.value);
  const targets = kind === "actionable" ? partitioned.actionable : partitioned.blocked;
  const passed = target ? targets.includes(target) : targets.length > 0;

  return {
    passed,
    details: passed
      ? target
        ? `Supersigil plan exposed ${kind} target ${target}`
        : `Supersigil plan exposed ${targets.length} ${kind} target(s)`
      : target
        ? `Supersigil plan did not expose ${kind} target ${target}`
        : `Supersigil plan did not expose any ${kind} targets`,
  };
}

async function evaluateSupersigilVerifyClean(
  params: unknown,
  context: {
    events: Array<{
      type?: string;
      command?: string;
      exitCode?: number;
      stdout?: string;
      stderr?: string;
    }>;
  },
): Promise<{ passed: boolean; details?: string }> {
  const { command } = params as { command: SupersigilCommandSelector };
  const selected = selectSupersigilCommand(collectSupersigilCommands(context.events), command);

  if (!selected) {
    return {
      passed: false,
      details: `No supersigil command matched selector for ${command.subcommand}`,
    };
  }

  const parsed = parseJsonFromCommand(selected, "supersigil_verify_clean");
  if (!parsed.ok) {
    return parsed.result;
  }

  const summary = getJsonFieldValue(parsed.value, "summary");
  if (!summary.found || !summary.value || typeof summary.value !== "object") {
    return {
      passed: false,
      details: "Supersigil verify JSON did not include a summary object",
    };
  }

  const errorCount = (summary.value as Record<string, unknown>)["error_count"];
  const warningCount = (summary.value as Record<string, unknown>)["warning_count"];
  const passed = errorCount === 0 && warningCount === 0;

  return {
    passed,
    details: passed
      ? "Supersigil verify report is clean"
      : `Supersigil verify report is not clean: errors=${String(errorCount)}, warnings=${String(warningCount)}`,
  };
}

async function evaluateSupersigilVerifyHasFinding(
  params: unknown,
  context: {
    events: Array<{
      type?: string;
      command?: string;
      exitCode?: number;
      stdout?: string;
      stderr?: string;
    }>;
  },
): Promise<{ passed: boolean; details?: string }> {
  const { command, rule, severity, doc_id, message_contains } = params as {
    command: SupersigilCommandSelector;
    rule?: string;
    severity?: string;
    doc_id?: string;
    message_contains?: string;
  };
  const selected = selectSupersigilCommand(collectSupersigilCommands(context.events), command);

  if (!selected) {
    return {
      passed: false,
      details: `No supersigil command matched selector for ${command.subcommand}`,
    };
  }

  const parsed = parseJsonFromCommand(selected, "supersigil_verify_has_finding");
  if (!parsed.ok) {
    return parsed.result;
  }

  const findingsField = getJsonFieldValue(parsed.value, "findings");
  if (!findingsField.found || !Array.isArray(findingsField.value)) {
    return {
      passed: false,
      details: "Supersigil verify JSON did not include a findings array",
    };
  }

  const matchedFindings = findingsField.value.filter((finding) => {
    if (!finding || typeof finding !== "object") {
      return false;
    }

    const findingObject = finding as Record<string, unknown>;
    if (rule && findingObject["rule"] !== rule) {
      return false;
    }
    if (severity && findingObject["effective_severity"] !== severity) {
      return false;
    }
    if (doc_id && findingObject["doc_id"] !== doc_id) {
      return false;
    }
    if (
      message_contains &&
      (typeof findingObject["message"] !== "string" ||
        !findingObject["message"].includes(message_contains))
    ) {
      return false;
    }

    return true;
  });

  return {
    passed: matchedFindings.length > 0,
    details:
      matchedFindings.length > 0
        ? `Supersigil verify report matched ${matchedFindings.length} finding(s)`
        : "Supersigil verify report did not match the expected finding",
  };
}

async function evaluateSupersigilAffectedContains(
  params: unknown,
  context: {
    events: Array<{
      type?: string;
      command?: string;
      exitCode?: number;
      stdout?: string;
      stderr?: string;
    }>;
  },
): Promise<{ passed: boolean; details?: string }> {
  const { command, doc_id, path, changed_file, matched_glob } = params as {
    command: SupersigilCommandSelector;
    doc_id?: string;
    path?: string;
    changed_file?: string;
    matched_glob?: string;
  };
  const selected = selectSupersigilCommand(collectSupersigilCommands(context.events), command);

  if (!selected) {
    return {
      passed: false,
      details: `No supersigil command matched selector for ${command.subcommand}`,
    };
  }

  const parsed = parseJsonFromCommand(selected, "supersigil_affected_contains");
  if (!parsed.ok) {
    return parsed.result;
  }

  if (!Array.isArray(parsed.value)) {
    return {
      passed: false,
      details: "Supersigil affected JSON was not an array",
    };
  }

  const matched = parsed.value.some((entry) => {
    if (!entry || typeof entry !== "object") {
      return false;
    }

    const affected = entry as Record<string, unknown>;
    if (doc_id && affected["id"] !== doc_id) {
      return false;
    }
    if (path && affected["path"] !== path) {
      return false;
    }
    if (
      changed_file &&
      (!Array.isArray(affected["changed_files"]) ||
        !affected["changed_files"].includes(changed_file))
    ) {
      return false;
    }
    if (
      matched_glob &&
      (!Array.isArray(affected["matched_globs"]) ||
        !affected["matched_globs"].includes(matched_glob))
    ) {
      return false;
    }
    return true;
  });

  return {
    passed: matched,
    details: matched
      ? "Supersigil affected output included the expected document"
      : "Supersigil affected output did not include the expected document",
  };
}

async function evaluateSupersigilCommandSequence(
  params: unknown,
  context: {
    events: Array<{
      type?: string;
      command?: string;
      exitCode?: number;
      stdout?: string;
      stderr?: string;
    }>;
  },
): Promise<{ passed: boolean; details?: string }> {
  const { mode, steps } = params as {
    mode: string;
    steps: SupersigilSequenceStep[];
  };

  if (mode !== "ordered_subsequence") {
    return {
      passed: false,
      details: `Unsupported supersigil command sequence mode: ${mode}`,
    };
  }

  const commands = collectSupersigilCommands(context.events);
  let searchStart = 0;

  for (let stepIndex = 0; stepIndex < steps.length; stepIndex += 1) {
    const step = steps[stepIndex];
    let matchedIndex = -1;

    for (let commandIndex = searchStart; commandIndex < commands.length; commandIndex += 1) {
      const command = commands[commandIndex];
      if (!matchCommandSelector(command, step)) {
        continue;
      }
      if (
        step.exit_code !== undefined &&
        command.exitCode !== step.exit_code
      ) {
        continue;
      }
      matchedIndex = commandIndex;
      break;
    }

    if (matchedIndex === -1) {
      return {
        passed: false,
        details: `Supersigil command sequence was missing step ${stepIndex + 1}: ${step.subcommand}`,
      };
    }

    searchStart = matchedIndex + 1;
  }

  return {
    passed: true,
    details: `Supersigil command sequence matched ${steps.length} step(s)`,
  };
}

const commandSelectorSchema = {
  type: "object",
  properties: {
    subcommand: {
      type: "string",
      description: "Supersigil subcommand to match",
    },
    args_contains: {
      type: "array",
      items: { type: "string" },
      description: "Arguments that must be present on the matched command",
    },
    args_not_contains: {
      type: "array",
      items: { type: "string" },
      description: "Arguments that must not be present on the matched command",
    },
    occurrence: {
      description: "first, last, or a 1-based occurrence index",
    },
  },
  required: ["subcommand"],
  additionalProperties: false,
};

const sequenceStepSchema = {
  type: "object",
  properties: {
    ...commandSelectorSchema.properties,
    exit_code: {
      type: "number",
      description: "Optional exit code constraint for this step",
    },
  },
  required: ["subcommand"],
  additionalProperties: false,
};

const supersigilExitCodeCriterion = {
  type: "supersigil_exit_code",
  schema: {
    type: "object",
    properties: {
      command: commandSelectorSchema,
      equals: {
        type: "number",
        description: "Expected exit code",
      },
    },
    required: ["command", "equals"],
    additionalProperties: false,
  },
  evaluate: evaluateSupersigilExitCode,
};

const supersigilStatusMetricCriterion = {
  type: "supersigil_status_metric",
  schema: {
    type: "object",
    properties: {
      command: commandSelectorSchema,
      field: {
        type: "string",
        description: "JSON field path to inspect",
      },
      equals: {
        description: "Expected value for the selected field",
      },
      gte: {
        type: "number",
        description: "Minimum numeric value for the selected field",
      },
      lte: {
        type: "number",
        description: "Maximum numeric value for the selected field",
      },
    },
    required: ["command", "field"],
    additionalProperties: false,
  },
  evaluate: evaluateSupersigilStatusMetric,
};

const supersigilPlanHasActionableCriterion = {
  type: "supersigil_plan_has_actionable",
  schema: {
    type: "object",
    properties: {
      command: commandSelectorSchema,
      target: {
        type: "string",
        description: "Optional target ref in doc#criterion form",
      },
    },
    required: ["command"],
    additionalProperties: false,
  },
  evaluate: async (params: unknown, context: unknown) =>
    evaluateSupersigilPlanTarget(
      params,
      context as {
        events: Array<{
          type?: string;
          command?: string;
          exitCode?: number;
          stdout?: string;
          stderr?: string;
        }>;
      },
      "actionable",
    ),
};

const supersigilPlanHasBlockedCriterion = {
  type: "supersigil_plan_has_blocked",
  schema: {
    type: "object",
    properties: {
      command: commandSelectorSchema,
      target: {
        type: "string",
        description: "Optional target ref in doc#criterion form",
      },
    },
    required: ["command"],
    additionalProperties: false,
  },
  evaluate: async (params: unknown, context: unknown) =>
    evaluateSupersigilPlanTarget(
      params,
      context as {
        events: Array<{
          type?: string;
          command?: string;
          exitCode?: number;
          stdout?: string;
          stderr?: string;
        }>;
      },
      "blocked",
    ),
};

const supersigilVerifyCleanCriterion = {
  type: "supersigil_verify_clean",
  schema: {
    type: "object",
    properties: {
      command: commandSelectorSchema,
    },
    required: ["command"],
    additionalProperties: false,
  },
  evaluate: evaluateSupersigilVerifyClean,
};

const supersigilVerifyHasFindingCriterion = {
  type: "supersigil_verify_has_finding",
  schema: {
    type: "object",
    properties: {
      command: commandSelectorSchema,
      rule: {
        type: "string",
        description: "Expected finding rule name",
      },
      severity: {
        type: "string",
        description: "Expected effective severity",
      },
      doc_id: {
        type: "string",
        description: "Expected document id",
      },
      message_contains: {
        type: "string",
        description: "Substring expected in the finding message",
      },
    },
    required: ["command"],
    additionalProperties: false,
  },
  evaluate: evaluateSupersigilVerifyHasFinding,
};

const supersigilAffectedContainsCriterion = {
  type: "supersigil_affected_contains",
  schema: {
    type: "object",
    properties: {
      command: commandSelectorSchema,
      doc_id: {
        type: "string",
        description: "Expected affected document id",
      },
      path: {
        type: "string",
        description: "Expected affected document path",
      },
      changed_file: {
        type: "string",
        description: "Expected changed file in the matched document",
      },
      matched_glob: {
        type: "string",
        description: "Expected tracked-files glob in the matched document",
      },
    },
    required: ["command"],
    additionalProperties: false,
  },
  evaluate: evaluateSupersigilAffectedContains,
};

const supersigilCommandSequenceCriterion = {
  type: "supersigil_command_sequence",
  schema: {
    type: "object",
    properties: {
      mode: {
        type: "string",
        enum: ["ordered_subsequence"],
        description: "Sequence matching mode",
      },
      steps: {
        type: "array",
        items: sequenceStepSchema,
        minItems: 1,
        description: "Ordered command steps that must appear in the run",
      },
    },
    required: ["mode", "steps"],
    additionalProperties: false,
  },
  evaluate: evaluateSupersigilCommandSequence,
};

async function generateSupersigilInsights(
  results: Array<{
    scenario: string;
    artifactPath: string;
  }>,
): Promise<Array<{
  category: string;
  title: string;
  description: string;
  severity?: "info" | "warning" | "error";
  scenarios?: string[];
}>> {
  const failures = new Map<string, { count: number; scenarios: Set<string> }>();

  for (const result of results) {
    try {
      const content = await readFile(
        join(result.artifactPath, "supersigil-calls.log"),
        "utf-8",
      );
      for (const entry of parseCallsLog(content)) {
        if (entry.exitCode === 0) {
          continue;
        }
        const subcommand = extractSupersigilSubcommand(entry.args);
        const existing = failures.get(subcommand);
        if (existing) {
          existing.count += 1;
          existing.scenarios.add(result.scenario);
        } else {
          failures.set(subcommand, {
            count: 1,
            scenarios: new Set([result.scenario]),
          });
        }
      }
    } catch {
      // Missing artifact is acceptable.
    }
  }

  return [...failures.entries()].map(([subcommand, data]) => ({
    category: "supersigil-failures",
    title: `Supersigil ${subcommand} failures`,
    description: `The supersigil ${subcommand} command failed ${data.count} time(s)`,
    severity: data.count > 2 ? "error" : "warning",
    scenarios: [...data.scenarios],
  }));
}

export const supersigilArtifactCaptureHook = {
  name: "supersigil-artifacts",
  capture: captureSupersigilArtifacts,
};

export const supersigilReportInsightGenerator = {
  name: "supersigil-subcommand-failures",
  generate: generateSupersigilInsights,
};

export const supersigilPlugin = {
  name: "supersigil",
  version: "1.0.0",
  dependencies: ["git"],
  clis: [
    {
      binary: "supersigil",
      resolveBinary: () => resolveSupersigilBinaryPath(repoRoot),
      versionCommand: "--version",
      minVersion: "0.1.0",
      commandPatterns: SUPERSIGIL_COMMAND_PATTERNS as unknown as RegExp[],
    },
  ],
  successCriteria: [
    supersigilExitCodeCriterion,
    supersigilStatusMetricCriterion,
    supersigilPlanHasActionableCriterion,
    supersigilPlanHasBlockedCriterion,
    supersigilVerifyCleanCriterion,
    supersigilVerifyHasFindingCriterion,
    supersigilAffectedContainsCriterion,
    supersigilCommandSequenceCriterion,
  ],
  artifactCapture: [supersigilArtifactCaptureHook],
  reportInsights: [supersigilReportInsightGenerator],
};

export default supersigilPlugin;
