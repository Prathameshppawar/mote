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
| `SHA256SUMS.txt` | SHA-256 of every file above |

If a build job fails for a transient reason (a network error, a runner problem), use **Re-run failed jobs**. A re-run uses the tagged commit, so for a code or workflow fix, commit the fix and move the tag:

```sh
git tag -d vX.Y.Z && git push origin :refs/tags/vX.Y.Z
git tag -a vX.Y.Z -m "Mote X.Y.Z" && git push origin vX.Y.Z
```

Either way the existing draft is reused and assets with the same names are replaced. Nothing is published until every build has succeeded.

After publishing, download one installer per platform, check it against `SHA256SUMS.txt`, and run a quick smoke test.

## Code signing

Without signing secrets, macOS builds are ad-hoc signed (`signingIdentity: "-"`), and Windows installers are unsigned. Users see Gatekeeper and SmartScreen prompts, as the release notes explain.

To sign and notarize macOS builds, add these repository secrets. `build.yml` exports only the ones that are set:

| Secret | |
|---|---|
| `APPLE_CERTIFICATE` | base64 of the Developer ID Application `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | its password |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Name (TEAMID)` |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | notarization (app-specific password) |

Windows signing (for example Azure Trusted Signing) isn't configured yet.

## Building without a release

Run **Build installers** (`build.yml`) from the Actions tab to get all three installers as workflow artifacts without creating a release.
