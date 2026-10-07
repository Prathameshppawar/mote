//! A string secret that never prints and is wiped from memory on drop.

use std::fmt;

use zeroize::Zeroize;

/// An API key. `Debug` and `Display` print `[redacted]`.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// Wraps a key, trimming surrounding whitespace pasted along with it.
    pub fn new(key: impl Into<String>) -> Self {
        let mut raw: String = key.into();
        let trimmed = raw.trim().to_string();
        raw.zeroize();
        Self(trimmed)
    }

    /// The key itself, for the `Authorization` header only.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The last four characters, for "…abcd" hints in the UI.
    pub fn hint(&self) -> String {
        let chars: Vec<char> = self.0.chars().collect();
        if chars.len() < 12 {
            return "…".into();
        }
        format!("…{}", chars[chars.len() - 4..].iter().collect::<String>())
    }

    /// Whether the key has the shape of a Groq key (`gsk_` + 52 characters).
    pub fn looks_like_groq_key(&self) -> bool {
        self.0.starts_with("gsk_")
            && self.0.len() >= 20
            && self.0.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    }
}

impl Drop for ApiKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey([redacted])")
    }
}

impl fmt::Display for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_prints_the_key() {
        let key = ApiKey::new("  gsk_abcdefghijklmnopqrstuvwxyz0123456789  ");
        assert_eq!(format!("{key:?}"), "ApiKey([redacted])");
        assert_eq!(key.to_string(), "[redacted]");
        assert_eq!(key.expose(), "gsk_abcdefghijklmnopqrstuvwxyz0123456789");
        assert_eq!(key.hint(), "…6789");
        assert!(key.looks_like_groq_key());
        assert!(!ApiKey::new("sk-not-groq").looks_like_groq_key());
        assert_eq!(ApiKey::new("short").hint(), "…");
    }
}
