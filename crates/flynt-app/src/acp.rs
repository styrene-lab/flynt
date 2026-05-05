//! ACP (Agent Client Protocol) client for communicating with Omegon.
//!
//! Spawns `omegon acp` as a child process and communicates via structured
//! JSON-RPC over stdio. Streams text deltas, tool calls, slash commands,
//! config options, and auth events back to the UI through a channel.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use agent_client_protocol::{
    Agent, Client, ClientSideConnection, ContentBlock, InitializeRequest,
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
}
