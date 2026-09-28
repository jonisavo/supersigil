import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it, afterEach } from "vitest";
import { commandDetect, commandPrepare } from "./index.mjs";

const BASE_REF = "v0.1.0";
const RELEASE_VERSION = "0.2.0";
const tempDirs = [];
const GIT_ENV_VARS_TO_CLEAR = [
  "GIT_COMMON_DIR",
  "GIT_DIR",
  "GIT_INDEX_FILE",
  "GIT_PREFIX",
  "GIT_WORK_TREE",
];

afterEach(() => {
  while (tempDirs.length > 0) {
    rmSync(tempDirs.pop(), { recursive: true, force: true });
  }
});

function createTempRepo() {
  const dir = mkdtempSync(join(tmpdir(), "release-targets-test-"));
  tempDirs.push(dir);
  return dir;
}

function writeTestFile(repoDir, relativePath, contents) {
  const path = join(repoDir, relativePath);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, contents, "utf8");
}

function sanitizedGitEnv() {
  const env = { ...process.env };
  for (const name of GIT_ENV_VARS_TO_CLEAR) {
    delete env[name];
  }
  return env;
}

function run(dir, program, args) {
  const output = spawnSync(program, args, {
    cwd: dir,
    encoding: "utf8",
    env: program === "git" ? sanitizedGitEnv() : process.env,
  });

  expect(output.status).toBe(0);

  return output.stdout;
}

function git(dir, ...args) {
  return run(dir, "git", args);
}

function gitCommit(dir, message = "fixture") {
  git(dir, "add", ".");
  git(dir, "commit", "-m", message);
}

function writeAndCommit(dir, relativePath, contents, message) {
  writeTestFile(dir, relativePath, contents);
  gitCommit(dir, message);
}

function readRepoFile(dir, relativePath) {
  return readFileSync(join(dir, relativePath), "utf8");
}

function checkedInPath(relativePath) {
  return join(dirname(fileURLToPath(import.meta.url)), relativePath);
}

function readCheckedInFile(relativePath) {
  return readFileSync(checkedInPath(relativePath), "utf8");
}

function normalizeNewlines(contents) {
  return contents.replaceAll("\r\n", "\n");
}

function expectRepoFileContains(dir, relativePath, text) {
  expect(readRepoFile(dir, relativePath)).toContain(text);
}

function readCheckedInReleaseTargets() {
  return JSON.parse(readCheckedInFile("../../release-targets.json"));
}

function buildFixtureRegistry() {
  return readCheckedInReleaseTargets();
}

function getCheckedInTarget(targetId) {
  const config = readCheckedInReleaseTargets();
  const target = config.targets.find(({ id }) => id === targetId);
  expect(target).toBeDefined();
  return target;
}

function expectTargetPaths(targetId, paths) {
  const target = getCheckedInTarget(targetId);
  for (const path of paths) {
    expect(target.impactPaths).toContain(path);
  }
  return target;
}

function formatWorkspaceCargoToml({
  corePackageName,
  cliDependencyName,
  coreCrateDirName,
  cliCrateDirName,
}) {
  return `[workspace]
members = ["crates/${coreCrateDirName}", "crates/${cliCrateDirName}"]

[workspace.dependencies]
${corePackageName} = { path = "crates/${coreCrateDirName}", version = "=0.1.0" }
${cliDependencyName} = { path = "crates/${cliCrateDirName}", version = "=0.1.0" }
`;
}

function initFixtureRepo({
  corePackageName = "supersigil-core",
  cliDependencyName = "supersigil-cli",
  coreCrateDirName = "supersigil-core",
  cliCrateDirName = "supersigil-cli",
  cliPackageName = "supersigil",
} = {}) {
  const dir = createTempRepo();

  git(dir, "init");
  git(dir, "config", "user.name", "Supersigil Tests");
  git(dir, "config", "user.email", "tests@supersigil.invalid");

  writeTestFile(
    dir,
    "release-targets.json",
    `${JSON.stringify(buildFixtureRegistry(), null, 2)}\n`,
  );
  writeTestFile(
    dir,
    "Cargo.toml",
    formatWorkspaceCargoToml({
      corePackageName,
      cliDependencyName,
      coreCrateDirName,
      cliCrateDirName,
    }),
  );
  writeTestFile(dir, "Cargo.lock", "# fixture lockfile\n");
  writeTestFile(
    dir,
    `crates/${coreCrateDirName}/Cargo.toml`,
    `[package]
name = "${corePackageName}"
version = "0.1.0"
`,
  );
  writeTestFile(
    dir,
    `crates/${coreCrateDirName}/src/lib.rs`,
    "pub const CORE: u32 = 1;\n",
  );
  writeTestFile(
    dir,
    `crates/${cliCrateDirName}/Cargo.toml`,
    `[package]
name = "${cliPackageName}"
version = "0.1.0"
include = ["src/", "skills/", "README.md"]
`,
  );
  writeTestFile(
    dir,
    `crates/${cliCrateDirName}/src/lib.rs`,
    "pub const CLI: u32 = 1;\n",
  );

  gitCommit(dir, "initial fixture");
  git(dir, "tag", BASE_REF);

  return dir;
}

function runReleaseTargets(dir, args) {
  const previousDir = process.cwd();
  process.chdir(dir);

  try {
    const [subcommand, ...rest] = args;
    const options = {};

    for (let index = 0; index < rest.length; index += 1) {
      const arg = rest[index];
      if (!arg.startsWith("--")) {
        continue;
      }

      const key = arg.slice(2);
      const value = rest[index + 1];
      if (value === undefined || value.startsWith("--")) {
        options[key] = true;
        continue;
      }

      options[key] = value;
      index += 1;
    }

    if (subcommand === "detect") {
      return commandDetect(options);
    }
    if (subcommand === "prepare") {
      return commandPrepare(options);
    }

    throw new Error(`unsupported subcommand: ${subcommand}`);
  } finally {
    process.chdir(previousDir);
  }
}

function touchCrateReadme(dir, coreCrateDirName = "supersigil-core") {
  writeAndCommit(
    dir,
    `crates/${coreCrateDirName}/README.md`,
    `# ${coreCrateDirName}\n`,
    "docs: refresh crate readme",
  );
}

function writeVersionOnlyFixtureFiles(dir, version) {
  writeTestFile(
    dir,
    "Cargo.toml",
    formatWorkspaceCargoToml({
      corePackageName: "supersigil-core",
      cliDependencyName: "supersigil-cli",
      coreCrateDirName: "supersigil-core",
      cliCrateDirName: "supersigil-cli",
    }).replaceAll("0.1.0", version),
  );
  writeTestFile(
    dir,
    "crates/supersigil-core/Cargo.toml",
    `[package]
name = "supersigil-core"
version = "${version}"
`,
  );
  writeTestFile(
    dir,
    "crates/supersigil-cli/Cargo.toml",
    `[package]
name = "supersigil"
version = "${version}"
include = ["src/", "skills/", "README.md"]
`,
  );
}

function runDetect(dir, extraArgs = []) {
  return runReleaseTargets(dir, [
    "detect",
    "--base-ref",
    BASE_REF,
    ...extraArgs,
  ]);
}

function runPrepare(dir, { version = RELEASE_VERSION, gitCliffBin } = {}) {
  const args = [
    "prepare",
    "--base-ref",
    BASE_REF,
    "--version",
    version,
  ];
  if (gitCliffBin) {
    args.push("--git-cliff-bin", gitCliffBin);
  }
  return runReleaseTargets(dir, args);
}

describe("release-targets helper", () => {
  it("tracks the aggregate crates target from published crate inputs", () => {
    expectTargetPaths("crates", [
      "Cargo.toml",
      "Cargo.lock",
      "crates/*/Cargo.toml",
      "crates/*/README.md",
      "crates/*/src/**",
      "crates/*/tests/**",
      "crates/*/build.rs",
    ]);
  });

  it("detects published crate source changes for the aggregate crates target", () => {
    const dir = initFixtureRepo();

    writeTestFile(
      dir,
      "crates/supersigil-cli/src/new_module.rs",
      "pub const NEW: u32 = 1;\n",
    );
    gitCommit(dir, "crate source change");

    const output = runDetect(dir);

    expect(output.targets.crates.impacted).toBe(true);
    expect(output.targets.crates.publish).toBe(true);
  });

  it("wires release workflow to the helper and target publish gates", () => {
    const workflow = normalizeNewlines(
      readCheckedInFile("../../.github/workflows/release.yml"),
    );

    expect(workflow).toContain(
      'node scripts/release-targets/index.mjs detect --head-ref "${GITHUB_REF_NAME}" --github-output "${GITHUB_OUTPUT}"',
    );
    expect(workflow).toContain("publish_crates:");
    expect(workflow).toContain("publish_crates: ${{ steps.targets.outputs.publish_crates }}");
    expect(workflow).toContain("needs: [release, target-metadata]");
    expect(workflow).toContain("if: needs.target-metadata.outputs.publish_crates == 'true'");
    expect(workflow).toMatch(
      /publish-homebrew:\n[\s\S]*?needs: \[release, target-metadata\]\n[\s\S]*?if: needs\.target-metadata\.outputs\.publish_crates == 'true'/,
    );
    expect(workflow).toMatch(
      /publish-aur:\n[\s\S]*?needs: \[release, target-metadata\]\n[\s\S]*?if: needs\.target-metadata\.outputs\.publish_crates == 'true'/,
    );
  });

  it("uses release-target prepare output to gate lockfile updates and staging", () => {
    const mise = readCheckedInFile("../../mise.toml");
    const releaseScript = readCheckedInFile("../../scripts/release.mjs");
    const unixWrapper = readCheckedInFile("../../scripts/release.sh");
    const windowsWrapper = readCheckedInFile("../../scripts/release.ps1");

    expect(mise).toContain('run = "bash scripts/release.sh"');
    expect(mise).toContain(
      'run_windows = "powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release.ps1"',
    );
    expect(releaseScript).toContain("commandPrepare(");
    expect(releaseScript).toContain("generate-lockfile");
    expect(releaseScript).toContain("stageIfChanged");
    expect(unixWrapper).toContain("release.mjs");
    expect(unixWrapper).toContain("node");
    expect(windowsWrapper).toContain("release.mjs");
    expect(windowsWrapper).toContain("node");
  });

  it("ignores version-only changes in version files", () => {
    const dir = initFixtureRepo();

    writeVersionOnlyFixtureFiles(dir, "0.1.1");
    gitCommit(dir, "version-only change");

    const output = runDetect(dir);

    expect(output.targets.crates.impacted).toBe(false);
  });

  it("bumps all crate manifests and workspace pins when the crates target is impacted", () => {
    const dir = initFixtureRepo();

    touchCrateReadme(dir);

    const output = runPrepare(dir);

    expect(output.targets.crates.impacted).toBe(true);
    expectRepoFileContains(dir, "Cargo.toml", 'version = "=0.2.0"');
    expectRepoFileContains(
      dir,
      "crates/supersigil-core/Cargo.toml",
      'version = "0.2.0"',
    );
    expectRepoFileContains(
      dir,
      "crates/supersigil-cli/Cargo.toml",
      'version = "0.2.0"',
    );
  });

  it("bumps crate manifests that use CRLF line endings", () => {
    const dir = initFixtureRepo();

    writeTestFile(
      dir,
      "crates/supersigil-cli/Cargo.toml",
      `[package]\r
name = "supersigil"\r
version = "0.1.0"\r
include = ["src/", "skills/", "README.md"]\r
`,
    );
    touchCrateReadme(dir);

    const output = runPrepare(dir);

    expect(output.targets.crates.impacted).toBe(true);
    expect(readRepoFile(dir, "crates/supersigil-cli/Cargo.toml")).toContain(
      'version = "0.2.0"\r\n',
    );
  });

  it("bumps workspace pins for crates that do not use the supersigil name prefix", () => {
    const dir = initFixtureRepo({
      corePackageName: "widget-core",
      cliDependencyName: "widget-cli",
      coreCrateDirName: "widget-core",
      cliCrateDirName: "widget-cli",
      cliPackageName: "widget",
    });

    touchCrateReadme(dir, "widget-core");
    runPrepare(dir);

    expectRepoFileContains(
      dir,
      "Cargo.toml",
      'widget-core = { path = "crates/widget-core", version = "=0.2.0" }',
    );
    expectRepoFileContains(
      dir,
      "Cargo.toml",
      'widget-cli = { path = "crates/widget-cli", version = "=0.2.0" }',
    );
    expectRepoFileContains(
      dir,
      "crates/widget-core/Cargo.toml",
      'version = "0.2.0"',
    );
    expectRepoFileContains(
      dir,
      "crates/widget-cli/Cargo.toml",
      'version = "0.2.0"',
    );
  });

  it("writes GitHub outputs with publish flags", () => {
    const dir = initFixtureRepo();
    const githubOutput = join(dir, "github-output.txt");

    touchCrateReadme(dir);
    runDetect(dir, ["--github-output", githubOutput]);

    const outputs = readRepoFile(dir, "github-output.txt");
    expect(outputs).toContain("impacted_crates=true");
    expect(outputs).toContain("publish_crates=true");
  });
});
