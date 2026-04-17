#!/usr/bin/env node

import { spawn } from "node:child_process";
import { basename, dirname, join } from "node:path";
import { cp, mkdir, rm } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { buildCommandInvocation } from "./command-invocation.mjs";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = dirname(scriptDir);
const command = process.argv[2];

const EXPLORE_ASSET_DESTINATION = join(
  repoRoot,
  "crates",
  "supersigil-cli",
  "src",
  "commands",
  "explore_assets",
);

const SKILLS_DESTINATION = join(
  repoRoot,
  "crates",
  "supersigil-cli",
  "skills",
);

const EXPLORE_ASSET_FILES = [
  "website/src/styles/landing-tokens.css",
  "website/src/components/explore/styles.css",
  "packages/preview/styles/supersigil-preview.css",
  "packages/preview/scripts/supersigil-preview.js",
  "packages/preview/dist/render-iife.js",
];

const SKILL_DIRECTORIES = [
  ".agents/skills/ss-ci-review",
  ".agents/skills/ss-feature-development",
  ".agents/skills/ss-feature-specification",
  ".agents/skills/ss-refactoring",
  ".agents/skills/ss-retroactive-specification",
  ".agents/skills/ss-spec-driven-development",
];

function run(commandName, args, cwd = repoRoot) {
  const invocation = buildCommandInvocation(commandName, args);

  return new Promise((resolve, reject) => {
    const child = spawn(invocation.command, invocation.args, {
      cwd,
      stdio: "inherit",
      env: process.env,
    });

    child.on("error", reject);
    child.on("close", (status) => {
      if (status !== 0) {
        process.exit(status ?? 1);
      }

      resolve();
    });
  });
}

async function copyFilesToDirectory(paths, destinationDirectory) {
  await mkdir(destinationDirectory, { recursive: true });

  for (const relativePath of paths) {
    const sourcePath = join(repoRoot, relativePath);
    const destinationPath = join(
      destinationDirectory,
      basename(relativePath),
    );
    await cp(sourcePath, destinationPath);
  }
}

async function bundleExploreAssets() {
  await mkdir(EXPLORE_ASSET_DESTINATION, { recursive: true });
  await Promise.all([
    run("pnpm", ["--filter", "@supersigil/preview", "run", "build"]),
    run("pnpm", ["--filter", "supersigil-website", "run", "bundle:standalone"]),
  ]);
  await copyFilesToDirectory(EXPLORE_ASSET_FILES, EXPLORE_ASSET_DESTINATION);
}

async function bundleCliAssets() {
  await bundleExploreAssets();
  await rm(SKILLS_DESTINATION, { recursive: true, force: true });
  await mkdir(SKILLS_DESTINATION, { recursive: true });

  for (const relativePath of SKILL_DIRECTORIES) {
    const sourcePath = join(repoRoot, relativePath);
    const destinationPath = join(SKILLS_DESTINATION, basename(relativePath));
    await cp(sourcePath, destinationPath, { recursive: true });
  }
}

switch (command) {
  case "explore-assets":
    await bundleExploreAssets();
    break;
  case "cli-assets":
    await bundleCliAssets();
    break;
  default:
    console.error(
      "usage: node scripts/bundle-cli-assets.mjs <explore-assets|cli-assets>",
    );
    process.exit(1);
}
