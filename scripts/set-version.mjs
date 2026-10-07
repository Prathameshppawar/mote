#!/usr/bin/env node
// Sets Mote's version in every manifest and lockfile:
//
//   node scripts/set-version.mjs 1.2.3
//
// Updates Cargo.toml ([workspace.package]), Cargo.lock (workspace crates),
// tauri.conf.json, package.json and package-lock.json. The CHANGELOG entry and
// docs/releases/v<version>.md are written by hand; see docs/development/releasing.md.

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const WORKSPACE_CRATES = ["mote-core", "mote-providers", "mote-storage", "mote-platform", "mote-desktop"];

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error("usage: node scripts/set-version.mjs <X.Y.Z>");
  process.exit(1);
}

function edit(path, transform) {
  const file = join(root, path);
  const before = readFileSync(file, "utf8");
  const after = transform(before);
  if (after === before) {
    console.log(`  ${path} (unchanged)`);
    return;
  }
  writeFileSync(file, after);
  console.log(`  ${path}`);
}

const editJson = (path, apply) =>
  edit(path, (text) => {
    const json = JSON.parse(text);
    apply(json);
    return `${JSON.stringify(json, null, 2)}\n`;
  });

console.log(`Setting version ${version}:`);

edit("Cargo.toml", (text) =>
  text.replace(/(^\[workspace\.package\][\s\S]*?^version\s*=\s*")[^"]+(")/m, `$1${version}$2`),
);

edit("Cargo.lock", (text) =>
  text
    .split("[[package]]")
    .map((block) => {
      const name = block.match(/^name = "([^"]+)"/m)?.[1];
      return WORKSPACE_CRATES.includes(name) ? block.replace(/^version = "[^"]+"/m, `version = "${version}"`) : block;
    })
    .join("[[package]]"),
);

// Hand-formatted: replace the top-level field in place rather than re-serialize.
edit("apps/desktop/src-tauri/tauri.conf.json", (text) => text.replace(/^(  "version":\s*")[^"]+(")/m, `$1${version}$2`));
// npm's own files round-trip through JSON.stringify unchanged.
editJson("apps/desktop/package.json", (json) => {
  json.version = version;
});
editJson("apps/desktop/package-lock.json", (json) => {
  json.version = version;
  if (json.packages?.[""]) json.packages[""].version = version;
});

console.log("\nNext: add a CHANGELOG entry and docs/releases/v" + version + ".md, then run scripts/check-version.mjs.");
