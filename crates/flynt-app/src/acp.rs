//! ACP (Agent Client Protocol) client for communicating with Omegon.
//!
//! Spawns `omegon acp` as a child process and communicates via structured
//! JSON-RPC over stdio. Streams text deltas, tool calls, slash commands,
//! config options, and auth events back to the UI through a channel.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use agent_client_protocol::{
    Agent, Client, ClientSideConnection, ContentBlock, ExtRequest, InitializeRequest,
    NewSessionRequest, PermissionOptionKind, PromptRequest, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome, SessionConfigId,
    SessionConfigKind, SessionConfigOption, SessionConfigSelectOptions,
    SessionConfigValueId, SessionId, SessionNotification, SessionUpdate,
    SetSessionConfigOptionRequest, TextContent,
};
use anyhow::Result;
use tokio::process::{Child, Command};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Events flowing from the ACP session to the UI.
#[derive(Debug, Clone)]
pub enum AcpEvent {
    /// Incremental text from the agent's response.
    TextDelta(String),
    /// Agent is thinking / internal reasoning.
    ThoughtDelta(String),
    /// A new tool call started.
    ToolCallStarted {
        id: String,
        title: String,
        kind: String,
        /// Raw input args from the agent — used to render call metadata
        /// alongside the tool name (None if the agent didn't supply any).
        args: Option<serde_json::Value>,
    },
    /// A tool call status changed.
    ToolCallUpdated {
        id: String,
        status: String,
        title: Option<String>,
    },
    /// Available slash commands changed.
    CommandsAvailable(Vec<SlashCommand>),
    /// Config options changed (model, thinking, posture, etc).
    ConfigChanged(Vec<ConfigOption>),
    /// The prompt completed.
    Done,
    /// An error occurred.
    Error(String),
}

/// A slash command advertised by the agent.
#[derive(Debug, Clone, PartialEq)]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
}

/// A config option (select dropdown) from the agent.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigOption {
    pub id: String,
    pub name: String,
    pub current_value: String,
    pub options: Vec<ConfigValue>,
}

/// A single selectable value in a config option.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigValue {
    pub value: String,
    pub name: String,
}

/// Extract config options from ACP SessionConfigOption list.
fn extract_config_options(opts: &[SessionConfigOption]) -> Vec<ConfigOption> {
    opts.iter()
        .filter_map(|opt| {
            if let SessionConfigKind::Select(sel) = &opt.kind {
                let values = match &sel.options {
                    SessionConfigSelectOptions::Ungrouped(list) => list
                        .iter()
                        .map(|o| ConfigValue {
                            value: o.value.to_string(),
                            name: o.name.clone(),
                        })
                        .collect(),
                    SessionConfigSelectOptions::Grouped(groups) => groups
                        .iter()
                        .flat_map(|g| {
                            g.options.iter().map(|o| ConfigValue {
                                value: o.value.to_string(),
                                name: o.name.clone(),
                            })
                        })
                        .collect(),
                    _ => return None,
                };
                Some(ConfigOption {
                    id: opt.id.to_string(),
                    name: opt.name.clone(),
                    current_value: sel.current_value.to_string(),
                    options: values,
                })
            } else {
                None
            }
        })
        .collect()
}

type EventSender = Rc<RefCell<std::sync::mpsc::Sender<AcpEvent>>>;

struct FlyntAcpClient {
    tx: EventSender,
}

#[async_trait::async_trait(?Send)]
impl Client for FlyntAcpClient {
    async fn request_permission(
        &self,
        args: RequestPermissionRequest,
    ) -> agent_client_protocol::Result<RequestPermissionResponse> {
        let option = args
            .options
            .iter()
            .find(|o| {
                matches!(
                    o.kind,
                    PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
                )
            })
            .or_else(|| args.options.first());

        match option {
            Some(o) => Ok(RequestPermissionResponse::new(
                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                    o.option_id.clone(),
                )),
            )),
            None => Ok(RequestPermissionResponse::new(
                RequestPermissionOutcome::Cancelled,
            )),
        }
    }

    async fn session_notification(
        &self,
        args: SessionNotification,
    ) -> agent_client_protocol::Result<()> {
        tracing::debug!("ACP session_notification received: {:?}", std::mem::discriminant(&args.update));
        let tx = self.tx.borrow();
        match args.update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                if let ContentBlock::Text(text) = chunk.content {
                    let _ = tx.send(AcpEvent::TextDelta(text.text));
                }
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                if let ContentBlock::Text(text) = chunk.content {
                    let _ = tx.send(AcpEvent::ThoughtDelta(text.text));
                }
            }
            SessionUpdate::ToolCall(tc) => {
                let _ = tx.send(AcpEvent::ToolCallStarted {
                    id: tc.tool_call_id.to_string(),
                    title: tc.title,
                    kind: format!("{:?}", tc.kind),
                    args: tc.raw_input,
                });
            }
            SessionUpdate::ToolCallUpdate(update) => {
                let _ = tx.send(AcpEvent::ToolCallUpdated {
                    id: update.tool_call_id.to_string(),
                    status: update
                        .fields
                        .status
                        .map(|s| format!("{s:?}"))
                        .unwrap_or_default(),
                    title: update.fields.title,
                });
            }
            SessionUpdate::AvailableCommandsUpdate(cmds) => {
                let commands: Vec<SlashCommand> = cmds
                    .available_commands
                    .into_iter()
                    .map(|c| SlashCommand {
                        name: c.name,
                        description: c.description,
                    })
                    .collect();
                let _ = tx.send(AcpEvent::CommandsAvailable(commands));
            }
            SessionUpdate::ConfigOptionUpdate(update) => {
                let opts = extract_config_options(&update.config_options);
                if !opts.is_empty() {
                    let _ = tx.send(AcpEvent::ConfigChanged(opts));
                }
            }
            other => {
                tracing::debug!("ACP unhandled session update: {:?}", std::mem::discriminant(&other));
            }
        }
        Ok(())
    }
}

/// A live ACP session connected to an Omegon child process.
pub struct AcpSession {
    conn: Rc<ClientSideConnection>,
    session_id: SessionId,
    tx: std::sync::mpsc::Sender<AcpEvent>,
    #[allow(dead_code)]
    auth_method_id: Option<String>,
    _child: Child,
}

impl AcpSession {
    /// Spawn `omegon acp` and perform the ACP handshake.
    pub async fn connect(
        omegon_binary: PathBuf,
        cwd: PathBuf,
        agent_id: Option<String>,
    ) -> Result<(Self, std::sync::mpsc::Receiver<AcpEvent>)> {
        let (tx, rx) = std::sync::mpsc::channel();
        let done_tx = tx.clone();

        let mut cmd = Command::new(&omegon_binary);
        cmd.arg("acp")
            .arg("--cwd")
            .arg(&cwd)
            .arg("-y")
            .env("FLYNT_VAULT", &cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());
        if let Some(ref id) = agent_id {
            cmd.arg("--agent").arg(id);
        }
        let mut child = cmd.spawn()?;

        let child_stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("no stdin"))?;
        let child_stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("no stdout"))?;

        let client = FlyntAcpClient {
            tx: Rc::new(RefCell::new(tx)),
        };

        let (conn, io_task) = ClientSideConnection::new(
            client,
            child_stdin.compat_write(),
            child_stdout.compat(),
            |fut| { dioxus::prelude::spawn(fut); },
        );

        dioxus::prelude::spawn(async move {
            if let Err(e) = io_task.await {
                tracing::error!("ACP I/O error: {e}");
            }
        });

        let conn = Rc::new(conn);

        // Initialize
        let init_resp = conn
            .initialize(
                InitializeRequest::new(agent_client_protocol::ProtocolVersion::LATEST)
                    .client_info(agent_client_protocol::Implementation::new("flynt", "0.1.0")),
            )
            .await
            .map_err(|e| anyhow::anyhow!("ACP init failed: {e}"))?;

        let auth_method_id = init_resp.auth_methods.first().map(|m| m.id().to_string());

        // Create session
        let session_resp = conn
            .new_session(NewSessionRequest::new(&cwd))
            .await
            .map_err(|e| anyhow::anyhow!("ACP session failed: {e}"))?;

        // Send initial config options
        if let Some(opts) = &session_resp.config_options {
            let config = extract_config_options(opts);
            if !config.is_empty() {
                let _ = done_tx.send(AcpEvent::ConfigChanged(config));
            }
        }

        Ok((
            Self {
                conn,
                session_id: session_resp.session_id,
                tx: done_tx,
                auth_method_id,
                _child: child,
            },
            rx,
        ))
    }

    /// Trigger OAuth login by spawning `omegon auth login [provider]`.
    /// This opens the browser for the OAuth flow.
    pub async fn login(&self, omegon_binary: &PathBuf, provider: &str) {
        let provider = if provider.is_empty() { "anthropic" } else { provider };
        let _ = self.tx.send(AcpEvent::TextDelta(
            format!("Opening {provider} login…\n"),
        ));

        let result = tokio::process::Command::new(omegon_binary)
            .arg("auth")
            .arg("login")
            .arg(provider)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await;

        match result {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let msg = if stdout.trim().is_empty() {
                    format!("Logged in to {provider}.")
                } else {
                    stdout.trim().to_string()
                };
                let _ = self.tx.send(AcpEvent::TextDelta(msg));
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                let msg = if !stderr.trim().is_empty() {
                    stderr.trim().to_string()
                } else if !stdout.trim().is_empty() {
                    stdout.trim().to_string()
                } else {
                    format!("Login to {provider} failed (exit code {})", output.status)
                };
                let _ = self.tx.send(AcpEvent::Error(msg));
            }
            Err(e) => {
                let _ = self.tx.send(AcpEvent::Error(format!("Failed to run omegon auth login: {e}")));
            }
        }
    }

    /// Send a user prompt.
    pub fn prompt(&self, text: &str) {
        tracing::info!("AcpSession::prompt sending to Omegon ({} chars)", text.len());
        let req = PromptRequest::new(
            self.session_id.clone(),
            vec![ContentBlock::Text(TextContent::new(text))],
        );
        let conn = self.conn.clone();
        let tx = self.tx.clone();
        dioxus::prelude::spawn(async move {
            match conn.prompt(req).await {
                Ok(_) => {
                    tracing::info!("AcpSession::prompt completed");
                    let _ = tx.send(AcpEvent::Done);
                }
                Err(e) => {
                    tracing::error!("AcpSession::prompt failed: {e}");
                    let _ = tx.send(AcpEvent::Error(format!("{e}")));
                }
            }
        });
    }

    /// Change a config option (model, thinking, posture).
    pub async fn set_config(&self, config_id: &str, value: &str) {
        let req = SetSessionConfigOptionRequest::new(
            self.session_id.clone(),
            SessionConfigId::new(config_id),
            SessionConfigValueId::new(value),
        );
        if let Err(e) = self.conn.set_session_config_option(req).await {
            let _ = self.tx.send(AcpEvent::Error(format!("Config change failed: {e}")));
        }
    }

    // ── Extension management ──────────────────────────────────────────

    async fn ext_call(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let raw_params = serde_json::value::RawValue::from_string(
            serde_json::to_string(&params)?,
        )?;
        let req = ExtRequest::new(method, raw_params.into());
        let resp = self.conn.ext_method(req).await
            .map_err(|e| anyhow::anyhow!("ext_method failed: {e}"))?;
        let value: serde_json::Value = serde_json::from_str(resp.0.get())?;
        if let Some(err) = value["error"].as_str() {
            anyhow::bail!("{err}");
        }
        Ok(value)
    }

    /// List all installed extensions with config schema, current values, and secret status.
    pub async fn extensions_list(&self) -> Result<serde_json::Value> {
        self.ext_call("extensions/list", serde_json::json!({})).await
    }

    /// Set a config value for an extension.
    pub async fn extensions_config_set(&self, extension: &str, key: &str, value: &str) -> Result<serde_json::Value> {
        self.ext_call("extensions/config_set", serde_json::json!({
            "extension": extension,
            "key": key,
            "value": value,
        })).await
    }

    /// Store a secret in the OS keychain for an extension.
    pub async fn extensions_secret_set(&self, extension: &str, name: &str, value: &str) -> Result<serde_json::Value> {
        self.ext_call("extensions/secret_set", serde_json::json!({
            "extension": extension,
            "name": name,
            "value": value,
        })).await
    }

    /// Delete a secret from the keychain.
    pub async fn extensions_secret_delete(&self, name: &str) -> Result<serde_json::Value> {
        self.ext_call("extensions/secret_delete", serde_json::json!({
            "name": name,
        })).await
    }

    /// Enable an extension.
    pub async fn extensions_enable(&self, extension: &str) -> Result<serde_json::Value> {
        self.ext_call("extensions/enable", serde_json::json!({
            "extension": extension,
        })).await
    }

    /// Disable an extension.
    pub async fn extensions_disable(&self, extension: &str) -> Result<serde_json::Value> {
        self.ext_call("extensions/disable", serde_json::json!({
            "extension": extension,
        })).await
    }

    /// Install an extension from a local path, git URL, or tarball URI.
    pub async fn extensions_install(&self, uri: &str) -> Result<serde_json::Value> {
        self.ext_call("extensions/install", serde_json::json!({
            "uri": uri,
        })).await
    }

    /// Remove an installed extension.
    pub async fn extensions_remove(&self, extension: &str) -> Result<serde_json::Value> {
        self.ext_call("extensions/remove", serde_json::json!({
            "extension": extension,
        })).await
    }

    /// Update an extension (git pull + rebuild). Pass None to update all.
    pub async fn extensions_update(&self, extension: Option<&str>) -> Result<serde_json::Value> {
        let mut params = serde_json::json!({});
        if let Some(name) = extension {
            params["extension"] = serde_json::Value::String(name.into());
        }
        self.ext_call("extensions/update", params).await
    }

    /// List available skills (bundled + project-local).
    pub async fn skills_list(&self) -> Result<serde_json::Value> {
        self.ext_call("skills/list", serde_json::json!({})).await
    }

    /// Install all bundled skills to ~/.omegon/skills/.
    pub async fn skills_install(&self) -> Result<serde_json::Value> {
        self.ext_call("skills/install", serde_json::json!({})).await
    }

    // ── Control requests (TUI parity) ──────────────────────────

    /// Generic control request — maps to TUI slash commands.
    async fn control_call(&self, command: &str, args: &str) -> Result<serde_json::Value> {
        let mut params = serde_json::json!({});
        if !args.is_empty() {
            params["args"] = serde_json::Value::String(args.into());
        }
        self.ext_call(&format!("control/{command}"), params).await
    }

    /// Session statistics (model, turns, context usage, etc.)
    pub async fn stats(&self) -> Result<serde_json::Value> {
        self.control_call("stats", "").await
    }

    /// Get or set max turns.
    pub async fn max_turns(&self, value: Option<u32>) -> Result<serde_json::Value> {
        let args = value.map(|v| v.to_string()).unwrap_or_default();
        self.control_call("max_turns", &args).await
    }

    /// List available personas.
    pub async fn persona_list(&self) -> Result<serde_json::Value> {
        self.control_call("persona_list", "").await
    }

    /// Switch persona.
    pub async fn persona_switch(&self, name: &str) -> Result<serde_json::Value> {
        self.control_call("persona_switch", name).await
    }

    /// View current profile (model, thinking, posture, context window).
    pub async fn profile_view(&self) -> Result<serde_json::Value> {
        self.control_call("profile_view", "").await
    }

    /// Context usage status.
    pub async fn context_status(&self) -> Result<serde_json::Value> {
        self.control_call("context_status", "").await
    }

    /// Get or set context class.
    pub async fn context_class(&self, class: Option<&str>) -> Result<serde_json::Value> {
        self.control_call("context_class", class.unwrap_or("")).await
    }

    /// Get or set runtime mode (slim/standard).
    pub async fn runtime_mode(&self, mode: Option<&str>) -> Result<serde_json::Value> {
        self.control_call("runtime_mode", mode.unwrap_or("")).await
    }

    /// View configured secrets (names + recipes, no values).
    pub async fn secrets_view(&self) -> Result<serde_json::Value> {
        self.control_call("secrets_view", "").await
    }

    /// Vault status.
    pub async fn vault_status(&self) -> Result<serde_json::Value> {
        self.control_call("vault_status", "").await
    }

    /// Auth status.
    pub async fn auth_status(&self) -> Result<serde_json::Value> {
        self.control_call("auth_status", "").await
    }

    /// Add a session note.
    pub async fn note_add(&self, text: &str) -> Result<serde_json::Value> {
        self.control_call("note_add", text).await
    }

    /// View all notes.
    pub async fn notes_view(&self) -> Result<serde_json::Value> {
        self.control_call("notes_view", "").await
    }

    /// Clear all notes.
    pub async fn notes_clear(&self) -> Result<serde_json::Value> {
        self.control_call("notes_clear", "").await
    }

    /// Workspace status.
    pub async fn workspace_status(&self) -> Result<serde_json::Value> {
        self.control_call("workspace_status", "").await
    }

    /// List all workspaces.
    pub async fn workspace_list(&self) -> Result<serde_json::Value> {
        self.control_call("workspace_list", "").await
    }
}
