# 0008. In-app updates from GitHub Releases, and a stable signing identity

- Status: Accepted
- Date: 2026-10-08

## Context

1.0 had no updater: every fix meant a manual download and install. Worse, its macOS builds were ad hoc signed. macOS ties an app's Accessibility permission to its code signature, and an ad hoc signature changes with every build, so each new version silently lost the permission until the user re-enabled it. Mote is useless without that permission.

Mote has no server of its own ([0002](0002-local-first.md)), and the repository is public on GitHub.

## Decision

- **Tauri's updater, with GitHub Releases as the source.** The app reads `https://github.com/Prathameshppawar/mote/releases/latest/download/latest.json` a minute after launch and every six hours (when "Update automatically" is on), downloads a newer version in the background and installs it only when the user chooses "Restart to Update". No identifiers or data are sent.
- **Updates are signed.** Release builds sign each update bundle with the updater's private key (kept in the repository's secrets). The app verifies downloads against the public key compiled into it and refuses anything else.
- **One manifest per release, written last.** The three platform builds run in parallel, so instead of letting each build edit `latest.json`, the release workflow writes it once from the uploaded signatures after every build has succeeded.
- **A stable, self-signed signing identity on macOS.** Builds are signed with Mote's own "Mote Code Signing" certificate. The designated requirement then names that certificate instead of the build's hash, so later versions satisfy the requirement recorded with the user's Accessibility permission and the permission survives updates. The release runner trusts the public certificate before signing, since `codesign` only uses trusted identities; the private key and password live in repository secrets.

## Alternatives considered

- **Apple Developer ID signing and notarization.** The best option: it also removes the Gatekeeper prompt on first install. It needs a paid Apple Developer membership; the workflow already supports it through the `APPLE_*` secrets, and switching later costs one more permission re-grant.
- **A Mote update server.** Unneeded: GitHub Releases serves the manifest and installers.
- **Installing updates silently.** On Windows the installer must close the app, and on macOS replacing the bundle under a running app is surprising. Installing on an explicit restart keeps the user in control.

## Consequences

- Users of 1.0 install 1.1 by hand once and re-grant Accessibility once; after that, updates arrive in the app and keep the permission.
- Losing the updater private key would strand installed apps on their current version: it is backed up outside the repository and must be kept safe.
- Builds are still not notarized, so a manual first install shows Gatekeeper's prompt. Updates installed by the app don't.
- Update checks are a second network destination besides the AI provider, documented in the [privacy model](../privacy/privacy-model.md#update-checks) and switchable off.
