//! Live session status panel — shows agent state at a glance.
//!
//! Polls control/stats via ACP every few seconds and renders:
//! model, posture, thinking, turns, context usage bar, persona.

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
                    // Format: "~1234 tokens (45% of 128000)"
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
                            let window = rest.trim_end_matches(')').trim();
                            stats.context_window = window.to_string();
                        }
                    }
                }
                _ => {}
            }
        }
    }
    stats
}

#[component]
pub fn SessionStatusPanel() -> Element {
    let shared_session = use_context::<Signal<Option<Rc<AcpSession>>>>();
    let mut stats = use_signal(SessionStats::default);
    let mut connected = use_signal(|| false);

    // Poll stats every 5 seconds
    use_future(move || async move {
        loop {
            if let Some(sess) = shared_session.read().clone() {
                match sess.stats().await {
                    Ok(resp) => {
                        if let Some(text) = resp["text"].as_str() {
                            *stats.write() = parse_stats(text);
                            *connected.write() = true;
                        }
                    }
                    Err(_) => *connected.write() = false,
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

    rsx! {
        section { class: "settings-section",
            h2 { class: "settings-heading", "Session" }
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
