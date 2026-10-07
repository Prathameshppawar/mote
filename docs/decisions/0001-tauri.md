# 0001. Tauri 2 with a Rust core and a React interface

- Status: Accepted
- Date: 2026-10-07

## Context

Mote runs all day in the menu bar or tray and acts on every app you type in. That sets hard requirements:

- **Deep OS integration** on macOS and Windows: Accessibility and UI Automation, synthetic keyboard input, the clipboard, the keychain, global shortcuts, a non-activating transparent overlay, tray menus.
- **A small resident footprint.** An always-on helper that costs hundreds of megabytes or measurable idle CPU gets uninstalled.
- **A rich interface** for settings, onboarding and a usage dashboard with charts.
- **One codebase** for two platforms, maintained by a small team.
- **Testable decision logic**, separate from any OS or UI.

## Decision

Build Mote with **Tauri 2** (stable 2.x, not the 3.0 previews):

- The product logic lives in **Rust crates** (`mote-core`, `mote-providers`, `mote-storage`, `mote-platform`) with no UI dependency.
- The desktop shell (`apps/desktop/src-tauri`) composes them and exposes IPC commands.
- The interface is **React 19 + TypeScript**, built with Vite, in three webview windows: main (settings, onboarding, usage), overlay and palette.
- IPC types are generated from Rust with `ts-rs`, so the UI can't drift from the backend.
- Each window gets only the commands it needs through Tauri capabilities, under a strict content security policy.

## Alternatives considered

- **Electron.** The most mature ecosystem, but it ships a full Chromium and Node per app (typically 150–300 MB of memory at rest), and OS integration would still need native modules in C++ or Rust.
- **Native per platform** (Swift/AppKit and C#/WinUI). The best platform fidelity, but two codebases for every feature, and the core logic (intent, language, usage accounting) would be duplicated or bridged.
- **Flutter or Qt desktop.** One codebase, but weaker access to accessibility APIs and a heavier toolkit for a mostly invisible app.

## Consequences

- The release app bundle is about 13 MB, and the macOS DMG about 6 MB. At idle the app process uses no measurable CPU and about 105 MB of resident memory, with the system webview's helper processes on top. It starts in about 0.3 s.
- OS APIs are called from Rust directly (`objc2`, `windows`), and decision logic is plain Rust tested with fakes.
- The system webviews differ (WKWebView on macOS, WebView2 on Windows), so CSS sticks to widely supported features with fallbacks.
- Contributors need a Rust toolchain; `rust-toolchain.toml` pins it.
- The transparent, non-focusable overlay needs Tauri's `macos-private-api` feature on macOS.
