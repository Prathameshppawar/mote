## What and why

<!-- What does this change, and what problem does it solve? Link issues: Fixes #123 -->

## How it was tested

<!-- Commands run, platforms tried, apps typed in. -->

## Checklist

- [ ] Title follows [Conventional Commits](https://www.conventionalcommits.org/) (`feat(core): …`, `fix(macos): …`)
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass
- [ ] `npm run typecheck`, `npm run lint` and `npm test` pass in `apps/desktop`
- [ ] TypeScript bindings regenerated if IPC types changed (`cargo test -p mote-desktop --features bindings export_bindings`)
- [ ] No API keys, private text, clipboard contents or prompts are logged or stored
- [ ] Nothing new runs on the typing path without debouncing and cancellation
- [ ] Docs and `CHANGELOG.md` (Unreleased) updated where behaviour changed
