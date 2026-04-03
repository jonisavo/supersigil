import * as esbuild from "esbuild";
import { copyFileSync, mkdirSync, existsSync } from "fs";
import { join, dirname } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));

const production = process.argv.includes("--production");
const watch = process.argv.includes("--watch");

// ---------------------------------------------------------------------------
// Copy preview kit assets into media/
// ---------------------------------------------------------------------------

const previewPkg = join(__dirname, "..", "..", "packages", "preview");
const previewDist = join(previewPkg, "dist");
const mediaDir = join(__dirname, "media");

// Build the preview kit if dist/ doesn't exist (clean checkout / CI).
if (!existsSync(join(previewDist, "render.js"))) {
  const { execSync } = await import("child_process");
  console.log("Building @supersigil/preview...");
  execSync("pnpm run build", { cwd: previewPkg, stdio: "inherit" });
}

function copyPreviewAssets() {
  mkdirSync(mediaDir, { recursive: true });
  const assets = ["supersigil-preview.css", "supersigil-preview.js"];
  for (const asset of assets) {
    const src = join(previewDist, asset);
    if (existsSync(src)) {
      copyFileSync(src, join(mediaDir, asset));
    }
  }
}

copyPreviewAssets();

// ---------------------------------------------------------------------------
// esbuild config
// ---------------------------------------------------------------------------

/** @type {esbuild.BuildOptions} */
const buildOptions = {
  entryPoints: ["src/extension.ts"],
  bundle: true,
  format: "cjs",
  platform: "node",
  target: "node18",
  outfile: "dist/extension.js",
  external: ["vscode"],
  alias: {
    "@supersigil/preview": join(previewDist, "render.js"),
  },
  minify: production,
  sourcemap: !production,
};

if (watch) {
  const ctx = await esbuild.context(buildOptions);
  await ctx.watch();
  console.log("Watching for changes...");
} else {
  await esbuild.build(buildOptions);
}
