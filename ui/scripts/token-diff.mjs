// Compare the design tokens the pen.dev canvas declares (GetVariables()) against the
// values ui/src/style.css actually defines, per theme. The canvas is the source of truth
// for DESIGN.md, so a divergence here is a token drift.
//
// Usage:
//   node scripts/token-diff.mjs <canvas-variables.json>
//
// The JSON is the output of the pencil MCP call:
//   pencil_execute({ filePath: "<...>/horon-router.pen", input: "Print(JSON.stringify(GetVariables()))" })
// captured verbatim. Exit code is 0 when every canvas token matches, 1 otherwise.
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const cssPath = resolve(here, "../src/style.css");
const canvasPath = process.argv[2];
if (!canvasPath) {
  console.error("usage: node scripts/token-diff.mjs <canvas-variables.json>");
  process.exit(2);
}

const canvas = JSON.parse(readFileSync(canvasPath, "utf8"));
const css = readFileSync(cssPath, "utf8");

// --- parse style.css into the :root and [data-theme="dark"] declaration blocks ---
function block(selector) {
  const start = css.indexOf(selector);
  if (start < 0) throw new Error(`selector not found in style.css: ${selector}`);
  const open = css.indexOf("{", start);
  const close = css.indexOf("}", open);
  return css.slice(open, close);
}
const rootBlock = block(":root {");
const darkBlock = block('[data-theme="dark"]');
// Radius and font tokens live in the Tailwind `@theme inline` block, not :root.
const themeBlock = css.includes("@theme inline") ? block("@theme inline") : "{";
function declarations(blk) {
  const out = new Map();
  for (const m of blk.matchAll(/--([a-z0-9-]+)\s*:\s*([^;]+);/gi)) out.set(m[1], m[2].trim());
  return out;
}
const light = new Map([...declarations(themeBlock), ...declarations(rootBlock)]);
const dark = declarations(darkBlock);

// Resolve a CSS custom property in a theme, following var() chains. A token defined only
// in :root (like `--body: var(--background)`) still resolves per theme through its alias.
function resolveCss(name, theme) {
  const seen = new Set();
  let current = name;
  for (;;) {
    if (seen.has(current)) return null;
    seen.add(current);
    const raw =
      (theme === "dark" ? dark.get(current) : undefined) ?? light.get(current);
    if (raw === undefined) return null;
    const alias = raw.match(/^var\(--([a-z0-9-]+)\)$/i);
    if (!alias) return normalize(raw);
    current = alias[1];
  }
}

// --- resolve the canvas variable set ---
function canvasValue(name, theme, depth = 0) {
  const entry = canvas.variables[name];
  if (!entry || depth > 10) return null;
  const value = entry.value;
  if (typeof value === "string") {
    const alias = value.match(/^\$([a-z0-9-]+)$/i);
    if (alias) return canvasValue(alias[1], theme, depth + 1);
    return normalize(value);
  }
  if (Array.isArray(value)) {
    const pick = value.find((v) => v.theme?.mode === theme) ?? value[0];
    return pick ? normalize(pick.value) : null;
  }
  return normalize(String(value));
}

// Compare colours and dimensions on one footing: lowercase, no spaces, 6-digit hex
// expanded to a comparable form, and a bare number treated as px.
function normalize(v) {
  let s = String(v).trim().toLowerCase().replace(/\s+/g, "");
  const hex = s.match(/^#([0-9a-f]{3,8})$/);
  if (hex) {
    let h = hex[1];
    if (h.length === 3) h = h.split("").map((c) => c + c).join("");
    if (h.length === 6) h += "ff"; // treat opaque 6-digit as full alpha
    return `#${h}`;
  }
  if (/^-?\d+(\.\d+)?$/.test(s)) return `${s}px`;
  return s;
}

// --- diff ---
const mismatches = [];
const names = Object.keys(canvas.variables);
for (const name of names) {
  const entry = canvas.variables[name];
  if (entry.type === "string") continue; // font families live in @font-face, checked by render
  for (const theme of ["light", "dark"]) {
    const want = canvasValue(name, theme);
    const got = resolveCss(name, theme);
    if (want !== got) mismatches.push(`${theme} --${name}: css=${got ?? "(missing)"} canvas=${want}`);
  }
}

console.log(`PEN_VARS ${names.length}`);
console.log(`MISMATCHES ${mismatches.length}`);
for (const line of mismatches) console.log(`  ${line}`);
process.exit(mismatches.length === 0 ? 0 : 1);
