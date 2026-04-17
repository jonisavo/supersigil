import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import {
  formatCommandFailure,
  runCaptured as runCapturedCommand,
  runStreaming as runStreamingCommand,
} from "./release-command.mjs";
import { commandPrepare } from "./release-targets/index.mjs";

const REPO_ROOT = fileURLToPath(new URL("../", import.meta.url));

class ReleaseError extends Error {
  constructor(message) {
    super(message);
    this.name = "ReleaseError";
  }
}

function fail(message) {
  throw new ReleaseError(message);
}

function ensureAllowedStatus(command, args, result, { allowStatus = [0] } = {}) {
  if (!allowStatus.includes(result.status ?? -1)) {
    fail(formatCommandFailure(command, args, result));
  }

  return result;
}

function runCaptured(command, args, { allowStatus = [0] } = {}) {
  const result = runCapturedCommand(command, args, { cwd: REPO_ROOT });
  return ensureAllowedStatus(command, args, result, { allowStatus });
}

function runStreaming(command, args, { allowStatus = [0] } = {}) {
  const result = runStreamingCommand(command, args, { cwd: REPO_ROOT });
  return ensureAllowedStatus(command, args, result, { allowStatus });
}

function parseVersion(argv) {
  if (argv.length !== 1 || argv[0].trim() === "") {
    fail("Usage: mise release <version>");
  }

  return argv[0].replace(/^v/, "");
}

function loadRegistry() {
  return JSON.parse(readFileSync(new URL("../release-targets.json", import.meta.url), "utf8"));
}

function targetIsImpacted(result, targetId) {
  return Boolean(result.targets[targetId]?.impacted);
}

function anyPnpmTargetImpacted(result, registry) {
  return registry.targets
    .filter((target) => target.versionKind !== "cargo-workspace")
    .some((target) => targetIsImpacted(result, target.id));
}

function pathHasChanges(path) {
  const result = runCaptured("git", ["diff", "--quiet", "--", path], {
    allowStatus: [0, 1],
  });

  if (result.status === 0) {
    return false;
  }
  if (result.status === 1) {
    return true;
  }

  fail(formatCommandFailure("git", ["diff", "--quiet", "--", path], result));
}

function stageIfChanged(path) {
  if (!pathHasChanges(path)) {
    return;
  }

  runStreaming("git", ["add", "--", path]);
}

function listWorkspaceCargoManifestPaths() {
  return runCaptured("git", ["ls-files", "--", ":(glob)crates/*/Cargo.toml"])
    .stdout
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
}

function printSummary(version) {
  console.log("");
  console.log(`Release v${version} prepared. Review with:`);
  console.log("  git log --oneline -1");
  console.log("  git diff HEAD~1");
  console.log("");
  console.log("To publish:");
  console.log("  git push && git push --tags");
  console.log("");
  console.log("To undo:");
  console.log(`  git tag -d v${version} && git reset --soft HEAD~1`);
}

function main(argv) {
  process.chdir(REPO_ROOT);

  const version = parseVersion(argv);
  const gitCliffBin = "git-cliff";

  console.log(`Preparing release v${version}...`);

  const prepareResult = commandPrepare({
    version,
    "git-cliff-bin": gitCliffBin,
  });
  const registry = loadRegistry();

  if (targetIsImpacted(prepareResult, "crates")) {
    runStreaming("cargo", ["generate-lockfile"]);
  }

  if (anyPnpmTargetImpacted(prepareResult, registry)) {
    runStreaming("pnpm", ["install", "--lockfile-only"]);
  }

  runStreaming(gitCliffBin, ["--tag", `v${version}`, "--output", "CHANGELOG.md"]);

  if (targetIsImpacted(prepareResult, "crates")) {
    stageIfChanged("Cargo.toml");
    stageIfChanged("Cargo.lock");
    for (const path of listWorkspaceCargoManifestPaths()) {
      stageIfChanged(path);
    }
  }

  if (targetIsImpacted(prepareResult, "npm-eslint-plugin")) {
    stageIfChanged("packages/eslint-plugin/package.json");
  }

  if (targetIsImpacted(prepareResult, "npm-vitest")) {
    stageIfChanged("packages/vitest/package.json");
  }

  if (targetIsImpacted(prepareResult, "vscode")) {
    stageIfChanged("editors/vscode/package.json");
    stageIfChanged("editors/vscode/CHANGELOG.md");
  }

  if (targetIsImpacted(prepareResult, "intellij")) {
    stageIfChanged("editors/intellij/gradle.properties");
    stageIfChanged("editors/intellij/CHANGELOG.md");
  }

  if (anyPnpmTargetImpacted(prepareResult, registry)) {
    stageIfChanged("pnpm-lock.yaml");
  }

  stageIfChanged("CHANGELOG.md");

  runStreaming("git", ["commit", "-m", `chore(release): prepare v${version}`]);
  runStreaming("git", ["tag", `v${version}`]);

  printSummary(version);
}

try {
  main(process.argv.slice(2));
} catch (error) {
  if (error instanceof ReleaseError) {
    console.error(`error: ${error.message}`);
    process.exit(1);
  }

  throw error;
}
