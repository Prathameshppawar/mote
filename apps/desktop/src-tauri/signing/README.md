# Code signing

`mote-code-signing.crt` is the **public** certificate Mote's macOS builds are signed with (a self-signed "Mote Code Signing" certificate). Signing every release with the same certificate keeps the app's identity stable, so macOS keeps the Accessibility permission across updates.

The release workflow marks this certificate as trusted on the build runner before signing (`codesign` only uses trusted identities). The private key never enters the repository: it lives in the `APPLE_CERTIFICATE` / `APPLE_CERTIFICATE_PASSWORD` repository secrets. See [docs/development/releasing.md](../../../../docs/development/releasing.md#code-signing-and-update-keys).
