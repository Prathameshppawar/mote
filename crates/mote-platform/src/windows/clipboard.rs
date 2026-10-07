//! The Windows clipboard, as plain text.
//!
//! Content that the copying application asks clipboard monitors to ignore
//! (`ExcludeClipboardContentFromMonitorProcessing`, `Clipboard Viewer Ignore`,
//! or `CanIncludeInClipboardHistory` set to 0, as password managers do) is
//! never read. Text Mote writes only to paste it carries the same markers, so
//! clipboard history (Win+V) and cloud clipboard do not record it.

use std::ffi::c_void;
use std::mem::size_of;
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use ::windows::core::{w, PCWSTR};
use ::windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use ::windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable,
    OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use ::windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use ::windows::Win32::System::Ole::{CF_BITMAP, CF_DIB, CF_DIBV5, CF_ENHMETAFILE, CF_HDROP, CF_UNICODETEXT};
use mote_core::platform::PlatformError;

use super::bounded_utf16;

/// Another application may hold the clipboard open for a moment.
const OPEN_ATTEMPTS: usize = 5;
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(10);

/// Serializes Mote's own clipboard access across threads.
static CLIPBOARD_LOCK: Mutex<()> = Mutex::new(());

fn failed(message: &str) -> PlatformError {
    PlatformError::Failed(message.into())
}

/// Changes whenever the clipboard changes; never reads content.
pub(super) fn sequence() -> u64 {
    // SAFETY: no arguments.
    u64::from(unsafe { GetClipboardSequenceNumber() })
}

/// Whether data in `format` is on the clipboard (does not open it).
fn format_available(format: u32) -> bool {
    // SAFETY: plain query without pointers.
    unsafe { IsClipboardFormatAvailable(format) }.is_ok()
}

/// The id of a registered clipboard format (0 if registration failed).
fn registered(name: PCWSTR) -> u32 {
    // SAFETY: `name` is a NUL-terminated static string; registering returns the existing id.
    unsafe { RegisterClipboardFormatW(name) }
}

/// Whether the copying application asked clipboard monitors to ignore its
/// content. Does not open the clipboard.
fn excluded_from_monitors() -> bool {
    [w!("ExcludeClipboardContentFromMonitorProcessing"), w!("Clipboard Viewer Ignore")]
        .into_iter()
        .map(registered)
        .any(|format| format != 0 && format_available(format))
}

/// Whether `CanIncludeInClipboardHistory` is present and set to 0. The
/// clipboard must be open on this thread. A value that cannot be read counts
/// as excluded.
fn excluded_from_history() -> bool {
    let format = registered(w!("CanIncludeInClipboardHistory"));
    if format == 0 || !format_available(format) {
        return false;
    }
    // SAFETY: the clipboard is open on this thread; the returned handle stays owned by the clipboard.
    let Ok(handle) = (unsafe { GetClipboardData(format) }) else { return true };
    let memory = HGLOBAL(handle.0);
    let Some(locked) = LockedGlobal::lock(memory) else { return true };
    // SAFETY: plain query on a valid memory handle.
    if unsafe { GlobalSize(memory) } < size_of::<u32>() {
        return true;
    }
    // SAFETY: the locked block holds at least four bytes; `read_unaligned` has no alignment requirement.
    let value = unsafe { locked.data.cast::<u32>().read_unaligned() };
    value == 0
}

/// The clipboard, open on this thread until dropped.
struct ClipboardSession;

impl ClipboardSession {
    fn open() -> Result<Self, PlatformError> {
        for attempt in 0..OPEN_ATTEMPTS {
            if attempt > 0 {
                thread::sleep(OPEN_RETRY_DELAY);
            }
            // SAFETY: no pointers; a None owner associates the clipboard with this task.
            if unsafe { OpenClipboard(None) }.is_ok() {
                return Ok(Self);
            }
        }
        Err(failed("the clipboard is in use by another application"))
    }
}

impl Drop for ClipboardSession {
    fn drop(&mut self) {
        // SAFETY: the clipboard was opened by this thread in `open`.
        let _ = unsafe { CloseClipboard() };
    }
}

/// A global memory block, locked until dropped.
struct LockedGlobal {
    memory: HGLOBAL,
    data: *mut c_void,
}

impl LockedGlobal {
    fn lock(memory: HGLOBAL) -> Option<Self> {
        // SAFETY: `memory` is a valid global memory handle (from the clipboard or GlobalAlloc).
        let data = unsafe { GlobalLock(memory) };
        (!data.is_null()).then_some(Self { memory, data })
    }
}

impl Drop for LockedGlobal {
    fn drop(&mut self) {
        // SAFETY: balances the successful GlobalLock in `lock`. GlobalUnlock reports an "error"
        // with no error code once the lock count reaches zero, so the result is ignored.
        let _ = unsafe { GlobalUnlock(self.memory) };
    }
}

/// A global memory block we own, freed on drop unless handed to the clipboard.
struct OwnedGlobal(Option<HGLOBAL>);

impl OwnedGlobal {
    /// Gives up ownership (the clipboard took the block).
    fn release(&mut self) {
        self.0 = None;
    }

    /// A block holding one DWORD, the data of the clipboard marker formats.
    fn dword(value: u32) -> Result<Self, PlatformError> {
        // SAFETY: plain allocation; owned by the returned guard until the clipboard takes it.
        let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, size_of::<u32>()) }
            .map_err(|_| failed("could not allocate clipboard memory"))?;
        let owned = Self(Some(memory));
        let locked = LockedGlobal::lock(memory).ok_or_else(|| failed("could not allocate clipboard memory"))?;
        // SAFETY: the locked block holds at least four bytes; `write_unaligned` has no alignment requirement.
        unsafe { locked.data.cast::<u32>().write_unaligned(value) };
        drop(locked);
        Ok(owned)
    }
}

impl Drop for OwnedGlobal {
    fn drop(&mut self) {
        if let Some(memory) = self.0.take() {
            // SAFETY: we allocated the block and still own it; it is freed exactly once.
            let _ = unsafe { GlobalFree(Some(memory)) };
        }
    }
}

/// Current clipboard text, at most `max_chars` characters. `Ok(None)` when the
/// clipboard holds no text, or text its owner asked monitors to ignore.
pub(super) fn text(max_chars: usize) -> Result<Option<String>, PlatformError> {
    let format = u32::from(CF_UNICODETEXT.0);
    if max_chars == 0 || !format_available(format) || excluded_from_monitors() {
        return Ok(None);
    }
    let _lock = CLIPBOARD_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let _session = ClipboardSession::open()?;
    if excluded_from_history() {
        return Ok(None);
    }
    // SAFETY: the clipboard is open on this thread; the returned handle stays owned by the clipboard.
    let handle = unsafe { GetClipboardData(format) }.map_err(|_| failed("could not read the clipboard"))?;
    let memory = HGLOBAL(handle.0);
    let locked = LockedGlobal::lock(memory).ok_or_else(|| failed("could not read the clipboard"))?;
    // SAFETY: plain query on a valid memory handle.
    let size = unsafe { GlobalSize(memory) };
    let units = locked.data.cast::<u16>();
    if !units.is_aligned() {
        return Err(failed("could not read the clipboard"));
    }
    // Only the first `max_chars` characters are needed: at most two UTF-16 units each.
    let readable = (size / size_of::<u16>()).min(max_chars.saturating_mul(2).saturating_add(1));
    // SAFETY: the locked block holds `size` bytes, so `readable <= size / 2` aligned u16 values
    // can be read while `locked` is alive.
    let units = unsafe { std::slice::from_raw_parts(units.cast_const(), readable) };
    let end = units.iter().position(|&unit| unit == 0).unwrap_or(units.len());
    let text = bounded_utf16(&units[..end], max_chars);
    drop(locked);
    Ok((!text.is_empty()).then_some(text))
}

/// Whether the clipboard holds images, files or rich content that a plain-text
/// round trip would lose.
pub(super) fn has_non_text() -> bool {
    let standard = [CF_BITMAP, CF_DIB, CF_DIBV5, CF_HDROP, CF_ENHMETAFILE].map(|format| u32::from(format.0));
    let registered = [w!("PNG"), w!("HTML Format"), w!("Rich Text Format")].map(|name| {
        // SAFETY: `name` is a NUL-terminated static string; registering returns the existing id.
        unsafe { RegisterClipboardFormatW(name) }
    });
    standard.into_iter().chain(registered.into_iter().filter(|&id| id != 0)).any(format_available)
}

/// Replaces the clipboard with `text`; returns the new sequence number. A
/// `transient` write is marked so clipboard history, cloud clipboard and
/// monitors ignore it.
pub(super) fn set_text(text: &str, transient: bool) -> Result<u64, PlatformError> {
    let units: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = units.len() * size_of::<u16>();
    // SAFETY: plain allocation; the block is owned by `owned` until the clipboard takes it.
    let memory =
        unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) }.map_err(|_| failed("could not allocate clipboard memory"))?;
    let mut owned = OwnedGlobal(Some(memory));
    {
        let locked = LockedGlobal::lock(memory).ok_or_else(|| failed("could not allocate clipboard memory"))?;
        let destination = locked.data.cast::<u16>();
        if !destination.is_aligned() {
            return Err(failed("could not allocate clipboard memory"));
        }
        // SAFETY: the locked block holds at least `bytes` bytes (GlobalAlloc) and is aligned for
        // u16; it cannot overlap `units`, a separate heap allocation.
        unsafe { std::ptr::copy_nonoverlapping(units.as_ptr(), destination, units.len()) };
    }
    // Allocated before opening the clipboard, which other applications wait on.
    let mut markers = Vec::new();
    if transient {
        for name in [
            w!("ExcludeClipboardContentFromMonitorProcessing"),
            w!("CanIncludeInClipboardHistory"),
            w!("CanUploadToCloudClipboard"),
        ] {
            let format = registered(name);
            if format != 0 {
                markers.push((format, OwnedGlobal::dword(0)?));
            }
        }
    }

    let _lock = CLIPBOARD_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let session = ClipboardSession::open()?;
    // SAFETY: the clipboard is open on this thread.
    unsafe { EmptyClipboard() }.map_err(|_| failed("could not clear the clipboard"))?;
    // SAFETY: the clipboard is open on this thread and emptied; `memory` is an unlocked
    // GMEM_MOVEABLE block holding NUL-terminated UTF-16. On success the system owns it.
    unsafe { SetClipboardData(u32::from(CF_UNICODETEXT.0), Some(HANDLE(memory.0))) }
        .map_err(|_| failed("could not write the clipboard"))?;
    owned.release();
    for (format, marker) in &mut markers {
        let Some(block) = marker.0 else { continue };
        // SAFETY: the clipboard is open on this thread; `block` is an unlocked GMEM_MOVEABLE
        // block holding one DWORD. On success the system owns it. Markers are best effort:
        // the text is on the clipboard either way.
        if unsafe { SetClipboardData(*format, Some(HANDLE(block.0))) }.is_ok() {
            marker.release();
        }
    }
    drop(session);
    Ok(sequence())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test touches the system clipboard (tests run in parallel), and puts
    /// back whatever text was there before.
    #[test]
    fn clipboard_text_round_trips_and_changes_the_sequence() {
        let saved = text(usize::MAX).ok().flatten();

        let before = sequence();
        let sample = "Mote ✓ clipboard test: नमस्ते 😀\nsecond line";
        let first = set_text(sample, false).expect("clipboard is writable");
        assert_ne!(first, before, "writing changes the sequence");
        assert_eq!(sequence(), first);
        assert_eq!(text(10_000).expect("readable").as_deref(), Some(sample));
        assert_eq!(text(4).expect("readable").as_deref(), Some("Mote"), "bounded to max chars");
        assert_eq!(text(0).expect("readable"), None);
        assert!(!has_non_text(), "plain text only");

        let second = set_text("another value", false).expect("clipboard is writable");
        assert_ne!(second, first);
        assert_eq!(text(100).expect("readable").as_deref(), Some("another value"));

        let third = set_text("paste buffer", true).expect("clipboard is writable");
        assert_ne!(third, second);
        assert!(excluded_from_monitors(), "transient writes carry the monitor-exclusion marker");
        assert_eq!(text(100).expect("readable"), None, "transient writes are not read back");
        assert!(!has_non_text(), "markers are not content");

        set_text("restored", false).expect("clipboard is writable");
        assert!(!excluded_from_monitors(), "a normal write clears the markers");
        assert_eq!(text(100).expect("readable").as_deref(), Some("restored"));

        set_text("", false).expect("clipboard is writable");
        assert_eq!(text(100).expect("readable"), None, "empty text reads as no text");

        if let Some(saved) = saved {
            let _ = set_text(&saved, false);
        }
    }
}
