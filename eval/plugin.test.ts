import { describe, expect, test } from "vitest";
import { chmod, mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  extractSupersigilSubcommand,
  resolveSupersigilBinaryPath,
  supersigilArtifactCaptureHook,
  supersigilPlugin,
  supersigilReportInsightGenerator,
} from "./plugin";

function getCriterion(type: string) {
  const criterion = supersigilPlugin.successCriteria?.find(
    (entry) => entry.type === type,
  );
  if (!criterion) {
    throw new Error(`Missing criterion: ${type}`);
  }
  return criterion;
}

function createEvaluationContext(events: Array<Record<string, unknown>>) {
  return {
    workspacePath: "/tmp/workspace",
    runId: "run-123",
    scenario: {
      version: 1,
      name: "test-scenario",
      description: "Test scenario",
      goal: "Test goal",
      template: "./template",
      timeout: 60_000,
      success_criteria: [],
    },
    events,
    getPluginData: () => undefined,
  };
}

function createVerifyReport(overrides?: {
  findings?: Array<Record<string, unknown>>;
  error_count?: number;
  warning_count?: number;
  info_count?: number;
}) {
  const findings = overrides?.findings ?? [];
  return JSON.stringify({
    findings,
    summary: {
      total_documents: 3,
      error_count: overrides?.error_count ?? 0,
      warning_count: overrides?.warning_count ?? 0,
      info_count: overrides?.info_count ?? 0,
    },
  });
}

describe("supersigil eval plugin", () => {
  test("exports the supersigil CLI definition", () => {
    expect(supersigilPlugin.name).toBe("supersigil");
    expect(supersigilPlugin.dependencies).toEqual(["git"]);
    expect(supersigilPlugin.clis?.[0]?.binary).toBe("supersigil");
    expect(
      supersigilPlugin.clis?.[0]?.commandPatterns?.some((pattern) =>
        pattern.test("supersigil refs --format json"),
      ),
    ).toBe(true);
  });

  test("extracts two-word subcommands", () => {
    expect(extractSupersigilSubcommand(["plan"])).toBe("plan");
    expect(extractSupersigilSubcommand(["context", "auth/req/login"])).toBe(
      "context",
    );
    expect(extractSupersigilSubcommand(["verify", "--format", "json"])).toBe(
      "verify",
    );
  });

  test("resolves the repo wrapper instead of target binaries", async () => {
    const baseDir = await mkdtemp(join(tmpdir(), "supersigil-eval-plugin-"));
    const binDir = join(baseDir, "eval", "bin");
    const debugDir = join(baseDir, "target", "debug");
    const releaseDir = join(baseDir, "target", "release");
    await mkdir(binDir, { recursive: true });
    await mkdir(debugDir, { recursive: true });
    await mkdir(releaseDir, { recursive: true });

    const suffix = process.platform === "win32" ? ".exe" : "";
    const wrapperSuffix = process.platform === "win32" ? ".cmd" : "";
    const wrapper = join(binDir, `supersigil${wrapperSuffix}`);
    const debugBinary = join(debugDir, `supersigil${suffix}`);
    const releaseBinary = join(releaseDir, `supersigil${suffix}`);

    await writeFile(wrapper, "");
    await writeFile(releaseBinary, "");
    expect(resolveSupersigilBinaryPath(baseDir)).toBe(wrapper);

    await writeFile(debugBinary, "");
    expect(resolveSupersigilBinaryPath(baseDir)).toBe(wrapper);

    await rm(wrapper, { force: true });
    expect(resolveSupersigilBinaryPath(baseDir)).toBeUndefined();

    await rm(baseDir, { recursive: true, force: true });
  });

  test("artifact capture copies calls log and config", async () => {
    const baseDir = await mkdtemp(join(tmpdir(), "supersigil-eval-plugin-"));
    const workspacePath = join(baseDir, "workspace");
    const runtimeDir = join(baseDir, "runtime");
    const runDir = join(baseDir, "run");

    await mkdir(workspacePath, { recursive: true });
    await mkdir(runtimeDir, { recursive: true });
    await mkdir(runDir, { recursive: true });

    await writeFile(
      join(workspacePath, "supersigil.toml"),
      'paths = ["specs/**/*.mdx"]\n',
    );
    await writeFile(
      join(runtimeDir, "supersigil-calls.log"),
      '{"timestamp":"2026-03-07T00:00:00Z","args":["verify"],"exitCode":1,"signal":null,"durationMs":10}\n',
    );

    const artifacts = await supersigilArtifactCaptureHook.capture({
      workspacePath,
      runId: "run-123",
      scenario: {
        version: 1,
        name: "test-scenario",
        description: "Test scenario",
        goal: "Test goal",
        template: "./template",
        timeout: 60_000,
        success_criteria: [],
      },
      events: [],
      outDir: runDir,
      runDir,
      config: {},
      resolvedCliPaths: {},
      getPluginData: () => undefined,
      setPluginData: () => {},
    });

    expect(artifacts.some((artifact) => artifact.path === "supersigil-calls.log"))
      .toBe(true);
    expect(artifacts.some((artifact) => artifact.path === "supersigil.toml")).toBe(
      true,
    );

    const copiedConfig = await readFile(join(runDir, "supersigil.toml"), "utf-8");
    expect(copiedConfig).toContain('paths = ["specs/**/*.mdx"]');

    await rm(baseDir, { recursive: true, force: true });
  });

  test("artifact capture records the resolved supersigil binary path", async () => {
    const baseDir = await mkdtemp(join(tmpdir(), "supersigil-eval-plugin-"));
    const workspacePath = join(baseDir, "workspace");
    const runtimeDir = join(baseDir, "runtime");
    const runDir = join(baseDir, "run");
    const resolvedBinary = join(baseDir, "eval", "bin", "supersigil");

    await mkdir(workspacePath, { recursive: true });
    await mkdir(runtimeDir, { recursive: true });
    await mkdir(runDir, { recursive: true });
    await mkdir(join(baseDir, "eval", "bin"), { recursive: true });

    await writeFile(
      join(workspacePath, "supersigil.toml"),
      'paths = ["specs/**/*.mdx"]\n',
    );
    await writeFile(resolvedBinary, "");

    const artifacts = await supersigilArtifactCaptureHook.capture({
      workspacePath,
      runId: "run-123",
      scenario: {
        version: 1,
        name: "test-scenario",
        description: "Test scenario",
        goal: "Test goal",
        template: "./template",
        timeout: 60_000,
        success_criteria: [],
      },
      events: [],
      outDir: runDir,
      runDir,
      config: {},
      resolvedCliPaths: { supersigil: resolvedBinary },
      getPluginData: () => undefined,
      setPluginData: () => {},
    });

    expect(artifacts.some((artifact) => artifact.path === "supersigil-binary.txt")).toBe(
      true,
    );

    const recordedBinary = await readFile(join(runDir, "supersigil-binary.txt"), "utf-8");
    expect(recordedBinary.trim()).toBe(resolvedBinary);

    await rm(baseDir, { recursive: true, force: true });
  });

  test("artifact capture preserves JSON stdout and splits stderr into sidecar files", async () => {
    const baseDir = await mkdtemp(join(tmpdir(), "supersigil-eval-plugin-"));
    const workspacePath = join(baseDir, "workspace");
    const runtimeDir = join(baseDir, "runtime");
    const runDir = join(baseDir, "run");
    const fakeBinary = join(baseDir, "fake-supersigil");

    await mkdir(workspacePath, { recursive: true });
    await mkdir(runtimeDir, { recursive: true });
    await mkdir(runDir, { recursive: true });

    await writeFile(
      join(workspacePath, "supersigil.toml"),
      'paths = ["specs/**/*.mdx"]\n',
    );
    await writeFile(
      fakeBinary,
      `#!/usr/bin/env bash
set -eu
case "$1" in
  status)
    printf '{"total_documents":1,"targets_total":1,"targets_covered":0}\\n'
    printf 'hint: status stderr\\n' >&2
    ;;
  verify)
    printf '{"findings":[],"summary":{"total_documents":1,"error_count":0,"warning_count":0,"info_count":0}}\\n'
    printf '[ok] 1 documents verified, no findings.\\n' >&2
    ;;
  lint)
    printf 'lint ok\\n'
    printf 'lint stderr\\n' >&2
    ;;
  *)
    printf 'unexpected command\\n' >&2
    exit 2
    ;;
esac
`,
    );
    await chmod(fakeBinary, 0o755);

    const artifacts = await supersigilArtifactCaptureHook.capture({
      workspacePath,
      runId: "run-123",
      scenario: {
        version: 1,
        name: "test-scenario",
        description: "Test scenario",
        goal: "Test goal",
        template: "./template",
        timeout: 60_000,
        success_criteria: [],
      },
      events: [],
      outDir: runDir,
      runDir,
      config: {},
      resolvedCliPaths: { supersigil: fakeBinary },
      getPluginData: () => undefined,
      setPluginData: () => {},
    });

    expect(artifacts.some((artifact) => artifact.path === "supersigil-verify.stderr.txt")).toBe(
      true,
    );

    const verifyStdout = await readFile(join(runDir, "supersigil-verify.json"), "utf-8");
    expect(verifyStdout.trim()).toBe(
      '{"findings":[],"summary":{"total_documents":1,"error_count":0,"warning_count":0,"info_count":0}}',
    );
    expect(verifyStdout).not.toContain("STDERR:");
    expect(verifyStdout).not.toContain("[ok]");

    const verifyStderr = await readFile(join(runDir, "supersigil-verify.stderr.txt"), "utf-8");
    expect(verifyStderr.trim()).toBe("[ok] 1 documents verified, no findings.");

    const statusStdout = await readFile(join(runDir, "supersigil-status.json"), "utf-8");
    expect(statusStdout.trim()).toBe('{"total_documents":1,"targets_total":1,"targets_covered":0}');
    const statusStderr = await readFile(join(runDir, "supersigil-status.stderr.txt"), "utf-8");
    expect(statusStderr.trim()).toBe("hint: status stderr");

    await rm(baseDir, { recursive: true, force: true });
  });

  test("artifact capture preserves per-invocation verify artifacts from command events", async () => {
    const baseDir = await mkdtemp(join(tmpdir(), "supersigil-eval-plugin-"));
    const workspacePath = join(baseDir, "workspace");
    const runtimeDir = join(baseDir, "runtime");
    const runDir = join(baseDir, "run");
    const fakeBinary = join(baseDir, "fake-supersigil");

    await mkdir(workspacePath, { recursive: true });
    await mkdir(runtimeDir, { recursive: true });
    await mkdir(runDir, { recursive: true });

    await writeFile(
      join(workspacePath, "supersigil.toml"),
      'paths = ["specs/**/*.mdx"]\n',
    );
    await writeFile(
      fakeBinary,
      `#!/usr/bin/env bash
set -eu
case "$1" in
  status)
    printf '{"total_documents":1,"targets_total":1,"targets_covered":0}\\n'
    ;;
  verify)
    printf '{"findings":[],"summary":{"total_documents":1,"error_count":0,"warning_count":0,"info_count":0}}\\n'
    ;;
  lint)
    printf 'lint ok\\n'
    ;;
  *)
    exit 2
    ;;
esac
`,
    );
    await chmod(fakeBinary, 0o755);

    const firstVerify = `${createVerifyReport({
      findings: [
        {
          rule: "missing_verification_evidence",
          doc_id: "auth/req/login",
          message: "criterion has no evidence",
          effective_severity: "error",
        },
      ],
      error_count: 1,
    })}\n` +
      "hint: Run `supersigil refs` to list canonical criterion refs you can copy into evidence.\n";
    const secondVerify = `${createVerifyReport()}\n[ok] 1 documents verified, no findings.\n`;

    const artifacts = await supersigilArtifactCaptureHook.capture({
      workspacePath,
      runId: "run-123",
      scenario: {
        version: 1,
        name: "test-scenario",
        description: "Test scenario",
        goal: "Test goal",
        template: "./template",
        timeout: 60_000,
        success_criteria: [],
      },
      events: [
        {
          type: "command_completed",
          command: "supersigil verify --format json",
          exitCode: 1,
          stdout: firstVerify,
          raw: {},
        },
        {
          type: "command_completed",
          command: "supersigil verify --format json",
          exitCode: 0,
          stdout: secondVerify,
          raw: {},
        },
      ],
      outDir: runDir,
      runDir,
      config: {},
      resolvedCliPaths: { supersigil: fakeBinary },
      getPluginData: () => undefined,
      setPluginData: () => {},
    });

    expect(artifacts.some((artifact) => artifact.path === "supersigil-verify-1.json")).toBe(
      true,
    );
    expect(artifacts.some((artifact) => artifact.path === "supersigil-verify-1.stderr.txt")).toBe(
      true,
    );
    expect(artifacts.some((artifact) => artifact.path === "supersigil-verify-2.json")).toBe(
      true,
    );
    expect(artifacts.some((artifact) => artifact.path === "supersigil-verify-2.stderr.txt")).toBe(
      true,
    );

    const firstJson = await readFile(join(runDir, "supersigil-verify-1.json"), "utf-8");
    expect(JSON.parse(firstJson).summary.error_count).toBe(1);
    const firstStderr = await readFile(join(runDir, "supersigil-verify-1.stderr.txt"), "utf-8");
    expect(firstStderr).toContain("supersigil refs");

    const secondJson = await readFile(join(runDir, "supersigil-verify-2.json"), "utf-8");
    expect(JSON.parse(secondJson).summary.error_count).toBe(0);
    const secondStderr = await readFile(join(runDir, "supersigil-verify-2.stderr.txt"), "utf-8");
    expect(secondStderr).toContain("[ok] 1 documents verified, no findings.");

    await rm(baseDir, { recursive: true, force: true });
  });

  test("repo wrapper bootstraps the release binary before exec", async () => {
    const evalDir = dirname(fileURLToPath(import.meta.url));
    const shellWrapper = await readFile(join(evalDir, "bin", "supersigil"), "utf-8");
    const cmdWrapper = await readFile(join(evalDir, "bin", "supersigil.cmd"), "utf-8");

    expect(shellWrapper).toContain("target/release/supersigil");
    expect(shellWrapper).toContain("cargo build");
    expect(shellWrapper).not.toContain("cargo run");

    expect(cmdWrapper).toContain("target\\release\\supersigil.exe");
    expect(cmdWrapper).toContain("cargo build");
    expect(cmdWrapper).not.toContain("cargo run");
  });

  test("report insights aggregate failing subcommands", async () => {
    const baseDir = await mkdtemp(join(tmpdir(), "supersigil-eval-plugin-"));
    const artifactPath = join(baseDir, "run-1");
    await mkdir(join(artifactPath), { recursive: true });
    await writeFile(
      join(artifactPath, "supersigil-calls.log"),
      [
        '{"timestamp":"2026-03-07T00:00:00Z","args":["verify"],"exitCode":1,"signal":null,"durationMs":10}',
        '{"timestamp":"2026-03-07T00:00:01Z","args":["plan","auth"],"exitCode":2,"signal":null,"durationMs":20}',
        '{"timestamp":"2026-03-07T00:00:02Z","args":["verify"],"exitCode":1,"signal":null,"durationMs":15}',
      ].join("\n"),
    );

    const insights = await supersigilReportInsightGenerator.generate([
      {
        runId: "run-1",
        scenario: "cli-workflow",
        verdict: "fail",
        duration: 1_000,
        failureDetails: "failed",
        commandCount: 3,
        artifactPath,
      },
    ]);

    expect(insights.some((insight) => insight.title.includes("verify failures"))).toBe(
      true,
    );
    expect(insights.some((insight) => insight.title.includes("plan failures"))).toBe(
      true,
    );

    await rm(baseDir, { recursive: true, force: true });
  });

  test("exports the initial supersigil success criteria", () => {
    expect(supersigilPlugin.successCriteria?.map((criterion) => criterion.type)).toEqual([
      "supersigil_exit_code",
      "supersigil_status_metric",
      "supersigil_plan_has_actionable",
      "supersigil_plan_has_blocked",
      "supersigil_verify_clean",
      "supersigil_verify_has_finding",
      "supersigil_affected_contains",
      "supersigil_command_sequence",
    ]);
  });

  test("supersigil_exit_code selects the last matching command", async () => {
    const criterion = getCriterion("supersigil_exit_code");

    const result = await criterion.evaluate(
      {
        command: {
          subcommand: "verify",
          args_contains: ["--format", "json"],
          occurrence: "last",
        },
        equals: 0,
      },
      createEvaluationContext([
        {
          type: "command_completed",
          command: "ls -la",
          exitCode: 0,
          raw: {},
        },
        {
          type: "command_completed",
          command: "/bin/zsh -lc 'supersigil verify --format json'",
          exitCode: 2,
          raw: {},
        },
        {
          type: "command_completed",
          command: "supersigil verify --format json",
          exitCode: 0,
          raw: {},
        },
      ]),
    );

    expect(result.passed).toBe(true);
  });

  test("supersigil_command_sequence matches an ordered subsequence", async () => {
    const criterion = getCriterion("supersigil_command_sequence");

    const result = await criterion.evaluate(
      {
        mode: "ordered_subsequence",
        steps: [
          {
            subcommand: "verify",
            args_contains: ["--format", "json"],
            exit_code: 1,
          },
          {
            subcommand: "verify",
            args_contains: ["--format", "json"],
            exit_code: 0,
          },
        ],
      },
      createEvaluationContext([
        {
          type: "command_completed",
          command: "supersigil status --format json",
          exitCode: 0,
          raw: {},
        },
        {
          type: "command_completed",
          command: "supersigil verify --format json",
          exitCode: 1,
          stdout: createVerifyReport({
            findings: [
              {
                rule: "missing_verification_evidence",
                doc_id: "auth/req/login",
                message: "Missing verification evidence for valid-creds",
                effective_severity: "error",
              },
            ],
            error_count: 1,
          }),
          raw: {},
        },
        {
          type: "command_completed",
          command: "cat specs/auth/req/login.mdx",
          exitCode: 0,
          raw: {},
        },
        {
          type: "command_completed",
          command: "supersigil verify --format json",
          exitCode: 0,
          stdout: createVerifyReport(),
          raw: {},
        },
      ]),
    );

    expect(result.passed).toBe(true);
  });

  test("supersigil_verify_has_finding matches rule, severity, doc, and message filters", async () => {
    const criterion = getCriterion("supersigil_verify_has_finding");

    const result = await criterion.evaluate(
      {
        command: {
          subcommand: "verify",
          args_contains: ["--format", "json"],
        },
        rule: "missing_verification_evidence",
        severity: "error",
        doc_id: "auth/req/login",
        message_contains: "valid-creds",
      },
      createEvaluationContext([
        {
          type: "command_completed",
          command: "supersigil verify --format json",
          exitCode: 1,
          stdout: createVerifyReport({
            findings: [
              {
                rule: "missing_verification_evidence",
                doc_id: "auth/req/login",
                message: "Missing verification evidence for valid-creds",
                effective_severity: "error",
              },
            ],
            error_count: 1,
          }),
          raw: {},
        },
      ]),
    );

    expect(result.passed).toBe(true);
  });

  test("supersigil_verify_clean accepts info-only reports", async () => {
    const criterion = getCriterion("supersigil_verify_clean");

    const result = await criterion.evaluate(
      {
        command: {
          subcommand: "verify",
          args_contains: ["--format", "json"],
        },
      },
      createEvaluationContext([
        {
          type: "command_completed",
          command: "supersigil verify --format json",
          exitCode: 0,
          stdout: createVerifyReport({ info_count: 1 }),
          raw: {},
        },
      ]),
    );

    expect(result.passed).toBe(true);
  });

  test("supersigil_verify_clean handles mixed stdout/stderr from agents", async () => {
    const criterion = getCriterion("supersigil_verify_clean");

    // Simulate an agent that merges stderr into stdout
    const jsonOutput = createVerifyReport();
    const mixedOutput = `${jsonOutput}\n[ok] 1 documents verified, no findings.\n`;

    const result = await criterion.evaluate(
      {
        command: {
          subcommand: "verify",
          args_contains: ["--format", "json"],
        },
      },
      createEvaluationContext([
        {
          type: "command_completed",
          command: "supersigil verify --format json",
          exitCode: 0,
          stdout: mixedOutput,
          raw: {},
        },
      ]),
    );

    expect(result.passed).toBe(true);
  });

  test("supersigil_verify_clean handles mixed output with trailing braces in hints", async () => {
    const criterion = getCriterion("supersigil_verify_clean");

    const jsonOutput = createVerifyReport();
    const mixedOutput =
      `${jsonOutput}\n` +
      "hint: Authored fix: add <VerifiedBy strategy=\"file-glob\" paths={[\"tests/auth/login_test.rs\"]} /> evidence.\n";

    const result = await criterion.evaluate(
      {
        command: {
          subcommand: "verify",
          args_contains: ["--format", "json"],
        },
      },
      createEvaluationContext([
        {
          type: "command_completed",
          command: "supersigil verify --format json",
          exitCode: 0,
          stdout: mixedOutput,
          raw: {},
        },
      ]),
    );

    expect(result.passed).toBe(true);
  });

  test("supersigil_verify_has_finding handles mixed stdout/stderr from agents", async () => {
    const criterion = getCriterion("supersigil_verify_has_finding");

    const jsonOutput = createVerifyReport({
      findings: [
        {
          rule: "missing_verification_evidence",
          doc_id: "auth/req/login",
          message: "criterion has no evidence",
          effective_severity: "error",
        },
      ],
      error_count: 1,
    });
    const mixedOutput = `${jsonOutput}\nhint: Run \`supersigil plan\` to see outstanding work.\n`;

    const result = await criterion.evaluate(
      {
        command: {
          subcommand: "verify",
          args_contains: ["--format", "json"],
        },
        rule: "missing_verification_evidence",
      },
      createEvaluationContext([
        {
          type: "command_completed",
          command: "supersigil verify --format json",
          exitCode: 1,
          stdout: mixedOutput,
          raw: {},
        },
      ]),
    );

    expect(result.passed).toBe(true);
  });

  test("supersigil_verify_has_finding handles mixed output with trailing braces in hints", async () => {
    const criterion = getCriterion("supersigil_verify_has_finding");

    const jsonOutput = createVerifyReport({
      findings: [
        {
          rule: "missing_verification_evidence",
          doc_id: "auth/req/login",
          message: "criterion has no evidence",
          effective_severity: "error",
        },
      ],
      error_count: 1,
    });
    const mixedOutput =
      `${jsonOutput}\n` +
      "hint: Authored fix: add <VerifiedBy strategy=\"file-glob\" paths={[\"tests/auth/login_test.rs\"]} /> evidence.\n";

    const result = await criterion.evaluate(
      {
        command: {
          subcommand: "verify",
          args_contains: ["--format", "json"],
        },
        rule: "missing_verification_evidence",
      },
      createEvaluationContext([
        {
          type: "command_completed",
          command: "supersigil verify --format json",
          exitCode: 1,
          stdout: mixedOutput,
          raw: {},
        },
      ]),
    );

    expect(result.passed).toBe(true);
  });

  test("supersigil_status_metric reads a numeric field from status json", async () => {
    const criterion = getCriterion("supersigil_status_metric");

    const result = await criterion.evaluate(
      {
        command: {
          subcommand: "status",
          args_contains: ["--format", "json"],
        },
        field: "targets_total",
        equals: 3,
      },
      createEvaluationContext([
        {
          type: "command_completed",
          command: "supersigil status --format json",
          exitCode: 0,
          stdout: JSON.stringify({
            total_documents: 2,
            targets_total: 3,
            targets_covered: 1,
          }),
          raw: {},
        },
      ]),
    );

    expect(result.passed).toBe(true);
  });

  test("supersigil_plan_has_actionable finds a specific actionable target", async () => {
    const criterion = getCriterion("supersigil_plan_has_actionable");

    const result = await criterion.evaluate(
      {
        command: {
          subcommand: "plan",
          args_contains: ["--format", "json"],
        },
        target: "auth/req/login#valid-creds",
      },
      createEvaluationContext([
        {
          type: "command_completed",
          command: "supersigil plan auth/req/login --format json",
          exitCode: 0,
          stdout: JSON.stringify({
            outstanding_targets: [
              {
                doc_id: "auth/req/login",
                target_id: "valid-creds",
              },
              {
                doc_id: "auth/req/login",
                target_id: "rate-limit",
              },
            ],
            pending_tasks: [
              {
                tasks_doc_id: "auth/tasks",
                task_id: "task-1",
                status: "todo",
                body_text: "Implement login",
                implements: [["auth/req/login", "valid-creds"]],
                depends_on: [],
              },
              {
                tasks_doc_id: "auth/tasks",
                task_id: "task-2",
                status: "todo",
                body_text: "Add rate limiting",
                implements: [["auth/req/login", "rate-limit"]],
                depends_on: ["task-1"],
              },
            ],
            completed_tasks: [],
          }),
          raw: {},
        },
      ]),
    );

    expect(result.passed).toBe(true);
  });

  test("supersigil_plan_has_blocked finds a specific blocked target", async () => {
    const criterion = getCriterion("supersigil_plan_has_blocked");

    const result = await criterion.evaluate(
      {
        command: {
          subcommand: "plan",
          args_contains: ["--format", "json"],
        },
        target: "auth/req/login#rate-limit",
      },
      createEvaluationContext([
        {
          type: "command_completed",
          command: "supersigil plan auth/req/login --format json",
          exitCode: 0,
          stdout: JSON.stringify({
            outstanding_targets: [
              {
                doc_id: "auth/req/login",
                target_id: "valid-creds",
              },
              {
                doc_id: "auth/req/login",
                target_id: "rate-limit",
              },
            ],
            pending_tasks: [
              {
                tasks_doc_id: "auth/tasks",
                task_id: "task-1",
                status: "todo",
                body_text: "Implement login",
                implements: [["auth/req/login", "valid-creds"]],
                depends_on: [],
              },
              {
                tasks_doc_id: "auth/tasks",
                task_id: "task-2",
                status: "todo",
                body_text: "Add rate limiting",
                implements: [["auth/req/login", "rate-limit"]],
                depends_on: ["task-1"],
              },
            ],
            completed_tasks: [],
          }),
          raw: {},
        },
      ]),
    );

    expect(result.passed).toBe(true);
  });

  test("supersigil_affected_contains matches a changed document", async () => {
    const criterion = getCriterion("supersigil_affected_contains");

    const result = await criterion.evaluate(
      {
        command: {
          subcommand: "affected",
          args_contains: ["--format", "json"],
        },
        doc_id: "auth/req/login",
        changed_file: "src/auth.rs",
      },
      createEvaluationContext([
        {
          type: "command_completed",
          command: "supersigil affected --since HEAD~1 --format json",
          exitCode: 0,
          stdout: JSON.stringify([
            {
              id: "auth/req/login",
              path: "specs/auth/req/login.mdx",
              matched_globs: ["src/**/*.rs"],
              changed_files: ["src/auth.rs"],
            },
          ]),
          raw: {},
        },
      ]),
    );

    expect(result.passed).toBe(true);
  });
});
