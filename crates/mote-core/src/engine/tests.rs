//! End-to-end engine tests with a fake platform, fake shell and scripted
//! provider, on virtual time.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use super::*;
use crate::ai::{AiClient, Routing};
use crate::context::ContextEventKind;
use crate::platform::AppInfo;
use crate::providers::resilient::ResilientProvider;
use crate::providers::testing::{Script, ScriptedProvider};
use crate::providers::ProviderError;
use crate::testing::{focused_input, FakePlatform, FakeShell, PlatformAction, RecordingClipboardLog};
use crate::usage::MemoryUsageSink;

struct Harness {
    handle: EngineHandle,
    platform: Arc<FakePlatform>,
    shell: Arc<FakeShell>,
    provider: Arc<ScriptedProvider>,
    sink: Arc<MemoryUsageSink>,
    events: mpsc::UnboundedReceiver<ContextEvent>,
}

fn harness(scripts: Vec<Script>, settings: Settings) -> Harness {
    let provider = Arc::new(ScriptedProvider::new(scripts));
    let sink = Arc::new(MemoryUsageSink::default());
    let resilient = Arc::new(ResilientProvider::new(provider.clone(), sink.clone()));
    let ai = Arc::new(AiClient::new(resilient, Routing { configured: true, ..Routing::default() }));
    let platform = Arc::new(FakePlatform::default());
    let shell = Arc::new(FakeShell::default());
    let (events_tx, events) = mpsc::unbounded_channel();
    let (engine, handle, rx) = Engine::new(
        EngineDeps {
            platform: platform.clone(),
            ai,
            shell: shell.clone(),
            own_clipboard_writes: Arc::new(RecordingClipboardLog::default()),
            events: Some(events_tx),
        },
        settings,
    );
    tokio::spawn(engine.run(rx));
    Harness { handle, platform, shell, provider, sink, events }
}

impl Harness {
    async fn send(&self, input: EngineInput) {
        self.handle.tx.send(input).await.expect("engine running");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    async fn app(&self, id: &str, name: &str, title: Option<&str>) -> AppInfo {
        let app = AppInfo::new(id, name);
        self.send(EngineInput::Observe(Observation::App {
            app: app.clone(),
            title: title.map(str::to_string),
            decision: ObservationDecision::Allowed,
        }))
        .await;
        app
    }

    async fn focus(&self, input: FocusedInput) {
        self.platform.set_focus(Some(input.clone()));
        self.send(EngineInput::Observe(Observation::Focus(Some(input)))).await;
    }

    async fn type_more(&self, input: &FocusedInput, extra: &str) -> FocusedInput {
        let mut next = input.clone();
        next.text_before_caret.push_str(extra);
        self.focus(next.clone()).await;
        next
    }

    async fn wait(&self, ms: u64) {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }

    fn event_types(&mut self) -> Vec<&'static str> {
        let mut types = Vec::new();
        while let Ok(e) = self.events.try_recv() {
            types.push(e.kind.type_name());
        }
        types
    }
}

const SLACK: (&str, &str) = ("com.tinyspeck.slackmacgap", "Slack");

#[tokio::test(start_paused = true)]
async fn completion_is_suggested_and_accepted_with_tab() {
    let mut h =
        harness(vec![Script::ok("the Redis container was unavailable during startup.", 120, 12)], Settings::default());
    let slack = h.app(SLACK.0, SLACK.1, None).await;
    h.focus(focused_input(slack, 1, "The deployment failed because")).await;
    assert!(h.shell.visible().is_none(), "nothing before the debounce");
    h.wait(600).await;

    let view = h.shell.visible().expect("suggestion visible");
    assert_eq!(view.kind, OverlayKind::Completion);
    assert_eq!(view.text, " the Redis container was unavailable during startup.");
    assert_eq!(view.accept_hint.as_deref(), Some("Tab"));
    assert!(view.anchor.is_some());
    assert!(h.shell.keys_active(), "Tab/Esc registered while visible");

    h.send(EngineInput::Shortcut(ShortcutAction::Accept)).await;
    h.wait(50).await;
    assert_eq!(h.platform.typed(), vec![" the Redis container was unavailable during startup.".to_string()]);
    assert!(h.shell.visible().is_none());
    assert!(!h.shell.keys_active());

    let usage = h.sink.events();
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].feature, Feature::InlineCompletion);
    let types = h.event_types();
    assert!(types.contains(&"suggestion_shown"));
    assert!(types.contains(&"suggestion_accepted"));
}

#[tokio::test(start_paused = true)]
async fn a_failure_releases_the_keys_and_the_engine_carries_on() {
    let h = harness(
        vec![Script::ok("the cache was cold", 50, 5), Script::ok("we lost the connection", 50, 5)],
        Settings::default(),
    );
    let slack = h.app(SLACK.0, SLACK.1, None).await;
    let input = focused_input(slack.clone(), 1, "The deployment failed because");
    h.focus(input.clone()).await;
    h.wait(600).await;
    assert!(h.shell.keys_active(), "suggestion visible with keys registered");

    // Typing through the suggestion re-renders it; that render fails.
    h.shell.panic_on_next_show.store(true, std::sync::atomic::Ordering::SeqCst);
    h.type_more(&input, " the").await;
    assert!(!h.shell.keys_active(), "Tab and Esc are released");
    assert!(h.shell.visible().is_none(), "the overlay is hidden");

    // A fresh engine handles the next input on the same inbox.
    h.app(SLACK.0, SLACK.1, None).await;
    h.focus(focused_input(slack, 2, "The upload stopped because")).await;
    h.wait(600).await;
    assert_eq!(h.shell.visible().map(|v| v.text), Some(" we lost the connection".into()));
}

#[tokio::test(start_paused = true)]
async fn a_failed_classification_is_not_retried_on_every_keystroke() {
    let failures =
        (0..10).map(|_| Script::fail(ProviderError::Server { status: 503, message: "busy".into() })).collect();
    let h = harness(failures, Settings::default());
    // A generic browser field: too ambiguous to classify locally.
    let chrome = h.app("com.google.Chrome", "Google Chrome", Some("Untitled")).await;
    let mut input = focused_input(chrome, 3, "");
    input.role = crate::platform::InputRole::TextField;
    input.is_multiline = false;
    h.focus(input.clone()).await;
    let mut current = input;
    for word in ["something", " about", " the", " quarterly", " numbers", " and", " plans"] {
        current = h.type_more(&current, word).await;
        // Long enough for each failed request (and its retry) to finish.
        h.wait(1_500).await;
    }
    let classifications = h.provider.calls().iter().filter(|c| c.feature == Feature::IntentClassification).count();
    assert!(classifications >= 1, "the ambiguous field was sent for classification");
    assert!(classifications <= 2, "one request (plus its retry), not one per keystroke: {classifications}");
}

#[tokio::test(start_paused = true)]
async fn motes_own_windows_do_not_report_an_excluded_app() {
    let h = harness(vec![], Settings::default());
    h.send(EngineInput::Observe(Observation::App {
        app: AppInfo::new(crate::intent::apps::MOTE_BUNDLE_ID, "Mote"),
        title: Some("Mote".into()),
        decision: ObservationDecision::Excluded(crate::privacy::ExclusionReason::MoteItself),
    }))
    .await;
    assert_eq!(h.shell.last_status().map(|s| s.state), Some(EngineState::Idle));
    h.send(EngineInput::Observe(Observation::App {
        app: AppInfo::new("com.example.bank", "Bank"),
        title: None,
        decision: ObservationDecision::Excluded(crate::privacy::ExclusionReason::UserApp),
    }))
    .await;
    assert_eq!(h.shell.last_status().map(|s| s.state), Some(EngineState::Excluded));
}

#[tokio::test(start_paused = true)]
async fn typing_through_a_suggestion_keeps_it_until_text_diverges() {
    let h = harness(vec![Script::ok("the cache was cold", 50, 5)], Settings::default());
    let slack = h.app(SLACK.0, SLACK.1, None).await;
    let input = focused_input(slack, 1, "The deployment failed because");
    h.focus(input.clone()).await;
    h.wait(600).await;
    let typed = h.type_more(&input, " the c").await;
    assert_eq!(h.shell.visible().unwrap().text, "ache was cold");
    h.type_more(&typed, "lock").await;
    assert!(h.shell.visible().is_none(), "diverging text hides the suggestion");
}

#[tokio::test(start_paused = true)]
async fn typing_while_a_request_is_in_flight_discards_the_stale_result() {
    let h = harness(
        vec![Script::ok("stale", 10, 2).with_delay(Duration::from_millis(400)), Script::ok("fresh words", 10, 2)],
        Settings::default(),
    );
    let slack = h.app(SLACK.0, SLACK.1, None).await;
    let input = focused_input(slack, 1, "The deployment failed because");
    h.focus(input.clone()).await;
    h.wait(500).await; // request in flight
    h.type_more(&input, " of").await;
    h.wait(300).await;
    assert!(h
        .shell
        .calls()
        .iter()
        .all(|c| !matches!(c, crate::testing::ShellCall::Show(v) if v.text.contains("stale"))));
    h.wait(1_500).await;
    assert_eq!(h.shell.visible().map(|v| v.text), Some(" fresh words".to_string()));
    let statuses: Vec<_> = h.sink.events().iter().map(|e| e.status).collect();
    assert!(statuses.contains(&crate::usage::UsageStatus::Cancelled), "{statuses:?}");
}

#[tokio::test(start_paused = true)]
async fn dismissed_suggestions_are_not_requested_again() {
    let h = harness(vec![Script::ok("the cache was cold", 50, 5)], Settings::default());
    let slack = h.app(SLACK.0, SLACK.1, None).await;
    let input = focused_input(slack, 1, "The deployment failed because");
    h.focus(input.clone()).await;
    h.wait(600).await;
    assert!(h.shell.visible().is_some());
    h.send(EngineInput::Shortcut(ShortcutAction::Dismiss)).await;
    assert!(h.shell.visible().is_none());
    // Caret moves away and back: same text, no new request or suggestion.
    let mut moved = input.clone();
    moved.caret_rect = None;
    h.focus(moved).await;
    h.focus(input).await;
    h.wait(3_000).await;
    assert_eq!(h.provider.calls().len(), 1);
    assert!(h.shell.visible().is_none());
}

#[tokio::test(start_paused = true)]
async fn next_requests_an_alternative_and_previous_returns() {
    let h = harness(
        vec![Script::ok("the cache was cold", 50, 5), Script::ok("the database migrated slowly", 50, 6)],
        Settings::default(),
    );
    let slack = h.app(SLACK.0, SLACK.1, None).await;
    h.focus(focused_input(slack, 1, "The deployment failed because")).await;
    h.wait(600).await;
    h.send(EngineInput::Shortcut(ShortcutAction::Next)).await;
    h.wait(200).await;
    let view = h.shell.visible().unwrap();
    assert_eq!(view.text, " the database migrated slowly");
    assert_eq!((view.index, view.count), (1, 2));
    let second = &h.provider.calls()[1];
    assert!(second.messages[0].content.contains("different continuation"), "alternative asks for something different");
    h.send(EngineInput::Shortcut(ShortcutAction::Previous)).await;
    assert_eq!(h.shell.visible().unwrap().text, " the cache was cold");
}

#[tokio::test(start_paused = true)]
async fn spelling_correction_is_offered_and_applied() {
    let h = harness(vec![], Settings::default());
    let slack = h.app(SLACK.0, SLACK.1, None).await;
    h.focus(focused_input(slack, 1, "I wanted to inform you that the testing is completd. ")).await;
    h.wait(800).await;
    let view = h.shell.visible().expect("correction visible");
    assert_eq!(view.kind, OverlayKind::Correction);
    assert_eq!(view.detail.as_deref(), Some("completd → completed"));
    h.send(EngineInput::Shortcut(ShortcutAction::Accept)).await;
    h.wait(50).await;
    assert_eq!(
        h.platform.text_before_caret().as_deref(),
        Some("I wanted to inform you that the testing is completed. ")
    );
    assert!(h.platform.actions().contains(&PlatformAction::Key(Key::Backspace, 10)));
    assert!(h.provider.calls().is_empty(), "local spelling costs no tokens");
}

#[tokio::test(start_paused = true)]
async fn ai_grammar_check_runs_only_for_risky_sentences() {
    let h = harness(vec![Script::ok("We don't have the logs yet.", 40, 10)], Settings::default());
    let slack = h.app(SLACK.0, SLACK.1, None).await;
    let input = focused_input(slack, 1, "We shipped the release on time. ");
    h.focus(input.clone()).await;
    h.wait(1_000).await;
    assert!(h.provider.calls().is_empty(), "no risk, no request");
    h.type_more(&input, "We dont have the logs yet. ").await;
    h.wait(1_000).await;
    assert_eq!(h.provider.calls().len(), 1);
    assert_eq!(h.provider.calls()[0].feature, Feature::WritingAssistance);
    let view = h.shell.visible().unwrap();
    assert_eq!(view.kind, OverlayKind::Correction);
    assert_eq!(view.detail.as_deref(), Some("dont → don't"));
}

#[tokio::test(start_paused = true)]
async fn hinglish_is_completed_in_hinglish() {
    let h = harness(vec![Script::ok("kar denge", 40, 4)], Settings::default());
    let whatsapp = h.app("net.whatsapp.WhatsApp", "WhatsApp", None).await;
    h.focus(focused_input(whatsapp, 1, "bhai kal deployment ka kaam")).await;
    h.wait(600).await;
    let call = &h.provider.calls()[0];
    assert!(call.messages[0].content.contains("Hindi"), "{}", call.messages[0].content);
    assert!(call.messages[0].content.contains("Do not translate"));
    assert_eq!(h.shell.visible().unwrap().text, " kar denge");
}

#[tokio::test(start_paused = true)]
async fn code_and_terminal_contexts_get_no_completions() {
    let h = harness(vec![], Settings::default());
    let term = h.app("com.apple.Terminal", "Terminal", None).await;
    let mut input = focused_input(term, 1, "git commit -m \"fix the build\" && git push");
    input.role = crate::platform::InputRole::Terminal;
    input.is_multiline = false;
    h.focus(input).await;
    h.wait(3_000).await;
    assert!(h.provider.calls().is_empty());
    assert!(h.shell.visible().is_none());
}

#[tokio::test(start_paused = true)]
async fn excluded_applications_are_ignored_entirely() {
    let h = harness(vec![], Settings::default());
    let bank = AppInfo::new("com.example.bank", "Bank");
    h.send(EngineInput::Observe(Observation::App {
        app: bank.clone(),
        title: None,
        decision: ObservationDecision::Excluded(crate::privacy::ExclusionReason::UserApp),
    }))
    .await;
    h.focus(focused_input(bank, 1, "transfer the amount to the account because")).await;
    h.wait(3_000).await;
    assert!(h.provider.calls().is_empty());
    assert_eq!(h.shell.last_status().unwrap().state, EngineState::Excluded);
}

#[tokio::test(start_paused = true)]
async fn tab_without_a_suggestion_is_passed_through() {
    let h = harness(vec![], Settings::default());
    h.send(EngineInput::Shortcut(ShortcutAction::Accept)).await;
    h.wait(100).await;
    assert_eq!(h.platform.actions(), vec![PlatformAction::Key(Key::Tab, 1)]);
}

#[tokio::test(start_paused = true)]
async fn stale_suggestion_at_accept_time_passes_tab_through() {
    let h = harness(vec![Script::ok("the cache was cold", 50, 5)], Settings::default());
    let slack = h.app(SLACK.0, SLACK.1, None).await;
    let input = focused_input(slack.clone(), 1, "The deployment failed because");
    h.focus(input).await;
    h.wait(600).await;
    // The app's text changed but the observer has not reported it yet.
    h.platform.set_focus(Some(focused_input(slack, 1, "Something else entirely")));
    h.send(EngineInput::Shortcut(ShortcutAction::Accept)).await;
    h.wait(100).await;
    assert!(h.platform.typed().is_empty());
    assert_eq!(h.platform.actions(), vec![PlatformAction::Key(Key::Tab, 1)]);
}

#[tokio::test(start_paused = true)]
async fn tab_after_switching_apps_reads_nothing_from_the_new_app() {
    let h = harness(vec![Script::ok("the cache was cold", 50, 5)], Settings::default());
    let slack = h.app(SLACK.0, SLACK.1, None).await;
    h.focus(focused_input(slack, 1, "The deployment failed because")).await;
    h.wait(600).await;
    assert!(h.shell.visible().is_some(), "suggestion shown");
    // Focus moved to a password manager; the observer has not polled yet.
    let reads_before = h.platform.focused_reads.load(std::sync::atomic::Ordering::SeqCst);
    h.platform.set_focus(Some(focused_input(AppInfo::new("com.1password.1password", "1Password"), 9, "hunter2")));
    h.send(EngineInput::Shortcut(ShortcutAction::Accept)).await;
    h.wait(100).await;
    assert_eq!(h.platform.focused_reads.load(std::sync::atomic::Ordering::SeqCst), reads_before, "nothing was read");
    assert!(h.platform.typed().is_empty());
    assert_eq!(h.platform.actions(), vec![PlatformAction::Key(Key::Tab, 1)], "Tab reaches the new app");
}

#[tokio::test(start_paused = true)]
async fn copied_email_then_ide_prompt_offers_context() {
    let h = harness(vec![], Settings::default());
    let chrome = h.app("com.google.Chrome", "Google Chrome", Some("Inbox - Gmail")).await;
    h.send(EngineInput::Observe(Observation::Clipboard {
        text: "Hi team,\n\nThe CSV export fails for Safari users since Monday's release.\nPlease investigate.\n\nThanks,\nAsha".into(),
        source_app: Some(chrome),
    }))
    .await;
    let code = h.app("com.microsoft.VSCode", "Visual Studio Code", None).await;
    let mut input = focused_input(code, 7, "");
    input.placeholder = Some("Ask Copilot".into());
    h.focus(input).await;
    let view = h.shell.visible().expect("context chip");
    assert_eq!(view.kind, OverlayKind::Context);
    assert!(view.text.contains("Google Chrome"), "{}", view.text);
    assert!(view.detail.as_deref().unwrap_or("").contains("Create coding task"));
    assert!(!h.shell.keys_active(), "chips never capture Tab");
    assert!(h.handle.snapshot().context_suggestion.is_some());
    h.wait(9_000).await;
    assert!(h.shell.visible().is_none(), "chips expire");
}

#[tokio::test(start_paused = true)]
async fn rate_limits_pause_automatic_requests_and_report_status() {
    let h = harness(
        vec![Script::fail(ProviderError::RateLimited { retry_after: Some(Duration::from_secs(30)), snapshot: None })],
        Settings::default(),
    );
    let slack = h.app(SLACK.0, SLACK.1, None).await;
    let input = focused_input(slack, 1, "The deployment failed because");
    h.focus(input.clone()).await;
    h.wait(600).await;
    assert_eq!(h.shell.last_status().unwrap().state, EngineState::RateLimited);
    h.type_more(&input, " the").await;
    h.wait(3_000).await;
    assert_eq!(h.provider.calls().len(), 1, "no requests while rate limited");
}

#[tokio::test(start_paused = true)]
async fn prompt_fields_show_an_enhancement_hint_once() {
    let mut settings = Settings::default();
    settings.completion.in_prompts = false;
    let h = harness(vec![], settings);
    let chat = h.app("com.openai.chat", "ChatGPT", None).await;
    let mut input = focused_input(chat, 3, "fix this code it is giving error");
    input.placeholder = Some("Ask anything".into());
    h.focus(input.clone()).await;
    h.wait(2_500).await;
    let view = h.shell.visible().expect("hint");
    assert_eq!(view.kind, OverlayKind::PromptHint);
    h.wait(9_000).await;
    h.type_more(&input, " please").await;
    h.wait(3_000).await;
    let hints = h
        .shell
        .calls()
        .iter()
        .filter(|c| matches!(c, crate::testing::ShellCall::Show(v) if v.kind == OverlayKind::PromptHint))
        .count();
    assert_eq!(hints, 1);
}

#[tokio::test(start_paused = true)]
async fn missing_api_key_is_reported_without_requests() {
    let provider = Arc::new(ScriptedProvider::new(vec![]));
    let sink = Arc::new(MemoryUsageSink::default());
    let ai = Arc::new(AiClient::new(Arc::new(ResilientProvider::new(provider.clone(), sink)), Routing::default()));
    let shell = Arc::new(FakeShell::default());
    let (engine, handle, rx) = Engine::new(
        EngineDeps {
            platform: Arc::new(FakePlatform::default()),
            ai,
            shell: shell.clone(),
            own_clipboard_writes: Arc::new(RecordingClipboardLog::default()),
            events: None,
        },
        Settings::default(),
    );
    tokio::spawn(engine.run(rx));
    handle
        .tx
        .send(EngineInput::Observe(Observation::Focus(Some(focused_input(
            AppInfo::new(SLACK.0, SLACK.1),
            1,
            "The deployment failed because",
        )))))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(provider.calls().is_empty());
    assert_eq!(shell.last_status().unwrap().state, EngineState::NeedsApiKey);
}

#[test]
fn shortcut_labels() {
    let label = shortcut_label("CommandOrControl+Shift+Space");
    if cfg!(target_os = "macos") {
        assert_eq!(label, "⌘⇧Space");
    } else {
        assert_eq!(label, "Ctrl+Shift+Space");
    }
}

#[test]
fn context_event_kinds_are_metadata_only() {
    let kinds = [ContextEventKind::SuggestionShown { feature: Feature::InlineCompletion }, ContextEventKind::Resumed];
    for k in kinds {
        let json = serde_json::to_string(&k).unwrap();
        assert!(!json.contains("text"));
    }
}
