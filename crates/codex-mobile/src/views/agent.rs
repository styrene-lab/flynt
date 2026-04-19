use dioxus::prelude::*;
use futures_util::{SinkExt, StreamExt};
use crate::bootstrap::MobileRuntime;

#[component]
pub fn AgentView() -> Element {
    let rt = use_context::<Signal<MobileRuntime>>();
    let mut input = use_signal(String::new);
    let mut messages: Signal<Vec<(bool, String)>> = use_signal(Vec::new);
    let mut status = use_signal(|| "disconnected".to_string());
    let mut ws_tx: Signal<Option<futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        tokio_tungstenite::tungstenite::Message
    >>> = use_signal(|| None);

    let server_host = rt.read().vault.config.local_runtime
        .omegon_serve_host.clone()
        .unwrap_or_else(|| "127.0.0.1:7842".to_string());
    let server_host_display = server_host.clone();

    // Connect on mount
    use_effect(move || {
        let host = server_host.clone();
        spawn(async move {
            *status.write() = "connecting…".to_string();

            // Fetch token
            let token = match fetch_token(&host).await {
                Ok(t) => t,
                Err(e) => { *status.write() = format!("error: {e}"); return; }
            };

            // Connect WebSocket
            let ws_url = format!("ws://{host}/ws?token={token}");
            match tokio_tungstenite::connect_async(&ws_url).await {
                Ok((stream, _)) => {
                    let (sink, mut read) = stream.split();
                    *ws_tx.write() = Some(sink);
                    *status.write() = "connected".to_string();

                    // Read loop — handle omegon serve event protocol
                    spawn(async move {
                        while let Some(Ok(msg)) = read.next().await {
                            if let tokio_tungstenite::tungstenite::Message::Text(text) = msg {
                                if let Ok(event) = serde_json::from_str::<serde_json::Value>(&*text) {
                                    let event_type = event["type"].as_str().unwrap_or("");
                                    match event_type {
                                        "message_chunk" => {
                                            if let Some(t) = event["text"].as_str() {
                                                let mut msgs = messages.write();
                                                if let Some((false, last)) = msgs.last_mut() {
                                                    last.push_str(t);
                                                } else {
                                                    msgs.push((false, t.to_string()));
                                                }
                                            }
                                        }
                                        "tool_start" => {
                                            let name = event["name"].as_str().unwrap_or("tool");
                                            messages.write().push((false, format!("» {name}")));
                                        }
                                        "tool_end" => {
                                            let icon = if event["is_error"].as_bool().unwrap_or(false) { "x" } else { "ok" };
                                            let result = event["result"].as_str().unwrap_or("");
                                            let short = if result.len() > 100 { &result[..100] } else { result };
                                            messages.write().push((false, format!("{icon} {short}")));
                                        }
                                        "agent_end" => {
                                            // Turn complete — ensure next message_chunk starts fresh
                                        }
                                        "system_notification" => {
                                            if let Some(msg) = event["message"].as_str() {
                                                messages.write().push((false, format!("[sys] {msg}")));
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        *status.write() = "disconnected".to_string();
                    });
                }
                Err(e) => { *status.write() = format!("failed: {e}"); }
            }
        });
    });

    let status_text = status.read().clone();
    let status_class = if status_text == "connected" { "agent-mobile-status connected" }
        else if status_text.starts_with("error") || status_text.starts_with("failed") { "agent-mobile-status error" }
        else { "agent-mobile-status" };

    rsx! {
        div { class: "agent-mobile",
            div { class: "agent-mobile-header",
                h2 { "Omegon" }
                span { class: status_class, "{status_text}" }
            }

            div { class: "agent-mobile-messages",
                if messages.read().is_empty() {
                    div { class: "agent-mobile-empty",
                        p { "Ask Omegon about your vault." }
                        p { class: "muted", "Connected to {server_host_display}" }
                    }
                } else {
                    for (idx, (is_user, text)) in messages.read().iter().enumerate() {
                        div {
                            key: "msg-{idx}",
                            class: if *is_user { "agent-m-msg user" } else { "agent-m-msg assistant" },
                            div { class: "agent-m-role", if *is_user { "You" } else { "Omegon" } }
                            div { class: "agent-m-text", "{text}" }
                        }
                    }
                }
            }

            div { class: "agent-mobile-input",
                textarea {
                    class: "agent-m-textarea",
                    placeholder: "Ask Omegon…",
                    value: "{input}",
                    oninput: move |e| *input.write() = e.value(),
                    onkeydown: move |e| {
                        if e.key() == Key::Enter && !e.modifiers().shift() {
                            e.prevent_default();
                            let prompt = input.read().trim().to_string();
                            if prompt.is_empty() { return; }

                            messages.write().push((true, prompt.clone()));
                            *input.write() = String::new();

                            spawn(async move {
                                if let Some(ref mut tx) = *ws_tx.write() {
                                    let msg = if prompt.starts_with('/') {
                                        // Slash command
                                        let (name, args) = prompt[1..].split_once(' ').unwrap_or((&prompt[1..], ""));
                                        serde_json::json!({"type": "slash_command", "name": name, "args": args})
                                    } else {
                                        serde_json::json!({"type": "user_prompt", "text": prompt})
                                    };
                                    let _ = tx.send(tokio_tungstenite::tungstenite::Message::Text(
                                        msg.to_string().into()
                                    )).await;
                                } else {
                                    messages.write().push((false, "Not connected.".into()));
                                }
                            });
                        }
                    },
                }
            }
        }
    }
}

async fn fetch_token(host: &str) -> Result<String, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(host)
        .await.map_err(|e| format!("connect: {e}"))?;

    let req = format!("GET /api/startup HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).await.map_err(|e| format!("write: {e}"))?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.map_err(|e| format!("read: {e}"))?;
    let body = String::from_utf8_lossy(&buf);

    if let Some(pos) = body.find("\r\n\r\n") {
        let json_str = &body[pos + 4..];
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
            if let Some(token) = json["token"].as_str() {
                return Ok(token.to_string());
            }
        }
    }
    Err("could not extract token".into())
}
