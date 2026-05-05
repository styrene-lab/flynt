use flynt_core::daemon::{AgentDaemonConfig, DaemonState};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::process::Child;
use tracing::info;

use crate::bootstrap::OmegonRuntimeContext;

/// Manages the lifecycle of the per-vault omegon daemon process.
pub struct DaemonManager {
    state: Arc<Mutex<DaemonState>>,
    child: Arc<Mutex<Option<Child>>>,
    vault_root: PathBuf,
    omegon_ctx: OmegonRuntimeContext,
    port: Arc<Mutex<u16>>,
}

impl DaemonManager {
    pub fn new(config: &AgentDaemonConfig, vault_root: PathBuf, omegon_ctx: OmegonRuntimeContext) -> Self {
        let initial_state = if config.enabled {
            DaemonState::Stopped
        } else {
            DaemonState::Disabled
        };
        Self {
            state: Arc::new(Mutex::new(initial_state)),
            child: Arc::new(Mutex::new(None)),
            vault_root,
            omegon_ctx,
            port: Arc::new(Mutex::new(config.port)),
        }
    }

    pub fn state(&self) -> DaemonState {
        self.state.lock().unwrap().clone()
    }

    pub fn set_port(&self, port: u16) {
        *self.port.lock().unwrap() = port;
    }

    pub fn set_enabled(&self, enabled: bool) {
        let mut state = self.state.lock().unwrap();
        if !enabled {
            *state = DaemonState::Disabled;
        } else if state.is_stopped() {
            *state = DaemonState::Stopped;
        }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        // Don't start if already running
        if self.state().is_running() {
            return Ok(());
        }

        {
            *self.state.lock().unwrap() = DaemonState::Starting;
        }

        let port = *self.port.lock().unwrap();
        info!("Starting daemon on port {port} for vault {:?}", self.vault_root);

        match self.omegon_ctx.spawn_background_host(&self.vault_root).await {
            Ok(child) => {
                let pid = child.id().unwrap_or(0);
                {
                    *self.child.lock().unwrap() = Some(child);
                }

                // Spawn health poll task
                let state = self.state.clone();
                let poll_port = port;
                tokio::spawn(async move {
                    health_poll(state, poll_port, pid).await;
                });

                Ok(())
            }
            Err(e) => {
                *self.state.lock().unwrap() = DaemonState::Unhealthy(e.to_string());
                Err(e)
            }
        }
    }

    pub async fn stop(&self) -> anyhow::Result<()> {
        let mut child_guard = self.child.lock().unwrap();
        if let Some(ref mut child) = *child_guard {
            info!("Stopping daemon for vault {:?}", self.vault_root);
            let _ = child.kill().await;
        }
        *child_guard = None;
        drop(child_guard);

        *self.state.lock().unwrap() = DaemonState::Stopped;
        Ok(())
    }

    pub async fn restart(&self) -> anyhow::Result<()> {
        self.stop().await?;
        // Brief pause to let the port release
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        self.start().await
    }
}

/// Background health polling — checks if the daemon port is accepting connections.
async fn health_poll(state: Arc<Mutex<DaemonState>>, port: u16, pid: u32) {
    // Give the daemon a moment to start
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let addr = format!("127.0.0.1:{port}");
    let ws_url = format!("ws://127.0.0.1:{port}");

    loop {
        // Check if we've been stopped externally
        {
            let s = state.lock().unwrap();
            if s.is_stopped() {
                return;
            }
        }

        // TCP connect probe — if the port is open, the daemon is alive
        match tokio::net::TcpStream::connect(&addr).await {
            Ok(_) => {
                let mut s = state.lock().unwrap();
                if !s.is_running() {
                    *s = DaemonState::Running {
                        pid,
                        port,
                        ws_url: ws_url.clone(),
                    };
                }
            }
            Err(e) => {
                let mut s = state.lock().unwrap();
                if s.is_running() {
                    *s = DaemonState::Unhealthy(format!("Connection refused: {e}"));
                }
                // If we were Starting and still can't connect after initial delay,
                // stay in Starting for a bit longer (daemon may still be booting)
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}
