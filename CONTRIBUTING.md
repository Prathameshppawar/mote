# Contributing to Mote

Thanks for helping make Mote better. This guide covers how to propose changes and what a good pull request looks like. By participating you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Before you start

- **Bugs**: open an issue with the bug report template, including the diagnostics report (Settings → Diagnostics → Copy diagnostics). Never paste private text, prompts or keys.
- **Features and larger changes**: open an issue first to agree on the approach. Mote has firm principles (see [vision](docs/product/vision.md)): it stays in the flow, never blocks typing, and keeps content off disk.
- **Security issues**: follow [SECURITY.md](SECURITY.md), not the public tracker.

## Development

Setup, commands and debugging are in [docs/development/setup.md](docs/development/setup.md). In short:

```sh
cd apps/desktop && npm ci && npm run tauri dev
```

Read the [architecture overview](docs/architecture/overview.md) before larger changes. Most logic belongs in `crates/mote-core`, behind the existing traits, where it can be tested without an OS.

## Making a change

1. Branch from `main`: `git switch -c fix/palette-focus`.
2. Make focused commits using [Conventional Commits](https://www.conventionalcommits.org/):

   ```text
   feat(completion): offer alternatives after a dismissal
   fix(windows): read caret bounds from the selection range
   docs(privacy): describe clipboard retention
   test(ai): cover provider timeouts
   ```

   Types: `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, `build`, `ci`, `chore`, `style`. Common scopes: `core`, `engine`, `completion`, `intent`, `context`, `ai`, `usage`, `storage`, `macos`, `windows`, `desktop`, `ui`, `privacy`.
3. Add or update tests. Behaviour changes in the core come with unit tests or engine scenarios (see [testing](docs/development/testing.md)).
4. Update documentation in the same pull request: architecture docs, the privacy model if you read, send, store or log anything new, an ADR if you change a recorded decision, and the `[Unreleased]` section of `CHANGELOG.md` for user-visible changes.
5. Open a pull request and fill in the template.

## Checks

CI must pass before merging. Run the same checks locally:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd apps/desktop && npm run typecheck && npm run lint && npm test
```

If you changed a type that crosses IPC, regenerate the TypeScript bindings and commit them:

```sh
cargo test -p mote-desktop --features bindings export_bindings
```

## Standards

**Rust**
- Formatting follows `rustfmt.toml` (width 120). Clippy runs with `-D warnings` and the workspace lints in `Cargo.toml`.
- No `unwrap()` or `expect()` on paths that can fail at runtime; return errors or degrade gracefully.
- Every `unsafe` block has a `// SAFETY:` comment explaining why it is sound.
- Platform code stays in `mote-platform`. The core talks to the OS only through `PlatformAdapter`.

**TypeScript and React**
- Strict TypeScript, ESLint with zero warnings.
- Use the generated types in `src/bindings`; never hand-write IPC types.
- Interfaces must work with the keyboard, have accessible names, and support light and dark appearance.

**Privacy and performance (non-negotiable)**
- Never log, store or send typed text, prompts, outputs, clipboard contents, window titles or keys. Log lengths and kinds (`redact::describe`).
- Check the privacy policy before reading anything new from the OS.
- Nothing slow runs on the typing path: debounce, cancel and move work off the engine task.
- Every model request goes through `AiClient` and `ResilientProvider`, so it is metered exactly once.

## Releases

Maintainers cut releases by tagging `main`; see [docs/development/releasing.md](docs/development/releasing.md).

## License

Contributions are licensed under the [MIT License](LICENSE), the same as the project.
