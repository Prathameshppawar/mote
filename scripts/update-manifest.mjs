#!/usr/bin/env node
// Writes latest.json, the manifest Mote's updater reads, from a directory of
// downloaded release assets (installers plus their .sig files):
//
//   node scripts/update-manifest.mjs --dir assets --tag v1.2.3 --repo owner/name > assets/latest.json
//
// Each platform appears twice: `{os}-{arch}-{bundle}` (preferred by the
// updater) and `{os}-{arch}` (fallback).

import { readFileSync, existsSync } from "node:fs";
import { join } from "node:path";

function parseArgs(argv) {
  const args = {};
  for (let i = 0; i < argv.length; i += 2) {
    const key = argv[i]?.replace(/^--/, "");
    if (!key || argv[i + 1] === undefined) throw new Error(`bad arguments near ${argv[i]}`);
    args[key] = argv[i + 1];
  }
  for (const required of ["dir", "tag", "repo"]) {
    if (!args[required]) throw new Error(`--${required} is required`);
  }
  return args;
}

const { dir, tag, repo } = parseArgs(process.argv.slice(2));
const version = tag.replace(/^v/, "");

const targets = [
  { keys: ["darwin-aarch64-app", "darwin-aarch64"], asset: `Mote_${version}_aarch64.app.tar.gz` },
  { keys: ["darwin-x86_64-app", "darwin-x86_64"], asset: `Mote_${version}_x64.app.tar.gz` },
  { keys: ["windows-x86_64-nsis", "windows-x86_64"], asset: `Mote_${version}_x64-setup.exe` },
  { keys: ["windows-x86_64-msi"], asset: `Mote_${version}_x64_en-US.msi` },
];

const platforms = {};
const missing = [];
for (const { keys, asset } of targets) {
  const signature = join(dir, `${asset}.sig`);
  if (!existsSync(join(dir, asset)) || !existsSync(signature)) {
    missing.push(asset);
    continue;
  }
  const entry = {
    signature: readFileSync(signature, "utf8").trim(),
    url: `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(asset)}`,
  };
  for (const key of keys) platforms[key] = entry;
}
if (missing.length > 0) {
  console.error(`Missing installers or signatures: ${missing.join(", ")}`);
  process.exit(1);
}

const manifest = {
  version,
  notes: `Release notes: https://github.com/${repo}/releases/tag/${tag}`,
  pub_date: new Date().toISOString(),
  platforms,
};
process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`);
