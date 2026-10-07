//! Parsing of provider rate-limit headers.
//!
//! Groq reports limits in `x-ratelimit-*` headers: the request limit is per day,
//! the token limit per minute, and reset times use a compact duration format
//! such as `1m26.4s`, `2.159s` or `17ms`. A 429 response also carries
//! `retry-after` in seconds.

use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::header::HeaderMap;

use mote_core::providers::types::RateLimitSnapshot;

/// Parses durations such as `1h2m3.5s`, `1m26.4s`, `2.159s`, `250ms`.
pub fn parse_duration(input: &str) -> Option<Duration> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(seconds) = s.parse::<f64>() {
        return (seconds.is_finite() && seconds >= 0.0).then(|| Duration::from_secs_f64(seconds));
    }
    let mut total = 0.0f64;
    let mut number = String::new();
    let mut chars = s.chars().peekable();
    let mut matched = false;
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() || c == '.' {
            number.push(c);
            continue;
        }
        let value: f64 = number.parse().ok()?;
        number.clear();
        let factor = match c {
            'h' => 3_600.0,
            'm' if chars.peek() == Some(&'s') => {
                chars.next();
                0.001
            }
            'm' => 60.0,
            's' => 1.0,
            _ => return None,
        };
        total += value * factor;
        matched = true;
    }
    if !number.is_empty() || !matched || !total.is_finite() {
        return None;
    }
    Some(Duration::from_secs_f64(total))
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn header_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
    header(headers, name).and_then(|v| v.trim().parse().ok())
}

/// Builds a snapshot from `x-ratelimit-*` headers, if any are present.
pub fn parse_rate_limits(headers: &HeaderMap, now: DateTime<Utc>) -> Option<RateLimitSnapshot> {
    let snapshot = RateLimitSnapshot {
        requests_limit: header_u64(headers, "x-ratelimit-limit-requests"),
        requests_remaining: header_u64(headers, "x-ratelimit-remaining-requests"),
        requests_reset_secs: header(headers, "x-ratelimit-reset-requests")
            .and_then(parse_duration)
            .map(|d| d.as_secs_f64()),
        tokens_limit: header_u64(headers, "x-ratelimit-limit-tokens"),
        tokens_remaining: header_u64(headers, "x-ratelimit-remaining-tokens"),
        tokens_reset_secs: header(headers, "x-ratelimit-reset-tokens")
            .and_then(parse_duration)
            .map(|d| d.as_secs_f64()),
        observed_at: now,
    };
    let any = snapshot.requests_limit.is_some()
        || snapshot.requests_remaining.is_some()
        || snapshot.tokens_limit.is_some()
        || snapshot.tokens_remaining.is_some();
    any.then_some(snapshot)
}

/// How long to wait after a 429: `retry-after`, else the reset time of
/// whichever limit is exhausted.
pub fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    if let Some(d) = header(headers, "retry-after").and_then(parse_duration) {
        return Some(d);
    }
    let exhausted = |remaining: &str, reset: &str| {
        (header_u64(headers, remaining) == Some(0)).then(|| header(headers, reset).and_then(parse_duration)).flatten()
    };
    exhausted("x-ratelimit-remaining-tokens", "x-ratelimit-reset-tokens")
        .into_iter()
        .chain(exhausted("x-ratelimit-remaining-requests", "x-ratelimit-reset-requests"))
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderName, HeaderValue};

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(HeaderName::from_bytes(k.as_bytes()).unwrap(), HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("2.159s"), Some(Duration::from_secs_f64(2.159)));
        assert_eq!(parse_duration("1m26.4s"), Some(Duration::from_secs_f64(86.4)));
        assert_eq!(parse_duration("17ms"), Some(Duration::from_millis(17)));
        assert_eq!(parse_duration("1h2m3s"), Some(Duration::from_secs(3_723)));
        assert_eq!(parse_duration("7"), Some(Duration::from_secs(7)));
        assert_eq!(parse_duration("0.5"), Some(Duration::from_millis(500)));
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("soon"), None);
        assert_eq!(parse_duration("5x"), None);
        assert_eq!(parse_duration("-1"), None);
    }

    #[test]
    fn snapshot_from_groq_headers() {
        // Values observed from Groq's API on 2026-10-07.
        let h = headers(&[
            ("x-ratelimit-limit-requests", "1000"),
            ("x-ratelimit-limit-tokens", "8000"),
            ("x-ratelimit-remaining-requests", "999"),
            ("x-ratelimit-remaining-tokens", "7761"),
            ("x-ratelimit-reset-requests", "1m26.4s"),
            ("x-ratelimit-reset-tokens", "1.792s"),
        ]);
        let s = parse_rate_limits(&h, Utc::now()).unwrap();
        assert_eq!((s.requests_limit, s.requests_remaining), (Some(1000), Some(999)));
        assert_eq!((s.tokens_limit, s.tokens_remaining), (Some(8000), Some(7761)));
        assert!((s.requests_reset_secs.unwrap() - 86.4).abs() < 1e-9);
        assert!(parse_rate_limits(&HeaderMap::new(), Utc::now()).is_none());
    }

    #[test]
    fn retry_after_prefers_header_then_exhausted_limit() {
        assert_eq!(parse_retry_after(&headers(&[("retry-after", "12")])), Some(Duration::from_secs(12)));
        let h = headers(&[
            ("x-ratelimit-remaining-tokens", "0"),
            ("x-ratelimit-reset-tokens", "7.5s"),
            ("x-ratelimit-remaining-requests", "10"),
            ("x-ratelimit-reset-requests", "1h"),
        ]);
        assert_eq!(parse_retry_after(&h), Some(Duration::from_secs_f64(7.5)));
        assert_eq!(parse_retry_after(&HeaderMap::new()), None);
    }
}
