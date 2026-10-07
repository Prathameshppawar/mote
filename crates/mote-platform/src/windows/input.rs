//! Keyboard synthesis with `SendInput`.
//!
//! Text is typed as Unicode key events (one down/up pair per UTF-16 unit), so
//! the result does not depend on the keyboard layout. Named keys and shortcuts
//! use virtual keys with their scan codes.

use std::mem::size_of;
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use ::windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC, VIRTUAL_KEY, VK_A, VK_BACK, VK_CONTROL,
    VK_ESCAPE, VK_LEFT, VK_LWIN, VK_MENU, VK_RETURN, VK_RIGHT, VK_RWIN, VK_SHIFT, VK_TAB, VK_V,
};
use mote_core::platform::{Key, PlatformError};

/// Events per `SendInput` call. Batches are cut only where no key is held.
const BATCH_SIZE: usize = 64;
/// Pause between batches, so slow applications keep up.
const BATCH_PAUSE: Duration = Duration::from_millis(2);
/// How long to wait for the user to release Shift/Ctrl/Alt/Win before injecting.
const MODIFIER_WAIT: Duration = Duration::from_millis(300);
const MODIFIER_POLL: Duration = Duration::from_millis(10);
/// Upper bound on repeated key presses in one call.
const MAX_REPEAT: usize = 10_000;
/// An unassigned virtual key, pressed inside an Alt tap so the tap does not
/// activate a menu bar.
const VK_MASK: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);
/// Modifiers that would combine with injected keys if the user still holds them.
const MODIFIERS: [VIRTUAL_KEY; 5] = [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN];

/// Serializes injected sequences from different threads.
static INPUT_LOCK: Mutex<()> = Mutex::new(());

/// What a key event presses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stroke {
    /// A UTF-16 code unit, typed independently of the keyboard layout.
    Unicode(u16),
    Virtual(VIRTUAL_KEY),
}

/// A key press or release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct KeyEvent {
    pub(super) stroke: Stroke,
    pub(super) up: bool,
}

impl KeyEvent {
    const fn down(stroke: Stroke) -> Self {
        Self { stroke, up: false }
    }

    const fn up(stroke: Stroke) -> Self {
        Self { stroke, up: true }
    }

    /// `KEYEVENTF_*` flags for this event.
    fn flags(self) -> KEYBD_EVENT_FLAGS {
        let mut flags = match self.stroke {
            Stroke::Unicode(_) => KEYEVENTF_UNICODE,
            Stroke::Virtual(vk) if is_extended(vk) => KEYEVENTF_EXTENDEDKEY,
            Stroke::Virtual(_) => KEYBD_EVENT_FLAGS(0),
        };
        if self.up {
            flags |= KEYEVENTF_KEYUP;
        }
        flags
    }

    fn to_input(self) -> INPUT {
        let (vk, scan) = match self.stroke {
            Stroke::Unicode(unit) => (VIRTUAL_KEY(0), unit),
            Stroke::Virtual(vk) => (vk, scan_code(vk)),
        };
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: self.flags(), time: 0, dwExtraInfo: 0 },
            },
        }
    }
}

/// Arrow keys live on the extended part of the keyboard.
fn is_extended(vk: VIRTUAL_KEY) -> bool {
    matches!(vk, VK_LEFT | VK_RIGHT)
}

/// Hardware scan code of `vk` in the current keyboard layout (some
/// applications ignore events without one).
fn scan_code(vk: VIRTUAL_KEY) -> u16 {
    // SAFETY: MapVirtualKeyW only reads the current keyboard layout.
    let scan = unsafe { MapVirtualKeyW(u32::from(vk.0), MAPVK_VK_TO_VSC) };
    u16::try_from(scan).unwrap_or(0)
}

fn tap(vk: VIRTUAL_KEY) -> [KeyEvent; 2] {
    [KeyEvent::down(Stroke::Virtual(vk)), KeyEvent::up(Stroke::Virtual(vk))]
}

fn chord(modifier: VIRTUAL_KEY, vk: VIRTUAL_KEY) -> [KeyEvent; 4] {
    [
        KeyEvent::down(Stroke::Virtual(modifier)),
        KeyEvent::down(Stroke::Virtual(vk)),
        KeyEvent::up(Stroke::Virtual(vk)),
        KeyEvent::up(Stroke::Virtual(modifier)),
    ]
}

fn virtual_key(key: Key) -> VIRTUAL_KEY {
    match key {
        Key::Backspace => VK_BACK,
        Key::Tab => VK_TAB,
        Key::Escape => VK_ESCAPE,
        Key::ShiftEnter => VK_RETURN,
        Key::Left => VK_LEFT,
        Key::Right => VK_RIGHT,
    }
}

/// Events that press `key` once.
fn key_stroke(key: Key) -> Vec<KeyEvent> {
    match key {
        Key::ShiftEnter => chord(VK_SHIFT, VK_RETURN).to_vec(),
        other => tap(virtual_key(other)).to_vec(),
    }
}

/// Events that press `key` `count` times.
pub(super) fn key_events(key: Key, count: usize) -> Vec<KeyEvent> {
    key_stroke(key).repeat(count)
}

/// Events that type `text`. Line feeds become Shift+Enter (a plain Enter would
/// send chat messages); carriage returns are skipped.
pub(super) fn text_events(text: &str) -> Vec<KeyEvent> {
    let mut events = Vec::with_capacity(text.len() * 2);
    for ch in text.chars() {
        match ch {
            '\r' => {}
            '\n' => events.extend(chord(VK_SHIFT, VK_RETURN)),
            _ => {
                let mut buffer = [0u16; 2];
                for &unit in ch.encode_utf16(&mut buffer).iter() {
                    events.push(KeyEvent::down(Stroke::Unicode(unit)));
                    events.push(KeyEvent::up(Stroke::Unicode(unit)));
                }
            }
        }
    }
    events
}

/// Splits `events` into batches of about `max` events, cutting only where no
/// key is held so a chord never straddles two `SendInput` calls.
pub(super) fn batches(events: &[KeyEvent], max: usize) -> Vec<&[KeyEvent]> {
    let mut batches = Vec::new();
    let mut start = 0;
    let mut held = 0usize;
    for (index, event) in events.iter().enumerate() {
        if event.up {
            held = held.saturating_sub(1);
        } else {
            held += 1;
        }
        if held == 0 && index + 1 - start >= max {
            batches.push(&events[start..=index]);
            start = index + 1;
        }
    }
    if start < events.len() {
        batches.push(&events[start..]);
    }
    batches
}

/// Releases for keys that `sent` pressed but did not release, innermost first.
pub(super) fn unreleased(sent: &[KeyEvent]) -> Vec<KeyEvent> {
    let mut pressed: Vec<Stroke> = Vec::new();
    for event in sent {
        if event.up {
            if let Some(index) = pressed.iter().rposition(|stroke| *stroke == event.stroke) {
                pressed.remove(index);
            }
        } else {
            pressed.push(event.stroke);
        }
    }
    pressed.into_iter().rev().map(KeyEvent::up).collect()
}

fn modifiers_held() -> bool {
    MODIFIERS.iter().any(|vk| {
        // SAFETY: GetAsyncKeyState only reads the asynchronous key state.
        let state = unsafe { GetAsyncKeyState(i32::from(vk.0)) };
        state < 0 // The most significant bit is set while the key is down.
    })
}

/// Waits until the user has released all modifiers, so a held shortcut
/// modifier does not combine with the injected keys.
fn wait_for_modifiers() -> Result<(), PlatformError> {
    let deadline = Instant::now() + MODIFIER_WAIT;
    while modifiers_held() {
        if Instant::now() >= deadline {
            return Err(PlatformError::Failed("modifier keys are still held down".into()));
        }
        thread::sleep(MODIFIER_POLL);
    }
    Ok(())
}

/// Injects `inputs`; returns how many events the system accepted.
fn inject(inputs: &[INPUT]) -> usize {
    if inputs.is_empty() {
        return 0;
    }
    // SAFETY: `inputs` holds fully initialized keyboard INPUT structures and the size
    // argument is the size of one INPUT.
    let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
    usize::try_from(sent).unwrap_or(0)
}

/// Waits for modifiers to be released, then injects `events` in batches.
fn send(events: &[KeyEvent]) -> Result<(), PlatformError> {
    if events.is_empty() {
        return Ok(());
    }
    let _lock = INPUT_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    wait_for_modifiers()?;
    for (index, batch) in batches(events, BATCH_SIZE).into_iter().enumerate() {
        if index > 0 {
            thread::sleep(BATCH_PAUSE);
        }
        let inputs: Vec<INPUT> = batch.iter().map(|event| event.to_input()).collect();
        let sent = inject(&inputs);
        if sent < inputs.len() {
            // Never leave a key (e.g. Ctrl or Shift) stuck down after a partial injection.
            let releases: Vec<INPUT> =
                unreleased(&batch[..sent.min(batch.len())]).into_iter().map(KeyEvent::to_input).collect();
            inject(&releases);
            tracing::debug!(requested = inputs.len(), sent, "SendInput accepted fewer events than requested");
            return Err(PlatformError::Failed(
                "Windows blocked the synthesized keyboard input (the application may be running as administrator)"
                    .into(),
            ));
        }
    }
    Ok(())
}

/// Types `text` at the caret of the focused application.
pub(super) fn type_text(text: &str) -> Result<(), PlatformError> {
    send(&text_events(text))
}

/// Presses `key` `count` times.
pub(super) fn press_key(key: Key, count: usize) -> Result<(), PlatformError> {
    if count > MAX_REPEAT {
        return Err(PlatformError::Failed(format!("refusing to press a key more than {MAX_REPEAT} times")));
    }
    send(&key_events(key, count))
}

/// Ctrl+A.
pub(super) fn select_all() -> Result<(), PlatformError> {
    send(&chord(VK_CONTROL, VK_A))
}

/// Ctrl+V.
pub(super) fn paste() -> Result<(), PlatformError> {
    send(&chord(VK_CONTROL, VK_V))
}

/// Taps Alt (masked by an unassigned key so no menu bar activates). Windows
/// allows the foreground to change after an Alt press.
pub(super) fn tap_alt() {
    let _lock = INPUT_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let inputs = chord(VK_MENU, VK_MASK).map(KeyEvent::to_input);
    inject(&inputs);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unicode(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn balanced(events: &[KeyEvent]) -> bool {
        unreleased(events).is_empty() && events.iter().filter(|e| e.up).count() * 2 == events.len()
    }

    #[test]
    fn text_becomes_unicode_down_up_pairs() {
        let events = text_events("ab");
        assert_eq!(
            events,
            vec![
                KeyEvent::down(Stroke::Unicode(u16::from(b'a'))),
                KeyEvent::up(Stroke::Unicode(u16::from(b'a'))),
                KeyEvent::down(Stroke::Unicode(u16::from(b'b'))),
                KeyEvent::up(Stroke::Unicode(u16::from(b'b'))),
            ]
        );
        assert!(text_events("").is_empty());
    }

    #[test]
    fn astral_and_indic_characters_use_every_utf16_unit() {
        let emoji = text_events("😀");
        assert_eq!(emoji.len(), 4, "a surrogate pair is two units");
        let units: Vec<u16> = emoji
            .iter()
            .filter(|e| !e.up)
            .filter_map(|e| if let Stroke::Unicode(u) = e.stroke { Some(u) } else { None })
            .collect();
        assert_eq!(units, unicode("😀"));
        assert_eq!(text_events("मला").len(), unicode("मला").len() * 2);
    }

    #[test]
    fn line_feeds_become_shift_enter_and_carriage_returns_are_skipped() {
        let events = text_events("a\r\nb");
        assert_eq!(events.len(), 2 + 4 + 2);
        assert_eq!(&events[2..6], &chord(VK_SHIFT, VK_RETURN));
        assert!(text_events("\r").is_empty());
    }

    #[test]
    fn keys_map_to_virtual_keys() {
        assert_eq!(virtual_key(Key::Backspace), VK_BACK);
        assert_eq!(virtual_key(Key::Tab), VK_TAB);
        assert_eq!(virtual_key(Key::Escape), VK_ESCAPE);
        assert_eq!(virtual_key(Key::Left), VK_LEFT);
        assert_eq!(virtual_key(Key::Right), VK_RIGHT);
        assert_eq!(key_events(Key::Backspace, 3), tap(VK_BACK).repeat(3));
        assert_eq!(key_events(Key::ShiftEnter, 2), chord(VK_SHIFT, VK_RETURN).repeat(2));
        assert!(key_events(Key::Tab, 0).is_empty());
        assert!(press_key(Key::Tab, MAX_REPEAT + 1).is_err(), "absurd repeat counts are refused before injecting");
    }

    #[test]
    fn event_flags_match_the_stroke() {
        let letter = KeyEvent::down(Stroke::Unicode(0x61));
        assert_eq!(letter.flags(), KEYEVENTF_UNICODE);
        assert_eq!(KeyEvent::up(Stroke::Unicode(0x61)).flags(), KEYEVENTF_UNICODE | KEYEVENTF_KEYUP);
        assert_eq!(KeyEvent::down(Stroke::Virtual(VK_LEFT)).flags(), KEYEVENTF_EXTENDEDKEY);
        assert_eq!(KeyEvent::up(Stroke::Virtual(VK_RIGHT)).flags(), KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP);
        assert_eq!(KeyEvent::down(Stroke::Virtual(VK_BACK)).flags(), KEYBD_EVENT_FLAGS(0));
        let input = letter.to_input();
        assert_eq!(input.r#type, INPUT_KEYBOARD);
        // SAFETY: `to_input` always fills the keyboard member of the union.
        let keyboard = unsafe { input.Anonymous.ki };
        assert_eq!(
            (keyboard.wVk, keyboard.wScan),
            (VIRTUAL_KEY(0), 0x61),
            "Unicode events carry the unit as scan code"
        );
    }

    #[test]
    fn batches_split_only_between_complete_strokes() {
        let text: String = "x".repeat(100);
        let events = text_events(&text);
        let parts = batches(&events, BATCH_SIZE);
        assert_eq!(parts.iter().map(|part| part.len()).sum::<usize>(), events.len());
        assert!(parts.iter().all(|part| part.len() <= BATCH_SIZE && balanced(part)));

        let mixed = text_events(&"ab\n".repeat(30));
        let parts = batches(&mixed, BATCH_SIZE);
        assert_eq!(parts.concat(), mixed);
        assert!(parts.iter().all(|part| balanced(part)), "Shift+Enter never straddles two batches");

        let long_chord = key_events(Key::ShiftEnter, 40);
        assert!(batches(&long_chord, 3).iter().all(|part| part.len() == 4 && balanced(part)));
        assert!(batches(&[], BATCH_SIZE).is_empty());
    }

    #[test]
    fn partial_injections_release_what_was_pressed() {
        let ctrl_a = chord(VK_CONTROL, VK_A);
        assert_eq!(
            unreleased(&ctrl_a[..2]),
            vec![KeyEvent::up(Stroke::Virtual(VK_A)), KeyEvent::up(Stroke::Virtual(VK_CONTROL))]
        );
        assert_eq!(unreleased(&ctrl_a[..3]), vec![KeyEvent::up(Stroke::Virtual(VK_CONTROL))]);
        assert!(unreleased(&ctrl_a).is_empty());
        assert!(unreleased(&[]).is_empty());
        let typed = text_events("hi");
        assert_eq!(unreleased(&typed[..3]), vec![KeyEvent::up(Stroke::Unicode(u16::from(b'i')))]);
    }

    #[test]
    fn empty_input_is_a_no_op() {
        assert_eq!(type_text(""), Ok(()));
        assert_eq!(press_key(Key::Backspace, 0), Ok(()));
    }
}
