// Shim: map d3 imports to the global d3 object for the standalone IIFE bundle.
// In standalone HTML, d3 is loaded via CDN as globalThis.d3.
// Used via esbuild --alias:d3=./src/components/explore/d3-global.js
// CJS module.exports ensures esbuild's __toESM copies all properties dynamically.
module.exports = globalThis.d3;
