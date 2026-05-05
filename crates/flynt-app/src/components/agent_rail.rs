use crate::acp::{AcpEvent, AcpSession, ConfigOption, SlashCommand};
use crate::bootstrap::AppContext;
use comrak::{Options, markdown_to_html};
use dioxus::prelude::*;
use std::path::PathBuf;
use std::rc::Rc;

/// Resolve the Omegon binary using the centralized channel-aware resolver.
pub fn find_omegon_binary_public() -> Option<PathBuf> {
    // Caller should use ctx.omegon().resolve_binary() when context is available.
    // This fallback uses default config for contexts where AppContext isn't accessible.
    let path = flynt_core::models::resolve_omegon_binary(&flynt_core::models::LocalRuntimeConfig::default());
    if path.exists() { Some(path) } else { None }
}

fn find_omegon_binary_from_ctx(ctx: &crate::bootstrap::AppContext) -> Option<PathBuf> {
    let path = ctx.omegon().resolve_binary();
    if path.exists() { Some(path) } else { None }
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
        "Read" => "Read", "Edit" => "Edit", "Delete" => "Delete",
        "Move" => "Move", "Search" => "Search", "Execute" => "Run",
        "Think" => "Think", "Fetch" => "Fetch", "SwitchMode" => "Mode",
        _ => "Tool",
    }
}

/// Save a config option to operator settings on disk.
fn persist_config(ctx: &AppContext, config_id: &str, value: &str) {
    let omegon = ctx.omegon();
    let mut settings = omegon.load_operator_settings();
    settings.acp_config.insert(config_id.to_string(), value.to_string());
    if let Err(e) = omegon.save_operator_settings(&settings) {
        tracing::warn!("Failed to persist config: {e}");
    }
}

/// Start the event polling loop for an ACP session.
fn start_event_loop(
    rx: std::sync::mpsc::Receiver<AcpEvent>,
    items: Signal<Vec<ChatItem>>,
    agent_status: Signal<AgentStatus>,
    available_commands: Signal<Vec<SlashCommand>>,
    config_options: Signal<Vec<ConfigOption>>,
) {
    let mut items = items;
    let mut agent_status = agent_status;
    let mut available_commands = available_commands;
    let mut config_options = config_options;
    spawn(async move {
        loop {
            while let Ok(event) = rx.try_recv() {
                handle_acp_event(event, &mut items, &mut agent_status, &mut available_commands, &mut config_options);
            }
            tokio::time::sleep(std::time::Duration::from_millis(16)).await;
        }
    });
}

#[derive(Clone, PartialEq)]
enum ChatRole { User, Assistant }

#[derive(Clone, PartialEq)]
struct ToolCallBlock { id: String, title: String, kind: String, status: String }

#[derive(Clone, PartialEq)]
enum ChatItem {
    Message { role: ChatRole, content: String },
    ToolCall(ToolCallBlock),
}

#[derive(Clone, Copy, PartialEq)]
enum AgentStatus { Idle, Connecting, Thinking, ToolRunning }

impl AgentStatus {
    fn label(&self) -> &'static str {
        match self {
            Self::Idle => "ready", Self::Connecting => "connecting…",
            Self::Thinking => "thinking…", Self::ToolRunning => "running tool…",
        }
    }
    fn css_class(&self) -> &'static str {
        match self {
            Self::Idle => "agent-status-badge connected",
            Self::Connecting => "agent-status-badge connecting",
            _ => "agent-status-badge active",
        }
    }
    fn is_busy(&self) -> bool { !matches!(self, Self::Idle) }
}

#[component]
pub fn AgentRail() -> Element {
    let ctx = use_context::<AppContext>();

    let mut input = use_signal(String::new);
    let mut items: Signal<Vec<ChatItem>> = use_signal(Vec::new);
    let mut agent_status = use_signal(|| AgentStatus::Connecting);
    let mut session: Signal<Option<Rc<AcpSession>>> = use_signal(|| None);
    let mut shared_session = use_context::<Signal<Option<Rc<AcpSession>>>>();
    let available_commands: Signal<Vec<SlashCommand>> = use_signal(Vec::new);
    let config_options: Signal<Vec<ConfigOption>> = use_signal(Vec::new);

    // Input history (up/down arrow)
    let mut history: Signal<Vec<String>> = use_signal(Vec::new);
    let mut history_idx: Signal<Option<usize>> = use_signal(|| None);

    let omegon_binary = find_omegon_binary_from_ctx(&ctx);
    let binary_found = omegon_binary.is_some();

    // ── Eager connect on mount + apply saved config ─────────
    use_effect(move || {
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
        let vault = ctx.vault_root();
        tracing::info!("Connecting ACP session: vault={}, binary={}", vault.display(), binary.display());
        let operator_settings = ctx.omegon().load_operator_settings();
        let saved_config = operator_settings.acp_config.clone();
        let agent_id = operator_settings.agent_id.clone();

        spawn(async move {
            tracing::info!("ACP connect starting…");
            match AcpSession::connect(binary, vault, agent_id).await {
                Ok((s, rx)) => {
                    tracing::info!("ACP session connected successfully");
                    let sess = Rc::new(s);

                    // Apply persisted config options
                    for (cfg_id, value) in &saved_config {
                        tracing::debug!("Restoring config {cfg_id}={value}");
                        sess.set_config(cfg_id, value).await;
                    }

                    *session.write() = Some(sess.clone());
                    *shared_session.write() = Some(sess);
                    start_event_loop(rx, items, agent_status, available_commands, config_options);
                    *agent_status.write() = AgentStatus::Idle;
                    tracing::info!("ACP event loop started, agent ready");
                }
                Err(e) => {
                    tracing::error!("ACP connect failed: {e}");
                    *agent_status.write() = AgentStatus::Idle;
                }
            }
        });
    });

    // Slash command menu
    let input_val = input.read().clone();
    let slash_prefix = input_val.starts_with('/');
    let filter_text = if slash_prefix { input_val.trim_start_matches('/').to_lowercase() } else { String::new() };

    let launch_error = use_context::<Signal<Option<String>>>();

    rsx! {
        div { class: "agent-rail",
            // ── Status bar ───────────────────────────────────────
            div { class: "agent-status-bar",
                div { class: "agent-status-row",
                    span { class: "agent-status-label", "Omegon" }
                    span { class: agent_status.read().css_class(), {agent_status.read().label()} }
                }
            }

            // ── Launch/connection error (only if no active session) ──
            if session.read().is_none() {
                if let Some(err) = launch_error.read().as_ref() {
                    div { class: "agent-error-banner",
                        p { "Could not start the agent: {err}" }
                        p { class: "agent-error-hint", "Make sure Omegon is installed. Check Settings for the runtime path." }
                }
                }
            }
            if session.read().is_none() && !binary_found && launch_error.read().is_none() {
                div { class: "agent-error-banner",
                    p { "Omegon binary not found." }
                    p { class: "agent-error-hint",
                        "Install Omegon or set the runtime path in Settings > Local Runtime."
                    }
                }
            }

            // ── Chat messages ────────────────────────────────────
            div { class: "agent-messages",
                if items.read().is_empty() && binary_found {
                    div { class: "agent-empty",
                        p { "Ask Omegon about your vault, notes, or projects." }
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
                                rsx! {
                                    div { key: "tc-{idx}", class: "agent-tool-call",
                                        div { class: "agent-tool-header",
                                            span { class: "agent-tool-kind", "{kind_label}" }
                                            span { class: "agent-tool-title", "{tc.title}" }
                                            span { class: format!("agent-tool-status {}", tc.status.to_lowercase()), "{tc.status}" }
                                        }
                                    }
                                }
                            },
                        }
                    }
                    if *agent_status.read() == AgentStatus::Thinking {
                        div { class: "agent-msg assistant",
                            div { class: "agent-msg-role", "Omegon" }
                            div { class: "agent-msg-content typing", "Thinking…" }
                        }
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

                            let binary = omegon_binary.clone().unwrap();
                            spawn(async move {
                                let sess = session.read().clone().unwrap();

                                let trimmed = prompt.trim();
                                if trimmed == "/login" || trimmed.starts_with("/login ") {
                                    let provider = trimmed.strip_prefix("/login").unwrap().trim();
                                    let vault = use_context::<AppContext>().vault_root();
                                    *agent_status.write() = AgentStatus::Thinking;
                                    sess.login(&binary, provider).await;

                                    // Reconnect with new credentials
                                    *agent_status.write() = AgentStatus::Connecting;
                                    drop(sess);
                                    *session.write() = None;
                                    let reconnect_settings = use_context::<AppContext>().omegon().load_operator_settings();
                                    let saved_config = reconnect_settings.acp_config.clone();
                                    let agent_id = reconnect_settings.agent_id.clone();
                                    match AcpSession::connect(binary.clone(), vault, agent_id).await {
                                        Ok((s, rx)) => {
                                            let new_sess = Rc::new(s);
                                            for (cfg_id, value) in &saved_config {
                                                new_sess.set_config(cfg_id, value).await;
                                            }
                                            *session.write() = Some(new_sess.clone());
                                            *shared_session.write() = Some(new_sess);
                                            start_event_loop(rx, items, agent_status, available_commands, config_options);
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
                            rsx! {
                                div { class: "agent-config-item",
                                    select {
                                        class: "agent-config-select",
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
    items: &mut Signal<Vec<ChatItem>>,
    status: &mut Signal<AgentStatus>,
    commands: &mut Signal<Vec<SlashCommand>>,
    config: &mut Signal<Vec<ConfigOption>>,
) {
    match event {
        AcpEvent::TextDelta(ref text) => {
            tracing::info!("ACP TextDelta: {} bytes", text.len());
            let mut list = items.write();
            if let Some(ChatItem::Message { role: ChatRole::Assistant, content }) = list.last_mut() {
                content.push_str(text);
            } else {
                list.push(ChatItem::Message { role: ChatRole::Assistant, content: text.clone() });
            }
        }
        AcpEvent::ThoughtDelta(ref text) => {
            tracing::info!("ACP ThoughtDelta: {} bytes", text.len());
        }
        AcpEvent::ToolCallStarted { ref id, ref title, ref kind } => {
            tracing::info!("ACP ToolCallStarted: {kind} — {title} (id={id})");
            *status.write() = AgentStatus::ToolRunning;
            items.write().push(ChatItem::ToolCall(ToolCallBlock {
                id: id.clone(), title: title.clone(), kind: kind.clone(), status: "InProgress".into(),
            }));
        }
        AcpEvent::ToolCallUpdated { ref id, status: ref st, ref title } => {
            tracing::debug!("ACP ToolCallUpdated: id={id} status={st}");
            let mut list = items.write();
            for item in list.iter_mut() {
                if let ChatItem::ToolCall(tc) = item {
                    if tc.id == *id {
                        if !st.is_empty() { tc.status = st.clone(); }
                        if let Some(t) = title { tc.title = t.clone(); }
                        break;
                    }
                }
            }
        }
        AcpEvent::CommandsAvailable(ref cmds) => {
            tracing::info!("ACP CommandsAvailable: {} commands", cmds.len());
            *commands.write() = cmds.clone();
        }
        AcpEvent::ConfigChanged(ref opts) => {
            tracing::info!("ACP ConfigChanged: {} options", opts.len());
            *config.write() = opts.clone();
        }
        AcpEvent::Done => {
            tracing::info!("ACP Done");
            *status.write() = AgentStatus::Idle;
        }
        AcpEvent::Error(ref msg) => {
            tracing::error!("ACP Error: {msg}");
            let lower = msg.to_lowercase();
            let display = if lower.contains("auth") || lower.contains("401") || lower.contains("unauthorized") || lower.contains("expired") || lower.contains("credential") {
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
