//! Keyboard synthesis with Quartz events.
//!
//! Text is typed as Unicode strings attached to key events, which is
//! independent of the keyboard layout. Only layout-independent keys (Delete,
//! Tab, Escape, Return, arrows) are sent by keycode. Letter shortcuts such as
//! ⌘V are never synthesized: on AZERTY the QWERTY "A" position types "Q", so a
//! synthetic ⌘A could quit the user's application. See `menu.rs` instead.

use std::thread;
use std::time::Duration;

use objc2_core_graphics::{CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation, CGKeyCode};

use mote_core::platform::{Key, PlatformError};

const KEY_DELETE: CGKeyCode = 51;
const KEY_TAB: CGKeyCode = 48;
const KEY_ESCAPE: CGKeyCode = 53;
const KEY_RETURN: CGKeyCode = 36;
const KEY_LEFT: CGKeyCode = 123;
const KEY_RIGHT: CGKeyCode = 124;
/// CGEventKeyboardSetUnicodeString accepts at most 20 UTF-16 units per event.
const MAX_UNITS_PER_EVENT: usize = 20;

fn source() -> Result<objc2_core_foundation::CFRetained<CGEventSource>, PlatformError> {
    CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .ok_or_else(|| PlatformError::Failed("could not create an event source".into()))
}

fn post_key(source: &CGEventSource, key: CGKeyCode, down: bool, flags: CGEventFlags) -> Result<(), PlatformError> {
    let event = CGEvent::new_keyboard_event(Some(source), key, down)
        .ok_or_else(|| PlatformError::Failed("could not create a key event".into()))?;
    CGEvent::set_flags(Some(&event), flags);
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
    Ok(())
}

fn tap(source: &CGEventSource, key: CGKeyCode, flags: CGEventFlags) -> Result<(), PlatformError> {
    post_key(source, key, true, flags)?;
    post_key(source, key, false, flags)
}

/// Splits UTF-16 text into chunks that never break a surrogate pair.
pub fn utf16_chunks(text: &str, max_units: usize) -> Vec<Vec<u16>> {
    let mut chunks = Vec::new();
    let mut current: Vec<u16> = Vec::new();
    for ch in text.chars() {
        let mut buf = [0u16; 2];
        let units = ch.encode_utf16(&mut buf);
        if current.len() + units.len() > max_units && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
        }
        current.extend_from_slice(units);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Types `text` at the caret. Line breaks are sent as Shift+Return.
pub fn type_text(text: &str) -> Result<(), PlatformError> {
    let source = source()?;
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            tap(&source, KEY_RETURN, CGEventFlags::MaskShift)?;
        }
        for chunk in utf16_chunks(&line.replace('\r', ""), MAX_UNITS_PER_EVENT) {
            for down in [true, false] {
                let event = CGEvent::new_keyboard_event(Some(&source), 0, down)
                    .ok_or_else(|| PlatformError::Failed("could not create a key event".into()))?;
                CGEvent::set_flags(Some(&event), CGEventFlags::empty());
                // SAFETY: `chunk` is a live UTF-16 buffer of the given length.
                unsafe { CGEvent::keyboard_set_unicode_string(Some(&event), chunk.len() as _, chunk.as_ptr()) };
                CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
            }
            // Give the target application time to process each chunk.
            thread::sleep(Duration::from_millis(2));
        }
    }
    Ok(())
}

/// Presses a layout-independent key `count` times.
pub fn press(key: Key, count: usize) -> Result<(), PlatformError> {
    let source = source()?;
    let (code, flags) = match key {
        Key::Backspace => (KEY_DELETE, CGEventFlags::empty()),
        Key::Tab => (KEY_TAB, CGEventFlags::empty()),
        Key::Escape => (KEY_ESCAPE, CGEventFlags::empty()),
        Key::ShiftEnter => (KEY_RETURN, CGEventFlags::MaskShift),
        Key::Left => (KEY_LEFT, CGEventFlags::empty()),
        Key::Right => (KEY_RIGHT, CGEventFlags::empty()),
    };
    for i in 0..count {
        tap(&source, code, flags)?;
        if i % 20 == 19 {
            thread::sleep(Duration::from_millis(2));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_respect_limits_and_surrogate_pairs() {
        let chunks = utf16_chunks(&"a".repeat(45), 20);
        assert_eq!(chunks.iter().map(Vec::len).collect::<Vec<_>>(), vec![20, 20, 5]);
        let emoji = "😀".repeat(11); // 22 units, pairs of 2
        let chunks = utf16_chunks(&emoji, 20);
        assert_eq!(chunks.iter().map(Vec::len).collect::<Vec<_>>(), vec![20, 2]);
        for chunk in chunks {
            assert!(String::from_utf16(&chunk).is_ok(), "no split pairs");
        }
        assert!(utf16_chunks("", 20).is_empty());
    }
}
