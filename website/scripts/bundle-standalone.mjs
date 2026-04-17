#!/usr/bin/env node

import { build } from "esbuild";
import { mkdir } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const websiteDir = dirname(scriptDir);
const outputFile = join(
  websiteDir,
  "..",
  "crates",
  "supersigil-cli",
  "src",
  "commands",
  "explore_assets",
  "explore-standalone.js",
);

await mkdir(dirname(outputFile), { recursive: true });

await build({
  absWorkingDir: websiteDir,
  entryPoints: ["src/components/explore/graph-explorer.js"],
  bundle: true,
  format: "iife",
  globalName: "SupersigilExplorer",
  platform: "browser",
  target: "es2020",
  alias: {
    d3: "./src/components/explore/d3-global.cjs",
  },
  mainFields: ["module", "main"],
  outfile: outputFile,
});
