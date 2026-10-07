#!/usr/bin/env node
// Verifies that every place declaring Mote's version agrees.
//
//   node scripts/check-version.mjs               manifests and lockfiles agree
//   node scripts/check-version.mjs --tag v1.2.3  ...and match the tag, with a
//                                                CHANGELOG entry and release notes
//
// Use scripts/set-version.mjs to change the version everywhere at once.

import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (path) => readFileSync(join(root, path), "utf8");
const readJson = (path) => JSON.parse(read(path));

const WORKSPACE_CRATES = ["mote-core", "mote-providers", "mote-storage", "mote-platform", "mote-desktop"];

function workspaceVersion() {
  const section = read("Cargo.toml").match(/^\[workspace\.package\]([\s\S]*?)(?=^\[|(?![\s\S]))/m);
  const version = section?.[1].match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!version) throw new Error("Cargo.toml: [workspace.package] version not found");
  return version;
}

function lockedCrateVersions() {
  const versions = {};
  for (const block of read("Cargo.lock").split("[[package]]")) {
    const name = block.match(/^name = "([^"]+)"/m)?.[1];
    if (WORKSPACE_CRATES.includes(name)) versions[name] = block.match(/^version = "([^"]+)"/m)?.[1];
  }
  return versions;
}

function parseArgs(argv) {
  const args = { tag: null };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--tag") args.tag = argv[++i] ?? "";
    else throw new Error(`unknown argument: ${argv[i]}`);
  }
  return args;
}

const args = parseArgs(process.argv.slice(2));
const expected = workspaceVersion();
const problems = [];
const check = (label, actual) => {
  if (actual !== expected) problems.push(`${label} is ${JSON.stringify(actual)}, expected "${expected}"`);
};

if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(expected)) {
  problems.push(`Cargo.toml version "${expected}" is not semantic (X.Y.Z)`);
}

const locked = lockedCrateVersions();
for (const name of WORKSPACE_CRATES) check(`Cargo.lock ${name}`, locked[name]);

check("apps/desktop/src-tauri/tauri.conf.json version", readJson("apps/desktop/src-tauri/tauri.conf.json").version);
check("apps/desktop/package.json version", readJson("apps/desktop/package.json").version);
const lock = readJson("apps/desktop/package-lock.json");
check("apps/desktop/package-lock.json version", lock.version);
check('apps/desktop/package-lock.json packages[""].version', lock.packages?.[""]?.version);

if (args.tag !== null) {
  if (args.tag !== `v${expected}`) problems.push(`tag "${args.tag}" does not match version v${expected}`);
  const heading = new RegExp(`^## \\[${expected.replace(/\./g, "\\.")}\\]`, "m");
  if (!existsSync(join(root, "CHANGELOG.md"))) problems.push("CHANGELOG.md is missing");
  else if (!heading.test(read("CHANGELOG.md"))) problems.push(`CHANGELOG.md has no "## [${expected}]" entry`);
  const notes = `docs/releases/v${expected}.md`;
  if (!existsSync(join(root, notes)) || read(notes).trim() === "") problems.push(`${notes} is missing or empty`);
}

if (problems.length > 0) {
  for (const problem of problems) console.error(`✗ ${problem}`);
  console.error("\nRun `node scripts/set-version.mjs <version>` to bring every manifest in line.");
  process.exit(1);
}
console.log(`✓ version ${expected} is consistent${args.tag ? ` and matches ${args.tag}` : ""}`);
