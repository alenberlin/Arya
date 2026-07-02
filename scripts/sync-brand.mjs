// Keeps brand.json (the single source of truth for product name, bundle
// identifier, and URL scheme) in sync with the places Tauri needs literal
// values: tauri.conf.json and index.html's <title>.
//
// Usage: node scripts/sync-brand.mjs [--check]
//   --check  exit 1 if anything is out of sync instead of writing.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const checkOnly = process.argv.includes("--check");

const brand = JSON.parse(readFileSync(join(root, "brand.json"), "utf8"));
const confPath = join(root, "src-tauri", "tauri.conf.json");
const conf = JSON.parse(readFileSync(confPath, "utf8"));

let dirty = false;
const drift = [];

function expect(actual, wanted, label, apply) {
  if (actual !== wanted) {
    drift.push(`${label}: ${JSON.stringify(actual)} != ${JSON.stringify(wanted)}`);
    dirty = true;
    apply();
  }
}

expect(conf.productName, brand.name, "tauri.conf.json productName", () => {
  conf.productName = brand.name;
});
expect(conf.identifier, brand.identifier, "tauri.conf.json identifier", () => {
  conf.identifier = brand.identifier;
});
expect(conf.app?.windows?.[0]?.title, brand.name, "tauri.conf.json window title", () => {
  conf.app.windows[0].title = brand.name;
});

const htmlPath = join(root, "index.html");
const html = readFileSync(htmlPath, "utf8");
const wantedTitle = `<title>${brand.name}</title>`;
const fixedHtml = html.replace(/<title>[^<]*<\/title>/, wantedTitle);
if (!html.includes(wantedTitle)) {
  drift.push("index.html <title>");
  dirty = true;
}

if (!dirty) {
  console.log("brand: in sync");
  process.exit(0);
}
if (checkOnly) {
  console.error(`brand: OUT OF SYNC\n  ${drift.join("\n  ")}`);
  process.exit(1);
}
writeFileSync(confPath, `${JSON.stringify(conf, null, 2)}\n`);
writeFileSync(htmlPath, fixedHtml);
console.log(`brand: synced\n  ${drift.join("\n  ")}`);
