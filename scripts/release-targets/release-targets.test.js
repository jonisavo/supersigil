import {
  chmodSync,
  mkdtempSync,
  mkdirSync,
  readdirSync,
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

function listReleaseTargetTempDirs() {
  return new Set(
    readdirSync(tmpdir(), { withFileTypes: true })
      .filter((entry) => entry.isDirectory() && entry.name.startsWith("release-targets-"))
      .map((entry) => entry.name),
  );
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

function snapshotRepoFiles(dir, paths) {
  return Object.fromEntries(paths.map((path) => [path, readRepoFile(dir, path)]));
}

function expectRepoFilesMatchSnapshot(dir, snapshot) {
  for (const [path, contents] of Object.entries(snapshot)) {
    expect(readRepoFile(dir, path)).toBe(contents);
  }
}

function readCheckedInReleaseTargets() {
  return JSON.parse(readCheckedInFile("../../release-targets.json"));
}

/**
 * Synthetic secondary targets used only by fixture repos in this test file.
 *
 * They are not read from the checked-in `release-targets.json` (which now
 * defines only the `crates` target) because the behaviors they exercise —
 * disabled-but-impacted detection, exclusion patterns, selective changelog
 * generation, and git-cliff failure handling — are generic to the helper and
 * do not depend on any specific production target existing.
 */
const SYNTHETIC_ALPHA_TARGET = {
  id: "alpha",
  enabled: false,
  versionFile: "fixtures/alpha/version.properties",
  versionKind: "gradle-property",
  versionKey: "pluginVersion",
  changelogFile: "fixtures/alpha/CHANGELOG.md",
  cliffConfig: "cliff.fixture.toml",
  impactPaths: ["fixtures/alpha/src/**", "fixtures/shared/**"],
  excludePaths: [
    "fixtures/shared/**/__tests__/**",
    "fixtures/shared/**/*.test.ts",
  ],
};

const SYNTHETIC_BETA_TARGET = {
  id: "beta",
  enabled: true,
  versionFile: "fixtures/beta/package.json",
  versionKind: "package-json",
  versionKey: "version",
  changelogFile: "fixtures/beta/CHANGELOG.md",
  cliffConfig: "cliff.fixture.toml",
  impactPaths: ["fixtures/beta/src/**"],
  excludePaths: [],
};

function buildFixtureRegistry() {
  const config = readCheckedInReleaseTargets();
  return {
    targets: [
      ...config.targets.filter(({ id }) => id === "crates"),
      SYNTHETIC_ALPHA_TARGET,
      SYNTHETIC_BETA_TARGET,
    ],
  };
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
  writeTestFile(dir, "cliff.fixture.toml", "[changelog]\nbody = \"\"\n");
  writeTestFile(dir, "fixtures/alpha/version.properties", "pluginVersion = 0.1.0\n");
  writeTestFile(dir, "fixtures/alpha/CHANGELOG.md", "# Changelog\n");
  writeTestFile(dir, "fixtures/beta/package.json", '{\n  "version": "0.1.0"\n}\n');
  writeTestFile(dir, "fixtures/beta/CHANGELOG.md", "# Changelog\n");
  writeTestFile(dir, "fixtures/shared/src/render.ts", "export const render = 1;\n");
  writeTestFile(
    dir,
    "fixtures/shared/__tests__/render.test.ts",
    "export const renderTest = 1;\n",
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

function writeStubGitCliff(dir, { writeOutput = true } = {}) {
  const path = join(dir, "git-cliff-stub.mjs");
  const writeOutputSource = writeOutput
    ? `writeFileSync(
  output,
  "# Changelog\\n\\n## [" +
    tag.replace(/^v/, "") +
    "]\\n\\n### Features\\n\\n- Generated by stub git-cliff\\n",
);
`
    : "";
  writeFileSync(
    path,
    `#!/usr/bin/env node
import { writeFileSync } from "node:fs";

const args = process.argv.slice(2);

if (args[0] === "--version") {
  console.log("git-cliff-stub 0.0.0");
  process.exit(0);
}

let output = "";
let tag = "";

for (let index = 0; index < args.length; index += 1) {
  if (args[index] === "--output") {
    output = args[index + 1] ?? "";
    index += 1;
    continue;
  }

  if (args[index] === "--tag") {
    tag = args[index + 1] ?? "";
    index += 1;
  }
}

${writeOutputSource}
`,
    "utf8",
  );
  chmodSync(path, 0o755);
  return path;
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

  it("detects impacted shipped-input changes for disabled targets", () => {
    const dir = initFixtureRepo();

    writeAndCommit(
      dir,
      "fixtures/shared/src/render.ts",
      "export const render = 2;\n",
      "shipped change",
    );

    const output = runDetect(dir);

    expect(output.targets.alpha.impacted).toBe(true);
    expect(output.targets.alpha.publish).toBe(false);
  });

  it("ignores excluded test-only changes", () => {
    const dir = initFixtureRepo();

    writeAndCommit(
      dir,
      "fixtures/shared/__tests__/render.test.ts",
      "export const renderTest = 2;\n",
      "test-only change",
    );

    const output = runDetect(dir);

    expect(output.targets.alpha.impacted).toBe(false);
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

  it("prepares only impacted target versions and changelogs", () => {
    const dir = initFixtureRepo();
    const gitCliff = writeStubGitCliff(dir);

    writeAndCommit(
      dir,
      "fixtures/alpha/src/Fixture.kt",
      "class Fixture\n",
      "feat(alpha): add fixture behavior",
    );

    const output = runPrepare(dir, { gitCliffBin: gitCliff });

    expect(output.targets.alpha.impacted).toBe(true);
    expectRepoFileContains(
      dir,
      "fixtures/alpha/version.properties",
      "pluginVersion = 0.2.0",
    );
    expectRepoFileContains(
      dir,
      "fixtures/beta/package.json",
      '"version": "0.1.0"',
    );
    expectRepoFileContains(
      dir,
      "fixtures/alpha/CHANGELOG.md",
      "Generated by stub git-cliff",
    );
  });

  it("generates a target changelog only when that target is impacted", () => {
    const dir = initFixtureRepo();
    const gitCliff = writeStubGitCliff(dir);

    writeAndCommit(
      dir,
      "fixtures/beta/src/extension.ts",
      "export const beta = 2;\n",
      "feat: add beta behavior",
    );

    const alphaChangelogBefore = readRepoFile(dir, "fixtures/alpha/CHANGELOG.md");

    const output = runPrepare(dir, { gitCliffBin: gitCliff });

    expect(output.targets.beta.impacted).toBe(true);
    expectRepoFileContains(
      dir,
      "fixtures/beta/package.json",
      '"version": "0.2.0"',
    );
    expectRepoFileContains(
      dir,
      "fixtures/beta/CHANGELOG.md",
      "Generated by stub git-cliff",
    );
    expect(readRepoFile(dir, "fixtures/alpha/CHANGELOG.md")).toBe(alphaChangelogBefore);
  });

  it("leaves target files unchanged when the target is not impacted", () => {
    const dir = initFixtureRepo();
    const snapshot = snapshotRepoFiles(dir, [
      "fixtures/alpha/version.properties",
      "fixtures/alpha/CHANGELOG.md",
    ]);

    const output = runPrepare(dir);

    expect(output.targets.alpha.impacted).toBe(false);
    expectRepoFilesMatchSnapshot(dir, snapshot);
  });

  it("does not rewrite target files when git-cliff validation fails", () => {
    const dir = initFixtureRepo();
    const snapshot = snapshotRepoFiles(dir, [
      "fixtures/alpha/version.properties",
      "fixtures/alpha/CHANGELOG.md",
    ]);

    writeAndCommit(
      dir,
      "fixtures/shared/src/render.ts",
      "export const render = 2;\n",
      "shipped change",
    );

    expect(() =>
      runPrepare(dir, {
        gitCliffBin: "/definitely/missing/git-cliff",
      }),
    ).toThrow(/failed/);
    expectRepoFilesMatchSnapshot(dir, snapshot);
  });

  it("cleans up temporary changelog directories when the renderer omits output", () => {
    const dir = initFixtureRepo();
    const gitCliff = writeStubGitCliff(dir, { writeOutput: false });
    const beforeTempDirs = listReleaseTargetTempDirs();

    writeAndCommit(
      dir,
      "fixtures/shared/src/render.ts",
      "export const render = 2;\n",
      "shipped change",
    );

    expect(() =>
      runPrepare(dir, { gitCliffBin: gitCliff }),
    ).toThrow(/failed to read rendered changelog/);

    const afterTempDirs = listReleaseTargetTempDirs();
    expect([...afterTempDirs].filter((name) => !beforeTempDirs.has(name))).toEqual([]);
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
