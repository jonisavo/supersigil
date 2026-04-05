import * as esbuild from "esbuild";
import { copyFileSync, mkdirSync } from "node:fs";

mkdirSync("dist", { recursive: true });

// Bundle render.ts as an ES module
await esbuild.build({
  entryPoints: ["src/render.ts"],
  bundle: true,
  format: "esm",
  platform: "neutral",
  target: "es2022",
  outfile: "dist/render.js",
  sourcemap: true,
});

// Build IIFE bundle for environments that can't use ES modules (IntelliJ JCEF).
// Exposes renderComponentTree and filterNovelEdges on window.__supersigilRender.
await esbuild.build({
  entryPoints: ["src/render-iife-entry.ts"],
  bundle: true,
  format: "iife",
  globalName: "__supersigilRenderModule",
  platform: "browser",
  target: "es2020",
  outfile: "dist/render-iife.js",
  footer: {
    js: "window.__supersigilRender = __supersigilRenderModule;",
  },
});

// Copy CSS and client-side JS to dist
copyFileSync("styles/supersigil-preview.css", "dist/supersigil-preview.css");
copyFileSync("scripts/supersigil-preview.js", "dist/supersigil-preview.js");
