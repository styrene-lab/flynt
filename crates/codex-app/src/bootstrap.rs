use dioxus::prelude::{ReadableExt, Signal};
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
    #[serde(default)]
    pub known_vaults: Vec<KnownVault>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_setup: Option<PendingVaultSetup>,
    /// Path to a cloned vault manifest repo. If set, known_vaults are
    /// supplemented from the manifest's `vaults.toml`.
    #[serde(default)]
    pub manifest_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownVault {
    pub name: String,
    pub root: PathBuf,
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
    /// Omegon release channel for binary resolution.
    pub omegon_channel: codex_core::models::OmegonChannel,
    /// Explicit binary path override.
    pub omegon_bin_override: Option<String>,
}

impl OmegonRuntimeContext {
    fn launcher_profile_path() -> PathBuf {
        std::env::var("CODEX_LAUNCHER_PROFILE")
            .map(PathBuf::from)
            .ok()
            .filter(|path| path.is_absolute())
            .or_else(|| dirs::config_local_dir().map(|dir| {
                // Prefer codyx/ for new installs, fall back to codex/ for backwards compat
                let new_path = dir.join("codyx/launcher-profile.json");
                let old_path = dir.join("codex/launcher-profile.json");
                if new_path.exists() || !old_path.exists() { new_path } else { old_path }
            }))
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".codyx-launcher-profile.json")
            })
    }

    pub fn load_launcher_profile() -> LauncherProfile {
        let path = Self::launcher_profile_path();
        let mut profile: LauncherProfile = std::fs::read_to_string(path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default();
        // Prune vaults whose root no longer exists on disk
        profile.known_vaults.retain(|v| v.root.exists());
        profile.recent_vaults.retain(|v| v.exists());
        // Merge any vaults from the manifest
        Self::sync_from_manifest(&mut profile);
        profile
    }

    pub fn save_launcher_profile(profile: &LauncherProfile) -> anyhow::Result<()> {
        let path = Self::launcher_profile_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(profile)?)?;
        Ok(())
    }

    /// Merge vaults from the manifest into known_vaults.
    /// Only adds vaults that have a local_path set (cloned on this device).
    pub fn sync_from_manifest(profile: &mut LauncherProfile) {
        let Some(ref manifest_dir) = profile.manifest_dir else { return };
        let Ok(manifest) = codex_core::manifest::load_manifest_with_local(manifest_dir) else { return };

        for vault in &manifest.vaults {
            let Some(ref local_path) = vault.local_path else { continue };
            if !local_path.exists() { continue; }
            // Add if not already known
            if !profile.known_vaults.iter().any(|kv| kv.root == *local_path) {
                profile.known_vaults.push(KnownVault {
                    name: vault.name.clone(),
                    root: local_path.clone(),
                });
            }
        }
        profile.known_vaults.sort_by(|a, b| a.name.cmp(&b.name));
    }

    pub fn register_known_vault(profile: &mut LauncherProfile, root: &Path, name: &str) {
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());

        // Prune vaults whose root no longer exists on disk
        profile.known_vaults.retain(|v| v.root.exists());
        profile.recent_vaults.retain(|v| v.exists());

        if let Some(existing) = profile.known_vaults.iter_mut().find(|vault| vault.root == root) {
            existing.name = name.to_string();
        } else {
            profile.known_vaults.push(KnownVault {
                name: name.to_string(),
                root: root.clone(),
            });
            profile.known_vaults.sort_by(|left, right| left.name.cmp(&right.name));
        }
        if !profile.recent_vaults.contains(&root) {
            profile.recent_vaults.push(root.clone());
        }
        profile.last_vault_root = Some(root);
    }

    /// Add a vault to the manifest and clone it locally.
    pub fn add_vault_to_manifest(
        profile: &mut LauncherProfile,
        name: &str,
        repo: &str,
        branch: &str,
        token: Option<&str>,
    ) -> anyhow::Result<PathBuf> {
        use codex_core::manifest::{self, ManifestVault, VaultRole};

        let manifest_dir = profile.manifest_dir.clone()
            .ok_or_else(|| anyhow::anyhow!("No manifest configured. Connect a manifest first."))?;

        // Add to manifest
        let mut m = manifest::load_manifest_with_local(&manifest_dir)?;
        let local_path = dirs::document_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(name);

        let vault = ManifestVault {
            name: name.into(),
            repo: repo.into(),
            branch: branch.into(),
            role: VaultRole::Owner,
            hub: None,
            local_path: Some(local_path.clone()),
            auto_commit_seconds: 60,
        };
        manifest::add_vault(&mut m, vault)?;
        manifest::save_manifest(&manifest_dir, &m)?;
        manifest::save_local_manifest(&manifest_dir, &m)?;

        // Clone the repo
        if let Some(tk) = token {
            codex_store::sync::GitSync::clone_repo_with_token(repo, branch, &local_path, tk)?;
        } else {
            codex_store::sync::GitSync::clone_repo(repo, branch, &local_path)?;
        }

        // Register in launcher profile
        Self::register_known_vault(profile, &local_path, name);

        // Commit manifest changes
        let _ = Self::commit_manifest(&manifest_dir, &format!("Add vault: {name}"));

        Ok(local_path)
    }

    /// Remove a vault from the manifest. Optionally delete local files.
    pub fn remove_vault_from_manifest(
        profile: &mut LauncherProfile,
        vault_name: &str,
        delete_local: bool,
    ) -> anyhow::Result<()> {
        use codex_core::manifest;

        let manifest_dir = profile.manifest_dir.clone()
            .ok_or_else(|| anyhow::anyhow!("No manifest configured."))?;

        let mut m = manifest::load_manifest_with_local(&manifest_dir)?;

        // Find the local path before removal (for cleanup)
        let local_path = m.vaults.iter()
            .find(|v| v.name == vault_name)
            .and_then(|v| v.local_path.clone());

        manifest::remove_vault(&mut m, vault_name)?;
        manifest::save_manifest(&manifest_dir, &m)?;
        manifest::save_local_manifest(&manifest_dir, &m)?;

        // Remove from known vaults
        profile.known_vaults.retain(|v| v.name != vault_name);

        // Delete local clone if requested
        if delete_local {
            if let Some(ref path) = local_path {
                if path.exists() {
                    std::fs::remove_dir_all(path)?;
                }
            }
        }

        let _ = Self::commit_manifest(&manifest_dir, &format!("Remove vault: {vault_name}"));

        Ok(())
    }

    /// Auto-commit manifest changes so they sync to other devices.
    fn commit_manifest(manifest_dir: &Path, message: &str) -> anyhow::Result<()> {
        let git = codex_store::sync::git::GitSync::new(
            manifest_dir.to_path_buf(), "origin", "main",
        );
        let _ = git.auto_commit(message);
        let _ = codex_core::sync::SyncBackend::sync(&git);
        Ok(())
    }

    pub fn spawn_new_instance_for_vault(root: &Path) -> anyhow::Result<()> {
        let exe = std::env::current_exe()?;
        Command::new(exe)
            .arg("--vault")
            .arg(root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(())
    }

    pub fn initialize_vault(path: &Path, name: &str, sync: SyncConfig) -> anyhow::Result<Vault> {
        Self::initialize_vault_with_indexing(path, name, sync, Default::default())
    }

    pub fn initialize_vault_with_indexing(
        path: &Path,
        name: &str,
        sync: SyncConfig,
        indexing: codex_core::models::IndexingConfig,
    ) -> anyhow::Result<Vault> {
        std::fs::create_dir_all(path)?;
        let vault = Vault::open(path)?;
        let mut config: VaultConfig = vault.config.clone();
        config.vault_name = name.to_string();
        config.sync = sync;
        config.indexing = indexing;
        vault.save_config(&config)?;
        let vault = Vault::open(path)?;
        let mut profile = Self::load_launcher_profile();
        Self::register_known_vault(&mut profile, path, name);
        Self::save_launcher_profile(&profile)?;
        Ok(vault)
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

    /// Clone a remote git repo into `local_path` and open it as a Codex vault.
    pub fn clone_remote_vault(
        local_path: &Path,
        repo_url: &str,
        branch: &str,
    ) -> anyhow::Result<Vault> {
        use codex_store::sync::git::GitSync;

        std::fs::create_dir_all(local_path)?;

        let repo = GitSync::clone_repo(repo_url, branch, local_path)?;

        let name = local_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Codyx")
            .to_string();

        let remote_name = repo
            .remotes()?
            .iter()
            .flatten()
            .next()
            .unwrap_or("origin")
            .to_string();

        Self::initialize_vault(
            local_path,
            &name,
            SyncConfig::Git {
                remote: remote_name,
                branch: branch.to_string(),
                auto_commit_seconds: 60,
            },
        )
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
            "import { defineConfig } from 'astro/config';\n\nexport default defineConfig({\n  site: 'https://example-org.github.io/codex-site',\n});\n",
        )?;

        std::fs::write(
            repo_root.join("src/pages/index.astro"),
            "---\nconst title = 'Codex Publication Demo';\n---\n<html lang=\"en\">\n  <head>\n    <meta charset=\"utf-8\" />\n    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />\n    <title>{title}</title>\n    <style>body{font-family:system-ui,sans-serif;max-width:860px;margin:0 auto;padding:3rem;background:#06080e;color:#c4d8e4}a{color:#6ecad8}code{background:#0e1622;padding:.2rem .4rem;border-radius:4px}</style>\n  </head>\n  <body>\n    <h1>{title}</h1>\n    <p>This Astro site demonstrates what a published Codex vault can look like.</p>\n    <p>Copy local publication preview artifacts into <code>public/preview/</code> or evolve this into a richer adapter over the publication manifest.</p>\n    <ul>\n      <li><a href=\"/preview/home.html\">Preview exported home page</a></li>\n      <li><a href=\"https://github.com/example-org/codex\">Codex source</a></li>\n    </ul>\n  </body>\n</html>\n",
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
            omegon_channel: runtime.omegon_channel.clone(),
            omegon_bin_override: runtime.omegon_bin_override.clone(),
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

    /// Resolve the Omegon binary path using channel + override config.
    pub fn resolve_binary(&self) -> PathBuf {
        let cfg = codex_core::models::LocalRuntimeConfig {
            omegon_channel: self.omegon_channel.clone(),
            omegon_bin_override: self.omegon_bin_override.clone(),
            ..Default::default()
        };
        codex_core::models::resolve_omegon_binary(&cfg)
    }

    pub async fn spawn_background_host(&self, vault_root: &Path) -> anyhow::Result<tokio::process::Child> {
        let binary = self.resolve_binary();
        let child = Command::new(&binary)
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
    use super::{publication_output_path, KnownVault, LauncherProfile, OmegonRuntimeContext, PendingVaultSetup};
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
                styrene_identity_profile: Some("example-org".into()),
                omegon_serve_host: None,
                omegon_channel: Default::default(),
                omegon_bin_override: None,
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
            last_vault_root: Some(tmp.path().join("vaults/example-org")),
            wizard_completed: true,
            recent_vaults: vec![tmp.path().join("vaults/example-org")],
            known_vaults: vec![KnownVault {
                name: "Black Meridian".into(),
                root: tmp.path().join("vaults/example-org"),
            }],
            pending_setup: Some(PendingVaultSetup::LinkGithub {
                local_path: tmp.path().join("vaults/example-org"),
                repo: "git@github.com:example-org/codex-vault.git".into(),
                branch: "main".into(),
            }),
            manifest_dir: None,
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
            "git@github.com:example-org/codex-vault.git",
            "main",
        )
        .unwrap();

        assert_eq!(vault.config.vault_name, "Black Meridian");
        assert_eq!(
            vault.config.sync,
            SyncConfig::Git {
                remote: "git@github.com:example-org/codex-vault.git".into(),
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
            "https://github.com/example-org/codex-site.git",
            "gh-pages",
        )
        .unwrap();

        let home = vault.store.get_document_by_path(std::path::Path::new("home.md")).unwrap().unwrap();
        assert!(home.frontmatter.publication.enabled);
        assert_eq!(home.frontmatter.publication.visibility, codex_core::models::PublicationVisibility::Public);
        assert_eq!(
            home.frontmatter.publication.target,
            Some(PublicationTarget {
                repo: "https://github.com/example-org/codex-site.git".into(),
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
            omegon_channel: Default::default(),
            omegon_bin_override: None,
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
            omegon_channel: Default::default(),
            omegon_bin_override: None,
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
    /// Background git sync handle — kept alive as long as RuntimeState exists.
    pub _sync_handle: Option<Arc<codex_store::sync::AutoSyncHandle>>,
    /// Sync status receiver — toolbar polls this for live sync state.
    pub sync_status_rx: Option<tokio::sync::watch::Receiver<codex_store::sync::AutoSyncStatus>>,
    /// Agent daemon lifecycle manager.
    pub daemon: Arc<crate::daemon_manager::DaemonManager>,
}

#[derive(Clone, Copy)]
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

    pub fn omegon(&self) -> OmegonRuntimeContext {
        self.runtime.read().omegon.clone()
    }

    pub fn daemon(&self) -> Arc<crate::daemon_manager::DaemonManager> {
        self.runtime.read().daemon.clone()
    }

    pub fn set_runtime(&mut self, runtime: RuntimeState) {
        use dioxus::prelude::WritableExt;
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

pub(crate) fn runtime_state_for_vault_root(vault_root: PathBuf) -> RuntimeState {
    if let Err(e) = std::fs::create_dir_all(&vault_root) {
        // Fatal but non-panic: log clearly so the user can see what happened
        tracing::error!("Cannot create vault directory at {}: {e}", vault_root.display());
        tracing::error!("Check that the path exists and you have write permission.");
        panic!("Cannot create vault directory at {}: {e}", vault_root.display());
    }

    let vault = match Vault::open(&vault_root) {
        Ok(v) => Arc::new(v),
        Err(e) => {
            tracing::error!("Failed to open vault at {}: {e}", vault_root.display());
            tracing::error!("The vault directory may be corrupted. Try removing .codex/ and reopening.");
            panic!("Failed to open vault at {}: {e}", vault_root.display());
        }
    };

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
                    if let Some(ref p) = path {
                        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                        if ext == "md" {
                            if let Err(e) = vault_clone.index_file(p) {
                                warn!("Re-index failed for {}: {e}", p.display());
                            }
                        }
                        // .excalidraw files: schedule SVG export via webview
                        // (handled by the UI layer listening on vault_events)
                    }
                    let _ = tx_clone.send(evt);
                }
                Err(_) => break,
            }
        }
    });

    // Ensure default templates exist
    let _ = codex_core::templates::ensure_default_templates(&vault_root);

    // iCloud: download any placeholder files before indexing
    if matches!(vault.config.sync, codex_core::models::SyncConfig::ICloud) {
        if let Err(e) = codex_store::sync::icloud::ensure_downloaded(&vault_root) {
            warn!("iCloud download check failed: {e}");
        }
    }

    let omegon = OmegonRuntimeContext::discover(&vault_root, &vault.config.local_runtime);

    // Start background git sync if configured
    let (sync_handle, sync_status_rx) = match &vault.config.sync {
        codex_core::models::SyncConfig::Git { remote, branch, auto_commit_seconds } if *auto_commit_seconds > 0 => {
            let interval = std::time::Duration::from_secs(*auto_commit_seconds);
            let vault_for_reindex = Arc::clone(&vault);
            let reindex_cb: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                if let Err(e) = vault_for_reindex.reindex() {
                    warn!("Post-pull reindex failed: {e}");
                }
            });
            let (handle, status_rx) = codex_store::sync::start_auto_sync(
                vault_root.clone(),
                remote.clone(),
                branch.clone(),
                interval,
                Some(reindex_cb),
            );
            info!("Auto-sync started: every {}s to {remote}/{branch}", auto_commit_seconds);

            // Clone for the toolbar, original for logging
            let ui_rx = status_rx.clone();
            tokio::spawn(async move {
                let mut rx = status_rx;
                while rx.changed().await.is_ok() {
                    let status = rx.borrow().clone();
                    match &status {
                        codex_store::sync::AutoSyncStatus::Error(e) => warn!("sync: {e}"),
                        codex_store::sync::AutoSyncStatus::Conflict(c) => warn!("sync conflicts: {c:?}"),
                        _ => {}
                    }
                }
            });

            (Some(Arc::new(handle)), Some(ui_rx))
        }
        _ => (None, None),
    };
    // Initialize daemon manager from operator settings
    let operator_settings = omegon.load_operator_settings();
    let daemon = Arc::new(crate::daemon_manager::DaemonManager::new(
        &operator_settings.agent_daemon,
        vault_root.clone(),
        omegon.clone(),
    ));

    // Auto-start daemon if configured
    if operator_settings.agent_daemon.enabled && operator_settings.agent_daemon.auto_start {
        let d = daemon.clone();
        tokio::spawn(async move {
            if let Err(e) = d.start().await {
                warn!("Daemon auto-start failed: {e}");
            }
        });
    }

    RuntimeState {
        vault_root,
        vault,
        vault_events: tx,
        omegon,
        _sync_handle: sync_handle,
        sync_status_rx,
        daemon,
    }
}

pub fn bootstrap_from_env() -> RuntimeState {
    let launcher_profile = OmegonRuntimeContext::load_launcher_profile();
    let default_root = || {
        dirs::document_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Codyx")
    };

    // CODEX_VAULT env var takes priority (explicit override), then launcher
    // profile's last vault (only if parent dir is accessible), then default.
    let vault_root = std::env::var("CODEX_VAULT")
        .map(PathBuf::from)
        .ok()
        .or_else(|| {
            launcher_profile.last_vault_root.clone().filter(|p| {
                // Accept if vault dir exists, or its parent exists (so we can create it)
                p.exists() || p.parent().is_some_and(|parent| parent.exists())
            })
        })
        .unwrap_or_else(default_root);

    runtime_state_for_vault_root(vault_root)
}
