//! Live session status panel — shows agent state at a glance.
//!
//! Polls control/stats and control/provider_status via ACP every few seconds.
//! Renders model, posture, thinking, turns, context bar, and provider warnings.

use crate::acp::AcpSession;
use dioxus::prelude::*;
use std::rc::Rc;

#[derive(Debug, Clone, Default, PartialEq)]
struct SessionStats {
    model: String,
    thinking: String,
    posture: String,
    turns: String,
    context_tokens: String,
    context_pct: f64,
    context_window: String,
    max_turns: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct ProviderState {
    name: String,
    status: String,
    detail: String,
}

fn parse_stats(text: &str) -> SessionStats {
    let mut stats = SessionStats::default();
    for line in text.lines() {
        if let Some((key, val)) = line.split_once(": ") {
            let val = val.trim();
            match key.trim() {
                "Model" => stats.model = val.to_string(),
                "Thinking" => stats.thinking = val.to_string(),
                "Posture" => stats.posture = val.to_string(),
                "Turns" => stats.turns = val.to_string(),
                "Max turns" => stats.max_turns = val.to_string(),
                "Context" => {
                    if let Some(tok) = val.strip_prefix('~') {
                        if let Some(idx) = tok.find(" tokens") {
                            stats.context_tokens = tok[..idx].to_string();
                        }
                    }
                    if let Some(start) = val.find('(') {
                        if let Some(end) = val.find('%') {
                            if let Ok(pct) = val[start+1..end].trim().parse::<f64>() {
                                stats.context_pct = pct;
                            }
                        }
                        if let Some(of_idx) = val.find("of ") {
                            let rest = &val[of_idx+3..];
                            stats.context_window = rest.trim_end_matches(')').trim().to_string();
                        }
                    }
                }
                _ => {}
            }
        }
    }
    stats
}

fn parse_provider_status(text: &str) -> Vec<ProviderState> {
    text.lines().filter_map(|line| {
        let parts: Vec<&str> = line.splitn(3, ':').collect();
        if parts.len() >= 3 {
            Some(ProviderState {
                name: parts[0].to_string(),
                status: parts[1].to_string(),
                detail: parts[2].to_string(),
            })
        } else {
            None
        }
    }).collect()
}

fn provider_for_model(model: &str) -> &str {
    // Explicit prefix takes priority — no heuristics needed
    if model.starts_with("anthropic:") { return "anthropic"; }
    if model.starts_with("openai:") { return "openai"; }
    if model.starts_with("ollama:") { return "ollama"; }
    if model.starts_with("openrouter:") { return "openrouter"; }

    // Heuristic fallback for unprefixed model names
    if model.starts_with("claude") { return "anthropic"; }
    if model.starts_with("gpt-") || model.starts_with("o1-") || model.starts_with("o3-") || model.starts_with("o4-") {
        return "openai";
    }

    // If none of the above matched, assume local/ollama
    "ollama"
}

#[component]
pub fn SessionStatusPanel() -> Element {
    let shared_session = use_context::<Signal<Option<Rc<AcpSession>>>>();
    let mut stats = use_signal(SessionStats::default);
    let mut providers = use_signal(Vec::<ProviderState>::new);
    let mut connected = use_signal(|| false);

    use_future(move || async move {
        loop {
            if let Some(sess) = shared_session.read().clone() {
                if let Ok(resp) = sess.stats().await {
                    if let Some(text) = resp["text"].as_str() {
                        *stats.write() = parse_stats(text);
                        *connected.write() = true;
                    }
                }
                if let Ok(resp) = sess.provider_status().await {
                    if let Some(text) = resp["text"].as_str() {
                        *providers.write() = parse_provider_status(text);
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });

    let s = stats.read();

    if !*connected.read() || s.model.is_empty() {
        return rsx! {};
    }

    let ctx_pct = s.context_pct.min(100.0);
    let ctx_color = if ctx_pct > 80.0 { "var(--warning)" }
        else if ctx_pct > 60.0 { "var(--primary-muted)" }
        else { "var(--primary)" };

    // Check if the active model's provider has issues
    let active_provider = provider_for_model(&s.model);
    let provider_warning = providers.read().iter().find(|p| {
        p.name == active_provider && (p.status == "expired" || p.status == "missing" || p.status == "unavailable")
    }).map(|p| {
        let action = if p.status == "expired" {
            format!("Token expired — use /login {} in the agent panel to refresh.", p.name)
        } else if p.status == "unavailable" && p.name == "ollama" {
            "Ollama is not running. Start it with: ollama serve".into()
        } else {
            format!("Not configured — use /login {} or set an API key.", p.name)
        };
        (p.status.clone(), p.name.clone(), action)
    });

    rsx! {
        section { class: "settings-section",
            h2 { class: "settings-heading", "Session" }

            if let Some((status, name, action)) = &provider_warning {
                div { class: "session-warning",
                    span { class: "session-warning-icon",
                        if status == "expired" { "⚠" } else { "✗" }
                    }
                    span { class: "session-warning-text",
                        strong { "{name}" }
                        " — {action}"
                    }
                }
            }

            div { class: "session-status-grid",
                div { class: "session-stat",
                    span { class: "session-stat-label", "Model" }
                    span { class: "session-stat-value", "{s.model}" }
                }
                div { class: "session-stat",
                    span { class: "session-stat-label", "Posture" }
                    span { class: "session-stat-value", "{s.posture}" }
                }
                div { class: "session-stat",
                    span { class: "session-stat-label", "Thinking" }
                    span { class: "session-stat-value", "{s.thinking}" }
                }
                div { class: "session-stat",
                    span { class: "session-stat-label", "Turns" }
                    span { class: "session-stat-value", "{s.turns} / {s.max_turns}" }
                }
            }
            div { class: "session-context-bar",
                span { class: "session-stat-label", "Context" }
                div { class: "context-bar-track",
                    div {
                        class: "context-bar-fill",
                        style: "width: {ctx_pct}%; background: {ctx_color};",
                    }
                }
                span { class: "session-stat-detail",
                    "~{s.context_tokens} / {s.context_window} tokens ({ctx_pct:.0}%)"
                }
            }
        }
    }
}

/// Compact inline version for the agent rail — context bar + provider warnings.
#[component]
pub fn InlineSessionStatus() -> Element {
    let shared_session = use_context::<Signal<Option<Rc<AcpSession>>>>();
    let mut stats = use_signal(SessionStats::default);
    let mut providers = use_signal(Vec::<ProviderState>::new);
    let mut connected = use_signal(|| false);
    let mut expanded = use_signal(|| false);

    use_future(move || async move {
        loop {
            if let Some(sess) = shared_session.read().clone() {
                if let Ok(resp) = sess.stats().await {
                    if let Some(text) = resp["text"].as_str() {
                        *stats.write() = parse_stats(text);
                        *connected.write() = true;
                    }
                }
                if let Ok(resp) = sess.provider_status().await {
                    if let Some(text) = resp["text"].as_str() {
                        *providers.write() = parse_provider_status(text);
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });

    let s = stats.read();
    if !*connected.read() || s.model.is_empty() {
        return rsx! {};
    }

    let ctx_pct = s.context_pct.min(100.0);
    let ctx_color = if ctx_pct > 80.0 { "var(--warning)" }
        else if ctx_pct > 60.0 { "var(--primary-muted)" }
        else { "var(--primary)" };

    let active_provider = provider_for_model(&s.model);
    let warning = providers.read().iter().find(|p| {
        p.name == active_provider && (p.status == "expired" || p.status == "missing" || p.status == "unavailable")
    }).map(|p| {
        if p.status == "expired" {
            format!("{} token expired — /login {}", p.name, p.name)
        } else if p.status == "unavailable" && p.name == "ollama" {
            "Ollama not running".into()
        } else {
            format!("{} not configured", p.name)
        }
    });

    rsx! {
        div {
            class: "rail-session-status",
            onclick: move |_| { let v = *expanded.read(); *expanded.write() = !v; },

            if let Some(ref warn) = warning {
                div { class: "rail-provider-warning",
                    "⚠ {warn}"
                }
            }

            div { class: "rail-context-row",
                span { class: "rail-context-label", "{s.turns}t" }
                div { class: "rail-context-track",
                    div {
                        class: "rail-context-fill",
                        style: "width: {ctx_pct}%; background: {ctx_color};",
                    }
                }
                span { class: "rail-context-label", "{ctx_pct:.0}%" }
            }

            if *expanded.read() {
                div { class: "rail-session-expanded",
                    div { class: "rail-stat-row",
                        span { class: "rail-stat-label", "Model" }
                        span { class: "rail-stat-value", "{s.model}" }
                    }
                    div { class: "rail-stat-row",
                        span { class: "rail-stat-label", "Posture" }
                        span { class: "rail-stat-value", "{s.posture}" }
                    }
                    div { class: "rail-stat-row",
                        span { class: "rail-stat-label", "Thinking" }
                        span { class: "rail-stat-value", "{s.thinking}" }
                    }
                    div { class: "rail-stat-row",
                        span { class: "rail-stat-label", "Context" }
                        span { class: "rail-stat-value", "~{s.context_tokens} / {s.context_window}" }
                    }
                }
            }
        }
    }
}
