//! Privacy policy: what Mote may observe, and what may leave the machine.
//!
//! Every observation passes through [`PrivacyPolicy::evaluate`] *before* any
//! content is read. Password managers, secure (password) fields and Mote's own
//! windows are always excluded in code; users add their own application and
//! window-title exclusions on top.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::intent::apps::{categorize, AppCategory};
use crate::platform::AppInfo;
use crate::settings::Settings;

/// What an exclusion rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ExclusionKind {
    /// Matches an application by bundle identifier / executable name, or by display name.
    App,
    /// Matches any window whose title contains the keyword (case-insensitive),
    /// e.g. a banking site open in a browser.
    WindowTitle,
}

/// A user-defined exclusion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ExclusionRule {
    pub id: i64,
    pub kind: ExclusionKind,
    /// Bundle id / executable name / app name for `App`; keyword for `WindowTitle`.
    pub pattern: String,
    pub display_name: String,
}

impl ExclusionRule {
    fn matches(&self, app: &AppInfo, title: Option<&str>) -> bool {
        let pattern = self.pattern.trim().to_lowercase();
        if pattern.is_empty() {
            return false;
        }
        match self.kind {
            ExclusionKind::App => app.id.to_lowercase() == pattern || app.name.to_lowercase() == pattern,
            ExclusionKind::WindowTitle => title.is_some_and(|t| t.to_lowercase().contains(&pattern)),
        }
    }
}

/// Why an application is not observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    PasswordManager,
    MoteItself,
    UserApp,
    UserWindowTitle,
}

/// Outcome of evaluating whether Mote may observe the current application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case", tag = "state", content = "reason")]
pub enum ObservationDecision {
    Allowed,
    /// The user paused Mote (globally or for a period).
    Paused,
    /// Assistance or application observation is switched off.
    Disabled,
    Excluded(ExclusionReason),
}

impl ObservationDecision {
    pub fn is_allowed(self) -> bool {
        self == Self::Allowed
    }
}

/// The effective privacy policy, derived from settings and exclusion rules.
#[derive(Debug, Clone, PartialEq)]
pub struct PrivacyPolicy {
    pub assistance_enabled: bool,
    pub paused_until: Option<DateTime<Utc>>,
    pub cloud_ai_enabled: bool,
    pub observe_applications: bool,
    pub observe_text: bool,
    pub observe_clipboard: bool,
    pub usage_analytics: bool,
    pub exclusions: Vec<ExclusionRule>,
}

impl Default for PrivacyPolicy {
    fn default() -> Self {
        Self::from_settings(&Settings::default(), Vec::new())
    }
}

impl PrivacyPolicy {
    pub fn from_settings(settings: &Settings, exclusions: Vec<ExclusionRule>) -> Self {
        Self {
            assistance_enabled: settings.general.assistance_enabled,
            paused_until: settings.general.paused_until,
            cloud_ai_enabled: settings.privacy.cloud_ai_enabled,
            observe_applications: settings.privacy.observe_applications,
            observe_text: settings.privacy.observe_text,
            observe_clipboard: settings.privacy.observe_clipboard,
            usage_analytics: settings.usage.analytics_enabled,
            exclusions,
        }
    }

    pub fn is_paused(&self, now: DateTime<Utc>) -> bool {
        self.paused_until.is_some_and(|until| until > now)
    }

    /// Whether Mote may observe `app` (with the given window title) at all.
    ///
    /// Application observation is a prerequisite for everything else: without
    /// knowing the active app, exclusions cannot be enforced, so nothing is read.
    pub fn evaluate(&self, app: &AppInfo, window_title: Option<&str>, now: DateTime<Utc>) -> ObservationDecision {
        if !self.assistance_enabled || !self.observe_applications {
            return ObservationDecision::Disabled;
        }
        if self.is_paused(now) {
            return ObservationDecision::Paused;
        }
        match categorize(app, None) {
            AppCategory::PasswordManager => return ObservationDecision::Excluded(ExclusionReason::PasswordManager),
            AppCategory::Mote => return ObservationDecision::Excluded(ExclusionReason::MoteItself),
            _ => {}
        }
        for rule in &self.exclusions {
            if rule.matches(app, window_title) {
                return ObservationDecision::Excluded(match rule.kind {
                    ExclusionKind::App => ExclusionReason::UserApp,
                    ExclusionKind::WindowTitle => ExclusionReason::UserWindowTitle,
                });
            }
        }
        ObservationDecision::Allowed
    }

    /// Whether text around the caret may be read for `decision`.
    pub fn may_read_text(&self, decision: ObservationDecision) -> bool {
        decision.is_allowed() && self.observe_text
    }

    /// Whether clipboard content may be read for `decision`.
    pub fn may_read_clipboard(&self, decision: ObservationDecision) -> bool {
        decision.is_allowed() && self.observe_clipboard
    }

    /// Whether content may be sent to the cloud AI provider.
    pub fn may_use_cloud(&self) -> bool {
        self.cloud_ai_enabled
    }
}

/// Names of always-excluded application families, for display.
pub fn always_excluded_descriptions() -> Vec<&'static str> {
    vec![
        "Password managers (1Password, Bitwarden, KeePassXC, LastPass, Dashlane, Keychain Access, Passwords, and others)",
        "Password and other secure text fields in every application",
        "Any application while macOS Secure Input is active",
        "Mote's own windows",
    ]
}

/// Secret-redaction helpers for anything that might reach a log or diagnostics.
pub mod redact {
    const SECRET_PREFIXES: &[&str] =
        &["gsk_", "sk-ant-", "sk-proj-", "sk-", "xoxb-", "xoxp-", "ghp_", "gho_", "github_pat_"];

    fn is_secret_char(c: char) -> bool {
        c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '~' | '+' | '/' | '=')
    }

    /// Replaces API keys and bearer tokens in `input` with `[redacted]`.
    pub fn redact_secrets(input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let mut rest = input;
        'outer: while !rest.is_empty() {
            // Bearer tokens.
            if let Some(after) = strip_prefix_ci(rest, "bearer ") {
                let token_len: usize = after.chars().take_while(|c| is_secret_char(*c)).map(char::len_utf8).sum();
                if token_len >= 8 {
                    out.push_str(&rest[..rest.len() - after.len()]);
                    out.push_str("[redacted]");
                    rest = &after[token_len..];
                    continue;
                }
            }
            for prefix in SECRET_PREFIXES {
                if rest.starts_with(prefix) && word_boundary_before(input, rest) {
                    let token_len: usize = rest.chars().take_while(|c| is_secret_char(*c)).map(char::len_utf8).sum();
                    if token_len >= prefix.len() + 12 {
                        out.push_str("[redacted]");
                        rest = &rest[token_len..];
                        continue 'outer;
                    }
                }
            }
            let ch = rest.chars().next().unwrap_or(' ');
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
        out
    }

    fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
        let head = s.get(..prefix.len())?;
        head.eq_ignore_ascii_case(prefix).then(|| &s[prefix.len()..])
    }

    fn word_boundary_before(full: &str, rest: &str) -> bool {
        let offset = full.len() - rest.len();
        full[..offset].chars().last().is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
    }

    /// Describes text by length only. Use this instead of logging content.
    pub fn describe(text: &str) -> String {
        format!("<{} chars>", text.chars().count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn policy() -> PrivacyPolicy {
        PrivacyPolicy::default()
    }

    #[test]
    fn password_managers_are_always_excluded() {
        let p = policy();
        let now = Utc::now();
        for id in ["com.1password.1password", "bitwarden.exe", "com.apple.keychainaccess"] {
            assert_eq!(
                p.evaluate(&AppInfo::new(id, "x"), None, now),
                ObservationDecision::Excluded(ExclusionReason::PasswordManager)
            );
        }
    }

    #[test]
    fn mote_never_observes_itself() {
        let app = AppInfo::new(crate::intent::apps::MOTE_BUNDLE_ID, "Mote");
        assert_eq!(
            policy().evaluate(&app, None, Utc::now()),
            ObservationDecision::Excluded(ExclusionReason::MoteItself)
        );
    }

    #[test]
    fn user_app_and_title_rules() {
        let mut p = policy();
        p.exclusions = vec![
            ExclusionRule {
                id: 1,
                kind: ExclusionKind::App,
                pattern: "com.example.bank".into(),
                display_name: "Bank".into(),
            },
            ExclusionRule {
                id: 2,
                kind: ExclusionKind::WindowTitle,
                pattern: "NetBanking".into(),
                display_name: "NetBanking".into(),
            },
        ];
        let now = Utc::now();
        assert_eq!(
            p.evaluate(&AppInfo::new("com.example.bank", "Bank"), None, now),
            ObservationDecision::Excluded(ExclusionReason::UserApp)
        );
        assert_eq!(
            p.evaluate(&AppInfo::new("com.google.chrome", "Chrome"), Some("HDFC netbanking - login"), now),
            ObservationDecision::Excluded(ExclusionReason::UserWindowTitle)
        );
        assert_eq!(
            p.evaluate(&AppInfo::new("com.google.chrome", "Chrome"), Some("News"), now),
            ObservationDecision::Allowed
        );
    }

    #[test]
    fn app_rules_match_by_name_case_insensitively() {
        let mut p = policy();
        p.exclusions = vec![ExclusionRule {
            id: 1,
            kind: ExclusionKind::App,
            pattern: "Private Notes".into(),
            display_name: "Private Notes".into(),
        }];
        assert!(!p.evaluate(&AppInfo::new("com.x.notes", "private notes"), None, Utc::now()).is_allowed());
    }

    #[test]
    fn pause_disables_observation_until_expiry() {
        let mut p = policy();
        let now = Utc::now();
        p.paused_until = Some(now + Duration::hours(1));
        let app = AppInfo::new("com.apple.notes", "Notes");
        assert_eq!(p.evaluate(&app, None, now), ObservationDecision::Paused);
        assert_eq!(p.evaluate(&app, None, now + Duration::hours(2)), ObservationDecision::Allowed);
    }

    #[test]
    fn switches_gate_reading() {
        let mut p = policy();
        let app = AppInfo::new("com.apple.notes", "Notes");
        let now = Utc::now();
        assert!(p.may_read_text(p.evaluate(&app, None, now)));
        p.observe_text = false;
        assert!(!p.may_read_text(p.evaluate(&app, None, now)));
        p.observe_clipboard = false;
        assert!(!p.may_read_clipboard(p.evaluate(&app, None, now)));
        p.observe_applications = false;
        assert_eq!(p.evaluate(&app, None, now), ObservationDecision::Disabled);
        p.observe_applications = true;
        p.assistance_enabled = false;
        assert_eq!(p.evaluate(&app, None, now), ObservationDecision::Disabled);
    }

    #[test]
    fn redacts_api_keys_and_bearer_tokens() {
        let input =
            "key=gsk_ABCDEFGHIJKLMNOPQRSTUVWXYZ012345 auth: Bearer abcdefghijklmnop123 sk-proj-abcdefghijklmnopqrstu";
        let out = redact::redact_secrets(input);
        assert!(!out.contains("gsk_ABC"), "{out}");
        assert!(!out.contains("abcdefghijklmnop123"), "{out}");
        assert!(!out.contains("sk-proj-abc"), "{out}");
        assert!(out.contains("key=[redacted]"), "{out}");
        assert!(out.contains("Bearer [redacted]"), "{out}");
    }

    #[test]
    fn redaction_leaves_normal_text() {
        let input = "the task-list skeleton is ready; ask me";
        assert_eq!(redact::redact_secrets(input), input);
        assert_eq!(redact::describe("héllo"), "<5 chars>");
    }
}
