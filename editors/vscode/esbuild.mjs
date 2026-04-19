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
// Copy webview assets into dist/webview/
// ---------------------------------------------------------------------------

const webviewDir = join(__dirname, "dist", "webview");
const websiteSrc = join(__dirname, "..", "..", "website", "src");

function copyWebviewAssets() {
  mkdirSync(webviewDir, { recursive: true });

  /** @type {Array<[string, string]>} source path -> dest filename */
  const copies = [
    [join(websiteSrc, "styles", "landing-tokens.css"), "landing-tokens.css"],
    [
      join(websiteSrc, "components", "explore", "styles.css"),
      "explorer-styles.css",
    ],
    [join(previewDist, "supersigil-preview.css"), "supersigil-preview.css"],
    [join(previewDist, "render-iife.js"), "render-iife.js"],
    [join(previewDist, "supersigil-preview.js"), "supersigil-preview.js"],
    [
      join(mediaDir, "vscode-theme-adapter.css"),
      "vscode-theme-adapter.css",
    ],
  ];

  for (const [src, destName] of copies) {
    if (!existsSync(src)) {
      throw new Error(`Webview asset not found: ${src}`);
    }
    copyFileSync(src, join(webviewDir, destName));
  }
}

copyWebviewAssets();

// ---------------------------------------------------------------------------
// esbuild config — main extension
// ---------------------------------------------------------------------------

/** @type {esbuild.BuildOptions} */
const extensionOptions = {
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

// ---------------------------------------------------------------------------
// esbuild config — webview: graph explorer
// ---------------------------------------------------------------------------

/** @type {esbuild.BuildOptions} */
const explorerOptions = {
  entryPoints: [
    join(websiteSrc, "components", "explore", "explorer-entry.js"),
  ],
  bundle: true,
  format: "iife",
  globalName: "SupersigilExplorer",
  platform: "browser",
  target: "es2020",
  outfile: join(webviewDir, "explorer.js"),
  mainFields: ["module", "main"],
  minify: production,
  sourcemap: !production,
};

// ---------------------------------------------------------------------------
// esbuild config — webview: bootstrap
// ---------------------------------------------------------------------------

/** @type {esbuild.BuildOptions} */
const bootstrapOptions = {
  entryPoints: ["src/explorerBootstrap.ts"],
  bundle: true,
  format: "iife",
  platform: "browser",
  target: "es2020",
  outfile: join(webviewDir, "bootstrap.js"),
  external: ["vscode"],
  minify: production,
  sourcemap: !production,
};

// ---------------------------------------------------------------------------
// Build
// ---------------------------------------------------------------------------

if (watch) {
  const [extCtx, explorerCtx, bootstrapCtx] = await Promise.all([
    esbuild.context(extensionOptions),
    esbuild.context(explorerOptions),
    esbuild.context(bootstrapOptions),
  ]);
  await Promise.all([extCtx.watch(), explorerCtx.watch(), bootstrapCtx.watch()]);
  console.log("Watching for changes...");
} else {
  await Promise.all([
    esbuild.build(extensionOptions),
    esbuild.build(explorerOptions),
    esbuild.build(bootstrapOptions),
  ]);
}
