#!/usr/bin/env node
/**
 * Builds the graph explorer as an IIFE bundle for embedding in editors.
 *
 * Output: website/dist/explorer-iife/explorer.js
 *
 * Usage:
 *   node build-explorer-iife.mjs            # production (minified, no sourcemap)
 *   node build-explorer-iife.mjs --dev      # development (unminified, sourcemap)
 */

import { build } from "esbuild";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const production = !process.argv.includes("--dev");

/** @type {import("esbuild").BuildOptions} */
const explorerOptions = {
  entryPoints: [
    join(__dirname, "src", "components", "explore", "explorer-entry.js"),
  ],
  bundle: true,
  format: "iife",
  globalName: "SupersigilExplorer",
  platform: "browser",
  target: "es2020",
  outfile: join(__dirname, "dist", "explorer-iife", "explorer.js"),
  mainFields: ["module", "main"],
  minify: production,
  sourcemap: !production,
};

await build(explorerOptions);
console.log(
  `Built explorer IIFE → dist/explorer-iife/explorer.js (${production ? "production" : "development"})`,
);
