//! Foreground window, processes and application metadata (Win32).

use std::collections::HashMap;
use std::mem::size_of;
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use ::windows::core::{BOOL, PCWSTR, PWSTR};
use ::windows::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, HANDLE, HWND, LPARAM, RECT, TRUE};
use ::windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use ::windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use ::windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumChildWindows, EnumWindows, GetClassNameW, GetForegroundWindow, GetWindow, GetWindowLongW,
    GetWindowRect, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsHungAppWindow, IsIconic,
    IsWindowVisible, SetForegroundWindow, ShowWindow, GWL_EXSTYLE, GW_OWNER, SW_RESTORE, SW_SHOW, WS_EX_APPWINDOW,
    WS_EX_TOOLWINDOW,
};
use mote_core::platform::{AppInfo, Rect};
use mote_core::text::head_chars;

use super::bounded_utf16;

/// Top-level window class of the frame that hosts UWP applications.
const FRAME_HOST_CLASS: &str = "ApplicationFrameWindow";
/// Shell processes that host other applications' windows.
const SHELL_HOSTS: &[&str] = &["applicationframehost.exe"];
/// Display names are labels: keep them short.
const NAME_MAX_CHARS: usize = 128;
/// Version resources larger than this are not parsed.
const MAX_VERSION_INFO_BYTES: u32 = 4 * 1024 * 1024;
/// Executable paths are cached; the cache is reset when it grows past this.
const MAX_CACHED_PATHS: usize = 512;
/// Pause after unlocking the foreground, so the system processes the input first.
const FOREGROUND_SETTLE: Duration = Duration::from_millis(30);

/// The foreground window.
pub(super) struct Foreground {
    pub(super) hwnd: HWND,
    /// The process that owns the window's content (the UWP app rather than its frame host).
    pub(super) pid: u32,
    pub(super) thread_id: u32,
    pub(super) class_name: String,
}

impl Foreground {
    /// Whether the window has stopped responding to messages.
    pub(super) fn is_hung(&self) -> bool {
        // SAFETY: plain query on a window handle; a stale handle yields FALSE.
        unsafe { IsHungAppWindow(self.hwnd) }.as_bool()
    }
}

/// The current foreground window, if any.
pub(super) fn foreground() -> Option<Foreground> {
    // SAFETY: no arguments; the result may be null.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return None;
    }
    let (pid, thread_id) = window_process(hwnd)?;
    let class_name = class_name(hwnd);
    let pid = content_process(hwnd, pid, &class_name);
    Some(Foreground { hwnd, pid, thread_id, class_name })
}

/// Process and thread that created `hwnd`.
fn window_process(hwnd: HWND) -> Option<(u32, u32)> {
    let mut pid = 0u32;
    // SAFETY: `pid` is a valid out-pointer for the duration of the call.
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    (thread_id != 0 && pid != 0).then_some((pid, thread_id))
}

fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    // SAFETY: `buffer` is writable; its length is passed with the slice.
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) };
    let len = usize::try_from(len).unwrap_or(0).min(buffer.len());
    String::from_utf16_lossy(&buffer[..len])
}

/// UWP applications run inside an `ApplicationFrameHost.exe` frame; their own
/// process owns a child `CoreWindow`. Returns that process when present.
fn content_process(hwnd: HWND, pid: u32, class_name: &str) -> u32 {
    if class_name != FRAME_HOST_CLASS {
        return pid;
    }
    child_windows(hwnd)
        .into_iter()
        .filter_map(window_process)
        .map(|(child, _)| child)
        .find(|&child| child != pid)
        .unwrap_or(pid)
}

/// Appends each enumerated window to the `Vec<HWND>` whose address is in `lparam`.
///
/// # Safety
///
/// Only for `EnumWindows`/`EnumChildWindows`, with `lparam` holding the address
/// of a live `Vec<HWND>` that nothing else accesses during the enumeration.
unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` carries the address of a `Vec<HWND>` owned by the caller of the
    // (synchronous) enumeration, which outlives it and is not otherwise accessed meanwhile.
    let windows = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
    windows.push(hwnd);
    TRUE
}

/// Top-level windows in Z order (frontmost first).
fn top_level_windows() -> Vec<HWND> {
    let mut windows: Vec<HWND> = Vec::new();
    // SAFETY: the callback only appends to `windows`, which lives until EnumWindows returns.
    let _ = unsafe { EnumWindows(Some(collect_window), LPARAM(&mut windows as *mut Vec<HWND> as isize)) };
    windows
}

fn child_windows(parent: HWND) -> Vec<HWND> {
    let mut windows: Vec<HWND> = Vec::new();
    // SAFETY: the callback only appends to `windows`, which lives until EnumChildWindows returns.
    let _ = unsafe {
        EnumChildWindows(Some(parent), Some(collect_window), LPARAM(&mut windows as *mut Vec<HWND> as isize))
    };
    windows
}

/// Whether DWM hides the window (suspended UWP frames, other virtual desktops).
fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    // SAFETY: `cloaked` is a writable u32 and its size is passed along.
    let queried = unsafe {
        DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&mut cloaked as *mut u32).cast(), size_of::<u32>() as u32)
    };
    queried.is_ok() && cloaked != 0
}

/// Whether `hwnd` is an application's main window: visible, not owned by
/// another window, not a tool window, not cloaked and (optionally) titled.
fn is_app_window(hwnd: HWND, require_title: bool) -> bool {
    // SAFETY: plain queries on a window handle from EnumWindows; stale handles yield FALSE, 0 or errors.
    let shown = unsafe {
        let visible = IsWindowVisible(hwnd).as_bool();
        let owned = GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid());
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let tool = (ex_style & WS_EX_TOOLWINDOW.0) != 0 && (ex_style & WS_EX_APPWINDOW.0) == 0;
        let titled = !require_title || GetWindowTextLengthW(hwnd) > 0;
        visible && !owned && !tool && titled
    };
    shown && !is_cloaked(hwnd)
}

/// Main windows of running applications, frontmost first, with the process
/// that owns their content.
pub(super) fn app_windows() -> Vec<(HWND, u32)> {
    top_level_windows()
        .into_iter()
        .filter(|&hwnd| is_app_window(hwnd, true))
        .filter_map(|hwnd| {
            let (pid, _) = window_process(hwnd)?;
            Some((hwnd, content_process(hwnd, pid, &class_name(hwnd))))
        })
        .collect()
}

/// The frontmost main window whose content process satisfies `matches`.
pub(super) fn app_window(matches: impl Fn(u32) -> bool) -> Option<(HWND, u32)> {
    top_level_windows().into_iter().filter(|&hwnd| is_app_window(hwnd, false)).find_map(|hwnd| {
        let (pid, _) = window_process(hwnd)?;
        let pid = content_process(hwnd, pid, &class_name(hwnd));
        matches(pid).then_some((hwnd, pid))
    })
}

/// Whether `app_id` is a shell process hosting other applications' windows.
pub(super) fn is_shell_host(app_id: &str) -> bool {
    SHELL_HOSTS.contains(&app_id)
}

/// The window's title, at most `max_chars` characters.
pub(super) fn window_title(hwnd: HWND, max_chars: usize) -> Option<String> {
    // SAFETY: plain query on a window handle.
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    let len = usize::try_from(len).ok().filter(|&len| len > 0)?.min(max_chars.saturating_mul(2));
    let mut buffer = vec![0u16; len + 1];
    // SAFETY: `buffer` is writable; its length is passed with the slice.
    let copied = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    let copied = usize::try_from(copied).unwrap_or(0).min(len);
    let title = bounded_utf16(&buffer[..copied], max_chars);
    let title = title.trim();
    (!title.is_empty()).then(|| title.to_string())
}

/// The window's bounds in physical pixels.
pub(super) fn window_rect(hwnd: HWND) -> Option<Rect> {
    let mut rect = RECT::default();
    // SAFETY: `rect` is a valid out-pointer.
    unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
    let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
    (width > 0 && height > 0).then(|| Rect {
        x: f64::from(rect.left),
        y: f64::from(rect.top),
        width: f64::from(width),
        height: f64::from(height),
    })
}

/// Brings `hwnd` (a main window of process `pid`) to the foreground.
pub(super) fn bring_to_front(hwnd: HWND, pid: u32) -> bool {
    if foreground_is(pid) {
        return true;
    }
    // SAFETY: plain window-management calls on a top-level window handle.
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
    }
    if set_foreground(hwnd, pid) {
        return true;
    }
    // Windows lets a process take the foreground after an Alt key press (the
    // documented SetForegroundWindow exception). The tap is masked so it does
    // not activate any window's menu bar.
    super::input::tap_alt();
    thread::sleep(FOREGROUND_SETTLE);
    if set_foreground(hwnd, pid) {
        return true;
    }
    // SAFETY: plain window-management calls on a top-level window handle.
    unsafe {
        let _ = ShowWindow(hwnd, if IsIconic(hwnd).as_bool() { SW_RESTORE } else { SW_SHOW });
        let _ = BringWindowToTop(hwnd);
    }
    set_foreground(hwnd, pid)
}

fn set_foreground(hwnd: HWND, pid: u32) -> bool {
    // SAFETY: plain call on a window handle.
    unsafe { SetForegroundWindow(hwnd) }.as_bool() || foreground_is(pid)
}

fn foreground_is(pid: u32) -> bool {
    foreground().is_some_and(|window| window.pid == pid)
}

/// Closes a process handle when dropped.
struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by us and is closed exactly once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Full path of the executable of process `pid`.
fn image_path(pid: u32) -> Option<String> {
    // SAFETY: plain call; the handle is closed by `OwnedHandle`.
    let process = OwnedHandle(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?);
    for capacity in [512usize, 32_768] {
        let mut buffer = vec![0u16; capacity];
        let mut len = capacity as u32;
        // SAFETY: `buffer` is writable for `len` units; `len` receives the length written.
        let queried =
            unsafe { QueryFullProcessImageNameW(process.0, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut len) };
        match queried {
            Ok(()) => {
                let len = usize::try_from(len).unwrap_or(0).min(capacity);
                return (len > 0).then(|| String::from_utf16_lossy(&buffer[..len]));
            }
            Err(error) if error.code() == ERROR_INSUFFICIENT_BUFFER.to_hresult() => continue,
            Err(_) => return None,
        }
    }
    None
}

/// The file name of a Windows path.
fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Stable application id: the lowercase executable file name (`slack.exe`).
pub(super) fn app_id(path: &str) -> String {
    file_name(path).to_lowercase()
}

/// Display name when the executable has no description: its file stem (`Slack`).
pub(super) fn fallback_name(path: &str) -> String {
    let file = file_name(path);
    match file.rfind('.') {
        Some(dot) if dot > 0 => file[..dot].to_string(),
        _ => file.to_string(),
    }
}

/// Bytes of the version-resource value at `sub_block`, located inside `block`.
/// `unit_bytes` is the size of the unit VerQueryValueW reports the length in:
/// 1 for binary values (`\VarFileInfo\Translation`), 2 for strings (characters).
fn version_value<'a>(block: &'a mut [u8], sub_block: &str, unit_bytes: usize) -> Option<&'a [u8]> {
    let sub_block: Vec<u16> = sub_block.encode_utf16().chain(std::iter::once(0)).collect();
    let mut value: *mut std::ffi::c_void = std::ptr::null_mut();
    let mut len = 0u32;
    // SAFETY: `block` holds a version resource from GetFileVersionInfoW (VerQueryValueW may
    // write converted strings into its reserved tail, hence the mutable pointer); `sub_block`
    // is NUL-terminated; the out-pointers are valid.
    let found = unsafe {
        VerQueryValueW(block.as_mut_ptr().cast_const().cast(), PCWSTR(sub_block.as_ptr()), &mut value, &mut len)
    };
    if !found.as_bool() || value.is_null() {
        return None;
    }
    // The value points into `block`: turn it into a bounds-checked sub-slice.
    let offset = (value as usize).checked_sub(block.as_ptr() as usize)?;
    let bytes = usize::try_from(len).ok()?.checked_mul(unit_bytes)?;
    let end = offset.checked_add(bytes)?.min(block.len());
    block.get(offset..end)
}

/// The executable's `FileDescription` (or `ProductName`) from its version resource.
fn file_description(path: &str) -> Option<String> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call.
    let size = unsafe { GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None) };
    if size == 0 || size > MAX_VERSION_INFO_BYTES {
        return None;
    }
    let mut block = vec![0u8; usize::try_from(size).ok()?];
    // SAFETY: `block` is writable for `size` bytes; `wide` is NUL-terminated.
    unsafe { GetFileVersionInfoW(PCWSTR(wide.as_ptr()), None, size, block.as_mut_ptr().cast()) }.ok()?;

    let mut languages: Vec<(u16, u16)> = version_value(&mut block, "\\VarFileInfo\\Translation", 1)
        .map(|bytes| {
            bytes
                .chunks_exact(4)
                .map(|entry| (u16::from_le_bytes([entry[0], entry[1]]), u16::from_le_bytes([entry[2], entry[3]])))
                .collect()
        })
        .unwrap_or_default();
    // Common fallbacks: US English with the Unicode and Windows-1252 code pages, and language-neutral.
    languages.extend([(0x0409, 0x04B0), (0x0409, 0x04E4), (0x0000, 0x04B0)]);

    for key in ["FileDescription", "ProductName"] {
        for &(language, codepage) in &languages {
            let sub_block = format!("\\StringFileInfo\\{language:04x}{codepage:04x}\\{key}");
            let Some(bytes) = version_value(&mut block, &sub_block, 2) else { continue };
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
                .take_while(|&u| u != 0)
                .collect();
            let name = String::from_utf16_lossy(&units);
            let name = head_chars(name.trim(), NAME_MAX_CHARS).trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Application ids and names by executable path. Looking up a name reads the
/// executable's version resource, which is too slow for every observer tick.
#[derive(Default)]
pub(super) struct AppCache {
    by_path: Mutex<HashMap<String, (String, String)>>,
}

impl AppCache {
    /// The application of process `pid`, or `None` when it cannot be identified
    /// (the process exited or denies access).
    pub(super) fn app_for_pid(&self, pid: u32) -> Option<AppInfo> {
        let path = image_path(pid)?;
        let (id, name) = self.describe(&path);
        Some(AppInfo { id, name, pid: Some(pid) })
    }

    fn describe(&self, path: &str) -> (String, String) {
        if let Some(known) = self.by_path.lock().unwrap_or_else(PoisonError::into_inner).get(path) {
            return known.clone();
        }
        // Read outside the lock: version resources come from disk.
        let id = app_id(path);
        let name = file_description(path).unwrap_or_else(|| fallback_name(path));
        let mut cache = self.by_path.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.len() >= MAX_CACHED_PATHS {
            cache.clear();
        }
        cache.insert(path.to_string(), (id.clone(), name.clone()));
        (id, name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_fallback_names_come_from_the_executable_path() {
        let slack = r"C:\Users\me\AppData\Local\slack\app-4.41.105\slack.exe";
        assert_eq!(app_id(slack), "slack.exe");
        assert_eq!(fallback_name(slack), "slack");
        let code = r"C:\Program Files\Microsoft VS Code\Code.EXE";
        assert_eq!(app_id(code), "code.exe");
        assert_eq!(fallback_name(code), "Code");
        assert_eq!(fallback_name(r"C:\tools.d\run"), "run");
        assert_eq!(fallback_name(r"C:\x\.hidden"), ".hidden");
        assert_eq!(app_id("plain.exe"), "plain.exe");
        assert_eq!(fallback_name("C:/mixed/separators/app.v2.exe"), "app.v2");
    }

    #[test]
    fn system_binaries_have_descriptions() {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let description = file_description(&format!(r"{root}\System32\kernel32.dll"));
        assert!(description.is_some_and(|name| !name.is_empty() && name.chars().count() <= NAME_MAX_CHARS));
        assert_eq!(file_description(r"C:\definitely\missing\nothing.exe"), None);
    }

    #[test]
    fn the_current_process_is_identified_and_cached() {
        let cache = AppCache::default();
        let app = cache.app_for_pid(std::process::id()).expect("own process is accessible");
        assert!(app.id.ends_with(".exe"), "{}", app.id);
        assert_eq!(app.id, app.id.to_lowercase());
        assert!(!app.name.is_empty());
        assert_eq!(app.pid, Some(std::process::id()));
        assert_eq!(cache.by_path.lock().map(|paths| paths.len()).unwrap_or_default(), 1);
        assert_eq!(cache.app_for_pid(std::process::id()), Some(app), "cached lookup is identical");
        assert_eq!(cache.app_for_pid(0), None, "the idle process cannot be opened");
    }

    #[test]
    fn shell_hosts_are_recognized() {
        assert!(is_shell_host("applicationframehost.exe"));
        assert!(!is_shell_host("notepad.exe"));
    }

    #[test]
    fn window_enumeration_does_not_fail() {
        for (hwnd, pid) in app_windows() {
            assert!(!hwnd.is_invalid());
            assert_ne!(pid, 0);
        }
    }
}
