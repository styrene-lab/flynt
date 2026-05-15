use crate::{
    acp::AcpSession,
    bootstrap::{AppContext, runtime_state_for_project_root},
    state::{SettingsOpen, SettingsPage},
};
use dioxus::prelude::*;
use rfd::FileDialog;
use std::{
    path::{Path, PathBuf},
    rc::Rc,
};
use tokio::process::Command;

pub const OMEGON_INSTALL_DOCS_URL: &str = "https://omegon.styrene.io/docs/install";
pub const OMEGON_INSTALL_SCRIPT_URL: &str = "https://omegon.styrene.io/install.sh";

#[derive(Clone, Copy)]
pub struct OmegonSetupRefresh(pub Signal<u64>);

impl OmegonSetupRefresh {
    pub fn bump(&mut self) {
        *self.0.write() += 1;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OmegonReadiness {
    pub binary_path: PathBuf,
    pub binary_exists: bool,
    pub flynt_extension_path: PathBuf,
    pub flynt_extension_installed: bool,
    pub homebrew_path: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
enum ProbeState {
    Unknown(String),
    Ok(String),
    Missing(String),
}

impl ProbeState {
    fn css_class(&self) -> &'static str {
        match self {
            Self::Ok(_) => "omegon-setup-check ok",
            Self::Missing(_) => "omegon-setup-check missing",
            Self::Unknown(_) => "omegon-setup-check pending",
        }
    }

    fn detail(&self) -> &str {
        match self {
            Self::Ok(msg) | Self::Missing(msg) | Self::Unknown(msg) => msg,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct AcpReadiness {
    flynt_extension: ProbeState,
    provider_auth: ProbeState,
}

pub fn evaluate(ctx: &AppContext) -> OmegonReadiness {
    let omegon = ctx.omegon();
    let binary_path = omegon.resolve_binary();
    let flynt_extension_path = omegon.extensions_dir.join("flynt");
    OmegonReadiness {
        binary_exists: binary_path.exists(),
        binary_path,
        flynt_extension_installed: flynt_extension_path.exists(),
        flynt_extension_path,
        homebrew_path: find_executable("brew"),
    }
}

pub fn save_binary_override(ctx: &AppContext, path: &Path) -> anyhow::Result<()> {
    let project = ctx.project();
    let mut config = project.config.clone();
    config.local_runtime.omegon_bin_override = Some(path.to_string_lossy().into_owned());
    project.save_config(&config)?;
    Ok(())
}

pub fn reload_project_runtime(ctx: &mut AppContext) {
    let root = ctx.project_root();
    ctx.set_runtime(runtime_state_for_project_root(root));
}

fn find_executable(name: &str) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(path_var) = std::env::var("PATH") {
        candidates.extend(std::env::split_paths(&path_var).map(|dir| dir.join(name)));
    }
    candidates.push(PathBuf::from(format!("/opt/homebrew/bin/{name}")));
    candidates.push(PathBuf::from(format!("/usr/local/bin/{name}")));
    candidates.into_iter().find(|path| path.exists())
}

async fn run_shell(label: &str, script: &str) -> anyhow::Result<String> {
    let output = Command::new("/bin/sh")
        .arg("-lc")
        .arg(script)
        .output()
        .await?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            Ok(format!("{label} completed."))
        } else {
            Ok(stdout)
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        anyhow::bail!("{label} failed: {detail}");
    }
}

pub async fn install_with_upstream_script() -> anyhow::Result<String> {
    run_shell(
        "Omegon installer",
        &format!(
            "mkdir -p \"$HOME/.local/bin\" && curl -fsSL {OMEGON_INSTALL_SCRIPT_URL} | INSTALL_DIR=\"$HOME/.local/bin\" sh -s -- --no-confirm"
        ),
    )
    .await
}

pub async fn install_with_homebrew() -> anyhow::Result<String> {
    let brew = find_executable("brew").unwrap_or_else(|| PathBuf::from("brew"));
    let brew = brew.to_string_lossy().replace('\'', "'\\''");
    run_shell(
        "Homebrew install",
        &format!("'{brew}' tap styrene-lab/tap && '{brew}' install omegon"),
    )
    .await
}

fn parse_provider_status(value: &serde_json::Value) -> ProbeState {
    let text = value["text"].as_str().unwrap_or("").trim();
    if text.is_empty() {
        return ProbeState::Unknown("Provider status returned no detail".into());
    }

    let lower = text.to_lowercase();
    if lower.contains(":expired:")
        || lower.contains(":missing:")
        || lower.contains("expired")
        || lower.contains("missing")
        || lower.contains("not authenticated")
        || lower.contains("unauthenticated")
    {
        ProbeState::Missing(
            "Provider auth needs attention. Use /login or Settings > Omegon > Providers.".into(),
        )
    } else if lower.contains(":authenticated:") || lower.contains("authenticated") {
        ProbeState::Ok("Provider auth is available".into())
    } else {
        ProbeState::Unknown(
            "Provider auth status is available; verify provider selection before first prompt."
                .into(),
        )
    }
}

fn parse_flynt_extension_status(value: &serde_json::Value, fallback_path: &Path) -> ProbeState {
    let extensions = crate::components::omegon::extension_config::parse_extensions_list(value);
    if let Some(ext) = extensions.iter().find(|ext| ext.name == "flynt") {
        if ext.enabled {
            ProbeState::Ok(format!("flynt v{} enabled", ext.version))
        } else {
            ProbeState::Missing(format!("flynt v{} installed but disabled", ext.version))
        }
    } else if fallback_path.exists() {
        ProbeState::Unknown("Found on disk; waiting for Omegon to report extension state".into())
    } else {
        ProbeState::Missing("Install from the Flynt release artifact".into())
    }
}

async fn probe_acp_readiness(sess: Rc<AcpSession>, flynt_extension_path: PathBuf) -> AcpReadiness {
    let flynt_extension = match sess.extensions_list().await {
        Ok(value) => parse_flynt_extension_status(&value, &flynt_extension_path),
        Err(err) => ProbeState::Unknown(format!("Could not query extensions: {err}")),
    };

    let provider_auth = match sess.provider_status().await {
        Ok(value) => parse_provider_status(&value),
        Err(err) => ProbeState::Unknown(format!("Could not query provider auth: {err}")),
    };

    AcpReadiness {
        flynt_extension,
        provider_auth,
    }
}

fn flynt_extension_release_uri() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let arch = std::env::consts::ARCH;
    let platform = if cfg!(target_os = "macos") {
        "universal-apple-darwin"
    } else if cfg!(target_os = "linux") {
        if arch == "aarch64" {
            "aarch64-unknown-linux-gnu"
        } else {
            "x86_64-unknown-linux-gnu"
        }
    } else {
        std::env::consts::OS
    };
    format!(
        "https://github.com/styrene-lab/flynt/releases/download/v{version}/flynt-agent-{version}-{platform}.tar.gz"
    )
}

#[derive(Clone, Debug, PartialEq)]
enum SetupAction {
    Running(String),
    Ok(String),
    Err(String),
}

#[component]
pub fn OmegonSetupPanel() -> Element {
    let ctx = use_context::<AppContext>();
    let mut refresh = use_context::<OmegonSetupRefresh>();
    let mut settings_page = use_context::<Signal<SettingsPage>>();
    let mut settings_open = use_context::<Signal<SettingsOpen>>();
    let shared_session = use_context::<Signal<Option<Rc<AcpSession>>>>();
    let mut action = use_signal(|| None::<SetupAction>);

    let _ = refresh.0.read();
    let readiness = evaluate(&ctx);
    let session_ready = shared_session.read().is_some();
    let install_running = matches!(&*action.read(), Some(SetupAction::Running(_)));
    let extension_uri = flynt_extension_release_uri();
    let probe_flynt_extension_path = readiness.flynt_extension_path.clone();
    let acp_readiness = use_resource(move || {
        let _ = refresh.0.read();
        let sess = shared_session.read().clone();
        let flynt_extension_path = probe_flynt_extension_path.clone();
        async move {
            match sess {
                Some(sess) => Some(probe_acp_readiness(sess, flynt_extension_path).await),
                None => None,
            }
        }
    });
    let acp_state = acp_readiness
        .read()
        .as_ref()
        .and_then(|state| state.clone());
    let flynt_extension_state = acp_state
        .as_ref()
        .map(|state| state.flynt_extension.clone())
        .unwrap_or_else(|| {
            if readiness.flynt_extension_installed {
                ProbeState::Unknown(format!("{}", readiness.flynt_extension_path.display()))
            } else if session_ready {
                ProbeState::Missing("Install from the Flynt release artifact".into())
            } else {
                ProbeState::Unknown("Requires a running Omegon session".into())
            }
        });
    let provider_auth_state = acp_state
        .as_ref()
        .map(|state| state.provider_auth.clone())
        .unwrap_or_else(|| {
            if session_ready {
                ProbeState::Unknown("Checking provider auth...".into())
            } else {
                ProbeState::Unknown("Requires a running Omegon session".into())
            }
        });
    let flynt_extension_ready = matches!(flynt_extension_state, ProbeState::Ok(_));
    let flynt_extension_disabled =
        matches!(&flynt_extension_state, ProbeState::Missing(msg) if msg.contains("disabled"));
    let provider_auth_needs_attention = matches!(provider_auth_state, ProbeState::Missing(_));
    if readiness.binary_exists
        && session_ready
        && flynt_extension_ready
        && matches!(provider_auth_state, ProbeState::Ok(_))
    {
        return rsx! {};
    }

    rsx! {
        div { class: "omegon-setup-panel",
            div { class: "omegon-setup-head",
                div {
                    div { class: "omegon-setup-title", "Omegon setup" }
                    div { class: "omegon-setup-subtitle",
                        "Flynt needs a local Omegon runtime before the agent panel can work."
                    }
                }
                button {
                    class: "btn btn-ghost btn-xs",
                    title: "Run readiness checks again",
                    onclick: move |_| refresh.bump(),
                    "Recheck"
                }
            }

            div { class: "omegon-setup-checks",
                div { class: if readiness.binary_exists { "omegon-setup-check ok" } else { "omegon-setup-check missing" },
                    span { class: "omegon-setup-dot" }
                    div {
                        div { class: "omegon-setup-check-title", "Omegon binary" }
                        div { class: "omegon-setup-check-detail", "{readiness.binary_path.display()}" }
                    }
                }
                div { class: if session_ready { "omegon-setup-check ok" } else { "omegon-setup-check pending" },
                    span { class: "omegon-setup-dot" }
                    div {
                        div { class: "omegon-setup-check-title", "ACP session" }
                        div { class: "omegon-setup-check-detail",
                            if session_ready { "Connected" } else if readiness.binary_exists { "Ready to start" } else { "Waiting for Omegon" }
                        }
                    }
                }
                div { class: "{flynt_extension_state.css_class()}",
                    span { class: "omegon-setup-dot" }
                    div {
                        div { class: "omegon-setup-check-title", "Flynt extension" }
                        div { class: "omegon-setup-check-detail", "{flynt_extension_state.detail()}" }
                    }
                }
                div { class: "{provider_auth_state.css_class()}",
                    span { class: "omegon-setup-dot" }
                    div {
                        div { class: "omegon-setup-check-title", "Provider auth" }
                        div { class: "omegon-setup-check-detail", "{provider_auth_state.detail()}" }
                    }
                }
            }

            if let Some(state) = action.read().as_ref() {
                {
                    let message = match state {
                        SetupAction::Running(msg) | SetupAction::Ok(msg) | SetupAction::Err(msg) => msg.clone(),
                    };
                    rsx! {
                        div {
                            class: match state {
                                SetupAction::Running(_) => "omegon-setup-status running",
                                SetupAction::Ok(_) => "omegon-setup-status ok",
                                SetupAction::Err(_) => "omegon-setup-status err",
                            },
                            "{message}"
                        }
                    }
                }
            }

            div { class: "omegon-setup-actions",
                if !readiness.binary_exists {
                    button {
                        class: "btn btn-primary",
                        disabled: install_running,
                        onclick: move |_| {
                            *action.write() = Some(SetupAction::Running("Installing Omegon from the upstream installer...".into()));
                            spawn(async move {
                                match install_with_upstream_script().await {
                                    Ok(_) => {
                                        action.set(Some(SetupAction::Ok("Omegon installer completed. Rechecking runtime...".into())));
                                    }
                                    Err(err) => {
                                        action.set(Some(SetupAction::Err(format!("{err}"))));
                                    }
                                }
                                refresh.bump();
                            });
                        },
                        if install_running { "Installing..." } else { "Install Omegon" }
                    }
                    if readiness.homebrew_path.is_some() {
                        button {
                            class: "btn btn-ghost",
                            disabled: install_running,
                            onclick: move |_| {
                                *action.write() = Some(SetupAction::Running("Installing Omegon with Homebrew...".into()));
                                spawn(async move {
                                    match install_with_homebrew().await {
                                        Ok(_) => {
                                            action.set(Some(SetupAction::Ok("Homebrew installed Omegon. Rechecking runtime...".into())));
                                        }
                                        Err(err) => {
                                            action.set(Some(SetupAction::Err(format!("{err}"))));
                                        }
                                    }
                                    refresh.bump();
                                });
                            },
                            "Use Homebrew"
                        }
                    }
                }
                if readiness.binary_exists && !session_ready {
                    button {
                        class: "btn btn-primary",
                        disabled: install_running,
                        onclick: move |_| {
                            *action.write() = Some(SetupAction::Running("Starting Omegon session...".into()));
                            refresh.bump();
                            *action.write() = Some(SetupAction::Ok("Session start requested. Rechecking...".into()));
                        },
                        "Start Session"
                    }
                }
                if session_ready && !flynt_extension_ready {
                    button {
                        class: "btn btn-primary",
                        disabled: install_running,
                        title: "{extension_uri}",
                        onclick: move |_| {
                            let sess = shared_session.read().clone();
                            let uri = extension_uri.clone();
                            let enable_existing = flynt_extension_disabled;
                            *action.write() = Some(SetupAction::Running(
                                if enable_existing {
                                    "Enabling the Flynt extension in Omegon..."
                                } else {
                                    "Installing the Flynt extension into Omegon..."
                                }.into()
                            ));
                            spawn(async move {
                                let Some(sess) = sess else {
                                    action.set(Some(SetupAction::Err("No Omegon session is running.".into())));
                                    return;
                                };
                                let result = if enable_existing {
                                    sess.extensions_enable("flynt").await
                                } else {
                                    sess.extensions_install(&uri).await
                                };
                                match result {
                                    Ok(_) => action.set(Some(SetupAction::Ok(
                                        if enable_existing {
                                            "Flynt extension enabled. Rechecking runtime..."
                                        } else {
                                            "Flynt extension installed. Rechecking runtime..."
                                        }.into()
                                    ))),
                                    Err(err) => action.set(Some(SetupAction::Err(format!("Flynt extension setup failed: {err}")))),
                                }
                                refresh.bump();
                            });
                        },
                        if flynt_extension_disabled { "Enable Flynt extension" } else { "Install Flynt extension" }
                    }
                }
                if provider_auth_needs_attention {
                    button {
                        class: "btn btn-primary",
                        onclick: move |_| {
                            *settings_page.write() = SettingsPage::OmegonProviders;
                            *settings_open.write() = SettingsOpen(true);
                        },
                        "Provider Settings"
                    }
                }
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| {
                        if let Some(path) = FileDialog::new()
                            .set_title("Choose the omegon executable")
                            .pick_file()
                        {
                            if !path.exists() {
                                *action.write() = Some(SetupAction::Err("Selected file does not exist.".into()));
                                return;
                            }
                            match save_binary_override(&ctx, &path) {
                                Ok(()) => {
                                    let mut ctx_reload = ctx;
                                    reload_project_runtime(&mut ctx_reload);
                                    *action.write() = Some(SetupAction::Ok(format!("Using {}.", path.display())));
                                    refresh.bump();
                                }
                                Err(err) => {
                                    *action.write() = Some(SetupAction::Err(format!("Could not save binary path: {err}")));
                                }
                            }
                        }
                    },
                    "Choose Binary"
                }
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| {
                        *settings_page.write() = SettingsPage::OmegonRuntime;
                        *settings_open.write() = SettingsOpen(true);
                    },
                    "Runtime Settings"
                }
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| {
                        let _ = open::that(OMEGON_INSTALL_DOCS_URL);
                    },
                    "Install Docs"
                }
            }
        }
    }
}
