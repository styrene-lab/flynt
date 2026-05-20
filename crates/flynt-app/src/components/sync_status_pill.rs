//! Sync status pill — shows the task's current upstream-sync state
//! at the right end of the metadata strip.
//!
//! Reads state from the PushPipeline held in AppContext. Re-renders
//! when the pipeline broadcasts a status change for our task id.
//!
//! ## Behavior
//!
//! - Renders the current state (LocalOnly / Synced / PendingPush /
//!   Pushing / PushFailed / Conflict)
//! - Click on a Synced pill opens the upstream issue URL in the
//!   system browser (the cheap Zed-handoff path)
//! - Click on a Conflict pill opens a popover with three actions:
//!   Pull Theirs, Force Push, Open in Browser — see
//!   [`PushPipeline::resolve_pull_theirs`] and
//!   [`PushPipeline::resolve_force_push`] for the wiring.
//! - PushFailed pill shows the error in its title attribute on hover
//!
//! ## What's not here yet
//!
//! - "Open in Zed" deep-link (would need the engagement to surface a
//!   filesystem path for the issue's mirror file)

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
        let Some(pipeline) = ctx.push_pipeline() else {
            return;
        };
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
    let is_conflict = matches!(status, SyncStatus::Conflict { .. });

    // Popover open state. Only meaningful for Conflict; other states
    // ignore it.
    let popover_open = use_signal(|| false);

    rsx! {
        if is_conflict {
            ConflictPill {
                task_id,
                class: class.clone(),
                label: label.clone(),
                title: title.clone(),
                url: click_url.clone(),
                popover_open,
            }
        } else if let Some(url) = click_url {
            // Synced state gets a click-through to upstream.
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

/// The Conflict variant — pill plus a popover with resolution actions.
///
/// Split out from the main component so the popover state and the
/// async resolve calls live in their own scope. Three actions:
///
/// - **Pull Theirs**: overwrite local with upstream. Useful when the
///   local change was experimental and upstream has the canonical edit.
/// - **Force Push**: realign last_hash to current upstream, then push
///   local on top. Useful when local IS the canonical edit and
///   upstream is the divergence.
/// - **Open in Browser**: the fallback Zed-handoff path. Operator
///   resolves manually using whatever git tooling they prefer, then
///   re-edits locally to trigger a fresh push.
#[component]
fn ConflictPill(
    task_id: Uuid,
    class: String,
    label: String,
    title: String,
    url: Option<String>,
    popover_open: Signal<bool>,
) -> Element {
    let ctx = use_context::<AppContext>();
    let mut popover_open = popover_open;

    // Per-action busy flag — disables the buttons while a resolve is
    // in flight so the operator can't fire two pulls in parallel.
    let mut busy = use_signal(|| false);
    let mut error_msg = use_signal::<Option<String>>(|| None);

    let url_for_browser = url.clone();

    rsx! {
        div { class: "sync-conflict-wrapper",
            button {
                class: "{class}",
                title: "{title}",
                onclick: move |_| {
                    let next = !*popover_open.read();
                    popover_open.set(next);
                },
                "{label}"
            }
            if *popover_open.read() {
                div { class: "sync-conflict-popover",
                    div { class: "sync-conflict-popover-title", "Resolve upstream conflict" }
                    div { class: "sync-conflict-popover-body",
                        "Upstream changed since the last sync. Pick how to reconcile."
                    }
                    if let Some(msg) = error_msg.read().as_ref() {
                        div { class: "sync-conflict-popover-error", "{msg}" }
                    }
                    div { class: "sync-conflict-popover-actions",
                        button {
                            class: "btn btn-secondary",
                            disabled: *busy.read(),
                            title: "Discard local diff, take upstream as truth.",
                            onclick: move |_| {
                                let pipeline = ctx.push_pipeline();
                                busy.set(true);
                                error_msg.set(None);
                                spawn(async move {
                                    let Some(p) = pipeline else {
                                        error_msg.set(Some("no push pipeline".into()));
                                        busy.set(false);
                                        return;
                                    };
                                    match p.resolve_pull_theirs(task_id).await {
                                        Ok(()) => {
                                            popover_open.set(false);
                                        }
                                        Err(e) => {
                                            tracing::warn!(error = %e, task = %task_id, "pull-theirs failed");
                                            error_msg.set(Some(format!("Pull failed: {e}")));
                                        }
                                    }
                                    busy.set(false);
                                });
                            },
                            "Pull Theirs"
                        }
                        button {
                            class: "btn btn-secondary",
                            disabled: *busy.read(),
                            title: "Realign sync state and re-push your local version.",
                            onclick: move |_| {
                                let pipeline = ctx.push_pipeline();
                                busy.set(true);
                                error_msg.set(None);
                                spawn(async move {
                                    let Some(p) = pipeline else {
                                        error_msg.set(Some("no push pipeline".into()));
                                        busy.set(false);
                                        return;
                                    };
                                    match p.resolve_force_push(task_id).await {
                                        Ok(()) => {
                                            popover_open.set(false);
                                        }
                                        Err(e) => {
                                            tracing::warn!(error = %e, task = %task_id, "force-push failed");
                                            error_msg.set(Some(format!("Force push failed: {e}")));
                                        }
                                    }
                                    busy.set(false);
                                });
                            },
                            "Force Push"
                        }
                        if let Some(u) = url_for_browser.clone() {
                            button {
                                class: "btn btn-tertiary",
                                disabled: *busy.read(),
                                onclick: move |_| {
                                    if let Err(e) = open::that(&u) {
                                        tracing::warn!(error = %e, url = %u, "failed to open issue URL");
                                    }
                                },
                                "Open in Browser"
                            }
                        }
                    }
                }
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
