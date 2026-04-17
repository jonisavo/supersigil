#!/usr/bin/env node

import { spawn } from "node:child_process";
import { createWriteStream } from "node:fs";
import { finished } from "node:stream/promises";
import { copyFile, mkdir, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { buildCommandInvocation } from "../../scripts/command-invocation.mjs";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const websiteDir = dirname(scriptDir);
const repoRoot = dirname(websiteDir);
const previewDir = join(repoRoot, "packages", "preview");
const publicExploreDir = join(websiteDir, "public", "explore");

const PREVIEW_ASSETS = [
  "supersigil-preview.css",
  "render-iife.js",
  "supersigil-preview.js",
];

function waitForExit(child, { allowFailure = false } = {}) {
  return new Promise((resolve, reject) => {
    child.on("error", reject);
    child.on("close", (status) => {
      if (!allowFailure && status !== 0) {
        process.exit(status ?? 1);
      }

      resolve({ status });
    });
  });
}

async function run(commandName, args, cwd) {
  const invocation = buildCommandInvocation(commandName, args);
  const child = spawn(invocation.command, invocation.args, {
    cwd,
    env: process.env,
    stdio: ["ignore", "inherit", "inherit"],
  });

  await waitForExit(child);
}

async function runToFile(
  commandName,
  args,
  cwd,
  outputPath,
  { allowFailure = false } = {},
) {
  const invocation = buildCommandInvocation(commandName, args);
  const child = spawn(invocation.command, invocation.args, {
    cwd,
    env: process.env,
    stdio: ["ignore", "pipe", "inherit"],
  });
  const output = createWriteStream(outputPath);

  child.stdout.pipe(output);

  const [result] = await Promise.all([
    waitForExit(child, { allowFailure }),
    finished(output),
  ]);

  return result;
}

await mkdir(publicExploreDir, { recursive: true });

await run("pnpm", ["run", "build"], previewDir);

for (const assetName of PREVIEW_ASSETS) {
  await copyFile(
    join(previewDir, "dist", assetName),
    join(publicExploreDir, assetName),
  );
}

const graphDataPath = join(publicExploreDir, "graph.json");
const renderDataPath = join(publicExploreDir, "render-data.json");

const [, exportResult] = await Promise.all([
  runToFile(
    "supersigil",
    ["graph", "--format", "json"],
    websiteDir,
    graphDataPath,
  ),
  runToFile(
    "supersigil",
    ["export", "--format", "json"],
    websiteDir,
    renderDataPath,
    { allowFailure: true },
  ),
]);

if (exportResult.status === 0) {
  // Keep the generated JSON artifact from the successful export.
} else {
  await writeFile(renderDataPath, "[]\n");
  console.warn("warning: `supersigil export --format json` failed during website prebuild");
}
