# Platform layer

Everything Mote does to and with other applications goes through one trait, `PlatformAdapter` (`crates/mote-core/src/platform.rs`), implemented per operating system in `crates/mote-platform`. The core never calls an OS API directly, which keeps it testable (`FakePlatform`) and keeps platform quirks in one place.

## The contract

| Method | Purpose |
|---|---|
| `permission_status`, `request_accessibility_permission`, `open_permission_settings` | Accessibility permission and Secure Input state |
| `active_application`, `active_window`, `running_applications`, `activate_application` | Which app is in front (name, bundle ID / executable, PID), its window title, the app picker for exclusions, refocusing your app after the palette |
| `focused_input(ReadLimits)` | The focused text input: app, role, secure flag, multi-line flag, placeholder, label, **bounded** text before and after the caret, selection, caret bounds, a stable element key |
| `selected_text(max)` | The selection, bounded |
| `clipboard_sequence`, `clipboard_text(max)`, `clipboard_has_non_text` | Change detection without reading; plain text only, skipping concealed or monitor-excluded content |
| `set_clipboard_text`, `set_transient_clipboard_text` | A normal write (restore, explicit copy), or a paste buffer hidden from clipboard history |
| `type_text`, `press_key(Key, count)`, `paste`, `select_all` | Applying accepted suggestions |

Every read takes a limit, and every implementation is written so that an excluded app, a secure field or Secure Input never yields text. `CoordinateSpace` tells the shell whether caret rectangles are in logical points (macOS) or physical pixels (Windows), so the overlay lands on the caret on any display scale.

## macOS

`crates/mote-platform/src/macos/`, using the `objc2` bindings.

**Permission.** Reading other apps' text requires the Accessibility permission (`AXIsProcessTrusted`). Onboarding requests it and opens System Settings → Privacy & Security → Accessibility. Without it, the observer stays blocked and reads nothing. Secure Input (`IsSecureEventInputEnabled`), turned on by password prompts and some terminals, blocks all reading and typing.

**Reading (`ax.rs`, `mod.rs`).**
- The system-wide element gives the focused application and its focused element (`AXFocusedUIElement`). Every call has a 0.25 s messaging timeout, so a hung app can never hang Mote.
- Apps that render with Chromium build their accessibility tree only when an assistive tool asks, and until then report no focused element at all. So before asking for focus, Mote switches the tree on for the frontmost app, once per process: `AXManualAccessibility` for every app (the switch Electron documents for third-party tools; native apps ignore it), plus `AXEnhancedUserInterface` (the switch VoiceOver uses) for Chromium browsers and apps that embed Chromium or Microsoft WebView2, detected from the bundle identifier and the frameworks in the app bundle. If the system-wide query still finds nothing, the frontmost application is asked directly.
- Role and subrole map to `InputRole` (text area, text field, search field, combo box, web document, terminal). `AXSecureTextField` marks a field as secure, and its value is never read.
- Text is read with `AXStringForRange` for just the bounded ranges around the caret (`AXSelectedTextRange`, `AXNumberOfCharacters`). If an app doesn't support ranges, `AXValue` is read and immediately trimmed to the same bounds.
- The caret rectangle comes from `AXBoundsForRange` on the character before or after the caret, with plausibility checks.
- Attributes that don't change while a field keeps focus (role, placeholder, secure flag) are cached per element; text is re-read when length or selection changes, and at least every 1.5 s.

**Typing (`keyboard.rs`).** Text is typed as Unicode strings attached to Quartz keyboard events, which doesn't depend on the keyboard layout (QWERTY, AZERTY, Dvorak, Hindi). Only layout-independent keys (Delete, Tab, Escape, Return, arrows) are sent by key code.

**Paste and select-all (`menu.rs`).** Mote never synthesises ⌘V or ⌘A. On a non-QWERTY layout the "V" key position can be a different letter, and a synthetic ⌘ shortcut could trigger the wrong command (on AZERTY, the QWERTY "A" position is "Q", so a synthetic ⌘A could quit the app). Instead, Mote finds the menu-bar item whose key equivalent is ⌘V or ⌘A (`AXMenuItemCmdChar`) and presses it (`AXPress`). That runs the app's own paste or select-all code path, which web and Electron apps treat as a user action. Select-all first tries setting `AXSelectedTextRange` directly. If an app has no such menu item, the core falls back to typing.

**Clipboard (`pasteboard.rs`).** The change count detects copies without reading. Content marked `org.nspasteboard.ConcealedType` or `TransientType` (password managers, other apps' temporary data) is never read. Mote's paste buffer is marked transient so clipboard managers ignore it.

**Applications (`workspace.rs`).** `NSWorkspace` provides the frontmost and running applications and activates your app again after the palette closes.

**Packaging.** The app is an agent app (`LSUIElement`), so it doesn't appear in the Dock until you open a window. It uses Tauri's `macos-private-api` for the transparent, non-activating overlay.

## Windows

`crates/mote-platform/src/windows/`, using the `windows` crate.

**Permission.** UI Automation needs no permission, so the observer is never blocked for that reason. Windows does prevent a normal process from reading or sending input to an app running as administrator (UIPI); such apps simply aren't assisted.

**COM (`com.rs`).** Each thread that uses UI Automation joins the multithreaded apartment once, and a single `IUIAutomation` client is shared. Nothing runs COM teardown at thread exit, where it could deadlock under the loader lock.

**Reading (`uia.rs`).**
- `GetFocusedElement` gives the element; the control type (edit, document, combo box, group) maps to `InputRole`, and `IsPassword` marks secure fields, which are never read.
- With `TextPattern`/`TextPattern2`, the caret range (`GetCaretRange`) or selection is cloned and its endpoints moved by at most the read limits on each side, so only the text around the caret is fetched. Whole documents are never requested.
- Fields with only `ValuePattern` (simple edits) return their value, which is trimmed to the tail before the caret.
- Caret bounds come from the bounding rectangles of the caret range, in physical pixels.

**Processes (`process.rs`).** The foreground window (`GetForegroundWindow`) gives the process ID, executable path and a display name from the executable's version information (`FileDescription` / `ProductName`). Window titles are read with a length bound. Shell hosts such as `ApplicationFrameHost` are resolved to the actual app.

**Typing (`input.rs`).** `SendInput` with `KEYEVENTF_UNICODE`, one down/up pair per UTF-16 unit, so typing is layout-independent. Named keys use virtual keys with their scan codes. Paste and select-all are sent as Ctrl+V and Ctrl+A.

**Clipboard (`clipboard.rs`).** Plain Unicode text through the Win32 clipboard API, opened briefly with retries because other apps may hold it. Content flagged with `ExcludeClipboardContentFromMonitorProcessing`, `Clipboard Viewer Ignore` or `CanIncludeInClipboardHistory = 0` (as password managers do) is never read. Mote's paste buffer carries those markers plus `CanUploadToCloudClipboard = 0`, so Win+V history and cloud clipboard skip it. Images, files and rich text are detected so they are never overwritten by a paste-insert.

**Packaging.** The NSIS installer installs per user (no administrator rights) and bootstraps the WebView2 runtime if it's missing; an MSI is also built.

## Other platforms

`unsupported.rs` reports everything as unsupported, so the workspace builds and the core's tests run on Linux (as CI does), but there is no Linux app.

## Known platform limits

- Apps that don't expose their text to accessibility APIs (terminals with custom renderers, remote desktops, games, some canvas-based editors) can't be assisted.
- Caret positions reported by some web apps can be approximate; the overlay is clamped to the visible work area of the right monitor.
- Apps built on Microsoft WebView2 (new Teams, new Outlook) expose their text to assistive tools only intermittently, even with both switches set.
- Since 1.1, builds are signed with Mote's own certificate, so the Accessibility permission survives updates; moving from an ad hoc signed 1.0 needs one re-grant.

## Testing

Platform-independent logic (UTF-16 splitting, element keys, role mapping, version-string parsing, rectangle parsing) has unit tests that run everywhere. The Windows clipboard and running-application tests run on Windows CI runners. Behaviour that needs a real desktop session and permissions (reading text in real apps, typing, menu presses) is verified manually; see [testing](../development/testing.md#manual-verification).
