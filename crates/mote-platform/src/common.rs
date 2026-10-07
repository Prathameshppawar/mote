//! Platform-independent helpers shared by the adapters.

use mote_core::platform::{InputRole, ReadLimits};
use mote_core::text::{head_chars, tail_chars};

/// Splits `text` around a caret at UTF-16 offset `caret` with a selection of
/// `selection_len` UTF-16 units, applying read limits.
///
/// Returns `(before, selected, after)`. Offsets outside the text are clamped;
/// offsets inside a surrogate pair snap to the previous boundary.
pub fn split_at_utf16(text: &str, caret: usize, selection_len: usize, limits: ReadLimits) -> (String, String, String) {
    let start = byte_offset_clamped(text, caret);
    let end = byte_offset_clamped(text, caret.saturating_add(selection_len)).max(start);
    let before = tail_chars(&text[..start], limits.before_caret).to_string();
    let selected = head_chars(&text[start..end], limits.selection).to_string();
    let after = head_chars(&text[end..], limits.after_caret).to_string();
    (before, selected, after)
}

/// Byte offset for a UTF-16 offset, clamped to the text and snapped to a char boundary.
pub fn byte_offset_clamped(text: &str, utf16_offset: usize) -> usize {
    let mut units = 0usize;
    for (byte, ch) in text.char_indices() {
        if units >= utf16_offset {
            return byte;
        }
        units += ch.len_utf16();
        if units > utf16_offset {
            return byte;
        }
    }
    text.len()
}

/// Maps an accessibility role (and subrole/control type) to an [`InputRole`].
pub fn input_role(role: &str, subrole: Option<&str>) -> Option<InputRole> {
    if subrole == Some("AXSearchField") {
        return Some(InputRole::SearchField);
    }
    match role {
        "AXTextArea" => Some(InputRole::TextArea),
        "AXTextField" => Some(InputRole::TextField),
        "AXComboBox" => Some(InputRole::ComboBox),
        "AXSearchField" => Some(InputRole::SearchField),
        _ => None,
    }
}

/// Combines identifying values into a stable element key.
pub fn element_key(parts: &[&[u8]]) -> u64 {
    let mut joined = Vec::new();
    for part in parts {
        joined.extend_from_slice(part);
        joined.push(0);
    }
    mote_core::text::fnv1a64(&joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_text_at_utf16_caret() {
        let limits = ReadLimits { before_caret: 100, after_caret: 100, selection: 100 };
        let (b, s, a) = split_at_utf16("hello world", 5, 0, limits);
        assert_eq!((b.as_str(), s.as_str(), a.as_str()), ("hello", "", " world"));
        let (b, s, a) = split_at_utf16("hello world", 6, 5, limits);
        assert_eq!((b.as_str(), s.as_str(), a.as_str()), ("hello ", "world", ""));
    }

    #[test]
    fn handles_surrogate_pairs_and_devanagari() {
        let limits = ReadLimits::default();
        // "a😀b": a(1) 😀(2) b(1)
        let (b, _, a) = split_at_utf16("a😀b", 3, 0, limits);
        assert_eq!((b.as_str(), a.as_str()), ("a😀", "b"));
        let (b, _, a) = split_at_utf16("a😀b", 2, 0, limits);
        assert_eq!((b.as_str(), a.as_str()), ("a", "😀b"), "inside a pair snaps back");
        let text = "मला काम आहे";
        let (b, _, _) = split_at_utf16(text, mote_core::text::utf16_len(text), 0, limits);
        assert_eq!(b, text);
    }

    #[test]
    fn clamps_out_of_range_offsets_and_applies_limits() {
        let limits = ReadLimits { before_caret: 3, after_caret: 2, selection: 1 };
        let (b, s, a) = split_at_utf16("abcdefgh", 4, 2, limits);
        assert_eq!((b.as_str(), s.as_str(), a.as_str()), ("bcd", "e", "gh"));
        let (b, _, a) = split_at_utf16("abc", 99, 0, limits);
        assert_eq!((b.as_str(), a.as_str()), ("abc", ""));
    }

    #[test]
    fn roles() {
        assert_eq!(input_role("AXTextArea", None), Some(InputRole::TextArea));
        assert_eq!(input_role("AXTextField", Some("AXSearchField")), Some(InputRole::SearchField));
        assert_eq!(input_role("AXButton", None), None);
    }

    #[test]
    fn element_keys_differ_by_part() {
        assert_ne!(element_key(&[b"a", b"bc"]), element_key(&[b"ab", b"c"]));
    }
}
