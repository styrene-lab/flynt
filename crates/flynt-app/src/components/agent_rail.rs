use crate::acp::{AcpEvent, AcpSession, ConfigOption, SlashCommand};
use crate::bootstrap::AppContext;
use crate::state::SettingsPage;
use comrak::{Options, markdown_to_html};
use dioxus::prelude::*;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::TryRecvError;

/// Resolve the Omegon binary using the centralized channel-aware resolver.
pub fn find_omegon_binary_public() -> Option<PathBuf> {
    // Caller should use ctx.omegon().resolve_binary() when context is available.
    // This fallback uses default config for contexts where AppContext isn't accessible.
    let path = flynt_core::models::resolve_omegon_binary(
        &flynt_core::models::LocalRuntimeConfig::default(),
    );
    if path.exists() { Some(path) } else { None }
}

fn find_omegon_binary_from_ctx(ctx: &crate::bootstrap::AppContext) -> Option<PathBuf> {
    let path = ctx.omegon().resolve_binary();
    if path.exists() { Some(path) } else { None }
}

/// Extract version from binary path like `~/.omegon/versions/0.18.5/omegon`.
fn version_from_binary_path(path: &Path) -> Option<String> {
    let parent = path.parent()?;
    let version_dir = parent.file_name()?.to_str()?;
    if version_dir.chars().next()?.is_ascii_digit() || version_dir.starts_with('v') {
        Some(version_dir.to_string())
    } else {
        None
    }
}

fn render_md(content: &str) -> String {
    let mut opts = Options::default();
    opts.extension.table = true;
    opts.extension.strikethrough = true;
    opts.extension.tasklist = true;
    opts.extension.autolink = true;
    opts.extension.footnotes = true;
    opts.render.unsafe_ = true;
    let html = markdown_to_html(content, &opts);

    // Replace autolinked external URLs with smart badges
    let mut out = String::with_capacity(html.len());
    let mut pos = 0;
    while let Some(start) = html[pos..].find("<a href=\"http") {
        let abs = pos + start;
        if let Some(close) = html[abs..].find("</a>") {
            let end = abs + close + 4;
            let tag = &html[abs..end];
            if let (Some(hs), Some(he)) = (tag.find("href=\""), tag.find("\">")) {
                let href = &tag[hs + 6..he];
                let text = &tag[he + 2..tag.len() - 4];
                if text.starts_with("http") {
                    let ext = flynt_core::external_ref::parse_ref(href);
                    if ext.provider != flynt_core::external_ref::Provider::Generic {
                        out.push_str(&html[pos..abs]);
                        out.push_str(&flynt_core::external_ref::render_html(&ext));
                        pos = end;
                        continue;
                    }
                }
            }
            out.push_str(&html[pos..end]);
            pos = end;
        } else {
            break;
        }
    }
    out.push_str(&html[pos..]);
    out
}

fn tool_kind_label(kind: &str) -> &str {
    match kind {
        "Read" => "Read",
        "Edit" => "Edit",
        "Delete" => "Delete",
        "Move" => "Move",
        "Search" => "Search",
        "Execute" => "Run",
        "Think" => "Think",
        "Fetch" => "Fetch",
        "SwitchMode" => "Mode",
        _ => "Tool",
    }
}

/// Render a one-line summary of a tool's input args, e.g.
/// `path="Foo.md", limit=20`. Truncates long values so the chat row
/// stays readable. Empty string when there are no args.
fn summarize_tool_args(args: Option<&serde_json::Value>) -> String {
    let Some(serde_json::Value::Object(map)) = args else {
        return String::new();
    };
    if map.is_empty() {
        return String::new();
    }
    let mut parts: Vec<String> = Vec::with_capacity(map.len());
    for (k, v) in map.iter() {
        let rendered = match v {
            serde_json::Value::String(s) => {
                let trimmed = if s.len() > 60 {
                    format!("{}…", &s[..57])
                } else {
                    s.clone()
                };
                format!("{k}={trimmed:?}")
            }
            serde_json::Value::Null => format!("{k}=null"),
            serde_json::Value::Bool(b) => format!("{k}={b}"),
            serde_json::Value::Number(n) => format!("{k}={n}"),
            serde_json::Value::Array(a) => format!("{k}=[{} items]", a.len()),
            serde_json::Value::Object(o) => format!("{k}={{{} fields}}", o.len()),
        };
        parts.push(rendered);
    }
    let joined = parts.join(", ");
    if joined.len() > 140 {
        format!("{}…", &joined[..137])
    } else {
        joined
    }
}

/// Save a config option to operator settings on disk.
fn persist_config(ctx: &AppContext, config_id: &str, value: &str) {
    let omegon = ctx.omegon();
    let mut settings = omegon.load_operator_settings();
    settings
        .acp_config
        .insert(config_id.to_string(), value.to_string());
    if let Err(e) = omegon.save_operator_settings(&settings) {
        tracing::warn!("Failed to persist config: {e}");
    }
}

fn ensure_explicit_acp_defaults(
    omegon: &crate::bootstrap::OmegonRuntimeContext,
    mut settings: flynt_core::models::FlyntOperatorSettings,
) -> flynt_core::models::FlyntOperatorSettings {
    let profile = omegon.load_project_profile();
    let config =
        crate::components::omegon::config_bridge::UnifiedOmegonConfig::load(&profile, &settings);
    let explicit_config = config.to_acp_config();

    if settings.acp_config != explicit_config {
        settings.acp_config = explicit_config;
        if let Err(e) = omegon.save_operator_settings(&settings) {
            tracing::warn!("Failed to persist explicit ACP defaults: {e}");
        }
    }

    settings
}

fn is_transport_disconnect(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("broken pipe")
        || lower.contains("os error 32")
        || lower.contains("connection closed")
        || lower.contains("closed connection")
        || lower.contains("connection reset")
        || lower.contains("transport disconnected")
        || lower.contains("extension closed connection")
        || lower.contains("extension process not running")
        || lower.contains("channel closed")
}

fn reconnect_acp_session(
    ctx: AppContext,
    mut session: Signal<Option<Rc<AcpSession>>>,
    mut shared_session: Signal<Option<Rc<AcpSession>>>,
    items: Signal<Vec<ChatItem>>,
    agent_status: Signal<AgentStatus>,
    available_commands: Signal<Vec<SlashCommand>>,
    config_options: Signal<Vec<ConfigOption>>,
    session_title: Signal<Option<String>>,
) {
    let mut items = items;
    let mut agent_status = agent_status;
    let mut available_commands = available_commands;
    let mut config_options = config_options;
    let mut session_title = session_title;

    spawn(async move {
        if *agent_status.read() == AgentStatus::Connecting {
            return;
        }

        let Some(binary) = find_omegon_binary_from_ctx(&ctx) else {
            items.write().push(ChatItem::Message {
                role: ChatRole::Assistant,
                content: "Agent transport disconnected, and the Omegon binary could not be resolved for reconnect.".into(),
            });
            *agent_status.write() = AgentStatus::Idle;
            return;
        };

        tracing::warn!("Reconnecting ACP session after transport disconnect");
        *agent_status.write() = AgentStatus::Connecting;
        *session.write() = None;
        *shared_session.write() = None;
        available_commands.write().clear();
        config_options.write().clear();
        *session_title.write() = None;

        let project = ctx.project_root();
        let operator_settings =
            ensure_explicit_acp_defaults(&ctx.omegon(), ctx.omegon().load_operator_settings());
        let saved_config = operator_settings.acp_config.clone();
        let agent_id = operator_settings.agent_id.clone();

        match AcpSession::connect(binary, project, agent_id).await {
            Ok((s, rx)) => {
                let sess = Rc::new(s);
                for (cfg_id, value) in &saved_config {
                    sess.set_config(cfg_id, value).await;
                }

                *session.write() = Some(sess.clone());
                *shared_session.write() = Some(sess);
                start_event_loop(
                    rx,
                    ctx.clone(),
                    items,
                    agent_status,
                    available_commands,
                    config_options,
                    session_title,
                    session,
                    shared_session,
                );
                items.write().push(ChatItem::Message {
                    role: ChatRole::Assistant,
                    content:
                        "Agent transport reconnected. Retry the last request if it was interrupted."
                            .into(),
                });
                *agent_status.write() = AgentStatus::Idle;
            }
            Err(e) => {
                tracing::error!("ACP reconnect failed: {e}");
                items.write().push(ChatItem::Message {
                    role: ChatRole::Assistant,
                    content: format!("Agent transport reconnect failed: {e}"),
                });
                *agent_status.write() = AgentStatus::Idle;
            }
        }
    });
}

/// Start the event polling loop for an ACP session.
fn start_event_loop(
    rx: std::sync::mpsc::Receiver<AcpEvent>,
    ctx: AppContext,
    items: Signal<Vec<ChatItem>>,
    agent_status: Signal<AgentStatus>,
    available_commands: Signal<Vec<SlashCommand>>,
    config_options: Signal<Vec<ConfigOption>>,
    session_title: Signal<Option<String>>,
    session: Signal<Option<Rc<AcpSession>>>,
    shared_session: Signal<Option<Rc<AcpSession>>>,
) {
    let mut items = items;
    let mut agent_status = agent_status;
    let mut available_commands = available_commands;
    let mut config_options = config_options;
    let mut session_title = session_title;
    spawn(async move {
        let mut pending_text = String::new();
        let mut pending_thought = String::new();
        let mut last_flush = std::time::Instant::now();

        loop {
            let mut saw_event = false;
            loop {
                match rx.try_recv() {
                    Ok(AcpEvent::TextDelta(text)) => {
                        tracing::info!("ACP TextDelta: {} bytes", text.len());
                        pending_text.push_str(&text);
                        saw_event = true;
                    }
                    Ok(AcpEvent::ThoughtDelta(text)) => {
                        tracing::info!("ACP ThoughtDelta: {} bytes", text.len());
                        pending_thought.push_str(&text);
                        saw_event = true;
                    }
                    Ok(event) => {
                        flush_pending_deltas(
                            &mut items,
                            &mut agent_status,
                            &mut pending_text,
                            &mut pending_thought,
                        );
                        last_flush = std::time::Instant::now();
                        handle_acp_event(
                            event,
                            ctx.clone(),
                            &mut items,
                            &mut agent_status,
                            &mut available_commands,
                            &mut config_options,
                            &mut session_title,
                            session,
                            shared_session,
                        );
                        saw_event = true;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        flush_pending_deltas(
                            &mut items,
                            &mut agent_status,
                            &mut pending_text,
                            &mut pending_thought,
                        );
                        tracing::warn!("ACP event channel disconnected");
                        return;
                    }
                }
            }

            if last_flush.elapsed() >= std::time::Duration::from_millis(50) {
                flush_pending_deltas(
                    &mut items,
                    &mut agent_status,
                    &mut pending_text,
                    &mut pending_thought,
                );
                last_flush = std::time::Instant::now();
            }

            let sleep_ms = if saw_event { 8 } else { 16 };
            tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
        }
    });
}

fn flush_pending_deltas(
    items: &mut Signal<Vec<ChatItem>>,
    status: &mut Signal<AgentStatus>,
    pending_text: &mut String,
    pending_thought: &mut String,
) {
    if !pending_text.is_empty() {
        append_assistant_text(items, pending_text);
    }

    if !pending_thought.is_empty() {
        *status.write() = AgentStatus::Thinking;
        append_thought_text(items, pending_thought);
    }
}

fn append_assistant_text(items: &mut Signal<Vec<ChatItem>>, text: &mut String) {
    let mut list = items.write();
    if let Some(ChatItem::Message {
        role: ChatRole::Assistant,
        content,
    }) = list.last_mut()
    {
        content.push_str(text);
    } else {
        list.push(ChatItem::Message {
            role: ChatRole::Assistant,
            content: text.clone(),
        });
    }
    text.clear();
}

fn append_thought_text(items: &mut Signal<Vec<ChatItem>>, text: &mut String) {
    let mut list = items.write();
    if let Some(ChatItem::Thought { content }) = list.last_mut() {
        content.push_str(text);
    } else {
        list.push(ChatItem::Thought {
            content: text.clone(),
        });
    }
    text.clear();
}

#[derive(Clone, PartialEq)]
enum ChatRole {
    User,
    Assistant,
}

#[derive(Clone, PartialEq)]
struct ToolCallBlock {
    id: String,
    title: String,
    kind: String,
    status: String,
    /// Short summary of the tool's input args, e.g. `path="Foo.md", limit=20`.
    /// Empty if no args were emitted.
    args_summary: String,
    /// Text output from the tool. Populated by ToolCallUpdated.content
    /// once omegon ships it. Empty until the tool produces output.
    output: String,
}

#[derive(Clone, PartialEq)]
enum ChatItem {
    Message {
        role: ChatRole,
        content: String,
    },
    Thought {
        content: String,
    },
    ToolCall(ToolCallBlock),
    /// The agent's full execution plan. Replaces any prior Plan item
    /// (omegon emits the complete entry list with each update).
    Plan(Vec<crate::acp::PlanItem>),
}

#[derive(Clone, Copy, PartialEq)]
enum AgentStatus {
    Idle,
    Connecting,
    Thinking,
    ToolRunning,
    AuthExpired,
}

impl AgentStatus {
    fn label(&self) -> &'static str {
        match self {
            Self::Idle => "ready",
            Self::Connecting => "connecting…",
            Self::Thinking => "thinking…",
            Self::ToolRunning => "running tool…",
            Self::AuthExpired => "auth expired",
        }
    }
    fn css_class(&self) -> &'static str {
        match self {
            Self::Idle => "agent-status-badge connected",
            Self::Connecting => "agent-status-badge connecting",
            Self::AuthExpired => "agent-status-badge disconnected",
            _ => "agent-status-badge active",
        }
    }
    fn is_busy(&self) -> bool {
        !matches!(self, Self::Idle | Self::AuthExpired)
    }
}

#[component]
pub fn AgentRail() -> Element {
    let ctx = use_context::<AppContext>();
    let setup_refresh = use_context::<crate::omegon_setup::OmegonSetupRefresh>();

    let mut input = use_signal(String::new);
    let mut items: Signal<Vec<ChatItem>> = use_signal(Vec::new);
    let mut agent_status = use_signal(|| AgentStatus::Connecting);
    let mut session: Signal<Option<Rc<AcpSession>>> = use_signal(|| None);
    let mut shared_session = use_context::<Signal<Option<Rc<AcpSession>>>>();
    let available_commands: Signal<Vec<SlashCommand>> = use_signal(Vec::new);
    // Session title pushed by omegon via SessionInfoUpdate (typically derived
    // from the first user prompt). None until omegon picks one or after a
    // clear; rendered in the status bar when present.
    let session_title: Signal<Option<String>> = use_signal(|| None);
    let config_options = use_context::<Signal<Vec<ConfigOption>>>();

    // Input history (up/down arrow)
    let mut history: Signal<Vec<String>> = use_signal(Vec::new);
    let mut history_idx: Signal<Option<usize>> = use_signal(|| None);

    let omegon_binary = find_omegon_binary_from_ctx(&ctx);
    let binary_found = omegon_binary.is_some();

    // ── Eager connect on mount + apply saved config ─────────
    use_effect(move || {
        let _ = setup_refresh.0.read();
        let binary = match find_omegon_binary_from_ctx(&ctx) {
            Some(b) => {
                tracing::info!("Omegon binary resolved: {}", b.display());
                b
            }
            None => {
                tracing::warn!("Omegon binary not found — agent panel disabled");
                *agent_status.write() = AgentStatus::Idle;
                return;
            }
        };
        let project = ctx.project_root();
        tracing::info!(
            "Connecting ACP session: project={}, binary={}",
            project.display(),
            binary.display()
        );
        let operator_settings =
            ensure_explicit_acp_defaults(&ctx.omegon(), ctx.omegon().load_operator_settings());
        let saved_config = operator_settings.acp_config.clone();
        let agent_id = operator_settings.agent_id.clone();

        spawn(async move {
            tracing::info!("ACP connect starting… saved_config={:?}", saved_config);
            match AcpSession::connect(binary, project, agent_id).await {
                Ok((s, rx)) => {
                    tracing::info!("ACP session connected successfully");
                    let sess = Rc::new(s);

                    // Apply persisted config options
                    for (cfg_id, value) in &saved_config {
                        tracing::info!("Restoring config {cfg_id}={value}");
                        sess.set_config(cfg_id, value).await;
                    }

                    *session.write() = Some(sess.clone());
                    *shared_session.write() = Some(sess.clone());
                    start_event_loop(
                        rx,
                        ctx.clone(),
                        items,
                        agent_status,
                        available_commands,
                        config_options,
                        session_title,
                        session,
                        shared_session,
                    );
                    *agent_status.write() = AgentStatus::Idle;
                    tracing::info!("ACP event loop started, agent ready");

                    // Check if the active model's provider is healthy
                    if let Ok(resp) = sess.provider_status().await {
                        if let Some(text) = resp["text"].as_str() {
                            let has_expired = text.lines().any(|l| l.contains(":expired:"));
                            if has_expired {
                                *agent_status.write() = AgentStatus::AuthExpired;
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("ACP connect failed: {e}");
                    *agent_status.write() = AgentStatus::Idle;
                }
            }
        });
    });

    // ── Sticky-bottom scroll behavior ────────────────────────────────
    // Auto-scrolls the messages area as content streams in, but only while
    // the user is parked at (or very near) the bottom. When the user
    // scrolls up to read history we stop yanking them back. Re-pinning is
    // either implicit (scroll to within 16px of bottom) or explicit (the
    // jump-to-bottom control). Implemented in JS because Dioxus doesn't
    // expose scrollTop/scrollHeight via signals; the JS surface also
    // backs the prev/next user-message navigation buttons below.
    use_effect(move || {
        document::eval(
            r#"
            (function install() {
                const root = document.querySelector('.agent-rail');
                const messages = root && root.querySelector('.agent-messages');
                if (!root || !messages) { return setTimeout(install, 100); }
                if (messages.dataset.flyntScrollWired === '1') return;
                messages.dataset.flyntScrollWired = '1';

                const PIN_THRESHOLD = 16;  // px from bottom that still counts as "pinned"

                function isAtBottom() {
                    return messages.scrollHeight - messages.scrollTop - messages.clientHeight <= PIN_THRESHOLD;
                }
                function syncPinClass() {
                    root.classList.toggle('agent-pinned', isAtBottom());
                }
                function scrollToBottom() {
                    messages.scrollTop = messages.scrollHeight;
                    syncPinClass();
                }

                // Observe DOM changes inside messages (new chat items, streaming
                // delta appends, tool-call status updates) and auto-scroll only
                // while the user is pinned.
                let scrollFrame = 0;
                function scheduleScrollToBottom() {
                    if (!root.classList.contains('agent-pinned') || scrollFrame) return;
                    scrollFrame = requestAnimationFrame(() => {
                        scrollFrame = 0;
                        if (root.classList.contains('agent-pinned')) scrollToBottom();
                    });
                }
                const obs = new MutationObserver(scheduleScrollToBottom);
                obs.observe(messages, { childList: true, subtree: true, characterData: true });

                messages.addEventListener('scroll', syncPinClass, { passive: true });

                // Public API for the floating control buttons.
                window.flyntAgentScroll = {
                    bottom: () => scrollToBottom(),
                    prevUser: () => {
                        const users = messages.querySelectorAll('.agent-msg.user');
                        const top = messages.scrollTop;
                        for (let i = users.length - 1; i >= 0; i--) {
                            if (users[i].offsetTop < top - 8) {
                                messages.scrollTop = users[i].offsetTop - 8;
                                syncPinClass();
                                return;
                            }
                        }
                        // No earlier user msg → jump to very top
                        messages.scrollTop = 0;
                        syncPinClass();
                    },
                    nextUser: () => {
                        const users = messages.querySelectorAll('.agent-msg.user');
                        const top = messages.scrollTop;
                        for (let i = 0; i < users.length; i++) {
                            if (users[i].offsetTop > top + 8) {
                                messages.scrollTop = users[i].offsetTop - 8;
                                syncPinClass();
                                return;
                            }
                        }
                        // No later user msg → jump to bottom
                        scrollToBottom();
                    },
                };

                // Start pinned.
                root.classList.add('agent-pinned');
                scrollToBottom();
            })();
        "#,
        );
    });

    // Slash command menu
    let input_val = input.read().clone();
    let slash_prefix = input_val.starts_with('/');
    let filter_text = if slash_prefix {
        input_val.trim_start_matches('/').to_lowercase()
    } else {
        String::new()
    };

    let launch_error = use_context::<Signal<Option<String>>>();
    let mut settings_page = use_context::<Signal<SettingsPage>>();
    let mut settings_open = use_context::<Signal<crate::state::SettingsOpen>>();

    let version_label = omegon_binary
        .as_ref()
        .and_then(|p| version_from_binary_path(p))
        .unwrap_or_default();

    rsx! {
        div { class: "agent-rail",

            // ── Status bar ───────────────────────────────────────
            div {
                class: "agent-status-bar agent-status-bar-clickable",
                onclick: move |_| {
                    // Land on the Profile sub-page when entering Omegon
                    // settings from the agent rail.
                    *settings_page.write() = SettingsPage::OmegonProfile;
                    *settings_open.write() = crate::state::SettingsOpen(true);
                },
                title: "Open Omegon settings",
                div { class: "agent-status-row",
                    div { class: "agent-status-left",
                        span { class: "agent-status-label", "Omegon" }
                        if !version_label.is_empty() {
                            span { class: "agent-status-version", "{version_label}" }
                        }
                    }
                    span { class: agent_status.read().css_class(), {agent_status.read().label()} }
                }
                // Session title — set by omegon from the first prompt's content.
                // Hidden when None so we don't carve out empty space pre-prompt.
                if let Some(title) = session_title.read().clone() {
                    div { class: "agent-session-title", title: "{title}", "{title}" }
                }
            }

            // ── Inline session status ──────────────────────────────
            if session.read().is_some() {
                crate::components::omegon::session_status::InlineSessionStatus {}
            }

            // ── Launch/connection error (only if no active session) ──
            if session.read().is_none() {
                if let Some(err) = launch_error.read().as_ref() {
                    div { class: "agent-error-banner",
                        p { "Could not start the agent: {err}" }
                        p { class: "agent-error-hint", "Make sure Omegon is installed. Check Settings for the runtime path." }
                }
                    crate::omegon_setup::OmegonSetupPanel {}
                }
            }
            if session.read().is_none() && !binary_found && launch_error.read().is_none() {
                crate::omegon_setup::OmegonSetupPanel {}
            }
            if session.read().is_none()
                && binary_found
                && launch_error.read().is_none()
                && *agent_status.read() == AgentStatus::Idle
            {
                crate::omegon_setup::OmegonSetupPanel {}
            }
            if session.read().is_some() {
                {
                    let setup = crate::omegon_setup::evaluate(&ctx);
                    (!setup.flynt_extension_installed).then(|| rsx! {
                        crate::omegon_setup::OmegonSetupPanel {}
                    })
                }
            }

            // ── Chat messages ────────────────────────────────────
            div { class: "agent-messages",
                if items.read().is_empty() && binary_found {
                    div { class: "agent-empty",
                        p { "Ask Omegon about your project, notes, or projects." }
                        div { class: "agent-suggestions",
                            button { class: "btn btn-ghost btn-xs", onclick: move |_| *input.write() = "/login".into(), "/login" }
                            button { class: "btn btn-ghost btn-xs", onclick: move |_| *input.write() = "/status".into(), "/status" }
                            button { class: "btn btn-ghost btn-xs", onclick: move |_| *input.write() = "Summarize the current note".into(), "Summarize note" }
                        }
                    }
                } else {
                    for (idx, item) in items.read().iter().enumerate() {
                        match item {
                            ChatItem::Message { role, content } => {
                                if *role == ChatRole::User {
                                    rsx! {
                                        div { key: "msg-{idx}", class: "agent-msg user",
                                            div { class: "agent-msg-role", "You" }
                                            div { class: "agent-msg-content", "{content}" }
                                        }
                                    }
                                } else if *agent_status.read() != AgentStatus::Idle
                                    && idx + 1 == items.read().len()
                                {
                                    rsx! {
                                        div { key: "msg-{idx}", class: "agent-msg assistant",
                                            div { class: "agent-msg-role", "Omegon" }
                                            div { class: "agent-msg-content", "{content}" }
                                        }
                                    }
                                } else {
                                    let html = render_md(content);
                                    rsx! {
                                        div { key: "msg-{idx}", class: "agent-msg assistant",
                                            div { class: "agent-msg-role", "Omegon" }
                                            div { class: "agent-msg-content markdown-body", dangerous_inner_html: "{html}" }
                                        }
                                    }
                                }
                            },
                            ChatItem::ToolCall(tc) => {
                                let kind_label = tool_kind_label(&tc.kind);
                                let _ = kind_label; // kept for tooltip / future grouping
                                rsx! {
                                    div { key: "tc-{idx}", class: "agent-tool-call",
                                        div { class: "agent-tool-header",
                                            span { class: "agent-tool-name", "{tc.title}" }
                                            if !tc.args_summary.is_empty() {
                                                span { class: "agent-tool-args", "{tc.args_summary}" }
                                            }
                                            span { class: format!("agent-tool-status {}", tc.status.to_lowercase()), "{tc.status}" }
                                        }
                                        if !tc.output.is_empty() {
                                            // Tool output (text only for now). Mono font, faint
                                            // color, scrollable on overflow so multi-line outputs
                                            // don't blow up the message area.
                                            pre { class: "agent-tool-output", "{tc.output}" }
                                        }
                                    }
                                }
                            },
                            ChatItem::Plan(entries) => {
                                rsx! {
                                    div { key: "plan-{idx}", class: "agent-plan",
                                        div { class: "agent-plan-header", "Plan" }
                                        for (i, entry) in entries.iter().enumerate() {
                                            {
                                                let (icon, status_class) = match entry.status {
                                                    crate::acp::PlanStatus::Pending     => ("○", "pending"),
                                                    crate::acp::PlanStatus::InProgress  => ("●", "in-progress"),
                                                    crate::acp::PlanStatus::Completed   => ("✓", "completed"),
                                                };
                                                let priority_class = match entry.priority {
                                                    crate::acp::PlanPriority::High   => "high",
                                                    crate::acp::PlanPriority::Medium => "medium",
                                                    crate::acp::PlanPriority::Low    => "low",
                                                };
                                                rsx! {
                                                    div { key: "{i}", class: format!("agent-plan-entry {status_class} priority-{priority_class}"),
                                                        span { class: "agent-plan-icon", "{icon}" }
                                                        span { class: "agent-plan-content", "{entry.content}" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            },
                            ChatItem::Thought { content } => rsx! {
                                div { key: "th-{idx}", class: "agent-msg thought",
                                    div { class: "agent-msg-role", "Thinking" }
                                    div { class: "agent-msg-content", "{content}" }
                                }
                            },
                        }
                    }
                    {
                        let last_has_content = match items.read().last() {
                            Some(ChatItem::Thought { content }) => !content.is_empty(),
                            Some(ChatItem::Message { role: ChatRole::Assistant, content }) => !content.is_empty(),
                            _ => false,
                        };
                        (*agent_status.read() == AgentStatus::Thinking && !last_has_content).then(|| rsx! {
                            div { class: "agent-msg assistant",
                                div { class: "agent-msg-role", "Omegon" }
                                div { class: "agent-msg-content typing", "Thinking…" }
                            }
                        })
                    }
                }
            }

            // ── Floating scroll controls ────────────────────────
            // Always visible while there's a session; jump-to-bottom hides
            // automatically when already pinned (CSS: .agent-pinned variant).
            // prev/next user navigate by jumping to the previous/next "You"
            // message above/below the current scroll position.
            if !items.read().is_empty() {
                div { class: "agent-scroll-controls",
                    button {
                        class: "agent-scroll-btn",
                        title: "Previous user message",
                        onclick: move |_| { document::eval("window.flyntAgentScroll && window.flyntAgentScroll.prevUser();"); },
                        "↑"
                    }
                    button {
                        class: "agent-scroll-btn",
                        title: "Next user message",
                        onclick: move |_| { document::eval("window.flyntAgentScroll && window.flyntAgentScroll.nextUser();"); },
                        "↓"
                    }
                    button {
                        class: "agent-scroll-btn agent-scroll-bottom",
                        title: "Jump to bottom",
                        onclick: move |_| { document::eval("window.flyntAgentScroll && window.flyntAgentScroll.bottom();"); },
                        "⤓"
                    }
                }
            }

            // ── Slash command menu ──────────────────────────────
            if slash_prefix && !agent_status.read().is_busy() && available_commands.read().iter().any(|sc| filter_text.is_empty() || sc.name.starts_with(&*filter_text)) {
                div { class: "agent-command-menu",
                    for sc in available_commands.read().iter() {
                        if filter_text.is_empty() || sc.name.starts_with(&*filter_text) {
                            {
                                let cmd_name = sc.name.clone();
                                let cmd_str = format!("/{}", sc.name);
                                rsx! {
                                    button { key: "cmd-{cmd_name}", class: "agent-command-item",
                                        onclick: move |_| { *input.write() = cmd_str.clone(); },
                                        span { class: "agent-command-name", "/{cmd_name}" }
                                        span { class: "agent-command-desc", "{sc.description}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // ── Input ────────────────────────────────────────────
            div { class: "agent-input-area",
                textarea {
                    class: "agent-textarea",
                    placeholder: if binary_found { "Ask Omegon… (type / for commands)" } else { "Omegon binary not found" },
                    value: "{input}",
                    disabled: !binary_found,
                    oninput: move |e| {
                        *input.write() = e.value();
                        *history_idx.write() = None;
                    },
                    onkeydown: move |e| {
                        // ── Up/Down arrow history ────────────
                        if e.key() == Key::ArrowUp {
                            e.prevent_default();
                            let (new_text, new_idx) = {
                                let hist = history.read();
                                if hist.is_empty() { return; }
                                let idx = match *history_idx.read() {
                                    None => hist.len() - 1,
                                    Some(i) => i.saturating_sub(1),
                                };
                                (hist[idx].clone(), idx)
                            };
                            *input.write() = new_text;
                            *history_idx.write() = Some(new_idx);
                            return;
                        }
                        if e.key() == Key::ArrowDown {
                            e.prevent_default();
                            let result = {
                                let hist = history.read();
                                let cur = *history_idx.read();
                                match cur {
                                    Some(i) if i + 1 < hist.len() => Some((hist[i + 1].clone(), i + 1)),
                                    _ => None,
                                }
                            };
                            match result {
                                Some((text, idx)) => {
                                    *input.write() = text;
                                    *history_idx.write() = Some(idx);
                                }
                                None => {
                                    *input.write() = String::new();
                                    *history_idx.write() = None;
                                }
                            }
                            return;
                        }

                        // ── Enter to send ────────────────────
                        if e.key() == Key::Enter && !e.modifiers().shift() {
                            e.prevent_default();
                            let prompt = input.read().trim().to_string();
                            if prompt.is_empty() {
                                tracing::debug!("Empty prompt, ignoring");
                                return;
                            }
                            if session.read().is_none() {
                                tracing::warn!("Prompt submitted but session is None — message will not be sent");
                                return;
                            }

                            tracing::info!("Sending prompt: {:?}", &prompt[..prompt.len().min(80)]);

                            // Push to history
                            history.write().push(prompt.clone());
                            *history_idx.write() = None;

                            items.write().push(ChatItem::Message { role: ChatRole::User, content: prompt.clone() });
                            *input.write() = String::new();
                            // Sending a prompt always re-pins to bottom — the
                            // user just initiated a turn, they want to see it.
                            document::eval("window.flyntAgentScroll && window.flyntAgentScroll.bottom();");

                            let is_login = prompt.trim().starts_with("/login");
                            let prompt_seq = items.read().len();

                            let binary = omegon_binary.clone().unwrap();
                            spawn(async move {
                                let sess = session.read().clone().unwrap();

                                let trimmed = prompt.trim();
                                if trimmed == "/login" || trimmed.starts_with("/login ") {
                                    let provider = trimmed.strip_prefix("/login").unwrap().trim();
                                    let project = use_context::<AppContext>().project_root();
                                    *agent_status.write() = AgentStatus::Thinking;
                                    sess.login(&binary, provider).await;

                                    // Reconnect with new credentials
                                    *agent_status.write() = AgentStatus::Connecting;
                                    drop(sess);
                                    *session.write() = None;
                                    let reconnect_settings = use_context::<AppContext>().omegon().load_operator_settings();
                                    let saved_config = reconnect_settings.acp_config.clone();
                                    let agent_id = reconnect_settings.agent_id.clone();
                                    match AcpSession::connect(binary.clone(), project, agent_id).await {
                                        Ok((s, rx)) => {
                                            let new_sess = Rc::new(s);
                                            for (cfg_id, value) in &saved_config {
                                                new_sess.set_config(cfg_id, value).await;
                                            }
                                            *session.write() = Some(new_sess.clone());
                                            *shared_session.write() = Some(new_sess);
                                            start_event_loop(
                                                rx,
                                                use_context::<AppContext>(),
                                                items,
                                                agent_status,
                                                available_commands,
                                                config_options,
                                                session_title,
                                                session,
                                                shared_session,
                                            );
                                            items.write().push(ChatItem::Message {
                                                role: ChatRole::Assistant,
                                                content: "Session reconnected with new credentials.".into(),
                                            });
                                        }
                                        Err(e) => {
                                            items.write().push(ChatItem::Message {
                                                role: ChatRole::Assistant,
                                                content: format!("Reconnect failed: {e}"),
                                            });
                                        }
                                    }
                                    *agent_status.write() = AgentStatus::Idle;
                                } else {
                                    tracing::info!("Sending prompt to ACP session");
                                    *agent_status.write() = AgentStatus::Thinking;
                                    sess.prompt(&prompt);
                                    tracing::info!("Prompt dispatched (non-blocking)");
                                }
                            });

                            // Watchdog: omegon may dispatch to a provider it can't execute and emit
                            // only a WARN log (no ACP Error event), leaving the client stuck. After
                            // 45s with no follow-up events we surface the failure ourselves.
                            if !is_login {
                                let mut watchdog_status = agent_status;
                                let mut watchdog_items = items;
                                spawn(async move {
                                    tokio::time::sleep(std::time::Duration::from_secs(45)).await;
                                    if watchdog_status.read().is_busy()
                                        && watchdog_items.read().len() <= prompt_seq
                                    {
                                        tracing::warn!("Watchdog: no agent activity 45s after prompt — assuming silent provider failure");
                                        watchdog_items.write().push(ChatItem::Message {
                                            role: ChatRole::Assistant,
                                            content: "⚠ No response after 45 s. The selected model may have no executable provider, or the upstream agent failed silently. Pick a different model in the dropdown below — entries marked **(unavailable)** can't be served by the current Omegon install.".into(),
                                        });
                                        *watchdog_status.write() = AgentStatus::Idle;
                                    }
                                });
                            }
                        }
                    },
                }
            }

            // ── Config bar (model, thinking, posture) ────────────
            if !config_options.read().is_empty() {
                div { class: "agent-config-bar",
                    for opt in config_options.read().iter() {
                        {
                            let opt_id = opt.id.clone();
                            let current = opt.current_value.clone();
                            // The persisted current_value may not be in the agent's option list
                            // (e.g. provider not executable). Without surfacing this the <select>
                            // silently falls back to the first option's label and the user thinks
                            // a different model is selected than what's actually persisted.
                            let current_available = opt.options.iter().any(|v| v.value == current);
                            let select_class = if current_available {
                                "agent-config-select"
                            } else {
                                "agent-config-select unavailable"
                            };
                            rsx! {
                                div { class: "agent-config-item",
                                    select {
                                        class: "{select_class}",
                                        value: "{current}",
                                        onchange: move |e| {
                                            let new_val = e.value();
                                            let cfg_id = opt_id.clone();
                                            // Persist to disk
                                            persist_config(&use_context::<AppContext>(), &cfg_id, &new_val);
                                            // Apply to session
                                            spawn(async move {
                                                if let Some(sess) = session.read().clone() {
                                                    sess.set_config(&cfg_id, &new_val).await;
                                                }
                                            });
                                        },
                                        if !current_available {
                                            option {
                                                value: "{current}",
                                                selected: true,
                                                disabled: true,
                                                "⚠ {current} (unavailable)"
                                            }
                                        }
                                        for v in opt.options.iter() {
                                            option { value: "{v.value}", selected: v.value == opt.current_value, "{v.name}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn handle_acp_event(
    event: AcpEvent,
    ctx: AppContext,
    items: &mut Signal<Vec<ChatItem>>,
    status: &mut Signal<AgentStatus>,
    commands: &mut Signal<Vec<SlashCommand>>,
    config: &mut Signal<Vec<ConfigOption>>,
    session_title: &mut Signal<Option<String>>,
    session: Signal<Option<Rc<AcpSession>>>,
    shared_session: Signal<Option<Rc<AcpSession>>>,
) {
    match event {
        AcpEvent::TextDelta(ref text) => {
            tracing::info!("ACP TextDelta: {} bytes", text.len());
            let mut list = items.write();
            if let Some(ChatItem::Message {
                role: ChatRole::Assistant,
                content,
            }) = list.last_mut()
            {
                content.push_str(text);
            } else {
                list.push(ChatItem::Message {
                    role: ChatRole::Assistant,
                    content: text.clone(),
                });
            }
        }
        AcpEvent::ThoughtDelta(ref text) => {
            tracing::info!("ACP ThoughtDelta: {} bytes", text.len());
            *status.write() = AgentStatus::Thinking;
            let mut list = items.write();
            if let Some(ChatItem::Thought { content }) = list.last_mut() {
                content.push_str(text);
            } else {
                list.push(ChatItem::Thought {
                    content: text.clone(),
                });
            }
        }
        AcpEvent::ToolCallStarted {
            ref id,
            ref title,
            ref kind,
            ref args,
        } => {
            tracing::info!("ACP ToolCallStarted: {kind} — {title} (id={id})");
            *status.write() = AgentStatus::ToolRunning;
            items.write().push(ChatItem::ToolCall(ToolCallBlock {
                id: id.clone(),
                title: title.clone(),
                kind: kind.clone(),
                status: "InProgress".into(),
                args_summary: summarize_tool_args(args.as_ref()),
                output: String::new(),
            }));
        }
        AcpEvent::ToolCallUpdated {
            ref id,
            status: ref st,
            ref title,
            ref output,
        } => {
            tracing::debug!("ACP ToolCallUpdated: id={id} status={st}");
            {
                let mut list = items.write();
                for item in list.iter_mut() {
                    if let ChatItem::ToolCall(tc) = item {
                        if tc.id == *id {
                            if !st.is_empty() {
                                tc.status = st.clone();
                            }
                            if let Some(t) = title {
                                tc.title = t.clone();
                            }
                            if let Some(o) = output {
                                tc.output = o.clone();
                            }
                            break;
                        }
                    }
                }
            }

            let disconnect_msg = output
                .as_deref()
                .filter(|msg| is_transport_disconnect(msg))
                .or_else(|| title.as_deref().filter(|msg| is_transport_disconnect(msg)))
                .or_else(|| (!st.is_empty() && is_transport_disconnect(st)).then_some(st.as_str()));

            if let Some(msg) = disconnect_msg {
                tracing::warn!("ACP tool call reported transport disconnect: {msg}");
                items.write().push(ChatItem::Message {
                    role: ChatRole::Assistant,
                    content: format!(
                        "Agent transport disconnected ({msg}). Reconnecting the Omegon session..."
                    ),
                });
                reconnect_acp_session(
                    ctx.clone(),
                    session,
                    shared_session,
                    *items,
                    *status,
                    *commands,
                    *config,
                    *session_title,
                );
            }
        }
        AcpEvent::PlanUpdated(ref plan) => {
            tracing::info!("ACP PlanUpdated: {} entries", plan.len());
            let mut list = items.write();
            // Replace the most-recent Plan item if there is one; otherwise
            // append. Plans are full snapshots, so we don't accumulate them.
            if let Some(idx) = list.iter().rposition(|i| matches!(i, ChatItem::Plan(_))) {
                list[idx] = ChatItem::Plan(plan.clone());
            } else {
                list.push(ChatItem::Plan(plan.clone()));
            }
        }
        AcpEvent::SessionTitleChanged(ref title) => {
            tracing::info!("ACP SessionTitleChanged: {:?}", title);
            *session_title.write() = title.clone();
        }
        AcpEvent::CommandsAvailable(ref cmds) => {
            tracing::info!("ACP CommandsAvailable: {} commands", cmds.len());
            *commands.write() = cmds.clone();
        }
        AcpEvent::ConfigChanged(ref opts) => {
            tracing::info!("ACP ConfigChanged: {} options", opts.len());
            for opt in opts {
                let in_list = opt.options.iter().any(|v| v.value == opt.current_value);
                tracing::info!(
                    "  opt id={} current={:?} in_list={}",
                    opt.id,
                    opt.current_value,
                    in_list
                );
            }
            // Reconcile persisted operator-settings.json with the agent's actual current_values.
            // Without this the file can claim a model the agent never accepted (e.g. a
            // saved set_config that omegon silently ignored), and the panel ends up lying
            // about what's running.
            let omegon = ctx.omegon();
            let mut settings = omegon.load_operator_settings();
            let mut changed = false;
            for opt in opts {
                let prev = settings.acp_config.get(&opt.id).cloned();
                if prev.as_deref() != Some(opt.current_value.as_str()) {
                    tracing::info!(
                        "Reconciling persisted {}: {:?} → {:?} (omegon-reported)",
                        opt.id,
                        prev,
                        opt.current_value
                    );
                    settings
                        .acp_config
                        .insert(opt.id.clone(), opt.current_value.clone());
                    changed = true;
                }
            }
            if changed {
                if let Err(e) = omegon.save_operator_settings(&settings) {
                    tracing::warn!("Failed to persist reconciled acp_config: {e}");
                }
            }
            *config.write() = opts.clone();
        }
        AcpEvent::Done => {
            tracing::info!("ACP Done");
            *status.write() = AgentStatus::Idle;
        }
        AcpEvent::Error(ref msg) => {
            tracing::error!("ACP Error: {msg}");
            if is_transport_disconnect(msg) {
                items.write().push(ChatItem::Message {
                    role: ChatRole::Assistant,
                    content: format!(
                        "Agent transport disconnected ({msg}). Reconnecting the Omegon session..."
                    ),
                });
                reconnect_acp_session(
                    ctx.clone(),
                    session,
                    shared_session,
                    *items,
                    *status,
                    *commands,
                    *config,
                    *session_title,
                );
                return;
            }
            let lower = msg.to_lowercase();
            let display = if lower.contains("auth")
                || lower.contains("401")
                || lower.contains("unauthorized")
                || lower.contains("expired")
                || lower.contains("credential")
            {
                format!("Authentication error: {msg}\n\nTry `/login anthropic` to re-authenticate.")
            } else {
                format!("Error: {msg}")
            };
            items.write().push(ChatItem::Message {
                role: ChatRole::Assistant,
                content: display,
            });
            *status.write() = AgentStatus::Idle;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_transport_disconnect;

    #[test]
    fn detects_broken_pipe_transport_errors() {
        assert!(is_transport_disconnect("Broken pipe (os error 32)"));
        assert!(is_transport_disconnect(
            "ACP transport disconnected: connection closed"
        ));
        assert!(is_transport_disconnect("extension process not running"));
    }

    #[test]
    fn ignores_non_transport_errors() {
        assert!(!is_transport_disconnect(
            "Authentication error: token expired"
        ));
        assert!(!is_transport_disconnect("invalid params: missing path"));
    }
}
