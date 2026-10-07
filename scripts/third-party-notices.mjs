#!/usr/bin/env node
// Regenerates the dependency tables in THIRD_PARTY_NOTICES.md:
//
//   node scripts/third-party-notices.mjs
//
// Rust: crates linked into the desktop app for the shipped targets (normal
// dependencies only, so build tools and test-only crates are excluded).
// npm: production packages bundled into the interface.

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const NOTICES = join(root, "THIRD_PARTY_NOTICES.md");
const TARGETS = ["aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-pc-windows-msvc"];
const BEGIN = "<!-- BEGIN GENERATED: scripts/third-party-notices.mjs -->";
const END = "<!-- END GENERATED -->";

function rustCrates() {
  const args = ["metadata", "--format-version", "1", "--locked", ...TARGETS.flatMap((t) => ["--filter-platform", t])];
  const meta = JSON.parse(execFileSync("cargo", args, { cwd: root, maxBuffer: 512 * 1024 * 1024 }));
  const packages = new Map(meta.packages.map((p) => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const app = meta.packages.find((p) => p.name === "mote-desktop");
  const workspace = new Set(meta.workspace_members);
  const seen = new Set();
  const queue = [app.id];
  while (queue.length > 0) {
    const id = queue.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const dep of nodes.get(id)?.deps ?? []) {
      if (dep.dep_kinds.some((k) => k.kind === null)) queue.push(dep.pkg);
    }
  }
  return [...seen]
    .filter((id) => !workspace.has(id))
    .map((id) => packages.get(id))
    .map((p) => ({ name: p.name, version: p.version, license: p.license ?? (p.license_file ? "see crate" : "unknown") }))
    .sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));
}

function npmPackages() {
  const cwd = join(root, "apps/desktop");
  const tree = JSON.parse(execFileSync("npm", ["ls", "--omit=dev", "--all", "--json"], { cwd, maxBuffer: 64 * 1024 * 1024 }));
  const found = new Map();
  const walk = (deps = {}) => {
    for (const [name, info] of Object.entries(deps)) {
      const key = `${name}@${info.version}`;
      if (found.has(key)) continue;
      let license = "unknown";
      try {
        const manifest = JSON.parse(readFileSync(join(cwd, "node_modules", name, "package.json"), "utf8"));
        license = typeof manifest.license === "string" ? manifest.license : (manifest.license?.type ?? license);
      } catch {
        // Not installed at the top level (nested duplicate); keep "unknown".
      }
      found.set(key, { name, version: info.version, license });
      walk(info.dependencies);
    }
  };
  walk(tree.dependencies);
  return [...found.values()].sort((a, b) => a.name.localeCompare(b.name));
}

function table(rows, heading) {
  const counts = new Map();
  for (const r of rows) counts.set(r.license, (counts.get(r.license) ?? 0) + 1);
  const summary = [...counts.entries()]
    .sort((a, b) => b[1] - a[1])
    .map(([license, n]) => `${license} (${n})`)
    .join(", ");
  return [
    `| ${heading} | Version | License |`,
    "|---|---|---|",
    ...rows.map((r) => `| ${r.name} | ${r.version} | ${r.license} |`),
    "",
    `Licenses: ${summary}.`,
  ].join("\n");
}

const crates = rustCrates();
const npm = npmPackages();
const generated = [
  BEGIN,
  "",
  `### Rust crates (${crates.length})`,
  "",
  table(crates, "Crate"),
  "",
  `### npm packages (${npm.length})`,
  "",
  table(npm, "Package"),
  "",
  END,
].join("\n");

const current = readFileSync(NOTICES, "utf8");
const start = current.indexOf(BEGIN);
const end = current.indexOf(END);
if (start === -1 || end === -1) {
  console.error(`THIRD_PARTY_NOTICES.md must contain the markers:\n${BEGIN}\n${END}`);
  process.exit(1);
}
writeFileSync(NOTICES, current.slice(0, start) + generated + current.slice(end + END.length));
console.log(`✓ ${crates.length} crates and ${npm.length} npm packages listed in THIRD_PARTY_NOTICES.md`);
