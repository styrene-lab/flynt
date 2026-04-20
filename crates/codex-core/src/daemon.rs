//! Vault-scoped agent daemon configuration.
//!
//! Defines the desired state for a per-vault Omegon daemon instance
//! and Vox communication channel bindings. Codex manages the daemon
//! directly (Tier 1) or declares desired state for Auspex (Tier 2).

use serde::{Deserialize, Serialize};

/// Per-vault agent daemon configuration.
/// Stored in `.codex/operator-settings.json` under `agent_daemon`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDaemonConfig {
    /// Whether the daemon should be running for this vault.
    #[serde(default)]
    pub enabled: bool,

    /// Auto-start on app launch.
    #[serde(default)]
    pub auto_start: bool,

    /// Model to use (e.g., "anthropic:claude-sonnet-4-7").
    #[serde(default)]
    pub model: Option<String>,

    /// Behavioral posture (fabricator, architect, explorator, devastator).
    #[serde(default)]
    pub posture: Option<String>,

    /// Persona name for the agent.
    #[serde(default)]
    pub persona: Option<String>,

    /// Port for omegon serve (0 = auto-assign).
    #[serde(default = "default_port")]
    pub port: u16,

    /// Vox communication channels — how to reach this vault's agent externally.
    #[serde(default)]
    pub vox: VoxConfig,

    /// Inbound message processing rules.
    #[serde(default)]
    pub inbound_rules: Vec<InboundRule>,
}

fn default_port() -> u16 { 7842 }

impl Default for AgentDaemonConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_start: false,
            model: None,
            posture: None,
            persona: None,
            port: 7842,
            vox: VoxConfig::default(),
            inbound_rules: vec![
                InboundRule::new("link:*", InboundAction::ResearchAndStore),
                InboundRule::new("idea:*", InboundAction::CaptureToDailyNote),
                InboundRule::new("task:*", InboundAction::AddToBoard),
            ],
        }
    }
}

/// Vox communication channel configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VoxConfig {
    /// Signal bridge (desktop-side via signal-cli).
    #[serde(default)]
    pub signal: Option<SignalChannel>,

    /// Email inbound (IMAP watch).
    #[serde(default)]
    pub email: Option<EmailChannel>,

    /// Webhook endpoint (generic HTTP POST inbound).
    #[serde(default)]
    pub webhook: Option<WebhookChannel>,

    /// RNS/LXMF channel (mesh/offline via Reticulum).
    #[serde(default)]
    pub rns: Option<RnsChannel>,

    /// iOS Share Sheet inbound (mobile only — items shared to Codex app).
    #[serde(default)]
    pub share_sheet: bool,
}

/// Signal messaging channel — runs on desktop via signal-cli.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalChannel {
    pub enabled: bool,
    /// Phone number registered with Signal.
    pub phone: String,
    /// Only accept messages from these numbers. Empty = accept all.
    #[serde(default)]
    pub allowed_senders: Vec<String>,
}

/// Email inbound channel — IMAP folder watch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailChannel {
    pub enabled: bool,
    /// IMAP server (e.g., "imap.gmail.com:993").
    pub server: String,
    /// Email address to watch.
    pub address: String,
    /// Folder to watch (default: INBOX).
    #[serde(default = "default_inbox")]
    pub folder: String,
    /// Only accept from these senders. Empty = accept all.
    #[serde(default)]
    pub allowed_senders: Vec<String>,
}

fn default_inbox() -> String { "INBOX".into() }

/// Generic webhook inbound — HTTP POST to the daemon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookChannel {
    pub enabled: bool,
    /// Path on the daemon's HTTP server (e.g., "/inbound").
    pub path: String,
    /// HMAC secret for request validation.
    #[serde(default)]
    pub secret: Option<String>,
}

/// RNS/LXMF mesh channel — offline-capable via Reticulum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RnsChannel {
    pub enabled: bool,
    /// RNS destination hash for this vault's agent.
    #[serde(default)]
    pub destination_hash: Option<String>,
    /// LXMF propagation node address (optional relay).
    #[serde(default)]
    pub propagation_node: Option<String>,
}

/// Rules for processing inbound messages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboundRule {
    /// Pattern to match (e.g., "link:*", "idea:*", "task:*", "*").
    pub pattern: String,
    /// What to do when matched.
    pub action: InboundAction,
}

impl InboundRule {
    pub fn new(pattern: impl Into<String>, action: InboundAction) -> Self {
        Self { pattern: pattern.into(), action }
    }

    /// Check if a message matches this rule's pattern.
    pub fn matches(&self, message: &str) -> bool {
        if self.pattern == "*" { return true; }
        if let Some(prefix) = self.pattern.strip_suffix('*') {
            message.to_lowercase().starts_with(&prefix.to_lowercase())
        } else {
            message.to_lowercase() == self.pattern.to_lowercase()
        }
    }
}

/// What to do with an inbound message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InboundAction {
    /// Send the message as a prompt to the daemon's Omegon agent.
    Prompt,
    /// Research a link and store findings as a new note.
    ResearchAndStore,
    /// Capture as an entry in today's daily note.
    CaptureToDailyNote,
    /// Add as a task to the default board.
    AddToBoard,
    /// Store as a raw note in a specific folder.
    StoreInFolder(String),
    /// Ignore (drop the message).
    Ignore,
}

/// Runtime state of the daemon (not persisted — computed).
#[derive(Debug, Clone, PartialEq)]
pub enum DaemonState {
    /// Not configured or disabled.
    Disabled,
    /// Should be running but isn't.
    Stopped,
    /// Starting up.
    Starting,
    /// Running and healthy.
    Running {
        pid: u32,
        port: u16,
        ws_url: String,
    },
    /// Running but unhealthy.
    Unhealthy(String),
    /// Managed by Auspex (Tier 2).
    AuspexManaged {
        instance_id: String,
    },
}
