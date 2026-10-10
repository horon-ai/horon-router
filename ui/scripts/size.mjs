// Reports the dashboard bundle sizes after `vite build` and `scripts/panel.mjs`.
// It copies dist/index.html to dist/management.html and prints the gzip sizes of the
// JavaScript, CSS, HTML and the single-file panel. The gzip ceilings this script used to
// enforce (157,300 B JS / 7,700 B CSS) were lifted 2026-10-11 by owner request: the build
// no longer fails when the bundle grows, it only prints what was measured.
import { readdirSync, readFileSync, copyFileSync } from "node:fs";
import { gzipSync } from "node:zlib";

copyFileSync("dist/index.html", "dist/management.html");
const html = readFileSync("dist/index.html", "utf8");
const assets = readdirSync("dist/assets");
const gz = (text) => gzipSync(text).length;
const inline = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)].map((m) => m[1]).join("\n");
const size = (ext) =>
  assets.filter((f) => f.endsWith(ext)).reduce((n, f) => n + gz(readFileSync(`dist/assets/${f}`)), 0);
const total = { js: size(".js") + (inline ? gz(inline) : 0), css: size(".css") };

for (const kind of ["js", "css"]) {
  console.log(`${kind.toUpperCase().padEnd(3)} ${String(total[kind]).padStart(6)} B gzip`);
}
console.log(`HTML ${gz(html)} B gzip (icon sprite and theme script)`);
const panel = readFileSync("dist-panel/management.html");
console.log(`Panel ${gz(panel)} B gzip, ${panel.length} B raw (dist-panel/management.html, font inlined)`);
