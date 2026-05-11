//! Sync status pill — shows the task's current upstream-sync state
//! at the right end of the metadata strip.
//!
//! Reads state from the PushPipeline held in AppContext. Re-renders
//! when the pipeline broadcasts a status change for our task id.
//!
//! ## v1 scope
//!
//! - Renders the current state (LocalOnly / Synced / PendingPush /
//!   Pushing / PushFailed / Conflict)
//! - Click on a Synced or Conflict pill opens the upstream issue
//!   URL in the system browser (the cheap Zed-handoff path; deep
//!   conflict resolution is a v2 popover)
//! - PushFailed pill shows the error in its title attribute on hover
//!
//! ## What's not here yet
//!
//! - Pull-theirs / Force-push popover (v2 — needs the resolve actions
//!   wired through PushPipeline)
//! - Auto-create toggle for engagements without it (v2 — depends on
//!   an engagement_update tool that doesn't exist yet)
//! - "Open in Zed" deep-link (v2 — once we know which file to point at)

use crate::bootstrap::AppContext;
use dioxus::prelude::*;
use flynt_forge::push::SyncStatus;
use uuid::Uuid;

#[component]
pub fn SyncStatusPill(task_id: Uuid) -> Element {
    let ctx = use_context::<AppContext>();
    let pipeline = ctx.push_pipeline();

    // Local refresh counter — bumped when the pipeline broadcasts a
    // status update for our task. Reading `bump()` makes the
    // surrounding render reactively re-run; we re-read the actual
    // status from the pipeline on every render.
    let mut bump = use_signal(|| 0u64);

    // Subscribe to pipeline events once. The receiver lives as long
    // as the spawned task; component drop will end the task because
    // spawned tasks are scope-bound to Dioxus components.
    use_effect(move || {
        let Some(pipeline) = ctx.push_pipeline() else { return };
        let mut rx = pipeline.subscribe();
        spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(update) => {
                        if update.task_id == task_id {
                            *bump.write() += 1;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // Caught up after a burst — bump anyway so we
                        // re-read the latest from the map.
                        *bump.write() += 1;
                    }
                    Err(_) => break, // channel closed, pipeline gone
                }
            }
        });
    });

    // Read the current status. `_dep` keeps the reactive link to bump.
    let _dep = *bump.read();
    let status = match pipeline.as_ref() {
        Some(p) => p.status_for(task_id),
        None => SyncStatus::LocalOnly,
    };

    let (class, label, title, click_url) = render_status(&status);

    rsx! {
        if let Some(url) = click_url {
            // Synced + Conflict states get a clickable pill that opens
            // the upstream URL in the system browser.
            //
            // Why not `<a target="_blank">`: wry webviews sandbox link
            // navigation; target="_blank" silently does nothing. The
            // `open` crate routes through the OS shell (open / xdg-open
            // / etc.) which actually launches a browser.
            {
                let url_for_click = url.clone();
                rsx! {
                    button {
                        class: "{class}",
                        title: "{title}",
                        onclick: move |_| {
                            if let Err(e) = open::that(&url_for_click) {
                                tracing::warn!(error = %e, url = %url_for_click, "failed to open issue URL");
                            }
                        },
                        "{label}"
                    }
                }
            }
        } else {
            span {
                class: "{class}",
                title: "{title}",
                "{label}"
            }
        }
    }
}

/// Translate SyncStatus into (css_class, pill_label, tooltip_text, optional_url).
fn render_status(status: &SyncStatus) -> (String, String, String, Option<String>) {
    match status {
        SyncStatus::LocalOnly => (
            "pill pill-sync pill-sync-local".into(),
            "Local only".into(),
            "Task isn't linked to an upstream issue. Set an engagement with auto_create_issues enabled to mirror.".into(),
            None,
        ),
        SyncStatus::Synced { issue_number, url } => (
            "pill pill-sync pill-sync-synced".into(),
            format!("✓ #{}", issue_number),
            "In sync with upstream issue. Click to open.".into(),
            url.clone(),
        ),
        SyncStatus::PendingPush { issue_number } => {
            let label = match issue_number {
                Some(n) => format!("Push pending… (#{})", n),
                None => "Push pending…".into(),
            };
            (
                "pill pill-sync pill-sync-pending".into(),
                label,
                "Edit landed; debouncing before push.".into(),
                None,
            )
        }
        SyncStatus::Pushing => (
            "pill pill-sync pill-sync-pushing".into(),
            "Pushing…".into(),
            "Update in flight to upstream.".into(),
            None,
        ),
        SyncStatus::PushFailed { issue_number, error } => {
            let label = match issue_number {
                Some(n) => format!("Push failed (#{})", n),
                None => "Push failed".into(),
            };
            (
                "pill pill-sync pill-sync-failed".into(),
                label,
                format!("Push failed: {error}. Will retry on next edit."),
                None,
            )
        }
        SyncStatus::Conflict { issue_number, url } => (
            "pill pill-sync pill-sync-conflict".into(),
            format!("Conflict (#{})", issue_number),
            "Upstream changed since last sync. Click to open the issue; resolve via your git tools (Zed, gh, etc.) and re-edit to retry.".into(),
            url.clone(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_only_has_no_url() {
        let (_class, label, _title, url) = render_status(&SyncStatus::LocalOnly);
        assert_eq!(label, "Local only");
        assert!(url.is_none());
    }

    #[test]
    fn synced_returns_url_for_click_through() {
        let (_class, label, _title, url) = render_status(&SyncStatus::Synced {
            issue_number: 42,
            url: Some("https://example.com/issues/42".into()),
        });
        assert_eq!(label, "✓ #42");
        assert_eq!(url.as_deref(), Some("https://example.com/issues/42"));
    }

    #[test]
    fn synced_without_url_still_renders() {
        // Pipeline can produce Synced { url: None } when an upstream
        // doesn't return a URL (unlikely but possible). Render the
        // pill; just no click-through.
        let (_class, label, _title, url) = render_status(&SyncStatus::Synced {
            issue_number: 7,
            url: None,
        });
        assert_eq!(label, "✓ #7");
        assert!(url.is_none());
    }

    #[test]
    fn push_failed_includes_error_in_tooltip() {
        let (_class, _label, title, _url) = render_status(&SyncStatus::PushFailed {
            issue_number: Some(99),
            error: "401 unauthorized".into(),
        });
        assert!(title.contains("401 unauthorized"), "{title}");
    }

    #[test]
    fn conflict_offers_upstream_link() {
        let (_class, label, _title, url) = render_status(&SyncStatus::Conflict {
            issue_number: 42,
            url: Some("https://example.com/issues/42".into()),
        });
        assert!(label.contains("Conflict"));
        assert!(url.is_some(), "conflict pill is clickable");
    }
}
