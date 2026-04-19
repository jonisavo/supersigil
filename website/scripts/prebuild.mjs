#!/usr/bin/env node

import { spawn } from "node:child_process";
import { createWriteStream } from "node:fs";
import { finished } from "node:stream/promises";
import {
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { buildCommandInvocation } from "../../scripts/command-invocation.mjs";
import {
  buildStaticExplorerAssets,
  buildStaticExplorerDocumentFileName,
} from "../src/components/explore/static-explorer-assets.js";

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

const tempDir = await mkdtemp(join(tmpdir(), "supersigil-website-prebuild-"));

try {
  const graphDataPath = join(tempDir, "graph.json");
  const renderDataPath = join(tempDir, "render-data.json");

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

  const graphData = JSON.parse(await readFile(graphDataPath, "utf8"));
  const renderData =
    exportResult.status === 0
      ? JSON.parse(await readFile(renderDataPath, "utf8"))
      : [];

  if (exportResult.status !== 0) {
    console.warn(
      "warning: `supersigil export --format json` failed during website prebuild",
    );
  }

  const { snapshot, documents } = buildStaticExplorerAssets(
    graphData,
    renderData,
  );
  const snapshotPath = join(publicExploreDir, "snapshot.json");
  const documentsDir = join(publicExploreDir, "documents");

  await rm(documentsDir, { recursive: true, force: true });
  await mkdir(documentsDir, { recursive: true });
  await writeFile(snapshotPath, `${JSON.stringify(snapshot, null, 2)}\n`);

  for (const [documentId, document] of Object.entries(documents)) {
    await writeFile(
      join(documentsDir, buildStaticExplorerDocumentFileName(documentId)),
      `${JSON.stringify(document, null, 2)}\n`,
    );
  }
} finally {
  await rm(tempDir, { recursive: true, force: true });
}
