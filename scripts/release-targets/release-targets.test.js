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
import { verifies } from "@supersigil/vitest";
import { commandDetect, commandPrepare } from "./index.mjs";

const BASE_REF = "v0.1.0";
const RELEASE_VERSION = "0.2.0";
const tempDirs = [];

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

function run(dir, program, args) {
  const output = spawnSync(program, args, {
    cwd: dir,
    encoding: "utf8",
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

function buildFixtureRegistry() {
  const config = readCheckedInReleaseTargets();

  return {
    ...config,
    targets: config.targets
      .filter(({ id }) => id === "crates" || id === "intellij" || id === "vscode")
      .map((target) =>
        target.id === "intellij"
          ? {
              ...target,
              enabled: false,
            }
          : target,
      ),
  };
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
    "cliff.editor.toml",
    "[changelog]\nbody = \"\"\n\n[git]\ncommit_parsers = [\n  { message = \"^chore\\\\(release\\\\): prepare\", skip = true },\n  { message = \"^ci\", skip = true }\n]\n",
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
  writeTestFile(
    dir,
    `crates/${cliCrateDirName}/src/commands/explore_assets/explorer.js`,
    "export const fixture = 1;\n",
  );
  writeTestFile(
    dir,
    `crates/${cliCrateDirName}/skills/ss-feature-development/SKILL.md`,
    "# Fixture skill\n",
  );
  writeTestFile(
    dir,
    ".agents/skills/ss-feature-development/SKILL.md",
    "# Upstream fixture skill\n",
  );
  writeTestFile(dir, "package.json", "{\n  \"private\": true\n}\n");
  writeTestFile(dir, "pnpm-workspace.yaml", "packages:\n  - packages/*\n");
  writeTestFile(dir, "pnpm-lock.yaml", "lockfileVersion: '9.0'\n");
  writeTestFile(dir, "packages/preview/package.json", "{\n  \"name\": \"preview\"\n}\n");
  writeTestFile(dir, "packages/preview/esbuild.mjs", "export default {};\n");
  writeTestFile(
    dir,
    "editors/intellij/gradle.properties",
    "pluginVersion = 0.1.0\n",
  );
  writeTestFile(dir, "editors/intellij/CHANGELOG.md", "# Changelog\n");
  writeTestFile(
    dir,
    "editors/vscode/package.json",
    '{\n  "version": "0.1.0"\n}\n',
  );
  writeTestFile(dir, "editors/vscode/CHANGELOG.md", "# Changelog\n");
  writeTestFile(dir, "packages/preview/src/render.ts", "export const render = 1;\n");
  writeTestFile(
    dir,
    "packages/preview/__tests__/render.test.ts",
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

function readCheckedInReleaseTargets() {
  return JSON.parse(readCheckedInFile("../../release-targets.json"));
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

function refreshPackagedSkill(dir) {
  writeAndCommit(
    dir,
    ".agents/skills/ss-feature-development/SKILL.md",
    "# Updated upstream skill\n",
    "docs: refresh packaged skill",
  );
}

function writeVersionOnlyFixtureFiles(dir, version) {
  writeTestFile(
    dir,
    "editors/intellij/gradle.properties",
    `pluginVersion = ${version}\n`,
  );
  writeTestFile(
    dir,
    "editors/vscode/package.json",
    `{\n  "version": "${version}"\n}\n`,
  );
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

describe("release-targets helper", () => {
  it(
    "defines a shared editor changelog config and a VS Code changelog file",
    verifies(
      "release-targets/req#req-2-6",
      "release-targets/req#req-5-1",
      "release-targets/req#req-5-2",
      "release-targets/req#req-5-3",
    ),
    () => {
      const vscode = getCheckedInTarget("vscode");
      const intellij = getCheckedInTarget("intellij");
      const editorCliff = readFileSync(
        new URL("../../cliff.editor.toml", import.meta.url),
        "utf8",
      );

      expect(vscode.changelogFile).toBe("editors/vscode/CHANGELOG.md");
      expect(vscode.cliffConfig).toBe("cliff.editor.toml");
      expect(intellij.cliffConfig).toBe("cliff.editor.toml");
      expect(editorCliff).toContain('{%- if version == "v0.1.0" %}');
      expect(editorCliff).toContain("Initial release.");
      expect(editorCliff).toContain('{ message = "^ci", skip = true }');
    },
  );

  it(
    "tracks the aggregate crates target from published crate inputs",
    verifies(
      "release-targets/req#req-1-3",
      "release-targets/req#req-1-4",
      "release-targets/req#req-1-6",
    ),
    () => {
      expectTargetPaths("crates", [
        "Cargo.toml",
        "Cargo.lock",
        "crates/*/Cargo.toml",
        "crates/*/examples/**",
        "crates/*/src/**",
        "crates/supersigil-cli/src/commands/explore_assets/**",
        "crates/supersigil-cli/skills/**",
        ".agents/skills/ss-feature-development/**",
        "packages/preview/src/**",
        "website/src/components/explore/**",
      ]);
    },
  );

  it(
    "detects published crate example changes for the aggregate crates target",
    verifies("release-targets/req#req-1-4", "release-targets/req#req-2-1"),
    () => {
      const dir = initFixtureRepo();

      writeTestFile(
        dir,
        "crates/supersigil-import/examples/check_dup_crit.rs",
        "fn main() {}\n",
      );
      gitCommit(dir, "crate example change");

      const output = runDetect(dir);

      expect(output.targets.crates.impacted).toBe(true);
      expect(output.targets.crates.publish).toBe(true);
    },
  );

  it(
    "wires release workflow to the helper and target publish gates",
    verifies(
      "release-targets/req#req-4-1",
      "release-targets/req#req-4-2",
      "release-targets/req#req-4-5",
      "release-targets/req#req-4-6",
    ),
    () => {
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
      expect(workflow).toContain("needs: [release, target-metadata, publish-crates]");
      expect(workflow).toContain("needs.publish-crates.result == 'success'");
      expect(workflow).toContain("needs.publish-crates.result == 'skipped'");
    },
  );

  it(
    "uses release-target prepare output to gate lockfile updates and staging",
    verifies(
      "release-targets/req#req-3-2",
      "release-targets/req#req-3-5",
      "release-targets/req#req-3-6",
    ),
    () => {
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
    },
  );

  it(
    "tracks shared pnpm manifests and VS Code shipped assets for JS targets",
    verifies("release-targets/req#req-1-3", "release-targets/req#req-1-4"),
    () => {
      for (const targetId of [
        "intellij",
        "vscode",
        "npm-vitest",
        "npm-eslint-plugin",
      ]) {
        const target = getCheckedInTarget(targetId);
        expect(target.impactPaths).toContain("pnpm-workspace.yaml");
        expect(target.impactPaths).toContain("pnpm-lock.yaml");
      }
      expectTargetPaths("vscode", [
        "editors/vscode/README.md",
        "editors/vscode/LICENSE",
        "packages/preview/src/**",
        "packages/preview/styles/**",
        "packages/preview/scripts/**",
        "packages/preview/esbuild.mjs",
        "packages/preview/package.json",
        "website/src/components/explore/**",
        "website/src/styles/landing-tokens.css",
      ]);
    },
  );

  it(
    "detects impacted shipped-input changes for disabled targets",
    verifies(
      "release-targets/req#req-1-4",
      "release-targets/req#req-2-1",
      "release-targets/req#req-4-3",
    ),
    () => {
      const dir = initFixtureRepo();

      writeAndCommit(dir, "packages/preview/src/render.ts", "export const render = 2;\n", "shipped change");

      const output = runDetect(dir);

      expect(output.targets.intellij.impacted).toBe(true);
      expect(output.targets.intellij.publish).toBe(false);
    },
  );

  it(
    "ignores excluded test-only changes",
    verifies("release-targets/req#req-2-4"),
    () => {
      const dir = initFixtureRepo();

      writeAndCommit(
        dir,
        "packages/preview/__tests__/render.test.ts",
        "export const renderTest = 2;\n",
        "test-only change",
      );

      const output = runDetect(dir);

      expect(output.targets.intellij.impacted).toBe(false);
    },
  );

  it(
    "ignores version-only changes in version files",
    verifies("release-targets/req#req-1-4", "release-targets/req#req-3-6"),
    () => {
      const dir = initFixtureRepo();

      writeVersionOnlyFixtureFiles(dir, "0.1.1");
      gitCommit(dir, "version-only change");

      const output = runDetect(dir);

      expect(output.targets.crates.impacted).toBe(false);
      expect(output.targets.intellij.impacted).toBe(false);
      expect(output.targets.vscode.impacted).toBe(false);
    },
  );

  it(
    "bumps all crate manifests and workspace pins when the crates target is impacted",
    verifies(
      "release-targets/req#req-3-2",
      "release-targets/req#req-3-5",
    ),
    () => {
      const dir = initFixtureRepo();

      refreshPackagedSkill(dir);

      const output = runPrepare(dir);

      expect(output.targets.crates.impacted).toBe(true);
      expectRepoFileContains(
        dir,
        "Cargo.toml",
        'version = "=0.2.0"',
      );
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
      expectRepoFileContains(
        dir,
        "editors/vscode/package.json",
        '"version": "0.1.0"',
      );
    },
  );

  it(
    "bumps crate manifests that use CRLF line endings",
    verifies("release-targets/req#req-3-2", "release-targets/req#req-3-5"),
    () => {
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
      refreshPackagedSkill(dir);

      const output = runPrepare(dir);

      expect(output.targets.crates.impacted).toBe(true);
      expect(readRepoFile(dir, "crates/supersigil-cli/Cargo.toml")).toContain(
        'version = "0.2.0"\r\n',
      );
    },
  );

  it(
    "bumps workspace pins for crates that do not use the supersigil name prefix",
    verifies("release-targets/req#req-3-5"),
    () => {
      const dir = initFixtureRepo({
        corePackageName: "widget-core",
        cliDependencyName: "widget-cli",
        coreCrateDirName: "widget-core",
        cliCrateDirName: "widget-cli",
        cliPackageName: "widget",
      });

      refreshPackagedSkill(dir);
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
    },
  );

  it(
    "prepares only impacted target versions and changelogs",
    verifies(
      "release-targets/req#req-3-2",
      "release-targets/req#req-3-3",
      "release-targets/req#req-5-1",
    ),
    () => {
      const dir = initFixtureRepo();
      const gitCliff = writeStubGitCliff(dir);

      writeAndCommit(
        dir,
        "editors/intellij/src/main/kotlin/org/supersigil/intellij/Fixture.kt",
        "class Fixture\n",
        "feat(intellij): add fixture behavior",
      );

      const output = runPrepare(dir, { gitCliffBin: gitCliff });

      expect(output.targets.intellij.impacted).toBe(true);
      expectRepoFileContains(
        dir,
        "editors/intellij/gradle.properties",
        "pluginVersion = 0.2.0",
      );
      expectRepoFileContains(
        dir,
        "editors/vscode/package.json",
        '"version": "0.1.0"',
      );
      expectRepoFileContains(
        dir,
        "editors/intellij/CHANGELOG.md",
        "Generated by stub git-cliff",
      );
    },
  );

  it(
    "generates the VS Code changelog only when the VS Code target is impacted",
    verifies(
      "release-targets/req#req-3-3",
      "release-targets/req#req-5-1",
      "release-targets/req#req-5-5",
    ),
    () => {
      const dir = initFixtureRepo();
      const gitCliff = writeStubGitCliff(dir);

      writeAndCommit(
        dir,
        "editors/vscode/src/extension.ts",
        "export const vscode = 2;\n",
        "feat: add vscode behavior",
      );

      const intellijChangelogBefore = readRepoFile(dir, "editors/intellij/CHANGELOG.md");

      const output = runPrepare(dir, { gitCliffBin: gitCliff });

      expect(output.targets.vscode.impacted).toBe(true);
      expectRepoFileContains(
        dir,
        "editors/vscode/package.json",
        '"version": "0.2.0"',
      );
      expectRepoFileContains(
        dir,
        "editors/vscode/CHANGELOG.md",
        "Generated by stub git-cliff",
      );
      expect(readRepoFile(dir, "editors/intellij/CHANGELOG.md")).toBe(intellijChangelogBefore);
    },
  );

  it(
    "leaves target files unchanged when the target is not impacted",
    verifies(
      "release-targets/req#req-3-4",
      "release-targets/req#req-5-3",
    ),
    () => {
      const dir = initFixtureRepo();
      const snapshot = snapshotRepoFiles(dir, [
        "editors/intellij/gradle.properties",
        "editors/intellij/CHANGELOG.md",
      ]);

      const output = runPrepare(dir);

      expect(output.targets.intellij.impacted).toBe(false);
      expectRepoFilesMatchSnapshot(dir, snapshot);
    },
  );

  it(
    "does not rewrite target files when git-cliff validation fails",
    verifies("release-targets/req#req-2-5"),
    () => {
      const dir = initFixtureRepo();
      const snapshot = snapshotRepoFiles(dir, [
        "editors/intellij/gradle.properties",
        "editors/intellij/CHANGELOG.md",
      ]);

      writeAndCommit(dir, "packages/preview/src/render.ts", "export const render = 2;\n", "shipped change");

      expect(() =>
        runPrepare(dir, {
          gitCliffBin: "/definitely/missing/git-cliff",
        }),
      ).toThrow(/failed/);
      expectRepoFilesMatchSnapshot(dir, snapshot);
    },
  );

  it(
    "cleans up temporary changelog directories when the renderer omits output",
    verifies("release-targets/req#req-2-5"),
    () => {
      const dir = initFixtureRepo();
      const gitCliff = writeStubGitCliff(dir, { writeOutput: false });
      const beforeTempDirs = listReleaseTargetTempDirs();

      writeAndCommit(dir, "packages/preview/src/render.ts", "export const render = 2;\n", "shipped change");

      expect(() =>
        runPrepare(dir, { gitCliffBin: gitCliff }),
      ).toThrow(/failed to read rendered changelog/);

      const afterTempDirs = listReleaseTargetTempDirs();
      expect([...afterTempDirs].filter((name) => !beforeTempDirs.has(name))).toEqual([]);
    },
  );

  it(
    "writes GitHub outputs with publish flags",
    verifies(
      "release-targets/req#req-2-2",
      "release-targets/req#req-4-1",
    ),
    () => {
      const dir = initFixtureRepo();
      const githubOutput = join(dir, "github-output.txt");

      writeAndCommit(dir, "editors/vscode/src/extension.ts", "export const vscode = 1;\n", "vscode change");
      runDetect(dir, ["--github-output", githubOutput]);

      const outputs = readRepoFile(dir, "github-output.txt");
      expect(outputs).toContain("impacted_vscode=true");
      expect(outputs).toContain("publish_vscode=true");
      expect(outputs).toContain("publish_intellij=false");
    },
  );
});
