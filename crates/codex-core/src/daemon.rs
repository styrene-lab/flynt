//! Vault-scoped agent daemon configuration.
//!
//! Defines the desired state for a per-vault Omegon daemon instance
//! and Vox communication channel bindings. Codex manages the daemon
//! directly (Tier 1) or declares desired state for Auspex (Tier 2).

use serde::{Deserialize, Serialize};
use std::fmt;

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

    /// Capabilities the agent can use when processing inbound messages.
    /// The agent decides which capability fits — the user just sends natural language.
    #[serde(default)]
    pub capabilities: Vec<InboundCapability>,
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
            capabilities: vec![
                InboundCapability::ResearchLinks,
                InboundCapability::CaptureIdeas,
                InboundCapability::ManageTasks,
                InboundCapability::AnswerQuestions,
                InboundCapability::DailyDigest,
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

/// Capabilities the agent can use for inbound messages.
/// The agent sees these as tools/permissions — it decides when to use each one
/// based on the natural language content of the message. No user-side formatting needed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InboundCapability {
    /// Detect URLs, fetch content, summarize, store as a research note.
    ResearchLinks,
    /// Capture thoughts/ideas into the daily note.
    CaptureIdeas,
    /// Create/update/query tasks on kanban boards.
    ManageTasks,
    /// Answer questions about vault contents using search + graph.
    AnswerQuestions,
    /// Generate a daily digest of due tasks, decaying items, recent changes.
    DailyDigest,
    /// Create full documents from detailed descriptions.
    CreateDocuments,
    /// Search the vault and return relevant excerpts.
    SearchVault,
    /// Update existing notes with new information.
    EnrichNotes,
}

impl InboundCapability {
    /// All known capability variants.
    pub fn all() -> Vec<Self> {
        vec![
            Self::ResearchLinks,
            Self::CaptureIdeas,
            Self::ManageTasks,
            Self::AnswerQuestions,
            Self::DailyDigest,
            Self::CreateDocuments,
            Self::SearchVault,
            Self::EnrichNotes,
        ]
    }

    /// Human-readable label for the settings UI.
    pub fn label(&self) -> &'static str {
        match self {
            Self::ResearchLinks => "Research Links",
            Self::CaptureIdeas => "Capture Ideas",
            Self::ManageTasks => "Manage Tasks",
            Self::AnswerQuestions => "Answer Questions",
            Self::DailyDigest => "Daily Digest",
            Self::CreateDocuments => "Create Documents",
            Self::SearchVault => "Search Vault",
            Self::EnrichNotes => "Enrich Notes",
        }
    }

    /// System prompt fragment describing this capability to the agent.
    pub fn system_prompt(&self) -> &'static str {
        match self {
            Self::ResearchLinks => "When the user sends a URL or link, fetch the content, analyze it, and create a research note in the vault with a summary, key points, and relevant tags.",
            Self::CaptureIdeas => "When the user shares a thought or idea, capture it as an entry in today's daily note under the Notes section.",
            Self::ManageTasks => "When the user mentions something they need to do, create a task on the appropriate board. When they ask about tasks, query the boards and respond.",
            Self::AnswerQuestions => "When the user asks a question about their vault, notes, or projects, search the vault and knowledge graph to provide accurate answers with references.",
            Self::DailyDigest => "When asked for a digest or summary, compile due tasks, decaying items needing attention, and recent vault activity.",
            Self::CreateDocuments => "When the user describes something in detail that should be a document, create a properly structured markdown note with frontmatter.",
            Self::SearchVault => "When the user asks to find something, search across documents, tasks, and the graph. Return relevant excerpts and links.",
            Self::EnrichNotes => "When the user provides new information about an existing topic, find the relevant note and suggest updates or additions.",
        }
    }
}

/// Build the system prompt for the daemon agent based on enabled capabilities.
pub fn build_daemon_system_prompt(config: &AgentDaemonConfig, vault_name: &str) -> String {
    let mut prompt = format!(
        "You are the agent for the \"{}\" vault in Codex. \
         Messages come from the vault operator via various channels (Signal, email, etc.). \
         Respond concisely. Use your vault tools to take action.\n\n",
        vault_name
    );

    if config.capabilities.is_empty() {
        prompt.push_str("Process all messages as general prompts using available vault tools.\n");
    } else {
        prompt.push_str("Your capabilities:\n\n");
        for cap in &config.capabilities {
            prompt.push_str("- ");
            prompt.push_str(cap.system_prompt());
            prompt.push('\n');
        }
    }

    prompt
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

impl fmt::Display for DaemonState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled => write!(f, "Disabled"),
            Self::Stopped => write!(f, "Stopped"),
            Self::Starting => write!(f, "Starting…"),
            Self::Running { port, .. } => write!(f, "Running (port {port})"),
            Self::Unhealthy(reason) => write!(f, "Unhealthy: {reason}"),
            Self::AuspexManaged { instance_id } => write!(f, "Auspex ({instance_id})"),
        }
    }
}

impl DaemonState {
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }

    pub fn is_stopped(&self) -> bool {
        matches!(self, Self::Disabled | Self::Stopped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_capabilities_have_non_empty_prompts() {
        let caps = [
            InboundCapability::ResearchLinks,
            InboundCapability::CaptureIdeas,
            InboundCapability::ManageTasks,
            InboundCapability::AnswerQuestions,
            InboundCapability::DailyDigest,
            InboundCapability::CreateDocuments,
            InboundCapability::SearchVault,
            InboundCapability::EnrichNotes,
        ];
        for cap in &caps {
            let prompt = cap.system_prompt();
            assert!(!prompt.is_empty(), "{cap:?} has empty system prompt");
            assert!(prompt.len() > 20, "{cap:?} system prompt too short: {prompt}");
        }
    }

    #[test]
    fn build_prompt_empty_capabilities() {
        let config = AgentDaemonConfig {
            enabled: true,
            auto_start: false,
            model: None,
            posture: None,
            persona: None,
            port: 7842,
            capabilities: vec![],
            vox: VoxConfig::default(),
        };
        let prompt = build_daemon_system_prompt(&config, "Test Vault");
        assert!(prompt.contains("Test Vault"));
        assert!(prompt.contains("general prompts"));
    }

    #[test]
    fn build_prompt_with_capabilities() {
        let config = AgentDaemonConfig {
            enabled: true,
            auto_start: false,
            model: None,
            posture: None,
            persona: None,
            port: 7842,
            capabilities: vec![
                InboundCapability::ManageTasks,
                InboundCapability::SearchVault,
            ],
            vox: VoxConfig::default(),
        };
        let prompt = build_daemon_system_prompt(&config, "My Vault");
        assert!(prompt.contains("My Vault"));
        assert!(prompt.contains("Your capabilities:"));
        assert!(prompt.contains("task"));
        assert!(prompt.contains("search"));
    }
}
