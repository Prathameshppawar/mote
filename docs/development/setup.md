# Development setup

## Prerequisites

| | macOS | Windows | Linux |
|---|---|---|---|
| Rust | [rustup](https://rustup.rs); `rust-toolchain.toml` pins 1.99 with rustfmt and clippy, installed on first use | same | same |
| Node.js | 20.19+ (22 LTS recommended) | same | same |
| System | Xcode Command Line Tools (`xcode-select --install`) | Visual Studio Build Tools with "Desktop development with C++"; WebView2 (built into Windows 11) | core crates only, see below |

There is no Linux app, but the platform-independent crates build and test on Linux, which is how CI runs them.

## First run

```sh
git clone https://github.com/Prathameshppawar/mote.git
cd mote/apps/desktop
npm ci
npm run tauri dev
```

`tauri dev` starts Vite on `http://localhost:1420` with hot reload and runs the Rust app in debug mode. The first build compiles the whole dependency tree and takes a few minutes; later builds are incremental.

On first launch, onboarding asks for the Accessibility permission and a Groq API key.

### Accessibility during development (macOS)

macOS attributes Accessibility to the *responsible* process. A binary started from a terminal counts as part of that terminal, so grant **your terminal or IDE** (Terminal, iTerm, VS Code, …) access under System Settings → Privacy & Security → Accessibility. The installed `Mote.app` gets its own entry.

The development build uses the same identifier (`io.github.prathameshppawar.mote`) as an installed release, so they share the database, logs and keychain item. Quit the installed app before running `tauri dev`; single-instance handling would otherwise focus the running copy.

## Working on the UI without Rust

The React app falls back to a mock backend when it isn't running inside Tauri, so the interface can be developed in any browser:

```sh
cd apps/desktop
npm run dev
# http://localhost:1420/            settings, onboarding, usage (use #/usage, #/privacy, …)
# http://localhost:1420/palette.html
# http://localhost:1420/overlay.html
```

The mock (`src/lib/mock.ts`) serves realistic settings, usage data and engine status.

## Repository layout

```text
Cargo.toml                 workspace, version, lints, release profile
crates/mote-core           pure logic (engine, privacy, intent, language, spelling, usage, …)
crates/mote-providers      Groq / OpenAI-compatible HTTP provider
crates/mote-storage        SQLite
crates/mote-platform       macOS and Windows adapters
apps/desktop/src-tauri     Tauri app (Rust)
apps/desktop/src           React UI; src/bindings is generated, don't edit
scripts/                   version, secret and label tooling
docs/                      architecture, privacy, development, product, decisions, releases
```

## Everyday commands

From the repository root:

```sh
cargo fmt --all                                         # format (max width 120)
cargo clippy --workspace --all-targets -- -D warnings   # lint
cargo test --workspace                                  # all Rust tests
```

From `apps/desktop`:

```sh
npm run typecheck    # TypeScript
npm run lint         # ESLint, zero warnings allowed
npm test             # Vitest
npm run build        # production frontend bundle
```

### Generated TypeScript bindings

IPC types are defined once, in Rust, and exported to `apps/desktop/src/bindings/` with `ts-rs`. After changing any type that crosses IPC, regenerate and commit the bindings:

```sh
cargo test -p mote-desktop --features bindings export_bindings
```

CI fails if the committed bindings are stale.

## Logs and debugging

Logs go to stdout in development and to daily files (7 kept):

- macOS: `~/Library/Logs/io.github.prathameshppawar.mote/`
- Windows: `%LOCALAPPDATA%\io.github.prathameshppawar.mote\logs\`

Set the filter with `MOTE_LOG` (same syntax as `RUST_LOG`):

```sh
MOTE_LOG=debug npm run tauri dev
MOTE_LOG=info,mote_core=debug,mote_platform=debug npm run tauri dev
```

Logs never contain typed text, prompts, outputs, clipboard contents, window titles or keys, at any level. Keep it that way: log lengths and kinds (`redact::describe(text)`), never values. See the [privacy model](../privacy/privacy-model.md).

The app's database is at `~/Library/Application Support/io.github.prathameshppawar.mote/mote.db` (macOS) or `%APPDATA%\io.github.prathameshppawar.mote\mote.db` (Windows) and can be inspected with `sqlite3`. Settings → Diagnostics shows component health and database statistics.

## Repository scripts

| Script | Purpose |
|---|---|
| `node scripts/check-version.mjs [--tag vX.Y.Z]` | All manifests and lockfiles agree on the version (and match a tag, with CHANGELOG entry and release notes) |
| `node scripts/set-version.mjs X.Y.Z` | Set the version everywhere |
| `bash scripts/check-secrets.sh` | Scan tracked files for real credentials |
| `scripts/sync-labels.sh [owner/repo]` | Create or update GitHub labels from `.github/labels.yml` |
| `node scripts/third-party-notices.mjs` | Regenerate the dependency license tables in `THIRD_PARTY_NOTICES.md` |

## Building installers locally

```sh
cd apps/desktop
CI=true npm run tauri build
```

`CI=true` skips the Finder AppleScript that decorates the DMG window, which otherwise waits for an Automation permission prompt. Output lands in `target/release/bundle/` (`macos/Mote.app`, `dmg/Mote_<version>_<arch>.dmg`; on Windows `nsis/…-setup.exe` and `msi/….msi`).

To build the Intel macOS app on Apple Silicon:

```sh
rustup target add x86_64-apple-darwin
CI=true npm run tauri build -- --target x86_64-apple-darwin
```
