# Releasing

Releases are cut from `main` by pushing a version tag. Everything after the tag (building, uploading, checksums, publishing) is automated by `.github/workflows/release.yml`.

## Versioning

Mote follows [Semantic Versioning](https://semver.org). The single source of truth is `[workspace.package] version` in the root `Cargo.toml`. These must agree, and CI checks that they do:

- `Cargo.toml` (workspace; every crate inherits it) and the workspace entries in `Cargo.lock`
- `apps/desktop/src-tauri/tauri.conf.json`
- `apps/desktop/package.json` and `package-lock.json`

`node scripts/set-version.mjs X.Y.Z` updates all of them; `node scripts/check-version.mjs` verifies them.

## Checklist

1. **Prepare on a branch.**
   ```sh
   git switch -c release/vX.Y.Z
   node scripts/set-version.mjs X.Y.Z
   ```
2. **Changelog.** In `CHANGELOG.md`, move the `[Unreleased]` entries under `## [X.Y.Z] - YYYY-MM-DD` (sections: Added, Changed, Fixed, Security), and update the comparison links at the bottom.
3. **Release notes.** Write `docs/releases/vX.Y.Z.md`. It becomes the GitHub Release body. Start with a `# Mote vX.Y.Z` title, use the sections Highlights, Features, Privacy, Supported Platforms, Known Limitations and Installation, and end with a `## Checksums` heading (the workflow appends the checksum list under it).
4. **Third-party notices.** Regenerate the dependency tables if dependencies changed:
   ```sh
   node scripts/third-party-notices.mjs
   ```
5. **Check.**
   ```sh
   node scripts/check-version.mjs --tag vX.Y.Z
   cargo test --workspace && (cd apps/desktop && npm test)
   ```
   Run the [manual verification](testing.md#manual-verification) checklist on macOS and Windows with a locally built installer.
6. **Merge** the pull request once CI is green.
7. **Tag** the merge commit on `main` and push the tag:
   ```sh
   git switch main && git pull
   git tag -a vX.Y.Z -m "Mote X.Y.Z"
   git push origin vX.Y.Z
   ```

## What the release workflow does

| Job | |
|---|---|
| Verify | `check-version.mjs --tag`: versions match the tag, CHANGELOG has the entry, release notes exist |
| Draft | Creates a **draft** GitHub Release titled "Mote vX.Y.Z" with the release notes |
| Build | Reuses `build.yml`: Apple Silicon and Intel macOS on macOS runners, Windows x64 on a Windows runner. `tauri-action` builds and uploads each installer to the draft |
| Checksums and publish | Downloads every asset, writes `SHA256SUMS.txt`, uploads it, appends the checksums to the notes, and publishes the release as latest |

Assets of a release:

| File | |
|---|---|
| `Mote_X.Y.Z_aarch64.dmg`, `Mote_X.Y.Z_x64.dmg` | macOS disk images |
| `Mote_X.Y.Z_aarch64.app.tar.gz`, `Mote_X.Y.Z_x64.app.tar.gz` | macOS app bundles |
| `Mote_X.Y.Z_x64-setup.exe` | Windows NSIS installer (per user, no administrator rights) |
| `Mote_X.Y.Z_x64_en-US.msi` | Windows MSI (per machine, needs administrator rights) |
| `*.sig`, `latest.json` | update signatures and the manifest the in-app updater reads |
| `SHA256SUMS.txt` | SHA-256 of every file above |

If a build job fails for a transient reason (a network error, a runner problem), use **Re-run failed jobs**. A re-run uses the tagged commit, so for a code or workflow fix, commit the fix and move the tag:

```sh
git tag -d vX.Y.Z && git push origin :refs/tags/vX.Y.Z
git tag -a vX.Y.Z -m "Mote X.Y.Z" && git push origin vX.Y.Z
```

Either way the existing draft is reused and assets with the same names are replaced. Nothing is published until every build has succeeded.

After publishing, download one installer per platform, check it against `SHA256SUMS.txt`, and run a quick smoke test.

## Code signing and update keys

Two keys sign every release. Both private keys live only in the repository's secrets and in an offline backup; never commit them.

| Secret | Purpose |
|---|---|
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD` | base64 `.p12` of the macOS signing identity, and its password |
| `APPLE_SIGNING_IDENTITY` | the identity's name, `Mote Code Signing` |
| `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | the updater key that signs update bundles; the app only installs updates that verify against the public key in `tauri.conf.json` (`plugins.updater.pubkey`) |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | optional notarization, once a Developer ID certificate replaces the self-signed one |

**macOS signing.** Mote is signed with a self-signed "Mote Code Signing" certificate, so its identity, and with it the Accessibility permission, stays the same across updates ([ADR 0008](../decisions/0008-updates-and-signing.md)). Its public half is committed at `apps/desktop/src-tauri/signing/mote-code-signing.crt`, and `build.yml` marks it as trusted on the runner before signing, because `codesign` only uses trusted identities. Changing the certificate changes the app's identity: every user would have to grant Accessibility again, so keep it (it is valid until 2036).

**Update key.** Losing `TAURI_SIGNING_PRIVATE_KEY` strands every installed copy on its current version (a new key means a new public key, which only a manual install delivers). Keep the backup safe.

Windows installers are not code-signed yet (for example with Azure Trusted Signing), so SmartScreen asks for confirmation.

### Update manifest

Each build uploads its update bundle and signature: `Mote_X.Y.Z_aarch64.app.tar.gz(.sig)`, `Mote_X.Y.Z_x64.app.tar.gz(.sig)`, `Mote_X.Y.Z_x64-setup.exe(.sig)` and `Mote_X.Y.Z_x64_en-US.msi(.sig)`. After all builds succeed, the release workflow runs `scripts/update-manifest.mjs` to write `latest.json` (platform keys `darwin-aarch64[-app]`, `darwin-x86_64[-app]`, `windows-x86_64[-nsis]`, `windows-x86_64-msi`) and uploads it with `SHA256SUMS.txt`. Installed apps read it from `releases/latest/download/latest.json`, so a release becomes an update the moment it is published as latest.

## Building without a release

Run **Build installers** (`build.yml`) from the Actions tab to get all three installers as workflow artifacts without creating a release.
