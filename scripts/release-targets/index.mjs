import {
  appendFileSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { buildCommandInvocation } from "../command-invocation.mjs";

/**
 * Repo-local helper for selective release target detection and preparation.
 *
 * The script reads `release-targets.json`, classifies which targets changed in
 * a release range, and optionally rewrites version files plus target-specific
 * changelogs for impacted targets.
 */

/**
 * @typedef {{
 *   id: string,
 *   enabled: boolean,
 *   versionFile?: string,
 *   versionKind?: string,
 *   versionKey?: string,
 *   changelogFile?: string,
 *   cliffConfig?: string,
 *   impactPaths: string[],
 *   excludePaths: string[],
 * }} ReleaseTarget
 */

/**
 * @typedef {{
 *   impacted: boolean,
 *   publish: boolean,
 *   enabled: boolean,
 *   changedFiles: string[],
 * }} DetectionResult
 */

/**
 * @typedef {{
 *   positional: string[],
 *   options: Record<string, string | boolean>,
 * }} ParsedArgs
 */

/**
 * Prints a terminal-facing error and exits the helper immediately.
 *
 * @param {string} message
 * @returns {never}
 */
class ReleaseTargetsError extends Error {
  /**
   * @param {string} message
   */
  constructor(message) {
    super(message);
    this.name = "ReleaseTargetsError";
  }
}

function fail(message) {
  throw new ReleaseTargetsError(message);
}

/**
 * Reads a UTF-8 file or exits with a file-specific error.
 *
 * @param {string} path
 * @param {string} label
 * @returns {string}
 */
function readUtf8File(path, label) {
  try {
    return readFileSync(path, "utf8");
  } catch (error) {
    fail(`failed to read ${label}: ${error.message}`);
  }
}

function replaceCargoPackageVersion(contents, version) {
  const lines = contents.split("\n");
  let section = "";
  let replaced = false;

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const sectionMatch = line.match(/^\s*\[([^\]]+)\]\s*$/);
    if (sectionMatch) {
      section = sectionMatch[1];
      continue;
    }

    if (replaced || section !== "package") {
      continue;
    }

    const versionMatch = line.match(
      /^(\s*version\s*=\s*")([^"]+)("[^\r\n]*)(\r?)$/,
    );
    if (!versionMatch) {
      continue;
    }

    lines[index] =
      `${versionMatch[1]}${version}${versionMatch[3]}${versionMatch[4]}`;
    replaced = true;
  }

  return replaced ? lines.join("\n") : null;
}

function replaceCargoWorkspacePins(contents, version) {
  const lines = contents.split("\n");
  let section = "";
  let replaced = false;

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const sectionMatch = line.match(/^\s*\[([^\]]+)\]\s*$/);
    if (sectionMatch) {
      section = sectionMatch[1];
      continue;
    }

    if (section !== "workspace.dependencies") {
      continue;
    }

    if (!line.includes('path = "crates/')) {
      continue;
    }

    const nextLine = line.replace(
      /version\s*=\s*"=[^"]+"/,
      `version = "=${version}"`,
    );
    if (nextLine !== line) {
      lines[index] = nextLine;
      replaced = true;
    }
  }

  return replaced ? lines.join("\n") : null;
}

function isCargoWorkspaceManifestPath(path) {
  return /^crates\/[^/]+\/Cargo\.toml$/.test(path);
}

function listCargoWorkspaceManifestPaths(target) {
  const manifestPaths = new Set([target.versionFile]);
  for (const path of gitTrimmedLines(["ls-files", "--", ":(glob)crates/*/Cargo.toml"])) {
    manifestPaths.add(path);
  }
  return [...manifestPaths];
}

const VERSION_STRATEGIES = {
  "package-json": {
    requiresVersionKey: true,
    supportsPath(target, path = target.versionFile) {
      return path === target.versionFile;
    },
    /**
     * @param {ReleaseTarget} target
     * @param {string} contents
     * @returns {string | null}
     */
    normalize(target, contents, path = target.versionFile) {
      if (path !== target.versionFile) {
        return null;
      }
      try {
        const parsed = JSON.parse(contents);
        if (!(target.versionKey in parsed)) {
          return null;
        }
        parsed[target.versionKey] = "__RELEASE_TARGET_VERSION__";
        return JSON.stringify(parsed);
      } catch {
        return null;
      }
    },
    /**
     * @param {ReleaseTarget} target
     * @param {string} contents
     * @param {string} version
     * @returns {string}
     */
    update(target, contents, version, path = target.versionFile) {
      if (path !== target.versionFile) {
        fail(`target "${target.id}" cannot update unsupported path ${path}`);
      }
      let parsed;
      try {
        parsed = JSON.parse(contents);
      } catch (error) {
        fail(`failed to parse ${target.versionFile}: ${error.message}`);
      }

      parsed[target.versionKey] = version;
      return `${JSON.stringify(parsed, null, 2)}\n`;
    },
  },
  "gradle-property": {
    requiresVersionKey: true,
    supportsPath(target, path = target.versionFile) {
      return path === target.versionFile;
    },
    /**
     * @param {ReleaseTarget} target
     * @param {string} contents
     * @returns {string | null}
     */
    normalize(target, contents, path = target.versionFile) {
      if (path !== target.versionFile) {
        return null;
      }
      const linePattern = new RegExp(`^${target.versionKey}\\s*=.*$`, "m");
      if (!linePattern.test(contents)) {
        return null;
      }

      return contents.replace(
        linePattern,
        `${target.versionKey} = __RELEASE_TARGET_VERSION__`,
      );
    },
    /**
     * @param {ReleaseTarget} target
     * @param {string} contents
     * @param {string} version
     * @returns {string}
     */
    update(target, contents, version, path = target.versionFile) {
      if (path !== target.versionFile) {
        fail(`target "${target.id}" cannot update unsupported path ${path}`);
      }
      const linePattern = new RegExp(`^${target.versionKey}\\s*=.*$`, "m");
      if (!linePattern.test(contents)) {
        fail(
          `target "${target.id}" could not find ${target.versionKey} in ${target.versionFile}`,
        );
      }

      return contents.replace(linePattern, `${target.versionKey} = ${version}`);
    },
  },
  "cargo-workspace": {
    requiresVersionKey: false,
    supportsPath(target, path = target.versionFile) {
      return path === target.versionFile || isCargoWorkspaceManifestPath(path);
    },
    normalize(target, contents, path = target.versionFile) {
      if (path === target.versionFile) {
        return replaceCargoWorkspacePins(
          contents,
          "__RELEASE_TARGET_VERSION__",
        );
      }
      if (isCargoWorkspaceManifestPath(path)) {
        return replaceCargoPackageVersion(contents, "__RELEASE_TARGET_VERSION__");
      }
      return null;
    },
    update(target, contents, version, path = target.versionFile) {
      if (path === target.versionFile) {
        const updated = replaceCargoWorkspacePins(contents, version);
        if (updated === null) {
          fail(
            `target "${target.id}" could not find workspace crate pins in ${target.versionFile}`,
          );
        }
        return updated;
      }
      if (isCargoWorkspaceManifestPath(path)) {
        const updated = replaceCargoPackageVersion(contents, version);
        if (updated === null) {
          fail(`target "${target.id}" could not find package version in ${path}`);
        }
        return updated;
      }
      fail(`target "${target.id}" cannot update unsupported path ${path}`);
    },
  },
};

/**
 * Parses the helper's minimal flag syntax.
 *
 * Boolean flags are represented by `true`, while named options consume the
 * next argv entry as their value.
 *
 * @param {string[]} argv
 * @returns {ParsedArgs}
 */
function parseArgs(argv) {
  const positional = [];
  const options = {};

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (!arg.startsWith("--")) {
      positional.push(arg);
      continue;
    }

    if (arg === "--json") {
      options.json = true;
      continue;
    }

    const key = arg.slice(2);
    const value = argv[i + 1];
    if (value === undefined || value.startsWith("--")) {
      fail(`missing value for --${key}`);
    }
    options[key] = value;
    i += 1;
  }

  return { positional, options };
}

function spawnCommand(command, args, options) {
  const invocation = buildCommandInvocation(command, args);
  return spawnSync(invocation.command, invocation.args, options);
}

/**
 * Runs a git command in the current repository and returns stdout.
 *
 * @param {string[]} args
 * @param {{ allowEmpty?: boolean }} [options]
 * @returns {string}
 */
function git(args, { allowEmpty = false } = {}) {
  const result = spawnSync("git", args, {
    cwd: process.cwd(),
    encoding: "utf8",
  });

  if (result.status !== 0) {
    fail(
      [
        `git ${args.join(" ")} failed`,
        result.error?.message ?? "",
        (result.stderr ?? "").trim(),
        (result.stdout ?? "").trim(),
      ]
        .filter(Boolean)
        .join("\n"),
    );
  }

  if (!allowEmpty && result.stdout.trim() === "") {
    return "";
  }

  return result.stdout;
}

/**
 * Returns non-empty trimmed stdout lines from a git command.
 *
 * @param {string[]} args
 * @returns {string[]}
 */
function gitTrimmedLines(args) {
  return git(args, { allowEmpty: true })
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
}

/**
 * Resolves the base ref used for release-range comparisons.
 *
 * If the caller does not provide a base ref, the helper picks the latest
 * reachable `v*` tag before `headRef`. When `headRef` itself is tagged, the
 * previous reachable tag becomes the base so a tag does not diff against
 * itself.
 *
 * @param {string | undefined} explicitBaseRef
 * @param {string} headRef
 * @returns {string | null}
 */
function resolveBaseRef(explicitBaseRef, headRef) {
  if (explicitBaseRef) {
    return explicitBaseRef;
  }

  const tags = gitTrimmedLines([
    "tag",
    "--merged",
    headRef,
    "--list",
    "v[0-9]*",
    "--sort=-v:refname",
  ]);

  if (tags.length === 0) {
    return null;
  }

  const headTags = new Set(
    gitTrimmedLines([
      "tag",
      "--points-at",
      headRef,
      "--list",
      "v[0-9]*",
      "--sort=-v:refname",
    ]),
  );

  if (tags[0] === headRef || headTags.has(tags[0])) {
    return tags[1] ?? null;
  }

  return tags[0];
}

/**
 * Validates and returns the version-editing strategy for a target.
 *
 * @param {ReleaseTarget} target
 * @returns {{
 *   normalize: (target: ReleaseTarget, contents: string, path?: string) => string | null,
 *   update: (target: ReleaseTarget, contents: string, version: string, path?: string) => string,
 * }}
 */
function getVersionStrategy(target) {
  if (typeof target.versionFile !== "string" || target.versionFile === "") {
    fail(`target "${target.id}" is missing "versionFile"`);
  }
  if (typeof target.versionKind !== "string" || target.versionKind === "") {
    fail(`target "${target.id}" is missing "versionKind"`);
  }
  const strategy = VERSION_STRATEGIES[target.versionKind];
  if (!strategy) {
    fail(`unsupported versionKind for target "${target.id}": ${target.versionKind}`);
  }
  if (
    strategy.requiresVersionKey &&
    (typeof target.versionKey !== "string" || target.versionKey === "")
  ) {
    fail(`target "${target.id}" is missing "versionKey"`);
  }

  return strategy;
}

/**
 * Loads and validates the checked-in release target registry.
 *
 * @param {string} registryPath
 * @returns {ReleaseTarget[]}
 */
function loadRegistry(registryPath) {
  let parsed;
  try {
    parsed = JSON.parse(
      readUtf8File(registryPath, `release target registry ${registryPath}`),
    );
  } catch (error) {
    fail(`failed to parse ${registryPath}: ${error.message}`);
  }

  if (!parsed || !Array.isArray(parsed.targets)) {
    fail(`${registryPath} must contain a top-level "targets" array`);
  }

  return parsed.targets.map((target, index) => {
    if (!target || typeof target !== "object") {
      fail(`target at index ${index} must be an object`);
    }
    if (typeof target.id !== "string" || target.id === "") {
      fail(`target at index ${index} is missing a non-empty "id"`);
    }
    if (!Array.isArray(target.impactPaths) || target.impactPaths.length === 0) {
      fail(`target "${target.id}" must define a non-empty "impactPaths" array`);
    }
    if (
      target.excludePaths !== undefined &&
      !Array.isArray(target.excludePaths)
    ) {
      fail(`target "${target.id}" has a non-array "excludePaths" value`);
    }
    getVersionStrategy(target);

    /** @type {ReleaseTarget} */
    return {
      ...target,
      enabled: target.enabled !== false,
      excludePaths: target.excludePaths ?? [],
    };
  });
}

/**
 * Converts a target ID into a GitHub Actions output-safe suffix.
 *
 * @param {string} targetId
 * @returns {string}
 */
function normalizeOutputKey(targetId) {
  return targetId.replace(/[^A-Za-z0-9]+/g, "_");
}

/**
 * Reads a file from a specific git ref.
 *
 * Missing files return `null` so detection can treat them as non-comparable.
 *
 * @param {string} ref
 * @param {string} path
 * @returns {string | null}
 */
function gitFileAtRef(ref, path) {
  const result = spawnSync("git", ["show", `${ref}:${path}`], {
    cwd: process.cwd(),
    encoding: "utf8",
  });

  if (result.status !== 0) {
    return null;
  }

  return result.stdout;
}

function normalizeVersionedPathContents(target, path, contents) {
  return getVersionStrategy(target).normalize(target, contents, path);
}

/**
 * Returns true when a changed version file differs only by its configured
 * version field across the release range.
 *
 * @param {{
 *   baseRef: string | null,
 *   headRef: string,
 *   path: string,
 *   target: ReleaseTarget,
 *   strategy: {
 *     supportsPath: (target: ReleaseTarget, path?: string) => boolean,
 *     normalize: (target: ReleaseTarget, contents: string, path?: string) => string | null,
 *   },
 * }} args
 * @returns {boolean}
 */
function isVersionOnlyFileChange({ baseRef, headRef, path, target, strategy }) {
  if (baseRef === null) {
    return false;
  }
  if (!strategy.supportsPath(target, path)) {
    return false;
  }

  const baseContents = gitFileAtRef(baseRef, path);
  const headContents = gitFileAtRef(headRef, path);
  if (baseContents === null || headContents === null) {
    return false;
  }

  const normalizedBase = normalizeVersionedPathContents(
    target,
    path,
    baseContents,
  );
  const normalizedHead = normalizeVersionedPathContents(
    target,
    path,
    headContents,
  );

  return normalizedBase !== null && normalizedBase === normalizedHead;
}

/**
 * Filters generated changelog outputs out of impact classification.
 *
 * @param {{ path: string, target: ReleaseTarget }} args
 * @returns {boolean}
 */
function isIgnoredOutputFileChange({ path, target }) {
  return typeof target.changelogFile === "string" && path === target.changelogFile;
}

/**
 * Detects whether one target is impacted in the chosen release range.
 *
 * @param {{
 *   baseRef: string | null,
 *   headRef: string,
 *   target: ReleaseTarget,
 * }} args
 * @returns {DetectionResult}
 */
function detectTarget({ baseRef, headRef, target }) {
  const strategy = getVersionStrategy(target);
  const args =
    baseRef === null
      ? ["ls-files", "--"]
      : ["diff", "--name-only", `${baseRef}..${headRef}`, "--"];

  for (const pattern of target.impactPaths) {
    args.push(`:(glob)${pattern}`);
  }
  for (const pattern of target.excludePaths) {
    args.push(`:(exclude,glob)${pattern}`);
  }

  const output = git(args, { allowEmpty: true });
  const changedFiles = output
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  const impactfulFiles = changedFiles.filter(
    (path) =>
      !isIgnoredOutputFileChange({ path, target }) &&
      !isVersionOnlyFileChange({ baseRef, headRef, path, target, strategy }),
  );

  return {
    impacted: impactfulFiles.length > 0,
    publish: impactfulFiles.length > 0 && target.enabled,
    enabled: target.enabled,
    changedFiles: impactfulFiles,
  };
}

/**
 * Rewrites the target's version file to the repository release version.
 *
 * @param {ReleaseTarget} target
 * @param {string} version
 * @param {string} [path]
 * @returns {string}
 */
function buildUpdatedVersionContents(target, version, path = target.versionFile) {
  const original = readUtf8File(
    path,
    `target version file ${path}`,
  );
  return getVersionStrategy(target).update(target, original, version, path);
}

/**
 * Renders a target-local changelog with git-cliff and returns its contents.
 *
 * @param {{
 *   target: ReleaseTarget,
  *   version: string,
  *   gitCliffBin: string,
 * }} args
 * @returns {string | null}
 */
function renderTargetChangelog({ target, version, gitCliffBin }) {
  if (typeof target.changelogFile !== "string" || target.changelogFile === "") {
    return null;
  }
  if (typeof target.cliffConfig !== "string" || target.cliffConfig === "") {
    fail(`target "${target.id}" is missing "cliffConfig"`);
  }

  const tempDir = mkdtempSync(join(tmpdir(), "release-targets-"));
  const tempOutput = join(tempDir, `${target.id}.md`);

  const args = [
    "--config",
    target.cliffConfig,
    "--tag",
    `v${version}`,
    "--output",
    tempOutput,
  ];

  for (const pattern of target.impactPaths) {
    args.push("--include-path", pattern);
  }
  for (const pattern of target.excludePaths) {
    args.push("--exclude-path", pattern);
  }

  try {
    const result = spawnCommand(gitCliffBin, args, {
      cwd: process.cwd(),
      encoding: "utf8",
    });

    if (result.status !== 0) {
      fail(
        [
          `${gitCliffBin} ${args.join(" ")} failed`,
          result.error?.message ?? "",
          (result.stderr ?? "").trim(),
          (result.stdout ?? "").trim(),
        ]
          .filter(Boolean)
          .join("\n"),
      );
    }

    return readUtf8File(
      tempOutput,
      `rendered changelog ${tempOutput}`,
    );
  } finally {
    rmSync(tempDir, { recursive: true, force: true });
  }
}

/**
 * Verifies that the configured git-cliff executable is callable before any
 * target files are rewritten.
 *
 * @param {string} gitCliffBin
 * @returns {void}
 */
function validateGitCliffExecutable(gitCliffBin) {
  const result = spawnCommand(gitCliffBin, ["--version"], {
    cwd: process.cwd(),
    encoding: "utf8",
  });

  if (result.status !== 0) {
    fail(
      [
        `${gitCliffBin} --version failed`,
        result.error?.message ?? "",
        (result.stderr ?? "").trim(),
        (result.stdout ?? "").trim(),
      ]
        .filter(Boolean)
        .join("\n"),
    );
  }
}

/**
 * Builds all file contents needed for one impacted target before any writes
 * are applied to the working tree.
 *
 * @param {{
 *   target: ReleaseTarget,
 *   version: string,
 *   gitCliffBin: string,
 * }} args
 * @returns {{
 *   target: ReleaseTarget,
 *   versionWrites: Array<{ path: string, contents: string }>,
 *   changelogContents: string | null,
 * }}
 */
function planTargetWrites({ target, version, gitCliffBin }) {
  const versionWrites =
    target.versionKind === "cargo-workspace"
      ? listCargoWorkspaceManifestPaths(target).map((path) => ({
          path,
          contents: buildUpdatedVersionContents(target, version, path),
        }))
      : [
          {
            path: target.versionFile,
            contents: buildUpdatedVersionContents(target, version),
          },
        ];

  return {
    target,
    versionWrites,
    changelogContents: renderTargetChangelog({ target, version, gitCliffBin }),
  };
}

/**
 * Applies a precomputed target write plan to the working tree.
 *
 * @param {{
 *   target: ReleaseTarget,
 *   versionWrites: Array<{ path: string, contents: string }>,
 *   changelogContents: string | null,
 * }} plan
 * @returns {void}
 */
function applyTargetWrites(plan) {
  for (const versionWrite of plan.versionWrites) {
    writeFileSync(versionWrite.path, versionWrite.contents, "utf8");
  }
  if (
    typeof plan.target.changelogFile === "string" &&
    plan.changelogContents !== null
  ) {
    writeFileSync(plan.target.changelogFile, plan.changelogContents, "utf8");
  }
}

/**
 * Writes per-target impact and publish booleans to a GitHub Actions output
 * file.
 *
 * @param {{
 *   targets: Record<string, DetectionResult>,
 * }} results
 * @param {string} outputPath
 * @returns {void}
 */
function writeGithubOutputs(results, outputPath) {
  for (const [targetId, result] of Object.entries(results.targets)) {
    const key = normalizeOutputKey(targetId);
    appendFileSync(
      outputPath,
      `impacted_${key}=${result.impacted}\npublish_${key}=${result.publish}\n`,
      "utf8",
    );
  }
}

/**
 * Executes the `detect` subcommand.
 *
 * @param {Record<string, string | boolean>} options
 * @returns {void}
 */
export function commandDetect(options) {
  const headRef = options["head-ref"] ?? "HEAD";
  const baseRef = resolveBaseRef(options["base-ref"], headRef);
  const registry = loadRegistry(options.registry ?? "release-targets.json");

  const targets = Object.fromEntries(
    registry.map((target) => [
      target.id,
      detectTarget({ baseRef, headRef, target }),
    ]),
  );

  const result = { baseRef, headRef, targets };

  if (options["github-output"]) {
    writeGithubOutputs(result, options["github-output"]);
  }

  return result;
}

/**
 * Executes the `prepare` subcommand.
 *
 * Preparation reuses the same impact classification as `detect`, validates the
 * changelog renderer up front, and rewrites only impacted targets.
 *
 * @param {Record<string, string | boolean>} options
 * @returns {void}
 */
export function commandPrepare(options) {
  const version = options.version;
  if (!version) {
    fail("prepare requires --version");
  }

  const headRef = options["head-ref"] ?? "HEAD";
  const baseRef = resolveBaseRef(options["base-ref"], headRef);
  const registry = loadRegistry(options.registry ?? "release-targets.json");
  const gitCliffBin = options["git-cliff-bin"] ?? "git-cliff";
  const detectedTargets = registry.map((target) => ({
    target,
    detection: detectTarget({ baseRef, headRef, target }),
  }));
  const impactedTargets = detectedTargets.filter(
    ({ detection }) => detection.impacted,
  );

  if (
    impactedTargets.some(
      ({ target }) =>
        typeof target.changelogFile === "string" && target.changelogFile !== "",
    )
  ) {
    validateGitCliffExecutable(gitCliffBin);
  }

  const plannedWrites = impactedTargets.map(({ target }) =>
    planTargetWrites({ target, version, gitCliffBin }),
  );

  for (const plan of plannedWrites) {
    applyTargetWrites(plan);
  }

  const targets = Object.fromEntries(
    detectedTargets.map(({ target, detection }) => [target.id, detection]),
  );

  const result = { baseRef, headRef, version, targets };

  if (options["github-output"]) {
    writeGithubOutputs(result, options["github-output"]);
  }

  return result;
}

/**
 * Dispatches the helper subcommand from `process.argv`.
 *
 * @returns {void}
 */
function main() {
  const { positional, options } = parseArgs(process.argv.slice(2));
  const subcommand = positional[0];

  if (subcommand === "detect") {
    const result = commandDetect(options);
    if (options.json || !options["github-output"]) {
      process.stdout.write(`${JSON.stringify(result)}\n`);
    }
    return;
  }
  if (subcommand === "prepare") {
    const result = commandPrepare(options);
    if (options.json || !options["github-output"]) {
      process.stdout.write(`${JSON.stringify(result)}\n`);
    }
    return;
  }

  fail(`unsupported subcommand: ${subcommand ?? "<none>"}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (error) {
    if (error instanceof ReleaseTargetsError) {
      console.error(`error: ${error.message}`);
      process.exit(1);
    }

    throw error;
  }
}
