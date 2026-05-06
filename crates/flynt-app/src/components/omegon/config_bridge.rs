//! Config bridge — unifies OmegonProfile, FlyntOperatorSettings.acp_config,
//! and the live ACP session into a single coherent config view.
//!
//! This module owns the merge/persist/sync logic so that changing model in
//! the Settings view and changing it in the agent rail dropdown produce the
//! same outcome. Designed for extraction into a shared `omegon-config` crate
//! when Auspex needs the same surface.

use std::collections::HashMap;
use flynt_core::models::{FlyntOperatorSettings, OmegonProfile, OmegonProfileModel};

/// Merged view of all Omegon configuration.
/// Authoritative source for what the agent should be running with.
#[derive(Debug, Clone)]
pub struct UnifiedOmegonConfig {
    // ── Session-level (acp_config is authoritative, profile is fallback) ──
    pub model: String,
    pub thinking: String,
    pub posture: String,

    // ── Profile-level (OmegonProfile is authoritative) ──
    pub max_turns: u32,
    pub provider_order: Vec<String>,
    pub avoid_providers: Vec<String>,
    pub downgrade_overrides: Vec<String>,
    pub context_floor_pin: Option<String>,
    pub embed_url: Option<String>,
    pub embed_model: Option<String>,

    // ── Operator-level (FlyntOperatorSettings is authoritative) ──
    pub active_persona: String,
    pub enabled_skills: Vec<String>,
    pub preferred_extensions: Vec<String>,
    pub agent_id: Option<String>,
}

impl UnifiedOmegonConfig {
    /// Load from the two persistence layers, with acp_config winning for
    /// session-level fields (model, thinking, posture).
    pub fn load(profile: &OmegonProfile, operator: &FlyntOperatorSettings) -> Self {
        let acp = &operator.acp_config;

        // Model: acp_config > profile.last_used_model > default
        let model = acp
            .get("model")
            .cloned()
            .or_else(|| {
                profile.last_used_model.as_ref().map(|m| {
                    format!("{}:{}", m.provider, m.model_id)
                })
            })
            .unwrap_or_else(|| "anthropic:claude-sonnet-4-6".into());

        // Thinking: acp_config > profile > default
        let thinking = acp
            .get("thinking")
            .cloned()
            .or_else(|| profile.thinking_level.clone())
            .unwrap_or_else(|| "minimal".into());

        // Posture: acp_config > default
        let posture = acp
            .get("posture")
            .cloned()
            .unwrap_or_else(|| "fabricator".into());

        Self {
            model,
            thinking,
            posture,
            max_turns: profile.max_turns.unwrap_or(50),
            provider_order: profile.provider_order.clone(),
            avoid_providers: profile.avoid_providers.clone(),
            downgrade_overrides: profile.downgrade_overrides.clone(),
            context_floor_pin: profile.context_floor_pin.clone(),
            embed_url: profile.embed_url.clone(),
            embed_model: profile.embed_model.clone(),
            active_persona: operator.active_persona.clone(),
            enabled_skills: operator.enabled_skills.clone(),
            preferred_extensions: operator.preferred_extensions.clone(),
            agent_id: operator.agent_id.clone(),
        }
    }

    /// Save to both persistence layers. Returns updated copies.
    pub fn save_to(
        &self,
        profile: &mut OmegonProfile,
        operator: &mut FlyntOperatorSettings,
    ) {
        // Session-level → acp_config (authoritative for next session)
        operator.acp_config.insert("model".into(), self.model.clone());
        operator.acp_config.insert("thinking".into(), self.thinking.clone());
        operator.acp_config.insert("posture".into(), self.posture.clone());

        // Also mirror model/thinking to profile (for cold starts / CLI use)
        if let Some((provider, model_id)) = self.model.split_once(':') {
            profile.last_used_model = Some(OmegonProfileModel {
                provider: provider.into(),
                model_id: model_id.into(),
            });
        }
        profile.thinking_level = Some(self.thinking.clone());

        // Profile-only fields
        profile.max_turns = Some(self.max_turns);
        profile.provider_order = self.provider_order.clone();
        profile.avoid_providers = self.avoid_providers.clone();
        profile.downgrade_overrides = self.downgrade_overrides.clone();
        profile.context_floor_pin = self.context_floor_pin.clone();
        profile.embed_url = self.embed_url.clone();
        profile.embed_model = self.embed_model.clone();

        // Operator-level fields
        operator.active_persona = self.active_persona.clone();
        operator.enabled_skills = self.enabled_skills.clone();
        operator.preferred_extensions = self.preferred_extensions.clone();
        operator.agent_id = self.agent_id.clone();
    }

    /// Sync session-level fields from an ACP ConfigChanged event.
    /// Returns the ACP config entries that should be persisted.
    pub fn sync_from_acp(&mut self, acp_config: &HashMap<String, String>) {
        if let Some(model) = acp_config.get("model") {
            self.model = model.clone();
        }
        if let Some(thinking) = acp_config.get("thinking") {
            self.thinking = thinking.clone();
        }
        if let Some(posture) = acp_config.get("posture") {
            self.posture = posture.clone();
        }
    }

    /// Build the acp_config HashMap for persistence.
    pub fn to_acp_config(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        map.insert("model".into(), self.model.clone());
        map.insert("thinking".into(), self.thinking.clone());
        map.insert("posture".into(), self.posture.clone());
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_acp_config_wins_over_profile() {
        let profile = OmegonProfile {
            last_used_model: Some(OmegonProfileModel {
                provider: "anthropic".into(),
                model_id: "claude-opus-4-6".into(),
            }),
            thinking_level: Some("high".into()),
            ..Default::default()
        };
        let mut operator = FlyntOperatorSettings::default();
        operator.acp_config.insert("model".into(), "openai:gpt-5.4".into());
        operator.acp_config.insert("thinking".into(), "low".into());

        let config = UnifiedOmegonConfig::load(&profile, &operator);
        assert_eq!(config.model, "openai:gpt-5.4");
        assert_eq!(config.thinking, "low");
    }

    #[test]
    fn load_falls_back_to_profile() {
        let profile = OmegonProfile {
            last_used_model: Some(OmegonProfileModel {
                provider: "anthropic".into(),
                model_id: "claude-sonnet-4-6".into(),
            }),
            thinking_level: Some("medium".into()),
            max_turns: Some(30),
            ..Default::default()
        };
        let operator = FlyntOperatorSettings::default();

        let config = UnifiedOmegonConfig::load(&profile, &operator);
        assert_eq!(config.model, "anthropic:claude-sonnet-4-6");
        assert_eq!(config.thinking, "medium");
        assert_eq!(config.max_turns, 30);
    }

    #[test]
    fn save_writes_to_both_layers() {
        let config = UnifiedOmegonConfig {
            model: "anthropic:claude-opus-4-7".into(),
            thinking: "high".into(),
            posture: "architect".into(),
            max_turns: 40,
            provider_order: vec!["anthropic".into(), "openai".into()],
            avoid_providers: vec![],
            downgrade_overrides: vec![],
            context_floor_pin: None,
            embed_url: Some("http://localhost:11434".into()),
            embed_model: None,
            active_persona: "systems-engineer".into(),
            enabled_skills: vec!["review".into()],
            preferred_extensions: vec!["flynt".into()],
            agent_id: None,
        };

        let mut profile = OmegonProfile::default();
        let mut operator = FlyntOperatorSettings::default();
        config.save_to(&mut profile, &mut operator);

        // acp_config gets session-level
        assert_eq!(operator.acp_config.get("model").unwrap(), "anthropic:claude-opus-4-7");
        assert_eq!(operator.acp_config.get("thinking").unwrap(), "high");

        // profile gets mirrored model + profile-only fields
        assert_eq!(profile.last_used_model.unwrap().model_id, "claude-opus-4-7");
        assert_eq!(profile.max_turns, Some(40));
        assert_eq!(profile.provider_order, vec!["anthropic", "openai"]);

        // operator gets operator-level
        assert_eq!(operator.active_persona, "systems-engineer");
        assert_eq!(operator.enabled_skills, vec!["review"]);
    }

    #[test]
    fn sync_from_acp_updates_session_fields() {
        let mut config = UnifiedOmegonConfig {
            model: "old".into(),
            thinking: "old".into(),
            posture: "old".into(),
            max_turns: 50,
            provider_order: vec![],
            avoid_providers: vec![],
            downgrade_overrides: vec![],
            context_floor_pin: None,
            embed_url: None,
            embed_model: None,
            active_persona: "off".into(),
            enabled_skills: vec![],
            preferred_extensions: vec![],
            agent_id: None,
        };

        let mut acp = HashMap::new();
        acp.insert("model".into(), "anthropic:claude-sonnet-4-7".into());
        acp.insert("thinking".into(), "medium".into());

        config.sync_from_acp(&acp);
        assert_eq!(config.model, "anthropic:claude-sonnet-4-7");
        assert_eq!(config.thinking, "medium");
        assert_eq!(config.posture, "old"); // not in acp update
    }
}
