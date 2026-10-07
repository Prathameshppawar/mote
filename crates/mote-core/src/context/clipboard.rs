//! Local classification of clipboard content.
//!
//! The kind of copied content (an email, an error log, code) drives contextual
//! suggestions. Classification runs locally; clipboard text is never persisted
//! and is never sent to a provider just because it changed.

use serde::{Deserialize, Serialize};

use crate::text::word_count;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ClipboardKind {
    Empty,
    Url,
    Path,
    Json,
    StackTrace,
    Email,
    Code,
    ShortText,
    Text,
}

impl ClipboardKind {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Empty => "nothing",
            Self::Url => "a link",
            Self::Path => "a file path",
            Self::Json => "JSON",
            Self::StackTrace => "an error log",
            Self::Email => "an email",
            Self::Code => "code",
            Self::ShortText => "a short text",
            Self::Text => "text",
        }
    }

    /// Whether this content is substantial enough to build a task around.
    pub fn is_substantial(self) -> bool {
        matches!(self, Self::StackTrace | Self::Email | Self::Code | Self::Json | Self::Text)
    }
}

const STACK_MARKERS: &[&str] = &[
    "traceback (most recent call last)",
    "exception in thread",
    "panicked at",
    "error[e",
    "caused by:",
    "stack trace:",
    "uncaught ",
    "unhandled rejection",
    "segmentation fault",
    "fatal error:",
];

/// Keywords that start a line of code.
const CODE_LINE_STARTS: &[&str] = &[
    "fn ",
    "pub ",
    "def ",
    "function ",
    "const ",
    "let ",
    "var ",
    "import ",
    "export ",
    "class ",
    "return ",
    "#include",
    "public ",
    "private ",
    "async ",
    "await ",
    "func ",
    "package ",
    "use ",
    "impl ",
    "struct ",
    "select ",
    "if (",
    "for (",
    "while (",
    "} else",
];
/// Symbols that appear anywhere in a line of code.
const CODE_SYMBOLS: &[&str] = &["=>", "->", "</", "/>", "::", "();", "){", ") {"];

/// Classifies clipboard text.
pub fn classify(text: &str) -> ClipboardKind {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return ClipboardKind::Empty;
    }
    let single_line = !trimmed.contains('\n');
    if single_line && !trimmed.contains(' ') {
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            return ClipboardKind::Url;
        }
        if trimmed.starts_with('/') || trimmed.starts_with("~/") || is_windows_path(trimmed) {
            return ClipboardKind::Path;
        }
    }
    let lower = trimmed.to_lowercase();
    if ((trimmed.starts_with('{') && trimmed.ends_with('}')) || (trimmed.starts_with('[') && trimmed.ends_with(']')))
        && trimmed.contains(':')
        && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
    {
        return ClipboardKind::Json;
    }
    let lines: Vec<&str> = trimmed.lines().collect();
    let stack_lines = lines.iter().filter(|l| is_stack_frame(l)).count();
    if STACK_MARKERS.iter().any(|m| lower.contains(m)) || stack_lines >= 2 {
        return ClipboardKind::StackTrace;
    }
    if looks_like_email(&lines, &lower) {
        return ClipboardKind::Email;
    }
    let code_lines = lines.iter().filter(|l| is_code_line(l)).count();
    let mostly_code = lines.len() >= 2 && code_lines * 10 >= lines.len() * 4;
    let code_statement = lines.len() == 1 && code_lines == 1 && trimmed.ends_with([';', '{', '}']);
    if mostly_code || code_statement {
        return ClipboardKind::Code;
    }
    if word_count(trimmed) <= 6 {
        return ClipboardKind::ShortText;
    }
    ClipboardKind::Text
}

fn is_windows_path(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() > 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

fn is_stack_frame(line: &str) -> bool {
    let t = line.trim_start();
    (t.starts_with("at ") && (t.contains('(') || t.contains(':')))
        || (t.starts_with("File \"") && t.contains("line "))
        || t.starts_with("--> ")
}

/// Whether a line reads as source code.
pub fn is_code_line(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    let lower = t.to_lowercase();
    t.ends_with(';')
        || t.ends_with('{')
        || t == "}"
        || t.starts_with("//")
        || CODE_LINE_STARTS.iter().any(|m| lower.starts_with(m))
        || CODE_SYMBOLS.iter().any(|m| t.contains(m))
}

fn looks_like_email(lines: &[&str], lower: &str) -> bool {
    let header_lines = lines
        .iter()
        .filter(|l| {
            let l = l.trim_start().to_lowercase();
            l.starts_with("from:")
                || l.starts_with("subject:")
                || l.starts_with("to:")
                || l.starts_with("sent:")
                || l.starts_with("date:")
        })
        .count();
    if header_lines >= 2 {
        return true;
    }
    let first = lines.first().map(|l| l.trim().to_lowercase()).unwrap_or_default();
    let greeting = ["hi ", "hi,", "hello", "dear ", "hey ", "good morning", "good afternoon", "greetings"]
        .iter()
        .any(|g| first.starts_with(g));
    let tail: String = lines.iter().rev().take(4).copied().collect::<Vec<_>>().join(" ").to_lowercase();
    let signoff = ["regards", "thanks", "thank you", "best,", "sincerely", "cheers", "warm wishes"]
        .iter()
        .any(|s| tail.contains(s));
    lines.len() >= 3 && greeting && (signoff || lower.len() > 200)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(classify("   "), ClipboardKind::Empty);
        assert_eq!(classify("https://github.com/rust-lang/rust/issues/1"), ClipboardKind::Url);
        assert_eq!(classify("/Users/me/project/src/main.rs"), ClipboardKind::Path);
        assert_eq!(classify(r"C:\Users\me\file.txt"), ClipboardKind::Path);
        assert_eq!(classify(r#"{"user": "a", "id": 3}"#), ClipboardKind::Json);
        assert_eq!(classify("ok thanks"), ClipboardKind::ShortText);
    }

    #[test]
    fn stack_traces() {
        let js = "TypeError: Cannot read properties of undefined (reading 'map')\n    at render (App.tsx:12:5)\n    at main (index.ts:3:1)";
        assert_eq!(classify(js), ClipboardKind::StackTrace);
        let py =
            "Traceback (most recent call last):\n  File \"app.py\", line 3, in <module>\n    main()\nValueError: bad";
        assert_eq!(classify(py), ClipboardKind::StackTrace);
        let rust = "thread 'main' panicked at src/main.rs:4:5:\nindex out of bounds";
        assert_eq!(classify(rust), ClipboardKind::StackTrace);
    }

    #[test]
    fn emails() {
        let mail = "Hi Prathamesh,\n\nThe client reported that the export button fails on Safari since yesterday's release.\nCan you take a look before Friday?\n\nThanks,\nAsha";
        assert_eq!(classify(mail), ClipboardKind::Email);
        let forwarded = "From: Asha <asha@example.com>\nSent: Monday\nSubject: Export bug\n\nPlease check.";
        assert_eq!(classify(forwarded), ClipboardKind::Email);
    }

    #[test]
    fn code() {
        let code = "fn main() {\n    let x = compute();\n    println!(\"{x}\");\n}";
        assert_eq!(classify(code), ClipboardKind::Code);
        assert_eq!(classify("const total = items.reduce((a, b) => a + b, 0);"), ClipboardKind::Code);
    }

    #[test]
    fn prose() {
        let prose = "We should move the launch to next week because the payment provider has not finished their review of our integration.";
        assert_eq!(classify(prose), ClipboardKind::Text);
    }
}
