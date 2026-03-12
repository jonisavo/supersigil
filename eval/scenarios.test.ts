import { describe, expect, test } from "bun:test";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { ScenarioLoader } from "../../sniff/packages/cli/src/scenario/scenario-loader.ts";
import { supersigilPlugin } from "./plugin";

const evalDir = dirname(fileURLToPath(import.meta.url));
const scenariosDir = join(evalDir, "scenarios");
const loader = new ScenarioLoader();

const coreCriterionTypes = new Set([
  "file_exists",
  "file_not_exists",
  "dir_exists",
  "dir_not_exists",
  "glob_exists",
  "file_contains",
  "file_not_contains",
  "json_field",
  "yaml_field",
  "transcript_contains",
]);

const pluginCriterionTypes = new Set(
  supersigilPlugin.successCriteria?.map((criterion) => criterion.type) ?? [],
);

async function loadScenarioSuite() {
  return loader.loadSuite(scenariosDir);
}

describe("supersigil eval scenarios", () => {
  test("suite contains the first seven roadmap scenarios", async () => {
    const scenarios = await loadScenarioSuite();

    expect(scenarios.map(({ scenario }) => scenario.name)).toEqual([
      "bootstrap-init",
      "missing-config-recovery",
      "rust-verifies",
      "status-plan-context-triage",
      "unknown-id-recovery",
      "verify-remediation",
      "warning-only-verify",
    ]);
  });

  test("every scenario uses only supported core or supersigil criteria", async () => {
    const scenarios = await loadScenarioSuite();

    for (const { scenario } of scenarios) {
      for (const criterion of scenario.success_criteria) {
        const supported =
          coreCriterionTypes.has(criterion.type) ||
          pluginCriterionTypes.has(criterion.type);
        expect(supported).toBe(true);
      }
    }
  });

  test("bootstrap-init requires init, new, and lint behavior", async () => {
    const scenarios = await loadScenarioSuite();
    const scenario = scenarios.find(
      ({ scenario: loaded }) => loaded.name === "bootstrap-init",
    )?.scenario;

    expect(scenario).toBeDefined();
    expect(scenario?.success_criteria.some((criterion) => criterion.type === "supersigil_command_sequence"))
      .toBe(true);
    expect(scenario?.success_criteria.some((criterion) => criterion.type === "supersigil_exit_code"))
      .toBe(true);
  });

  test("status-plan-context-triage uses plan and status behavior criteria", async () => {
    const scenarios = await loadScenarioSuite();
    const scenario = scenarios.find(
      ({ scenario: loaded }) => loaded.name === "status-plan-context-triage",
    )?.scenario;

    expect(scenario).toBeDefined();
    expect(scenario?.success_criteria.some((criterion) => criterion.type === "supersigil_status_metric"))
      .toBe(true);
    expect(scenario?.success_criteria.some((criterion) => criterion.type === "supersigil_plan_has_actionable"))
      .toBe(true);
    expect(scenario?.success_criteria.some((criterion) => criterion.type === "supersigil_plan_has_blocked"))
      .toBe(true);
  });

  test("missing-config-recovery requires failed status recovery via init", async () => {
    const scenarios = await loadScenarioSuite();
    const scenario = scenarios.find(
      ({ scenario: loaded }) => loaded.name === "missing-config-recovery",
    )?.scenario;

    expect(scenario).toBeDefined();
    expect(scenario?.success_criteria.some((criterion) => criterion.type === "supersigil_status_metric"))
      .toBe(true);
    expect(scenario?.success_criteria.some((criterion) => criterion.type === "supersigil_command_sequence"))
      .toBe(true);
    expect(scenario?.success_criteria.some((criterion) => criterion.type === "supersigil_exit_code"))
      .toBe(true);
  });

  test("missing-config-recovery preserves the seeded requirement file", async () => {
    const scenarios = await loadScenarioSuite();
    const scenario = scenarios.find(
      ({ scenario: loaded }) => loaded.name === "missing-config-recovery",
    )?.scenario;

    expect(scenario).toBeDefined();
    expect(scenario?.success_criteria).toContainEqual({
      type: "file_exists",
      path: "specs/auth.req.mdx",
    });
    expect(scenario?.success_criteria).toContainEqual({
      type: "file_contains",
      path: "specs/auth.req.mdx",
      contains: "id: auth/req/login",
    });
    expect(scenario?.success_criteria).toContainEqual({
      type: "file_contains",
      path: "specs/auth.req.mdx",
      contains: '<Criterion id="login-succeeds">',
    });
  });

  test("unknown-id-recovery requires failed lookup recovery via ls", async () => {
    const scenarios = await loadScenarioSuite();
    const scenario = scenarios.find(
      ({ scenario: loaded }) => loaded.name === "unknown-id-recovery",
    )?.scenario;

    expect(scenario).toBeDefined();
    expect(scenario?.success_criteria.some((criterion) => criterion.type === "supersigil_command_sequence"))
      .toBe(true);
    expect(scenario?.success_criteria.some((criterion) => criterion.type === "supersigil_exit_code"))
      .toBe(true);
  });

  test("verify-remediation requires pre-fix and post-fix verify behavior", async () => {
    const scenarios = await loadScenarioSuite();
    const scenario = scenarios.find(
      ({ scenario: loaded }) => loaded.name === "verify-remediation",
    )?.scenario;

    expect(scenario).toBeDefined();
    expect(scenario?.success_criteria.some((criterion) => criterion.type === "supersigil_verify_has_finding"))
      .toBe(true);
    expect(scenario?.success_criteria.some((criterion) => criterion.type === "supersigil_verify_clean"))
      .toBe(true);
    expect(scenario?.success_criteria.some((criterion) => criterion.type === "supersigil_command_sequence"))
      .toBe(true);
  });

  test("verify-remediation does not hard-code a specific evidence edit path", async () => {
    const scenarios = await loadScenarioSuite();
    const scenario = scenarios.find(
      ({ scenario: loaded }) => loaded.name === "verify-remediation",
    )?.scenario;

    expect(scenario).toBeDefined();
    expect(scenario?.success_criteria).not.toContainEqual({
      type: "file_contains",
      path: "specs/auth.req.mdx",
      contains: '<VerifiedBy strategy="file-glob" paths="tests/auth_login.rs" />',
    });
  });

  test("verify-remediation reviewer context points reviewers at split verify artifacts", async () => {
    const scenarios = await loadScenarioSuite();
    const scenario = scenarios.find(
      ({ scenario: loaded }) => loaded.name === "verify-remediation",
    )?.scenario;

    expect(scenario).toBeDefined();
    expect(scenario?.reviewer_context).toContain("Transcript stream fields may merge stdout/stderr");
    expect(scenario?.reviewer_context).toContain("supersigil-verify.json");
    expect(scenario?.reviewer_context).toContain("supersigil-verify.stderr.txt");
    expect(scenario?.reviewer_context).toContain("supersigil-verify-1.json");
  });

  test("warning-only-verify requires warning finding inspection", async () => {
    const scenarios = await loadScenarioSuite();
    const scenario = scenarios.find(
      ({ scenario: loaded }) => loaded.name === "warning-only-verify",
    )?.scenario;

    expect(scenario).toBeDefined();
    expect(scenario?.success_criteria.some((criterion) => criterion.type === "supersigil_verify_has_finding"))
      .toBe(true);
    expect(scenario?.success_criteria.some((criterion) => criterion.type === "supersigil_exit_code"))
      .toBe(true);
  });

  test("rust-verifies now gates on verify-clean behavior", async () => {
    const scenarios = await loadScenarioSuite();
    const scenario = scenarios.find(
      ({ scenario: loaded }) => loaded.name === "rust-verifies",
    )?.scenario;

    expect(scenario).toBeDefined();
    expect(scenario?.success_criteria.some((criterion) => criterion.type === "supersigil_verify_clean"))
      .toBe(true);
    expect(scenario?.success_criteria.some((criterion) => criterion.type === "supersigil_exit_code"))
      .toBe(true);
  });
});
