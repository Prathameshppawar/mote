//! Known desktop applications and their categories.
//!
//! The category of the active application is the strongest deterministic signal
//! for intent classification. macOS apps are matched by bundle identifier,
//! Windows apps by lowercase executable name, and browsers are refined by the
//! window title (a Gmail tab is email, a ChatGPT tab is an AI assistant).

use serde::{Deserialize, Serialize};

use crate::platform::AppInfo;

/// Broad application category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum AppCategory {
    Email,
    Chat,
    AiAssistant,
    Ide,
    Terminal,
    Notes,
    Browser,
    Launcher,
    PasswordManager,
    Mote,
    Other,
}

impl AppCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Chat => "chat",
            Self::AiAssistant => "ai_assistant",
            Self::Ide => "ide",
            Self::Terminal => "terminal",
            Self::Notes => "notes",
            Self::Browser => "browser",
            Self::Launcher => "launcher",
            Self::PasswordManager => "password_manager",
            Self::Mote => "mote",
            Self::Other => "other",
        }
    }
}

/// Mote's own bundle identifier and executable; Mote never observes itself.
pub const MOTE_BUNDLE_ID: &str = "io.github.prathameshppawar.mote";
pub const MOTE_EXECUTABLE: &str = "mote.exe";

struct Rule {
    ids: &'static [&'static str],
    prefixes: &'static [&'static str],
    category: AppCategory,
}

static RULES: &[Rule] = &[
    Rule {
        ids: &[MOTE_BUNDLE_ID, MOTE_EXECUTABLE, "mote-desktop", "mote-desktop.exe"],
        prefixes: &[],
        category: AppCategory::Mote,
    },
    Rule {
        ids: &[
            "com.1password.1password",
            "com.agilebits.onepassword7",
            "com.agilebits.onepassword-osx",
            "1password.exe",
            "com.bitwarden.desktop",
            "bitwarden.exe",
            "org.keepassxc.keepassxc",
            "keepassxc.exe",
            "keepass.exe",
            "com.lastpass.lastpass",
            "lastpass.exe",
            "com.dashlane.dashlanephonefinal",
            "dashlane.exe",
            "com.apple.keychainaccess",
            "com.apple.passwords",
            "nordpass.exe",
            "me.proton.pass.electron",
            "proton pass.exe",
            "in.sinew.enpass-desktop",
            "enpass.exe",
            "com.keepersecurity.passwordmanager",
            "keeperpasswordmanager.exe",
        ],
        prefixes: &["com.1password."],
        category: AppCategory::PasswordManager,
    },
    Rule {
        ids: &[
            "com.apple.mail",
            "com.microsoft.outlook",
            "outlook.exe",
            "olk.exe",
            "hxoutlook.exe",
            "com.readdle.smartemail-mac",
            "com.readdle.sparkdesktop",
            "spark.exe",
            "com.superhuman.electron",
            "superhuman.exe",
            "org.mozilla.thunderbird",
            "thunderbird.exe",
            "com.mimestream.mimestream",
            "it.bloop.airmail2",
            "com.mailspring.mailspring",
            "mailspring.exe",
            "mailclient.exe",
        ],
        prefixes: &[],
        category: AppCategory::Email,
    },
    Rule {
        ids: &[
            "com.tinyspeck.slackmacgap",
            "slack.exe",
            "com.microsoft.teams2",
            "com.microsoft.teams",
            "ms-teams.exe",
            "teams.exe",
            "com.hnc.discord",
            "discord.exe",
            "net.whatsapp.whatsapp",
            "desktop.whatsapp",
            "whatsapp.exe",
            "whatsapp.root.exe",
            "ru.keepcoder.telegram",
            "org.telegram.desktop",
            "telegram.exe",
            "com.apple.mobilesms",
            "org.whispersystems.signal-desktop",
            "signal.exe",
            "us.zoom.xos",
            "zoom.exe",
            "com.skype.skype",
            "skype.exe",
            "messenger.exe",
            "im.riot.app",
            "element.exe",
            "mattermost.desktop",
            "mattermost.exe",
            "com.tencent.xinwechat",
            "wechat.exe",
            "jp.naver.line.mac",
            "line.exe",
        ],
        prefixes: &[],
        category: AppCategory::Chat,
    },
    Rule {
        ids: &[
            "com.openai.chat",
            "chatgpt.exe",
            "com.anthropic.claudefordesktop",
            "claude.exe",
            "ai.perplexity.mac",
            "perplexity.exe",
            "com.quora.poe.electron",
            "poe.exe",
            "ai.elementlabs.lmstudio",
            "lm studio.exe",
            "jan.ai.app",
            "jan.exe",
            "msty.exe",
            "app.msty.app",
        ],
        prefixes: &[],
        category: AppCategory::AiAssistant,
    },
    Rule {
        ids: &[
            "com.microsoft.vscode",
            "com.microsoft.vscodeinsiders",
            "com.vscodium",
            "code.exe",
            "code - insiders.exe",
            "codium.exe",
            "com.todesktop.230313mzl4w4u92",
            "cursor.exe",
            "com.exafunction.windsurf",
            "windsurf.exe",
            "dev.zed.zed",
            "dev.zed.zed-preview",
            "zed.exe",
            "com.apple.dt.xcode",
            "com.sublimetext.4",
            "com.sublimetext.3",
            "sublime_text.exe",
            "com.panic.nova",
            "com.barebones.bbedit",
            "devenv.exe",
            "notepad++.exe",
            "eclipse.exe",
            "fleet.exe",
            "org.gnu.emacs",
            "emacs.exe",
            "com.google.android.studio",
            "idea64.exe",
            "pycharm64.exe",
            "webstorm64.exe",
            "goland64.exe",
            "clion64.exe",
            "rider64.exe",
            "rustrover64.exe",
            "datagrip64.exe",
            "phpstorm64.exe",
            "rubymine64.exe",
            "studio64.exe",
        ],
        prefixes: &["com.jetbrains."],
        category: AppCategory::Ide,
    },
    Rule {
        ids: &[
            "com.apple.terminal",
            "com.googlecode.iterm2",
            "dev.warp.warp-stable",
            "net.kovidgoyal.kitty",
            "io.alacritty",
            "org.alacritty",
            "com.mitchellh.ghostty",
            "co.zeit.hyper",
            "com.github.wez.wezterm",
            "windowsterminal.exe",
            "wt.exe",
            "cmd.exe",
            "powershell.exe",
            "pwsh.exe",
            "conhost.exe",
            "openconsole.exe",
            "alacritty.exe",
            "wezterm-gui.exe",
            "warp.exe",
            "mintty.exe",
            "tabby.exe",
            "putty.exe",
            "mobaxterm.exe",
        ],
        prefixes: &[],
        category: AppCategory::Terminal,
    },
    Rule {
        ids: &[
            "com.apple.notes",
            "notion.id",
            "notion.exe",
            "md.obsidian",
            "obsidian.exe",
            "net.shinyfrog.bear",
            "com.microsoft.onenote.mac",
            "onenote.exe",
            "com.evernote.evernote",
            "evernote.exe",
            "com.agiletortoise.drafts-osx",
            "com.logseq.logseq",
            "logseq.exe",
            "com.lukilabs.lukiapp",
            "com.apple.iwork.pages",
            "com.microsoft.word",
            "winword.exe",
            "notepad.exe",
            "com.apple.textedit",
            "abnerworks.typora",
            "typora.exe",
            "com.ulyssesapp.mac",
            "pro.writer.mac",
            "com.linear",
            "linear.exe",
        ],
        prefixes: &[],
        category: AppCategory::Notes,
    },
    Rule {
        ids: &[
            "com.apple.safari",
            "com.google.chrome",
            "com.google.chrome.canary",
            "org.mozilla.firefox",
            "com.microsoft.edgemac",
            "com.brave.browser",
            "company.thebrowser.browser",
            "company.thebrowser.dia",
            "com.operasoftware.opera",
            "com.vivaldi.vivaldi",
            "app.zen-browser.zen",
            "org.chromium.chromium",
            "ai.perplexity.comet",
            "chrome.exe",
            "msedge.exe",
            "firefox.exe",
            "brave.exe",
            "opera.exe",
            "vivaldi.exe",
            "arc.exe",
            "zen.exe",
            "chromium.exe",
            "comet.exe",
        ],
        prefixes: &[],
        category: AppCategory::Browser,
    },
    Rule {
        ids: &[
            "com.apple.spotlight",
            "com.raycast.macos",
            "com.runningwithcrayons.alfred",
            "searchhost.exe",
            "searchapp.exe",
            "powertoys.powerlauncher.exe",
            "flow.launcher.exe",
        ],
        prefixes: &[],
        category: AppCategory::Launcher,
    },
];

/// Browser window-title keywords, checked in order.
static TITLE_RULES: &[(&[&str], AppCategory)] = &[
    (
        &[
            "chatgpt",
            "claude",
            "gemini",
            "perplexity",
            "copilot",
            "grok",
            "deepseek",
            "poe",
            "le chat",
            "huggingchat",
            "meta ai",
            "ai studio",
            "groqchat",
        ],
        AppCategory::AiAssistant,
    ),
    (
        &[
            "gmail",
            "inbox",
            "outlook",
            "yahoo mail",
            "proton mail",
            "fastmail",
            "superhuman",
            "zoho mail",
            "icloud mail",
        ],
        AppCategory::Email,
    ),
    (
        &[
            "slack",
            "whatsapp",
            "discord",
            "microsoft teams",
            "telegram",
            "messenger",
            "google chat",
            "linkedin",
            "instagram",
        ],
        AppCategory::Chat,
    ),
    (
        &[
            "google docs",
            "notion",
            "confluence",
            "dropbox paper",
            "coda",
            "quip",
            "overleaf",
            "jira",
            "linear",
            "asana",
            "trello",
            "clickup",
        ],
        AppCategory::Notes,
    ),
];

/// Classifies an application, refining browsers by window title.
pub fn categorize(app: &AppInfo, window_title: Option<&str>) -> AppCategory {
    let id = app.id.to_lowercase();
    let base = RULES
        .iter()
        .find(|rule| rule.ids.contains(&id.as_str()) || rule.prefixes.iter().any(|p| id.starts_with(p)))
        .map(|rule| rule.category)
        .unwrap_or_else(|| categorize_by_name(&app.name));
    if base == AppCategory::Browser {
        if let Some(title) = window_title {
            let title = title.to_lowercase();
            for (keywords, category) in TITLE_RULES {
                if keywords.iter().any(|k| title_has_word(&title, k)) {
                    return *category;
                }
            }
        }
    }
    base
}

fn title_has_word(title: &str, keyword: &str) -> bool {
    title.match_indices(keyword).any(|(i, _)| {
        let before_ok = title[..i].chars().last().is_none_or(|c| !c.is_alphanumeric());
        let after_ok = title[i + keyword.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        before_ok && after_ok
    })
}

fn categorize_by_name(name: &str) -> AppCategory {
    let name = name.to_lowercase();
    let table: &[(&str, AppCategory)] = &[
        ("1password", AppCategory::PasswordManager),
        ("bitwarden", AppCategory::PasswordManager),
        ("keepass", AppCategory::PasswordManager),
        ("slack", AppCategory::Chat),
        ("whatsapp", AppCategory::Chat),
        ("discord", AppCategory::Chat),
        ("telegram", AppCategory::Chat),
        ("teams", AppCategory::Chat),
        ("chatgpt", AppCategory::AiAssistant),
        ("claude", AppCategory::AiAssistant),
        ("terminal", AppCategory::Terminal),
        ("outlook", AppCategory::Email),
    ];
    table.iter().find(|(needle, _)| name.contains(needle)).map_or(AppCategory::Other, |(_, c)| *c)
}

/// Whether the application is a known password manager.
pub fn is_password_manager(app: &AppInfo) -> bool {
    categorize(app, None) == AppCategory::PasswordManager
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat(id: &str, title: Option<&str>) -> AppCategory {
        categorize(&AppInfo::new(id, id), title)
    }

    #[test]
    fn macos_bundle_ids() {
        assert_eq!(cat("com.tinyspeck.slackmacgap", None), AppCategory::Chat);
        assert_eq!(cat("com.apple.mail", None), AppCategory::Email);
        assert_eq!(cat("com.microsoft.VSCode", None), AppCategory::Ide);
        assert_eq!(cat("com.jetbrains.intellij", None), AppCategory::Ide);
        assert_eq!(cat("com.googlecode.iterm2", None), AppCategory::Terminal);
        assert_eq!(cat("com.openai.chat", None), AppCategory::AiAssistant);
        assert_eq!(cat("com.1password.1password", None), AppCategory::PasswordManager);
        assert_eq!(cat(MOTE_BUNDLE_ID, None), AppCategory::Mote);
    }

    #[test]
    fn windows_executables() {
        assert_eq!(cat("slack.exe", None), AppCategory::Chat);
        assert_eq!(cat("WINWORD.EXE", None), AppCategory::Notes);
        assert_eq!(cat("WindowsTerminal.exe", None), AppCategory::Terminal);
        assert_eq!(cat("Code.exe", None), AppCategory::Ide);
        assert_eq!(cat("KeePassXC.exe", None), AppCategory::PasswordManager);
    }

    #[test]
    fn browsers_are_refined_by_title() {
        assert_eq!(cat("com.google.Chrome", Some("Inbox (3) - me@example.com - Gmail")), AppCategory::Email);
        assert_eq!(cat("chrome.exe", Some("ChatGPT")), AppCategory::AiAssistant);
        assert_eq!(cat("com.apple.Safari", Some("WhatsApp")), AppCategory::Chat);
        assert_eq!(cat("msedge.exe", Some("Project plan - Google Docs")), AppCategory::Notes);
        assert_eq!(cat("com.google.Chrome", Some("Rust documentation")), AppCategory::Browser);
        assert_eq!(cat("com.google.Chrome", None), AppCategory::Browser);
    }

    #[test]
    fn title_keywords_match_whole_words() {
        assert_eq!(cat("com.google.Chrome", Some("Claudette's recipes")), AppCategory::Browser);
        assert_eq!(cat("com.google.Chrome", Some("Poetry archive")), AppCategory::Browser);
    }

    #[test]
    fn unknown_apps_fall_back_to_name() {
        assert_eq!(
            categorize(&AppInfo::new("com.example.unknown", "Bitwarden Beta"), None),
            AppCategory::PasswordManager
        );
        assert_eq!(categorize(&AppInfo::new("com.example.unknown", "Calculator"), None), AppCategory::Other);
    }
}
