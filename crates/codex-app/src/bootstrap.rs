use dioxus::prelude::{Signal, Writable};
use codex_core::{
    models::{CodexOperatorSettings, LocalRuntimeConfig, OmegonProfile, PublicationTarget, SyncConfig, VaultConfig},
    store::VaultStore,
};
use codex_store::{vault::Vault, watcher::{VaultChangeEvent, VaultWatcher}};
use serde::{Deserialize, Serialize};
use std::{path::{Path, PathBuf}, process::Stdio, sync::Arc};
use tokio::{process::Command, sync::broadcast};
use tracing::{info, warn};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LauncherProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_vault_root: Option<PathBuf>,
    #[serde(default)]
    pub wizard_completed: bool,
    #[serde(default)]
    pub recent_vaults: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_setup: Option<PendingVaultSetup>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PendingVaultSetup {
    OpenExisting { path: PathBuf },
    CreateLocal { path: PathBuf, name: String },
    LinkGithub { local_path: PathBuf, repo: String, branch: String },
    PublishPreview { output_path: PathBuf, repo: String, branch: String },
    SeedDemoPublication { repo_root: PathBuf, site_name: String },
}

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
    fn launcher_profile_path() -> PathBuf {
        std::env::var("CODEX_LAUNCHER_PROFILE")
            .map(PathBuf::from)
            .ok()
            .filter(|path| path.is_absolute())
            .or_else(|| dirs::config_local_dir().map(|dir| dir.join("codex/launcher-profile.json")))
            .unwrap_or_else(|| PathBuf::from("/tmp/codex-launcher-profile.json"))
    }

    pub fn load_launcher_profile() -> LauncherProfile {
        let path = Self::launcher_profile_path();
        std::fs::read_to_string(path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    pub fn save_launcher_profile(profile: &LauncherProfile) -> anyhow::Result<()> {
        let path = Self::launcher_profile_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(profile)?)?;
        Ok(())
    }

    pub fn initialize_vault(path: &Path, name: &str, sync: SyncConfig) -> anyhow::Result<Vault> {
        std::fs::create_dir_all(path)?;
        let vault = Vault::open(path)?;
        let mut config: VaultConfig = vault.config.clone();
        config.vault_name = name.to_string();
        config.sync = sync;
        vault.save_config(&config)?;
        Ok(Vault::open(path)?)
    }

    pub fn initialize_github_linked_vault(
        local_path: &Path,
        name: &str,
        repo: &str,
        branch: &str,
    ) -> anyhow::Result<Vault> {
        Self::initialize_vault(
            local_path,
            name,
            SyncConfig::Git {
                remote: repo.to_string(),
                branch: branch.to_string(),
                auto_commit_seconds: 60,
            },
        )
    }

    pub fn initialize_github_pages_publication(
        local_path: &Path,
        name: &str,
        repo: &str,
        branch: &str,
    ) -> anyhow::Result<Vault> {
        let vault = Self::initialize_github_linked_vault(local_path, name, repo, branch)?;
        let home_path = local_path.join("home.md");
        if !home_path.exists() {
            std::fs::write(
                &home_path,
                format!(
                    "+++\ntitle = \"Home\"\n[publication]\nenabled = true\nvisibility = \"public\"\n[publication.target]\nrepo = \"{repo}\"\nbranch = \"{branch}\"\nsite_dir = \"site\"\n+++\n\n# Home\n\nWelcome to {name}.\n"
                ),
            )?;
            vault.index_file(&home_path)?;
        }
        Ok(vault)
    }

    pub fn export_publication_preview(vault: &Vault) -> anyhow::Result<PathBuf> {
        let target = publication_output_path(vault);
        std::fs::create_dir_all(&target)?;
        let report = vault.export_publication_tree(&target)?;
        if !report.errors.is_empty() {
            anyhow::bail!(report.errors.join("; "));
        }
        Ok(target)
    }

    pub fn publication_target(vault: &Vault) -> Option<PublicationTarget> {
        vault
            .store
            .list_documents()
            .ok()
            .and_then(|docs: Vec<codex_core::models::DocumentMeta>| {
                docs.into_iter().find_map(|meta| {
                    vault.store.get_document(&meta.id).ok().flatten().and_then(|doc| {
                        doc.frontmatter.publication.target.clone()
                    })
                })
            })
    }

    pub fn seed_demo_publication_repo(repo_root: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(repo_root.join("src/pages"))?;
        std::fs::create_dir_all(repo_root.join("public/preview"))?;

        std::fs::write(
            repo_root.join("package.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "name": "codex-publication-demo",
                "private": true,
                "type": "module",
                "scripts": {
                    "dev": "astro dev",
                    "build": "astro build",
                    "preview": "astro preview"
                },
                "dependencies": {
                    "astro": "^5.0.0"
                }
            }))?,
        )?;

        std::fs::write(
            repo_root.join("astro.config.mjs"),
            "import { defineConfig } from 'astro/config';\n\nexport default defineConfig({\n  site: 'https://black-meridian.github.io/codex-site',\n});\n",
        )?;

        std::fs::write(
            repo_root.join("src/pages/index.astro"),
            "---\nconst title = 'Codex Publication Demo';\n---\n<html lang=\"en\">\n  <head>\n    <meta charset=\"utf-8\" />\n    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />\n    <title>{title}</title>\n    <style>body{font-family:system-ui,sans-serif;max-width:860px;margin:0 auto;padding:3rem;background:#06080e;color:#c4d8e4}a{color:#6ecad8}code{background:#0e1622;padding:.2rem .4rem;border-radius:4px}</style>\n  </head>\n  <body>\n    <h1>{title}</h1>\n    <p>This Astro site demonstrates what a published Codex vault can look like.</p>\n    <p>Copy local publication preview artifacts into <code>public/preview/</code> or evolve this into a richer adapter over the publication manifest.</p>\n    <ul>\n      <li><a href=\"/preview/home.html\">Preview exported home page</a></li>\n      <li><a href=\"https://github.com/black-meridian/codex\">Codex source</a></li>\n    </ul>\n  </body>\n</html>\n",
        )?;

        std::fs::write(
            repo_root.join("README.md"),
            "# Codex Publication Demo\n\nThis Astro site is the example/demo publication target for a published Codex vault.\n\n## Workflow\n\n1. Export a local publication preview from Codex.\n2. Copy the generated preview tree into `public/preview/`.\n3. Run `npm install` and `npm run dev`.\n\nThe long-term path is to replace the raw preview copy step with a richer Astro adapter over Codex publication manifests.\n",
        )?;

        Ok(())
    }

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
    use super::{publication_output_path, LauncherProfile, OmegonRuntimeContext, PendingVaultSetup};
    use codex_core::{
        models::{CodexOperatorSettings, LocalRuntimeConfig, OmegonProfile, OmegonProfileModel, PublicationTarget, SyncConfig},
        store::VaultStore,
    };
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
                styrene_identity_profile: Some("black-meridian".into()),
            },
        );

        assert_eq!(runtime.local_state_root, tmp.path().join("state"));
        assert_eq!(runtime.codex_index_db_path, tmp.path().join("state/custom-index.db"));
        assert_eq!(runtime.omegon_runtime_root, tmp.path().join("state/omegon-runtime"));
        assert_eq!(runtime.omegon_mind_db_path, tmp.path().join("state/omegon-runtime/minds/codex-mind.db"));
    }

    #[test]
    fn round_trips_launcher_profile() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("launcher-profile.json");
        let profile = LauncherProfile {
            last_vault_root: Some(tmp.path().join("vaults/black-meridian")),
            wizard_completed: true,
            recent_vaults: vec![tmp.path().join("vaults/black-meridian")],
            pending_setup: Some(PendingVaultSetup::LinkGithub {
                local_path: tmp.path().join("vaults/black-meridian"),
                repo: "git@github.com:black-meridian/codex-vault.git".into(),
                branch: "main".into(),
            }),
        };

        std::fs::write(&path, serde_json::to_string_pretty(&profile).unwrap()).unwrap();
        let loaded: LauncherProfile = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded, profile);
    }

    #[test]
    fn initializes_github_linked_vault_with_git_sync_config() {
        let tmp = TempDir::new().unwrap();
        let local_path = tmp.path().join("vault");
        let vault = OmegonRuntimeContext::initialize_github_linked_vault(
            &local_path,
            "Black Meridian",
            "git@github.com:black-meridian/codex-vault.git",
            "main",
        )
        .unwrap();

        assert_eq!(vault.config.vault_name, "Black Meridian");
        assert_eq!(
            vault.config.sync,
            SyncConfig::Git {
                remote: "git@github.com:black-meridian/codex-vault.git".into(),
                branch: "main".into(),
                auto_commit_seconds: 60,
            }
        );
    }

    #[test]
    fn initializes_github_pages_publication_seed_document() {
        let tmp = TempDir::new().unwrap();
        let local_path = tmp.path().join("vault");
        let vault = OmegonRuntimeContext::initialize_github_pages_publication(
            &local_path,
            "Black Meridian",
            "https://github.com/black-meridian/codex-site.git",
            "gh-pages",
        )
        .unwrap();

        let home = vault.store.get_document_by_path(std::path::Path::new("home.md")).unwrap().unwrap();
        assert!(home.frontmatter.publication.enabled);
        assert_eq!(home.frontmatter.publication.visibility, codex_core::models::PublicationVisibility::Public);
        assert_eq!(
            home.frontmatter.publication.target,
            Some(PublicationTarget {
                repo: "https://github.com/black-meridian/codex-site.git".into(),
                branch: "gh-pages".into(),
                site_dir: "site".into(),
            })
        );
        assert_eq!(publication_output_path(&vault), local_path.join("site"));
    }

    #[test]
    fn seeds_demo_publication_repo_files() {
        let tmp = TempDir::new().unwrap();
        let repo_root = tmp.path().join("codex-site");
        OmegonRuntimeContext::seed_demo_publication_repo(&repo_root).unwrap();

        assert!(repo_root.join("package.json").exists());
        assert!(repo_root.join("astro.config.mjs").exists());
        assert!(repo_root.join("src/pages/index.astro").exists());
        assert!(repo_root.join("README.md").exists());
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
pub struct RuntimeState {
    pub vault_root: PathBuf,
    pub vault: Arc<Vault>,
    pub vault_events: broadcast::Sender<VaultChangeEvent>,
    pub omegon: OmegonRuntimeContext,
}

#[derive(Clone)]
pub struct AppContext {
    pub runtime: Signal<RuntimeState>,
}

impl AppContext {
    pub fn vault_root(&self) -> PathBuf {
        self.runtime.read().vault_root.clone()
    }

    pub fn vault(&self) -> Arc<Vault> {
        self.runtime.read().vault.clone()
    }

    pub fn vault_events(&self) -> broadcast::Sender<VaultChangeEvent> {
        self.runtime.read().vault_events.clone()
    }

    pub fn set_runtime(&mut self, runtime: RuntimeState) {
        *self.runtime.write() = runtime;
    }
}

/// Build AppContext at launch. Reads persisted launcher profile first, then CODEX_VAULT,
/// then falls back to ~/Documents/Codex.
fn publication_output_path(vault: &Vault) -> PathBuf {
    let target = vault
        .store
        .list_documents()
        .ok()
        .and_then(|docs| {
            docs.into_iter().find_map(|meta| {
                vault.store.get_document(&meta.id).ok().flatten().and_then(|doc| {
                    doc.frontmatter.publication.target.map(|target| target.site_dir)
                })
            })
        })
        .unwrap_or_else(|| "site".into());

    vault.root.join(target)
}

pub fn bootstrap_from_env() -> AppContext {
    let launcher_profile = OmegonRuntimeContext::load_launcher_profile();
    let vault_root = launcher_profile
        .last_vault_root
        .clone()
        .or_else(|| std::env::var("CODEX_VAULT").map(PathBuf::from).ok())
        .unwrap_or_else(|| {
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
    let runtime = Signal::new(RuntimeState {
        vault_root,
        vault,
        vault_events: tx,
        omegon,
    });

    AppContext { runtime }
}
