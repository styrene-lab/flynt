use codex_core::models::{CodexOperatorSettings, LocalRuntimeConfig, OmegonProfile};
use codex_store::{vault::Vault, watcher::{VaultChangeEvent, VaultWatcher}};
use std::{path::{Path, PathBuf}, process::Stdio, sync::Arc};
use tokio::{process::Command, sync::broadcast};
use tracing::{info, warn};

#[derive(Clone)]
pub struct OmegonRuntimeContext {
    pub local_state_root: PathBuf,
    pub codex_index_db_path: PathBuf,
    pub omegon_runtime_root: PathBuf,
    pub omegon_mind_db_path: PathBuf,
    pub home_dir: PathBuf,
    pub project_profile_path: PathBuf,
    pub global_profile_path: PathBuf,
    pub operator_settings_path: PathBuf,
    pub extensions_dir: PathBuf,
    pub vox_manifest_path: PathBuf,
}

impl OmegonRuntimeContext {
    fn discover(vault_root: &std::path::Path, runtime: &LocalRuntimeConfig) -> Self {
        let default_local_state_root = std::env::var("CODEX_LOCAL_STATE")
            .map(PathBuf::from)
            .ok()
            .filter(|path| path.is_absolute())
            .or_else(dirs::data_local_dir)
            .unwrap_or_else(|| vault_root.join(".codex-local"))
            .join("codex");
        let local_state_root = runtime
            .local_state_root
            .clone()
            .filter(|path| path.is_absolute())
            .unwrap_or(default_local_state_root);
        let omegon_runtime_root = runtime
            .omegon_runtime_root
            .clone()
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| local_state_root.join("omegon"));
        let codex_index_db_path = runtime
            .codex_index_db_path
            .clone()
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| local_state_root.join("codex-index.db"));
        let omegon_mind_db_path = runtime
            .omegon_mind_db_path
            .clone()
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| omegon_runtime_root.join("minds/codex.db"));
        let home_dir = std::env::var("OMEGON_HOME")
            .map(PathBuf::from)
            .ok()
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| omegon_runtime_root.clone());

        Self {
            local_state_root,
            codex_index_db_path,
            omegon_runtime_root,
            omegon_mind_db_path,
            project_profile_path: vault_root.join(".omegon/profile.json"),
            global_profile_path: home_dir.join("profile.json"),
            operator_settings_path: vault_root.join(".codex/operator-settings.json"),
            extensions_dir: home_dir.join("extensions"),
            vox_manifest_path: home_dir.join("extensions/vox/manifest.toml"),
            home_dir,
        }
    }

    pub fn load_project_profile(&self) -> OmegonProfile {
        std::fs::read_to_string(&self.project_profile_path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .or_else(|| {
                std::fs::read_to_string(&self.global_profile_path)
                    .ok()
                    .and_then(|content| serde_json::from_str(&content).ok())
            })
            .unwrap_or_default()
    }

    pub fn save_project_profile(&self, profile: &OmegonProfile) -> anyhow::Result<()> {
        if let Some(parent) = self.project_profile_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.project_profile_path, serde_json::to_string_pretty(profile)?)?;
        Ok(())
    }

    pub fn load_operator_settings(&self) -> CodexOperatorSettings {
        std::fs::read_to_string(&self.operator_settings_path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    pub fn save_operator_settings(&self, settings: &CodexOperatorSettings) -> anyhow::Result<()> {
        if let Some(parent) = self.operator_settings_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            &self.operator_settings_path,
            serde_json::to_string_pretty(settings)?,
        )?;
        Ok(())
    }

    pub async fn spawn_background_host(&self, vault_root: &Path) -> anyhow::Result<tokio::process::Child> {
        let binary = std::env::var("OMEGON_BIN").unwrap_or_else(|_| "omegon".into());
        let child = Command::new(binary)
            .current_dir(vault_root)
            .env("CODEX_VAULT", vault_root)
            .env("OMEGON_HOME", &self.home_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(child)
    }
}

#[cfg(test)]
mod tests {
    use super::OmegonRuntimeContext;
    use codex_core::models::{CodexOperatorSettings, LocalRuntimeConfig, OmegonProfile, OmegonProfileModel};
    use tempfile::TempDir;

    #[test]
    fn derives_runtime_paths_from_local_runtime_config() {
        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        let runtime = OmegonRuntimeContext::discover(
            &vault_root,
            &LocalRuntimeConfig {
                local_state_root: Some(tmp.path().join("state")),
                codex_index_db_path: Some(tmp.path().join("state/custom-index.db")),
                omegon_runtime_root: Some(tmp.path().join("state/omegon-runtime")),
                omegon_mind_db_path: Some(tmp.path().join("state/omegon-runtime/minds/codex-mind.db")),
            },
        );

        assert_eq!(runtime.local_state_root, tmp.path().join("state"));
        assert_eq!(runtime.codex_index_db_path, tmp.path().join("state/custom-index.db"));
        assert_eq!(runtime.omegon_runtime_root, tmp.path().join("state/omegon-runtime"));
        assert_eq!(runtime.omegon_mind_db_path, tmp.path().join("state/omegon-runtime/minds/codex-mind.db"));
    }

    #[test]
    fn loads_global_profile_when_project_profile_missing() {
        let tmp = TempDir::new().unwrap();
        let runtime = OmegonRuntimeContext {
            local_state_root: tmp.path().join("local"),
            codex_index_db_path: tmp.path().join("local/codex-index.db"),
            omegon_runtime_root: tmp.path().join("local/omegon"),
            omegon_mind_db_path: tmp.path().join("local/omegon/minds/codex.db"),
            home_dir: tmp.path().join("home"),
            project_profile_path: tmp.path().join("vault/.omegon/profile.json"),
            global_profile_path: tmp.path().join("home/profile.json"),
            operator_settings_path: tmp.path().join("vault/.codex/operator-settings.json"),
            extensions_dir: tmp.path().join("home/extensions"),
            vox_manifest_path: tmp.path().join("home/extensions/vox/manifest.toml"),
        };
        std::fs::create_dir_all(runtime.global_profile_path.parent().unwrap()).unwrap();
        std::fs::write(
            &runtime.global_profile_path,
            serde_json::to_string(&OmegonProfile {
                last_used_model: Some(OmegonProfileModel {
                    provider: "anthropic".into(),
                    model_id: "claude-sonnet-4-6".into(),
                }),
                ..OmegonProfile::default()
            })
            .unwrap(),
        )
        .unwrap();

        let loaded = runtime.load_project_profile();
        assert_eq!(loaded.last_used_model.unwrap().provider, "anthropic");
    }

    #[test]
    fn round_trips_operator_settings() {
        let tmp = TempDir::new().unwrap();
        let runtime = OmegonRuntimeContext {
            local_state_root: tmp.path().join("local"),
            codex_index_db_path: tmp.path().join("local/codex-index.db"),
            omegon_runtime_root: tmp.path().join("local/omegon"),
            omegon_mind_db_path: tmp.path().join("local/omegon/minds/codex.db"),
            home_dir: tmp.path().join("home"),
            project_profile_path: tmp.path().join("vault/.omegon/profile.json"),
            global_profile_path: tmp.path().join("home/profile.json"),
            operator_settings_path: tmp.path().join("vault/.codex/operator-settings.json"),
            extensions_dir: tmp.path().join("home/extensions"),
            vox_manifest_path: tmp.path().join("home/extensions/vox/manifest.toml"),
        };

        let settings = CodexOperatorSettings {
            active_persona: "scribe".into(),
            ..CodexOperatorSettings::default()
        };
        runtime.save_operator_settings(&settings).unwrap();

        let loaded = runtime.load_operator_settings();
        assert_eq!(loaded.active_persona, "scribe");
    }
}

/// Top-level runtime context injected into the Dioxus app.
#[derive(Clone)]
pub struct AppContext {
    pub vault: Arc<Vault>,
    pub vault_events: broadcast::Sender<VaultChangeEvent>,
    pub omegon: OmegonRuntimeContext,
}

/// Build AppContext at launch. Reads CODEX_VAULT env var or defaults to ~/Documents/Codex.
pub fn bootstrap_from_env() -> AppContext {
    let vault_root = std::env::var("CODEX_VAULT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::document_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("Codex")
        });

    std::fs::create_dir_all(&vault_root).expect("cannot create vault directory");

    let vault = Arc::new(Vault::open(&vault_root).expect("failed to open vault"));

    match vault.reindex() {
        Ok((n, errs)) => {
            info!("Vault indexed: {n} files");
            for e in &errs {
                warn!("Index error: {e}");
            }
        }
        Err(e) => warn!("Reindex failed: {e}"),
    }

    let (tx, _rx) = broadcast::channel::<VaultChangeEvent>(256);
    let tx_clone = tx.clone();
    let vault_root_clone = vault_root.clone();
    let vault_clone = Arc::clone(&vault);

    tokio::spawn(async move {
        let watcher = match VaultWatcher::new(&vault_root_clone) {
            Ok(w) => w,
            Err(e) => { warn!("VaultWatcher failed to start: {e}"); return; }
        };
        loop {
            match watcher.rx.recv() {
                Ok(evt) => {
                    let path = match &evt {
                        VaultChangeEvent::FileModified(p) | VaultChangeEvent::FileCreated(p) => {
                            Some(p.clone())
                        }
                        VaultChangeEvent::FileDeleted(_) => None,
                    };
                    if let Some(p) = path {
                        if let Err(e) = vault_clone.index_file(&p) {
                            warn!("Re-index failed for {}: {e}", p.display());
                        }
                    }
                    let _ = tx_clone.send(evt);
                }
                Err(_) => break,
            }
        }
    });

    let omegon = OmegonRuntimeContext::discover(&vault_root, &vault.config.local_runtime);

    AppContext { vault, vault_events: tx, omegon }
}
