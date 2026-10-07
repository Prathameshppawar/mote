//! The focused text input, read through UI Automation.
//!
//! Text is read only around the caret, by moving the endpoints of clones of
//! the selection range and fetching at most [`ReadLimits`] characters on each
//! side; the whole document is never fetched. Password fields are reported as
//! secure without reading anything from them.

use ::windows::core::{Error, Interface, Result, BOOL, BSTR, HRESULT};
use ::windows::Win32::Foundation::{
    CO_E_OBJNOTCONNECTED, E_ACCESSDENIED, HWND, POINT, RPC_E_DISCONNECTED, RPC_E_SERVER_DIED, RPC_E_SERVER_DIED_DNE,
};
use ::windows::Win32::Graphics::Gdi::ClientToScreen;
use ::windows::Win32::System::Variant::{VariantClear, VARIANT, VT_BOOL, VT_I4, VT_R8};
use ::windows::Win32::UI::Accessibility::{
    IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern, IUIAutomationTextPattern2, IUIAutomationTextRange,
    IUIAutomationValuePattern, TextPatternRangeEndpoint, TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start,
    TextUnit_Character, UIA_ComboBoxControlTypeId, UIA_DocumentControlTypeId, UIA_EditControlTypeId,
    UIA_IsReadOnlyAttributeId, UIA_TextPatternId, UIA_ValuePatternId, UIA_CONTROLTYPE_ID, UIA_E_ELEMENTNOTAVAILABLE,
    UIA_E_TIMEOUT,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    GetGUIThreadInfo, GetWindowLongW, ES_MULTILINE, GUITHREADINFO, GWL_STYLE,
};
use mote_core::platform::{AppInfo, FocusedInput, InputRole, ReadLimits, Rect};
use mote_core::text::{head_chars, tail_chars};

use super::com::SafeArray;
use super::{bounded_utf16, tail_units};
use crate::common::element_key;

/// Accessible names are labels, not content: keep them short.
const LABEL_MAX_CHARS: usize = 120;
const PLACEHOLDER_MAX_CHARS: usize = 200;
/// Class names, framework and automation ids, localized control types.
const META_MAX_CHARS: usize = 128;
/// Height of the caret drawn at the bottom-left of a field whose caret is unknown.
const FALLBACK_CARET_HEIGHT: f64 = 18.0;
/// An edit at least this many lines tall is treated as multi-line.
const MULTILINE_LINES: f64 = 2.5;

/// Window classes of terminal emulators (focused element or foreground window).
const TERMINAL_CLASSES: &[&str] = &[
    "ConsoleWindowClass",
    "CASCADIA_HOSTING_WINDOW_CLASS",
    "TermControl",
    "PseudoConsoleWindow",
    "VirtualConsoleClass",
    "mintty",
    "PuTTY",
    "org.wezfurlong.wezterm",
];

/// Executables of terminal emulators and shells (lowercase).
const TERMINAL_EXECUTABLES: &[&str] = &[
    "windowsterminal.exe",
    "wt.exe",
    "openconsole.exe",
    "conhost.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "wsl.exe",
    "bash.exe",
    "wezterm-gui.exe",
    "alacritty.exe",
    "mintty.exe",
    "putty.exe",
    "kitty.exe",
    "hyper.exe",
    "tabby.exe",
    "conemu.exe",
    "conemu64.exe",
    "mobaxterm.exe",
    "warp.exe",
    "rio.exe",
    "ghostty.exe",
];

/// The foreground application whose focused element is read.
pub(super) struct Target {
    pub(super) app: AppInfo,
    pub(super) pid: u32,
    pub(super) window_class: String,
    pub(super) thread_id: u32,
}

/// Whether `error` is a UI Automation timeout (the application is not responding).
pub(super) fn is_timeout(error: &Error) -> bool {
    hresult_is(error, UIA_E_TIMEOUT)
}

fn hresult_is(error: &Error, code: u32) -> bool {
    error.code() == HRESULT(code as i32)
}

/// Errors after which further calls are pointless: the application is hung,
/// gone, or refuses access (for example because it runs elevated).
fn is_fatal(error: &Error) -> bool {
    /// HRESULT_FROM_WIN32(RPC_S_SERVER_UNAVAILABLE) and HRESULT_FROM_WIN32(RPC_S_CALL_FAILED).
    const RPC_SERVER_UNAVAILABLE: u32 = 0x8007_06BA;
    const RPC_CALL_FAILED: u32 = 0x8007_06BE;
    [UIA_E_TIMEOUT, UIA_E_ELEMENTNOTAVAILABLE, RPC_SERVER_UNAVAILABLE, RPC_CALL_FAILED]
        .into_iter()
        .any(|code| hresult_is(error, code))
        || [E_ACCESSDENIED, RPC_E_DISCONNECTED, RPC_E_SERVER_DIED, RPC_E_SERVER_DIED_DNE, CO_E_OBJNOTCONNECTED]
            .contains(&error.code())
}

/// Treats a failed optional call as "not available", unless the failure
/// means the application is unresponsive or gone, which aborts the read.
fn soft<T>(result: Result<T>) -> Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if is_fatal(&error) => Err(error),
        Err(_) => Ok(None),
    }
}

/// The kind of control, from its UIA control type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Edit,
    ComboBox,
    Document,
    /// Any other control type; qualifies only as an editable text surface.
    Other,
}

impl Kind {
    fn of(control_type: UIA_CONTROLTYPE_ID) -> Self {
        if control_type == UIA_EditControlTypeId {
            Self::Edit
        } else if control_type == UIA_ComboBoxControlTypeId {
            Self::ComboBox
        } else if control_type == UIA_DocumentControlTypeId {
            Self::Document
        } else {
            Self::Other
        }
    }
}

/// Maps the control kind and hints to an [`InputRole`].
fn role_for(kind: Kind, terminal: bool, search: bool, multiline: bool) -> InputRole {
    if terminal {
        return InputRole::Terminal;
    }
    match kind {
        Kind::Edit | Kind::ComboBox if search => InputRole::SearchField,
        Kind::Edit if multiline => InputRole::TextArea,
        Kind::Edit => InputRole::TextField,
        Kind::ComboBox => InputRole::ComboBox,
        Kind::Document | Kind::Other => InputRole::Document,
    }
}

/// Whether the element accepts typing, given what the provider says about
/// read-only state (`None` when it does not say). Focusable text that is not
/// positively editable is a document being read (a web page, a viewer), not a
/// field being typed in; terminals accept input regardless.
fn is_editable(kind: Kind, terminal: bool, web: bool, read_only: Option<bool>) -> bool {
    match kind {
        _ if terminal => true,
        Kind::Other => read_only == Some(false),
        Kind::Document if web => read_only == Some(false),
        Kind::Edit | Kind::ComboBox | Kind::Document => read_only != Some(true),
    }
}

/// Whether the focused element belongs to a terminal emulator.
fn is_terminal(element_class: &str, window_class: &str, app_id: &str) -> bool {
    TERMINAL_CLASSES
        .iter()
        .any(|class| class.eq_ignore_ascii_case(element_class) || class.eq_ignore_ascii_case(window_class))
        || TERMINAL_EXECUTABLES.contains(&app_id)
}

/// Whether the field is a search box, judged by its localized control type,
/// automation id and accessible name.
fn looks_like_search(localized_control_type: &str, automation_id: &str, name: &str) -> bool {
    localized_control_type.to_lowercase().contains("search")
        || automation_id.to_lowercase().contains("search")
        || name.to_lowercase().split(|c: char| !c.is_alphanumeric()).any(|word| word.starts_with("search"))
}

/// Chromium (browsers, Electron, WebView2) and Gecko report these framework ids.
fn is_web_framework(framework_id: &str) -> bool {
    framework_id.eq_ignore_ascii_case("Chrome") || framework_id.eq_ignore_ascii_case("Gecko")
}

/// Win32 edit controls (and their WinForms and RichEdit relatives) carry ES_MULTILINE.
fn is_win32_edit_class(class_name: &str) -> bool {
    let class = class_name.to_ascii_lowercase();
    class == "edit" || class.starts_with("richedit") || (class.starts_with("windowsforms") && class.contains(".edit."))
}

/// Chromium and Gecko expose `aria-multiline` (and textareas) as `multiline=true`.
fn aria_multiline(aria_properties: &str) -> bool {
    aria_properties.split(';').any(|property| property.trim().eq_ignore_ascii_case("multiline=true"))
}

/// Normalizes line breaks to `\n`: Win32 edits use CRLF, RichEdit and Word use
/// CR for paragraphs and VT for manual line breaks.
fn normalize_newlines(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                normalized.push('\n');
            }
            '\u{000B}' | '\u{2028}' | '\u{2029}' => normalized.push('\n'),
            _ => normalized.push(ch),
        }
    }
    normalized
}

/// UTF-16 units to request for a range spanning `chars` characters. Providers
/// may count characters as grapheme clusters, so leave generous room; the
/// result is trimmed to `chars` afterwards.
fn span_cap(chars: usize) -> usize {
    chars.saturating_mul(4).saturating_add(16)
}

/// UIA counts are `i32`.
fn uia_count(chars: usize) -> i32 {
    i32::try_from(chars).unwrap_or(i32::MAX)
}

/// A rectangle from `IUIAutomationTextRange::GetBoundingRectangles`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct TextRect {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

impl TextRect {
    /// Parses the flat `[left, top, width, height, ...]` array, dropping empty rectangles.
    fn parse(values: &[f64]) -> Vec<Self> {
        values
            .chunks_exact(4)
            .map(|v| Self { left: v[0], top: v[1], width: v[2], height: v[3] })
            .filter(|r| {
                [r.left, r.top, r.width, r.height].iter().all(|v| v.is_finite()) && r.width >= 0.0 && r.height > 0.0
            })
            .collect()
    }

    /// A caret at the left edge of this rectangle.
    fn caret_at_left(self) -> Rect {
        Rect { x: self.left, y: self.top, width: 1.0, height: self.height }
    }

    /// A caret at the right edge of this rectangle (after its character).
    fn caret_at_right(self) -> Rect {
        Rect { x: self.left + self.width, y: self.top, width: 1.0, height: self.height }
    }
}

/// A secure (password) field: reported without any of its content.
fn secure_input(app: AppInfo, role: InputRole, element_key: u64) -> FocusedInput {
    FocusedInput {
        app,
        role,
        is_secure: true,
        is_multiline: false,
        is_web_content: false,
        placeholder: None,
        label: None,
        text_before_caret: String::new(),
        text_after_caret: String::new(),
        selected_text: None,
        total_length: None,
        caret_rect: None,
        element_key,
    }
}

/// A UI Automation element.
struct Element(IUIAutomationElement);

impl Element {
    fn focused(automation: &IUIAutomation) -> Result<Self> {
        // SAFETY: COM call on a live interface pointer.
        unsafe { automation.GetFocusedElement() }.map(Self)
    }

    fn process_id(&self) -> Result<u32> {
        // SAFETY: COM property getter on a live interface pointer.
        unsafe { self.0.CurrentProcessId() }.map(|pid| u32::try_from(pid).unwrap_or(0))
    }

    fn control_type(&self) -> Result<UIA_CONTROLTYPE_ID> {
        // SAFETY: COM property getter on a live interface pointer.
        unsafe { self.0.CurrentControlType() }
    }

    fn is_password(&self) -> Result<bool> {
        // SAFETY: COM property getter on a live interface pointer.
        unsafe { self.0.CurrentIsPassword() }.map(BOOL::as_bool)
    }

    fn is_keyboard_focusable(&self) -> Result<bool> {
        // SAFETY: COM property getter on a live interface pointer.
        unsafe { self.0.CurrentIsKeyboardFocusable() }.map(BOOL::as_bool)
    }

    fn is_enabled(&self) -> Result<bool> {
        // SAFETY: COM property getter on a live interface pointer.
        unsafe { self.0.CurrentIsEnabled() }.map(BOOL::as_bool)
    }

    fn class_name(&self) -> Result<String> {
        // SAFETY: COM property getter on a live interface pointer.
        unsafe { self.0.CurrentClassName() }.map(|value| bounded(&value, META_MAX_CHARS))
    }

    fn framework_id(&self) -> Result<String> {
        // SAFETY: COM property getter on a live interface pointer.
        unsafe { self.0.CurrentFrameworkId() }.map(|value| bounded(&value, META_MAX_CHARS))
    }

    fn automation_id(&self) -> Result<String> {
        // SAFETY: COM property getter on a live interface pointer.
        unsafe { self.0.CurrentAutomationId() }.map(|value| bounded(&value, META_MAX_CHARS))
    }

    fn localized_control_type(&self) -> Result<String> {
        // SAFETY: COM property getter on a live interface pointer.
        unsafe { self.0.CurrentLocalizedControlType() }.map(|value| bounded(&value, META_MAX_CHARS))
    }

    fn name(&self) -> Result<String> {
        // SAFETY: COM property getter on a live interface pointer.
        unsafe { self.0.CurrentName() }.map(|value| bounded(&value, LABEL_MAX_CHARS))
    }

    fn help_text(&self) -> Result<String> {
        // SAFETY: COM property getter on a live interface pointer.
        unsafe { self.0.CurrentHelpText() }.map(|value| bounded(&value, PLACEHOLDER_MAX_CHARS))
    }

    fn aria_properties(&self) -> Result<String> {
        // SAFETY: COM property getter on a live interface pointer.
        unsafe { self.0.CurrentAriaProperties() }.map(|value| bounded(&value, PLACEHOLDER_MAX_CHARS))
    }

    fn native_window(&self) -> Result<HWND> {
        // SAFETY: COM property getter on a live interface pointer.
        unsafe { self.0.CurrentNativeWindowHandle() }
    }

    fn bounds(&self) -> Result<Rect> {
        // SAFETY: COM property getter on a live interface pointer.
        let rect = unsafe { self.0.CurrentBoundingRectangle() }?;
        Ok(Rect {
            x: f64::from(rect.left),
            y: f64::from(rect.top),
            width: f64::from(rect.right - rect.left),
            height: f64::from(rect.bottom - rect.top),
        })
    }

    fn runtime_id(&self) -> Result<Vec<i32>> {
        // SAFETY: COM call on a live interface pointer; the returned SAFEARRAY is ours to destroy.
        let array = unsafe { SafeArray::from_raw(self.0.GetRuntimeId()?) };
        Ok(array.to_vec::<i32>(VT_I4))
    }

    fn text_pattern(&self) -> Result<IUIAutomationTextPattern> {
        // SAFETY: COM call on a live interface pointer; a missing pattern is reported as an error.
        unsafe { self.0.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
    }

    fn value_pattern(&self) -> Result<IUIAutomationValuePattern> {
        // SAFETY: COM call on a live interface pointer; a missing pattern is reported as an error.
        unsafe { self.0.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
    }

    /// Identifies the element across reads: its runtime id plus the owning process.
    fn key(&self, pid: u32, control_type: UIA_CONTROLTYPE_ID) -> Result<u64> {
        let runtime_id = soft(self.runtime_id())?.unwrap_or_default();
        let mut id: Vec<u8> = runtime_id.iter().flat_map(|part| part.to_le_bytes()).collect();
        if id.is_empty() {
            // No runtime id: fall back to the native window and the control type.
            let window = soft(self.native_window())?.map_or(0, |hwnd| hwnd.0 as usize);
            id.extend_from_slice(&window.to_le_bytes());
            id.extend_from_slice(&control_type.0.to_le_bytes());
        }
        Ok(element_key(&[&id, &pid.to_le_bytes()]))
    }
}

/// Converts a BSTR to a string of at most `max_chars` characters.
fn bounded(value: &BSTR, max_chars: usize) -> String {
    bounded_utf16(value, max_chars)
}

/// A UI Automation text range.
struct TextRange(IUIAutomationTextRange);

impl TextRange {
    fn duplicate(&self) -> Result<Self> {
        // SAFETY: COM call on a live interface pointer.
        unsafe { self.0.Clone() }.map(Self)
    }

    /// Makes this range (a clone of `of`) the empty range at the start of `of`.
    fn collapse_to_start(&self, of: &TextRange) -> Result<()> {
        // SAFETY: COM call on live interface pointers.
        unsafe { self.0.MoveEndpointByRange(TextPatternRangeEndpoint_End, &of.0, TextPatternRangeEndpoint_Start) }
    }

    /// Makes this range (a clone of `of`) the empty range at the end of `of`.
    fn collapse_to_end(&self, of: &TextRange) -> Result<()> {
        // SAFETY: COM call on live interface pointers.
        unsafe { self.0.MoveEndpointByRange(TextPatternRangeEndpoint_Start, &of.0, TextPatternRangeEndpoint_End) }
    }

    /// Moves one endpoint by `count` characters; returns how far it moved.
    fn move_endpoint(&self, endpoint: TextPatternRangeEndpoint, count: i32) -> Result<i32> {
        // SAFETY: COM call on a live interface pointer.
        unsafe { self.0.MoveEndpointByUnit(endpoint, TextUnit_Character, count) }
    }

    fn is_degenerate(&self) -> Result<bool> {
        // SAFETY: COM call on a live interface pointer.
        let order =
            unsafe { self.0.CompareEndpoints(TextPatternRangeEndpoint_Start, &self.0, TextPatternRangeEndpoint_End) }?;
        Ok(order == 0)
    }

    /// The range's text, at most `max_units` UTF-16 units from its start.
    fn text(&self, max_units: usize) -> Result<String> {
        if max_units == 0 {
            return Ok(String::new());
        }
        // SAFETY: COM call on a live interface pointer; the length bound is positive.
        let text = unsafe { self.0.GetText(uia_count(max_units)) }?;
        Ok(String::from_utf16_lossy(&text))
    }

    fn rects(&self) -> Result<Vec<TextRect>> {
        // SAFETY: COM call on a live interface pointer; the returned SAFEARRAY is ours to destroy.
        let array = unsafe { SafeArray::from_raw(self.0.GetBoundingRectangles()?) };
        Ok(TextRect::parse(&array.to_vec::<f64>(VT_R8)))
    }

    /// The range's `IsReadOnly` text attribute, when the provider reports one.
    fn is_read_only(&self) -> Result<Option<bool>> {
        // SAFETY: COM call on a live interface pointer; the VARIANT is cleared by `Variant`.
        let value = Variant(unsafe { self.0.GetAttributeValue(UIA_IsReadOnlyAttributeId) }?);
        Ok(value.as_bool())
    }
}

/// Owns a VARIANT and clears it when dropped.
struct Variant(VARIANT);

impl Variant {
    fn as_bool(&self) -> Option<bool> {
        // SAFETY: the VARIANT is initialized; its payload is read only when the tag says VT_BOOL.
        unsafe {
            let inner = &self.0.Anonymous.Anonymous;
            (inner.vt == VT_BOOL).then(|| inner.Anonymous.boolVal.0 != 0)
        }
    }
}

impl Drop for Variant {
    fn drop(&mut self) {
        // SAFETY: the VARIANT is initialized and owned here; clearing releases any object it holds
        // (such as UIA's "not supported" sentinel) exactly once.
        let _ = unsafe { VariantClear(&mut self.0) };
    }
}

/// The current selection, or (when `allow_caret`) the caret position from
/// TextPattern2 when the provider reports no selection.
fn selection(pattern: &IUIAutomationTextPattern, allow_caret: bool) -> Result<Option<TextRange>> {
    // SAFETY: COM call on a live interface pointer.
    if let Some(ranges) = soft(unsafe { pattern.GetSelection() })? {
        // SAFETY: COM call on a live interface pointer.
        if soft(unsafe { ranges.Length() })?.unwrap_or(0) > 0 {
            // SAFETY: COM call on a live interface pointer; index 0 exists.
            return soft(unsafe { ranges.GetElement(0) }).map(|range| range.map(TextRange));
        }
    }
    if !allow_caret {
        return Ok(None);
    }
    let Some(pattern2) = soft(pattern.cast::<IUIAutomationTextPattern2>())? else {
        return Ok(None);
    };
    let mut is_active = BOOL::default();
    // SAFETY: COM call on a live interface pointer; `is_active` is a valid out-pointer.
    soft(unsafe { pattern2.GetCaretRange(&mut is_active) }).map(|range| range.map(TextRange))
}

/// Text around the caret.
#[derive(Debug, Default)]
struct Around {
    before: String,
    after: String,
    selected: Option<String>,
}

impl Around {
    fn has_line_break(&self) -> bool {
        self.before.contains('\n')
            || self.after.contains('\n')
            || self.selected.as_deref().is_some_and(|s| s.contains('\n'))
    }
}

/// Reads the text before the selection, the selection and the text after it,
/// each bounded by `limits`.
fn read_around(selection: &TextRange, limits: ReadLimits) -> Result<Around> {
    let selected = if limits.selection > 0 && !selection.is_degenerate()? {
        let text = normalize_newlines(&selection.text(limits.selection.saturating_mul(2).saturating_add(2))?);
        Some(head_chars(&text, limits.selection).to_string()).filter(|text| !text.is_empty())
    } else {
        None
    };
    let before = if limits.before_caret > 0 {
        let range = selection.duplicate()?;
        range.collapse_to_start(selection)?;
        range.move_endpoint(TextPatternRangeEndpoint_Start, -uia_count(limits.before_caret))?;
        let text = normalize_newlines(&range.text(span_cap(limits.before_caret))?);
        tail_chars(&text, limits.before_caret).to_string()
    } else {
        String::new()
    };
    let after = if limits.after_caret > 0 {
        let range = selection.duplicate()?;
        range.collapse_to_end(selection)?;
        range.move_endpoint(TextPatternRangeEndpoint_End, uia_count(limits.after_caret))?;
        let text = normalize_newlines(&range.text(span_cap(limits.after_caret))?);
        head_chars(&text, limits.after_caret).to_string()
    } else {
        String::new()
    };
    Ok(Around { before, after, selected })
}

/// Fallback for fields without TextPattern: the value's tail, assuming the
/// caret is at the end. Also returns the value's length in UTF-16 units.
fn read_value(pattern: &IUIAutomationValuePattern, limits: ReadLimits) -> Result<(Around, usize)> {
    // SAFETY: COM property getter on a live interface pointer.
    let value = unsafe { pattern.CurrentValue() }?;
    let tail = String::from_utf16_lossy(tail_units(&value, span_cap(limits.before_caret)));
    let before = tail_chars(&normalize_newlines(&tail), limits.before_caret).to_string();
    Ok((Around { before, ..Around::default() }, value.len()))
}

/// Whether the field is read-only: from ValuePattern, else from the text
/// attribute of the selection. `None` when the provider does not say.
fn read_only(value: Option<&IUIAutomationValuePattern>, selection: Option<&TextRange>) -> Result<Option<bool>> {
    if let Some(pattern) = value {
        // SAFETY: COM property getter on a live interface pointer.
        if let Some(read_only) = soft(unsafe { pattern.CurrentIsReadOnly() })? {
            return Ok(Some(read_only.as_bool()));
        }
    }
    match selection {
        Some(range) => Ok(soft(range.is_read_only())?.flatten()),
        None => Ok(None),
    }
}

/// The bounding rectangle of the character next to the empty range `caret`.
fn neighbour_rect(caret: &TextRange, forward: bool) -> Result<Option<TextRect>> {
    let Some(range) = soft(caret.duplicate())? else {
        return Ok(None);
    };
    let (endpoint, step) =
        if forward { (TextPatternRangeEndpoint_End, 1) } else { (TextPatternRangeEndpoint_Start, -1) };
    if soft(range.move_endpoint(endpoint, step))?.unwrap_or(0) == 0 {
        return Ok(None);
    }
    let rects = soft(range.rects())?.unwrap_or_default();
    Ok(if forward { rects.first().copied() } else { rects.last().copied() })
}

/// The caret from text geometry: the empty range at the end of the
/// selection, else the right edge of the previous character, else the left
/// edge of the next one.
fn caret_from_text(selection: &TextRange, after_line_break: bool) -> Result<Option<Rect>> {
    let Some(caret) = soft(selection.duplicate())? else {
        return Ok(None);
    };
    if soft(caret.collapse_to_end(selection))?.is_none() {
        return Ok(None);
    }
    if let Some(rect) = soft(caret.rects())?.and_then(|rects| rects.first().copied()) {
        return Ok(Some(rect.caret_at_left()));
    }
    // After a line break the previous character sits at the end of the line above.
    let previous = if after_line_break { None } else { neighbour_rect(&caret, false)? };
    if let Some(rect) = previous {
        return Ok(Some(rect.caret_at_right()));
    }
    Ok(neighbour_rect(&caret, true)?.map(TextRect::caret_at_left))
}

/// The system caret of the foreground GUI thread (classic Win32 controls).
fn system_caret(thread_id: u32) -> Option<Rect> {
    let mut info = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
    // SAFETY: `info` is a writable GUITHREADINFO whose cbSize is set.
    unsafe { GetGUIThreadInfo(thread_id, &mut info) }.ok()?;
    let caret = info.rcCaret;
    let height = caret.bottom - caret.top;
    if info.hwndCaret.is_invalid() || height <= 0 {
        return None;
    }
    let mut origin = POINT { x: caret.left, y: caret.top };
    // SAFETY: `origin` is a writable POINT; a stale window handle makes the call fail.
    if !unsafe { ClientToScreen(info.hwndCaret, &mut origin) }.as_bool() {
        return None;
    }
    Some(Rect {
        x: f64::from(origin.x),
        y: f64::from(origin.y),
        width: f64::from((caret.right - caret.left).max(1)),
        height: f64::from(height),
    })
}

/// Last resort: a caret at the bottom-left corner of the field.
fn caret_from_bounds(bounds: Rect) -> Option<Rect> {
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return None;
    }
    let height = FALLBACK_CARET_HEIGHT.min(bounds.height);
    Some(Rect { x: bounds.x, y: bounds.y + bounds.height - height, width: 1.0, height })
}

/// Multi-line detection for Edit controls.
fn edit_is_multiline(
    element: &Element,
    class_name: &str,
    web: bool,
    around: &Around,
    text_caret: Option<Rect>,
) -> Result<bool> {
    if is_win32_edit_class(class_name) {
        if let Some(window) = soft(element.native_window())?.filter(|window| !window.is_invalid()) {
            // SAFETY: reading a window's style has no memory-safety preconditions; a stale handle yields 0.
            let style = unsafe { GetWindowLongW(window, GWL_STYLE) };
            return Ok((style & ES_MULTILINE) != 0);
        }
    }
    if around.has_line_break() {
        return Ok(true);
    }
    if web {
        // Web inputs are often padded to several line heights, so geometry is
        // unreliable there; Chromium and Gecko state multi-line explicitly.
        return Ok(soft(element.aria_properties())?.is_some_and(|aria| aria_multiline(&aria)));
    }
    let (Some(caret), Some(bounds)) = (text_caret, soft(element.bounds())?) else {
        return Ok(false);
    };
    Ok(bounds.height >= caret.height * MULTILINE_LINES)
}

/// Reads the focused text input of `target`, if any.
pub(super) fn focused_input(
    automation: &IUIAutomation,
    target: &Target,
    limits: ReadLimits,
) -> Result<Option<FocusedInput>> {
    let element = Element::focused(automation)?;
    let pid = element.process_id()?;
    if pid == std::process::id() {
        return Ok(None);
    }
    let control_type = element.control_type()?;
    let kind = Kind::of(control_type);
    let class_name = soft(element.class_name())?.unwrap_or_default();
    let terminal = is_terminal(&class_name, &target.window_class, &target.app.id);
    let text_pattern = soft(element.text_pattern())?;
    if kind == Kind::Other {
        // Other control types qualify only as focusable text surfaces.
        let surface =
            text_pattern.is_some() && (terminal || (element.is_keyboard_focusable()? && element.is_enabled()?));
        if !surface {
            return Ok(None);
        }
    }
    let element_key = element.key(pid, control_type)?;
    if element.is_password()? {
        return Ok(Some(secure_input(target.app.clone(), role_for(kind, terminal, false, false), element_key)));
    }

    let framework_id = soft(element.framework_id())?.unwrap_or_default();
    let is_web_content = is_web_framework(&framework_id);
    let value_pattern = soft(element.value_pattern())?;
    let selection = match &text_pattern {
        Some(pattern) => selection(pattern, true)?,
        None => None,
    };
    let read_only = read_only(value_pattern.as_ref(), selection.as_ref())?;
    if !is_editable(kind, terminal, is_web_content, read_only) {
        return Ok(None);
    }

    let (around, total_length) = match (&selection, &value_pattern) {
        (Some(range), _) => (read_around(range, limits)?, None),
        (None, Some(pattern)) => {
            let (around, length) = read_value(pattern, limits)?;
            (around, Some(length))
        }
        (None, None) => (Around::default(), None),
    };

    let after_line_break = around.selected.as_deref().unwrap_or(around.before.as_str()).ends_with('\n');
    let text_caret = match &selection {
        Some(range) => caret_from_text(range, after_line_break)?,
        None => None,
    };
    let caret_rect = match text_caret {
        Some(caret) => Some(caret),
        None => match system_caret(target.thread_id) {
            Some(caret) => Some(caret),
            None => soft(element.bounds())?.and_then(caret_from_bounds),
        },
    };

    let is_multiline = match kind {
        _ if terminal => false,
        Kind::Edit => edit_is_multiline(&element, &class_name, is_web_content, &around, text_caret)?,
        Kind::ComboBox => false,
        Kind::Document | Kind::Other => true,
    };
    let name = soft(element.name())?.unwrap_or_default();
    let search = matches!(kind, Kind::Edit | Kind::ComboBox) && {
        let automation_id = soft(element.automation_id())?.unwrap_or_default();
        let localized = soft(element.localized_control_type())?.unwrap_or_default();
        looks_like_search(&localized, &automation_id, &name)
    };
    let placeholder = soft(element.help_text())?.filter(|text| !text.trim().is_empty());

    Ok(Some(FocusedInput {
        app: target.app.clone(),
        role: role_for(kind, terminal, search, is_multiline),
        is_secure: false,
        is_multiline,
        is_web_content,
        placeholder,
        label: Some(name).filter(|name| !name.trim().is_empty()),
        text_before_caret: around.before,
        text_after_caret: around.after,
        selected_text: around.selected,
        total_length,
        caret_rect,
        element_key,
    }))
}

/// Reads the selected text of the focused element, bounded to `max_chars`.
pub(super) fn selected_text(automation: &IUIAutomation, max_chars: usize) -> Result<Option<String>> {
    let element = Element::focused(automation)?;
    if element.process_id()? == std::process::id() || element.is_password()? {
        return Ok(None);
    }
    let Some(pattern) = soft(element.text_pattern())? else {
        return Ok(None);
    };
    let Some(range) = selection(&pattern, false)? else {
        return Ok(None);
    };
    if range.is_degenerate()? {
        return Ok(None);
    }
    let text = normalize_newlines(&range.text(max_chars.saturating_mul(2).saturating_add(2))?);
    let text = head_chars(&text, max_chars);
    Ok((!text.is_empty()).then(|| text.to_string()))
}

#[cfg(test)]
mod tests {
    use ::windows::Win32::UI::Accessibility::{UIA_ButtonControlTypeId, UIA_GroupControlTypeId};

    use super::*;

    #[test]
    fn control_types_map_to_kinds() {
        assert_eq!(Kind::of(UIA_EditControlTypeId), Kind::Edit);
        assert_eq!(Kind::of(UIA_ComboBoxControlTypeId), Kind::ComboBox);
        assert_eq!(Kind::of(UIA_DocumentControlTypeId), Kind::Document);
        assert_eq!(Kind::of(UIA_GroupControlTypeId), Kind::Other);
        assert_eq!(Kind::of(UIA_ButtonControlTypeId), Kind::Other);
    }

    #[test]
    fn roles_follow_kind_and_hints() {
        assert_eq!(role_for(Kind::Edit, false, false, false), InputRole::TextField);
        assert_eq!(role_for(Kind::Edit, false, false, true), InputRole::TextArea);
        assert_eq!(role_for(Kind::Edit, false, true, true), InputRole::SearchField);
        assert_eq!(role_for(Kind::ComboBox, false, false, false), InputRole::ComboBox);
        assert_eq!(role_for(Kind::ComboBox, false, true, false), InputRole::SearchField);
        assert_eq!(role_for(Kind::Document, false, true, true), InputRole::Document);
        assert_eq!(role_for(Kind::Other, false, false, true), InputRole::Document);
        assert_eq!(role_for(Kind::Edit, true, true, false), InputRole::Terminal);
    }

    #[test]
    fn read_only_content_is_not_an_input() {
        assert!(is_editable(Kind::Edit, false, false, None), "edits are editable unless stated otherwise");
        assert!(!is_editable(Kind::Edit, false, true, Some(true)));
        assert!(is_editable(Kind::Document, false, false, None), "native documents (Word, Notepad)");
        assert!(!is_editable(Kind::Document, false, true, None), "a web page needs proof of editability");
        assert!(is_editable(Kind::Document, false, true, Some(false)));
        assert!(!is_editable(Kind::Other, false, false, None));
        assert!(is_editable(Kind::Other, false, false, Some(false)));
        assert!(is_editable(Kind::Other, true, false, Some(true)), "terminals take input regardless");
    }

    #[test]
    fn search_fields_are_recognized() {
        assert!(looks_like_search("search box", "", ""));
        assert!(looks_like_search("edit", "SearchTextBox", ""));
        assert!(looks_like_search("edit", "", "Search mail"));
        assert!(looks_like_search("edit", "", "Address and search bar"));
        assert!(!looks_like_search("edit", "", "Research notes"));
        assert!(!looks_like_search("edit", "messageInput", "Message #general"));
    }

    #[test]
    fn terminals_are_recognized_by_class_or_executable() {
        assert!(is_terminal("ConsoleWindowClass", "", "cmd.exe"));
        assert!(is_terminal("TermControl", "", "unknown.exe"));
        assert!(is_terminal("", "CASCADIA_HOSTING_WINDOW_CLASS", "unknown.exe"));
        assert!(is_terminal("", "", "pwsh.exe"));
        assert!(!is_terminal("Edit", "Notepad", "notepad.exe"));
    }

    #[test]
    fn web_content_and_multiline_hints() {
        assert!(is_web_framework("Chrome"));
        assert!(is_web_framework("gecko"));
        assert!(!is_web_framework("Win32"));
        assert!(!is_web_framework("XAML"));
        assert!(aria_multiline("readonly=false;multiline=true"));
        assert!(!aria_multiline("multiline=false"));
        assert!(is_win32_edit_class("Edit"));
        assert!(is_win32_edit_class("RICHEDIT50W"));
        assert!(is_win32_edit_class("WindowsForms10.EDIT.app.0.141b42a_r6_ad1"));
        assert!(!is_win32_edit_class("Chrome_RenderWidgetHostHWND"));
    }

    #[test]
    fn line_breaks_are_normalized() {
        assert_eq!(normalize_newlines("a\r\nb\rc\u{000B}d\u{2029}e\nf"), "a\nb\nc\nd\ne\nf");
        assert_eq!(normalize_newlines("\r\r\n"), "\n\n");
        assert_eq!(normalize_newlines("plain"), "plain");
    }

    #[test]
    fn text_rectangles_become_carets() {
        let rects = TextRect::parse(&[10.0, 20.0, 0.0, 16.0, 1.0, 2.0, 3.0, 0.0, 5.0, 6.0, 7.0]);
        assert_eq!(
            rects,
            vec![TextRect { left: 10.0, top: 20.0, width: 0.0, height: 16.0 }],
            "empty and partial dropped"
        );
        let rect = TextRect { left: 100.0, top: 50.0, width: 8.0, height: 18.0 };
        assert_eq!(rect.caret_at_left(), Rect { x: 100.0, y: 50.0, width: 1.0, height: 18.0 });
        assert_eq!(rect.caret_at_right(), Rect { x: 108.0, y: 50.0, width: 1.0, height: 18.0 });
        let field = Rect { x: 10.0, y: 100.0, width: 300.0, height: 40.0 };
        assert_eq!(caret_from_bounds(field), Some(Rect { x: 10.0, y: 122.0, width: 1.0, height: 18.0 }));
        assert_eq!(caret_from_bounds(Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 }), None);
    }

    #[test]
    fn secure_inputs_carry_no_content() {
        let input = secure_input(AppInfo::new("app.exe", "App"), InputRole::TextField, 7);
        assert!(input.is_secure);
        assert!(input.text_before_caret.is_empty() && input.text_after_caret.is_empty());
        assert!(input.selected_text.is_none() && input.label.is_none() && input.placeholder.is_none());
        assert_eq!(input.element_key, 7);
    }

    #[test]
    fn read_bounds_are_generous_but_finite() {
        assert_eq!(span_cap(10), 56);
        assert_eq!(span_cap(usize::MAX), usize::MAX);
        assert_eq!(uia_count(5), 5);
        assert_eq!(uia_count(usize::MAX), i32::MAX);
    }

    #[test]
    fn fatal_errors_abort_reads() {
        let timeout = Error::from_hresult(HRESULT(UIA_E_TIMEOUT as i32));
        assert!(is_timeout(&timeout));
        assert!(is_fatal(&timeout));
        assert!(soft::<()>(Err(timeout)).is_err());
        assert!(is_fatal(&Error::from_hresult(E_ACCESSDENIED)));
        let unsupported = Error::from_hresult(::windows::Win32::Foundation::E_NOINTERFACE);
        assert!(!is_fatal(&unsupported));
        assert!(matches!(soft::<()>(Err(unsupported)), Ok(None)));
    }
}
