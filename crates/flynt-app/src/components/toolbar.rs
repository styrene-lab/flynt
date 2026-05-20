use crate::{
    bootstrap::{AppContext, OmegonRuntimeContext},
    state::{Route, SyncActivityState, SyncRunOutcome, SyncStatus, TabState},
};
use dioxus::prelude::*;
use flynt_core::{models::SearchResult, store::ProjectStore};
use flynt_store::sync::{
    AutoSyncStatus,
    git::{GitSync, SyncDiagnostic},
};
use rfd::FileDialog;

#[derive(Clone)]
struct SearchGroup {
    folder: String,
    items: Vec<SearchResult>,
}

fn top_level_folder(path: &std::path::Path) -> String {
    let mut comps = path.components();
    let Some(first) = comps.next() else {
        return String::new();
    };
    if comps.next().is_some() {
        first.as_os_str().to_string_lossy().into_owned()
    } else {
        String::new()
    }
}

fn group_results(list: &[SearchResult]) -> Vec<SearchGroup> {
    let mut groups: Vec<SearchGroup> = Vec::new();

    for item in list.iter().cloned() {
        let folder = top_level_folder(&item.path);
        if let Some(group) = groups.iter_mut().find(|group| group.folder == folder) {
            group.items.push(item);
        } else {
            groups.push(SearchGroup {
                folder,
                items: vec![item],
            });
        }
    }

    for group in &mut groups {
        group.items.sort_by(|a, b| b.score.total_cmp(&a.score));
    }

    groups.sort_by(|a, b| {
        let a_score = a
            .items
            .first()
            .map(|item| item.score)
            .unwrap_or(f32::NEG_INFINITY);
        let b_score = b
            .items
            .first()
            .map(|item| item.score)
            .unwrap_or(f32::NEG_INFINITY);
        b_score
            .total_cmp(&a_score)
            .then_with(|| a.folder.cmp(&b.folder))
    });

    groups
}

fn flatten_grouped_results(groups: &[SearchGroup]) -> Vec<SearchResult> {
    groups
        .iter()
        .flat_map(|group| group.items.iter().cloned())
        .collect()
}

fn cycle_active_index(current: Option<usize>, len: usize, step: isize) -> Option<usize> {
    if len == 0 {
        return None;
    }

    match (current, step.is_negative()) {
        (None, false) => Some(0),
        (None, true) => Some(len - 1),
        (Some(index), false) => Some((index + 1) % len),
        (Some(index), true) => Some((index + len - 1) % len),
    }
}

fn autosync_status_label(status: &AutoSyncStatus) -> (&'static str, String) {
    match status {
        AutoSyncStatus::Idle => ("Idle", "Last run is idle".into()),
        AutoSyncStatus::Committing => ("Committing", "Staging and committing local changes".into()),
        AutoSyncStatus::Pulling => ("Pulling", "Pulling remote changes".into()),
        AutoSyncStatus::Pushing => ("Pushing", "Pushing local commits".into()),
        AutoSyncStatus::Conflict(files) => {
            ("Conflict", format!("{} conflict file(s)", files.len()))
        }
        AutoSyncStatus::Error(error) => ("Error", error.clone()),
    }
}

#[component]
fn SyncActivityPanel(
    diagnostic: Option<Result<SyncDiagnostic, String>>,
    auto_status: Option<AutoSyncStatus>,
    activity_state: SyncActivityState,
    action_message: Option<String>,
    on_close: EventHandler<()>,
    on_refresh: EventHandler<()>,
    on_sync_now: EventHandler<()>,
    on_open_conflict: EventHandler<String>,
) -> Element {
    let (status_label, status_detail) =
        auto_status.as_ref().map(autosync_status_label).unwrap_or((
            "Not running",
            "Auto-sync is not active for this project".into(),
        ));
    let conflict_files = match &auto_status {
        Some(AutoSyncStatus::Conflict(files)) => files.clone(),
        _ => Vec::new(),
    };
    let last_started = activity_state
        .last_started_at
        .map(|ts| ts.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "none".into());
    let last_finished = activity_state
        .last_finished_at
        .map(|ts| ts.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "none".into());
    let (outcome_class, outcome_label) = match &activity_state.last_outcome {
        Some(SyncRunOutcome::Success) => ("ok", "Success".to_string()),
        Some(SyncRunOutcome::Error(error)) => ("error", format!("Error: {error}")),
        Some(SyncRunOutcome::Conflict(files)) => {
            ("warning", format!("Conflict: {} file(s)", files.len()))
        }
        None => ("", "No completed run yet".to_string()),
    };

    rsx! {
        div { class: "sync-activity-popover",
            div { class: "sync-activity-header",
                div {
                    div { class: "sync-activity-title", "Sync Activity" }
                    div { class: "sync-activity-subtitle", "{status_label}: {status_detail}" }
                }
                button {
                    class: "note-inspector-close",
                    title: "Close sync activity",
                    onclick: move |_| on_close.call(()),
                    "\u{00D7}"
                }
            }
            div { class: "sync-activity-actions",
                button { class: "btn btn-primary btn-sm", onclick: move |_| on_sync_now.call(()), "Sync now" }
                button { class: "btn btn-ghost btn-sm", onclick: move |_| on_refresh.call(()), "Refresh" }
            }
            if let Some(message) = action_message {
                div { class: "sync-activity-message", "{message}" }
            }
            div { class: "sync-activity-section",
                div { class: "sync-activity-section-title", "Session run state" }
                div { class: "sync-activity-run-grid",
                    div {
                        span { class: "sync-activity-label", "Current" }
                        span { class: "sync-activity-value", "{activity_state.current_phase.clone().unwrap_or_else(|| status_label.into())}" }
                    }
                    div {
                        span { class: "sync-activity-label", "Last outcome" }
                        span { class: "sync-activity-value outcome {outcome_class}", "{outcome_label}" }
                    }
                    div {
                        span { class: "sync-activity-label", "Started" }
                        span { class: "sync-activity-value", "{last_started}" }
                    }
                    div {
                        span { class: "sync-activity-label", "Finished" }
                        span { class: "sync-activity-value", "{last_finished}" }
                    }
                    div {
                        span { class: "sync-activity-label", "Successful" }
                        span { class: "sync-activity-value", "{activity_state.successful_runs}" }
                    }
                    div {
                        span { class: "sync-activity-label", "Failed" }
                        span { class: "sync-activity-value", "{activity_state.failed_runs}" }
                    }
                }
            }
            match diagnostic {
                None => rsx! { div { class: "sync-activity-empty", "Loading sync diagnostics..." } },
                Some(Err(error)) => rsx! { div { class: "sync-activity-error", "{error}" } },
                Some(Ok(diag)) => rsx! {
                    div { class: "sync-activity-grid",
                        div { class: "sync-activity-cell",
                            span { class: "sync-activity-label", "Backend" }
                            span { class: "sync-activity-value", "{diag.backend}" }
                        }
                        div { class: "sync-activity-cell",
                            span { class: "sync-activity-label", "Remote" }
                            span { class: "sync-activity-value", "{diag.remote}/{diag.branch}" }
                        }
                        div { class: "sync-activity-cell",
                            span { class: "sync-activity-label", "Ahead" }
                            span { class: "sync-activity-value", "{diag.ahead.map(|n| n.to_string()).unwrap_or_else(|| \"unknown\".into())}" }
                        }
                        div { class: "sync-activity-cell",
                            span { class: "sync-activity-label", "Behind" }
                            span { class: "sync-activity-value", "{diag.behind.map(|n| n.to_string()).unwrap_or_else(|| \"unknown\".into())}" }
                        }
                    }
                    div { class: "sync-activity-section",
                        div { class: "sync-activity-section-title", "Head" }
                        div { class: "sync-activity-mono",
                            "{diag.head.as_ref().map(|head| head.chars().take(12).collect::<String>()).unwrap_or_else(|| \"none\".into())}"
                        }
                        if !diag.remote_ref_available {
                            div { class: "sync-activity-warning", "Remote tracking ref is not available locally yet." }
                        }
                    }
                    div { class: "sync-activity-section",
                        div { class: "sync-activity-section-title", "Dirty files" }
                        if diag.dirty_files.is_empty() {
                            div { class: "sync-activity-empty", "Working tree is clean" }
                        } else {
                            div { class: "sync-activity-list",
                                for file in diag.dirty_files {
                                    div { class: "sync-activity-file", "{file}" }
                                }
                            }
                        }
                    }
                },
            }
            if !conflict_files.is_empty() {
                div { class: "sync-activity-section",
                    div { class: "sync-activity-section-title", "Conflicts" }
                    div { class: "sync-activity-list",
                        for file in conflict_files {
                            {
                                let open_file = file.clone();
                                rsx! {
                                    button {
                                        class: "sync-activity-file button",
                                        onclick: move |_| on_open_conflict.call(open_file.clone()),
                                        "{file}"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn Toolbar(
    sync_status: Signal<SyncStatus>,
    mut show_agent: Signal<bool>,
    mut active_route: Signal<Route>,
    mut search_query: Signal<String>,
) -> Element {
    let ctx = use_context::<AppContext>();
    let mut tab_state = use_context::<Signal<TabState>>();
    let mut omegon_child = use_context::<Signal<Option<tokio::process::Child>>>();
    let mut omegon_pid = use_context::<Signal<Option<u32>>>();
    let mut omegon_launch_error = use_context::<Signal<Option<String>>>();
    let sync_activity = use_context::<Signal<SyncActivityState>>();
    let mut results: Signal<Vec<SearchResult>> = use_signal(Vec::new);
    let mut focused = use_signal(|| false);
    let mut active_index = use_signal(|| None::<usize>);
    let mut update_action = use_signal(|| None::<String>);
    let mut sync_panel_open = use_signal(|| false);
    let mut sync_refresh = use_signal(|| 0u64);
    let mut sync_action_message: Signal<Option<String>> = use_signal(|| None);
    let update_state =
        use_resource(|| async move { crate::self_update::check_latest_release().await });
    let sync_diag_ctx = ctx.clone();
    let sync_diagnostic = use_resource(move || {
        let open = *sync_panel_open.read();
        let _refresh = *sync_refresh.read();
        let project = sync_diag_ctx.project();
        async move {
            if !open {
                return None;
            }
            Some(
                tokio::task::spawn_blocking(move || match &project.config.sync {
                    flynt_core::models::SyncConfig::Git { remote, branch, .. } => {
                        let git = GitSync::new(project.root.clone(), remote.clone(), branch.clone());
                        git.diagnostic().map_err(|e| e.to_string())
                    }
                    flynt_core::models::SyncConfig::ICloud => Err("iCloud projects sync through the filesystem provider; Git diagnostics are not available.".into()),
                    flynt_core::models::SyncConfig::None => Err("This project has no sync backend configured.".into()),
                    other => Err(format!("{other:?} sync diagnostics are not implemented yet.")),
                })
                .await
                .unwrap_or_else(|e| Err(format!("Sync diagnostics interrupted: {e}"))),
            )
        }
    });

    let ctx_search = ctx.clone();
    let on_input = move |e: Event<FormData>| {
        let q = e.value();
        *search_query.write() = q.clone();
        *active_index.write() = None;
        if q.trim().is_empty() {
            *results.write() = Vec::new();
            return;
        }
        let project = ctx_search.project();
        spawn(async move {
            let hits = tokio::task::spawn_blocking(move || {
                project.store.search_documents(&q).unwrap_or_default()
            })
            .await
            .unwrap_or_default();
            *results.write() = hits;
        });
    };

    let (sync_label, sync_class, sync_title) = match *sync_status.read() {
        SyncStatus::Idle => ("\u{2713}", "sync-badge synced", "Synced".to_string()),
        SyncStatus::Syncing => (
            "\u{27F3}",
            "sync-badge syncing",
            "Syncing\u{2026}".to_string(),
        ),
        SyncStatus::Conflict(n) => (
            "\u{26A0}",
            "sync-badge conflict",
            format!("{n} conflict(s)"),
        ),
    };

    let grouped_results = group_results(&results.read());
    let flat_results = flatten_grouped_results(&grouped_results);
    let project_name = ctx.project().config.project_name.clone();
    let project_root = ctx.project_root();
    let omegon = ctx.omegon();
    let auto_status = ctx
        .runtime
        .read()
        .sync_status_rx
        .as_ref()
        .map(|rx| rx.borrow().clone());

    rsx! {
        div { class: "toolbar",
            span { class: "toolbar-project-name", "{project_name}" }
            {
                const BUILD: &str = env!("FLYNT_BUILD_HASH");
                rsx! { span { class: "toolbar-build-hash", title: "Build {BUILD}", "{BUILD}" } }
            }

            div { class: "toolbar-search-wrap",
                input {
                    class: "toolbar-search",
                    r#type: "text",
                    placeholder: "Search notes…  ↵ for full results",
                    value: "{search_query}",
                    oninput:  on_input,
                    onfocus:  move |_| *focused.write() = true,
                    onkeydown: move |e| {
                        if e.key() == Key::ArrowDown {
                            e.prevent_default();
                            let current = *active_index.read();
                            *active_index.write() = cycle_active_index(current, flat_results.len(), 1);
                        }
                        if e.key() == Key::ArrowUp {
                            e.prevent_default();
                            let current = *active_index.read();
                            *active_index.write() = cycle_active_index(current, flat_results.len(), -1);
                        }
                        if e.key() == Key::Enter {
                            let selected_index = *active_index.read();
                            if let Some(index) = selected_index {
                                if let Some(item) = flat_results.get(index) {
                                    tab_state.write().open(item.document_id.clone(), item.title.clone());
                                    *active_route.write() = Route::Notes;
                                    *focused.write() = false;
                                    *results.write() = Vec::new();
                                    *active_index.write() = None;
                                    return;
                                }
                            }
                            *active_route.write() = Route::Search;
                            *focused.write()  = false;
                            *results.write()  = Vec::new();
                            *active_index.write() = None;
                        }
                        if e.key() == Key::Escape {
                            *focused.write()  = false;
                            *results.write()  = Vec::new();
                            *active_index.write() = None;
                        }
                    },
                    onblur: move |_| {
                        spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                            *focused.write() = false;
                            *results.write() = Vec::new();
                            *active_index.write() = None;
                        });
                    },
                }

                if *focused.read() && !results.read().is_empty() {
                    div { class: "search-overlay",
                        {
                            let mut result_index = 0usize;
                            rsx! {
                                for group in grouped_results {
                                    if !group.folder.is_empty() {
                                        div { class: "search-group-header",
                                            span { class: "search-folder-icon", "▶" }
                                            span { class: "search-group-name", "{group.folder}" }
                                            span { class: "search-group-badge", "{group.items.len()}" }
                                        }
                                    }

                                    for item in group.items {
                                        {
                                            let id = item.document_id.clone();
                                            let title = item.title.clone();
                                            let t2 = title.clone();
                                            let path = item.path.to_string_lossy().to_string();
                                            let excerpt = item.excerpt.clone();
                                            let is_active = *active_index.read() == Some(result_index);
                                            let item_index = result_index;
                                            result_index += 1;
                                            let breadcrumb: String = {
                                                let mut parts: Vec<&str> = path.split('/').collect();
                                                if parts.len() > 1 { parts.pop(); }
                                                parts.join(" › ")
                                            };
                                            rsx! {
                                                button {
                                                    class: if is_active { "search-overlay-item active" } else { "search-overlay-item" },
                                                    onmouseenter: move |_| *active_index.write() = Some(item_index),
                                                    onmousedown: move |_| {
                                                        tab_state.write().open(id.clone(), t2.clone());
                                                        *active_route.write() = Route::Notes;
                                                        *focused.write() = false;
                                                        *results.write() = Vec::new();
                                                        *active_index.write() = None;
                                                    },
                                                    span { class: "search-overlay-title", "{title}" }
                                                    if !breadcrumb.is_empty() {
                                                        span { class: "search-overlay-path", "{breadcrumb}" }
                                                    }
                                                    if !excerpt.is_empty() {
                                                        div {
                                                            class: "src-excerpt",
                                                            dangerous_inner_html: "{excerpt}",
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        div { class: "search-overlay-enter",
                            if flat_results.is_empty() {
                                "Press ↵ to see all results"
                            } else if active_index.read().is_some() {
                                "↵ open selected • ↑↓ move • Esc close"
                            } else {
                                "Press ↵ to see all results • ↑↓ to select"
                            }
                        }
                    }
                }
            }

            div { class: "toolbar-right",
                if let Some(message) = update_action.read().as_ref() {
                    span { class: "update-status", title: "{message}", "{message}" }
                }
                if let Some(state) = update_state.read().as_ref().cloned() {
                    match state {
                        crate::self_update::UpdateState::Available {
                            channel,
                            latest,
                            release_commit,
                            html_url,
                            checksum_url,
                            install_source,
                            verified,
                            verification,
                            selected_artifact,
                            ..
                        } => {
                            let update_key = format!("{:?}:{latest}", channel);
                            let skipped = crate::bootstrap::OmegonRuntimeContext::load_launcher_profile()
                                .skipped_update_version
                                .as_deref()
                                .is_some_and(|skipped| skipped == update_key || skipped == latest);
                            let verified_artifact = if cfg!(target_os = "macos") && verified && install_source.should_open_direct_artifact() {
                                selected_artifact.clone()
                            } else {
                                None
                            };
                            let update_url = verified_artifact
                                .as_ref()
                                .map(|artifact| artifact.url.clone())
                                .unwrap_or_else(|| html_url.clone());
                            let checksum_note = if checksum_url.is_some() { " Checksum asset is available." } else { "" };
                            let update_class = if verified { "update-badge available verified" } else { "update-badge available unverified" };
                            let channel_note = if matches!(channel, crate::self_update::UpdateChannel::Nightly) {
                                release_commit
                                    .as_deref()
                                    .map(|commit| format!(" Nightly build {commit}."))
                                    .unwrap_or_else(|| " Nightly channel.".into())
                            } else {
                                String::new()
                            };
                            if skipped {
                                rsx! {}
                            } else {
                                rsx! {
                                    div { class: "update-pill",
                                        button {
                                            class: "{update_class}",
                                            title: "{install_source.label()} Flynt {latest}. {verification}{channel_note}{checksum_note}",
                                            onclick: move |_| {
                                                if let Some(artifact) = verified_artifact.clone() {
                                                    *update_action.write() = Some("Downloading verified update...".into());
                                                    spawn(async move {
                                                        match crate::self_update::download_verified_artifact(artifact).await {
                                                            Ok(path) => {
                                                                *update_action.write() = Some("Verified update downloaded.".into());
                                                                let _ = open::that(path);
                                                            }
                                                            Err(err) => {
                                                                *update_action.write() = Some(format!("Update verification failed: {err}"));
                                                            }
                                                        }
                                                    });
                                                } else {
                                                    let _ = open::that(&update_url);
                                                }
                                            },
                                            "{install_source.label()} {latest}"
                                        }
                                        button {
                                            class: "update-dismiss",
                                            title: "Skip Flynt {latest}",
                                            onclick: move |_| {
                                                let mut profile = crate::bootstrap::OmegonRuntimeContext::load_launcher_profile();
                                                profile.skipped_update_version = Some(update_key.clone());
                                                let _ = crate::bootstrap::OmegonRuntimeContext::save_launcher_profile(&profile);
                                            },
                                            "×"
                                        }
                                    }
                                }
                            }
                        }
                        crate::self_update::UpdateState::Unknown { channel, message } => rsx! {
                            button {
                                class: "update-badge unknown",
                                title: "Could not check {channel.label()} updates: {message}",
                                onclick: move |_| {
                                    let _ = open::that(crate::self_update::release_page_url(channel));
                                },
                                "Updates"
                            }
                        },
                        crate::self_update::UpdateState::Current { .. } => rsx! {},
                    }
                }
                button {
                    class: "btn btn-ghost",
                    title: "Open another project in a new window",
                    onclick: move |_| {
                        let _ = FileDialog::new()
                            .pick_folder()
                            .and_then(|path| OmegonRuntimeContext::spawn_new_instance_for_project(&path).ok());
                    },
                    span { class: "nav-icon", dangerous_inner_html: crate::icons::ICON_SCROLL }
                }
                if *sync_status.read() != SyncStatus::Idle || matches!(ctx.project().config.sync, flynt_core::models::SyncConfig::Git { .. }) {
                    button {
                        class: "{sync_class}",
                        title: "{sync_title}. Click for sync activity.",
                            onclick: move |_| {
                                let open = *sync_panel_open.read();
                                *sync_panel_open.write() = !open;
                                let next_refresh = {
                                    let current = *sync_refresh.peek();
                                    current.wrapping_add(1)
                                };
                                *sync_refresh.write() = next_refresh;
                            },
                        "{sync_label}"
                    }
                    if *sync_panel_open.read() {
                        SyncActivityPanel {
                            diagnostic: sync_diagnostic.read().clone().flatten(),
                            auto_status: auto_status.clone(),
                            activity_state: sync_activity.read().clone(),
                            action_message: sync_action_message.read().clone(),
                            on_close: move |_| *sync_panel_open.write() = false,
                                on_refresh: move |_| {
                                    *sync_action_message.write() = None;
                                    let next_refresh = {
                                        let current = *sync_refresh.peek();
                                        current.wrapping_add(1)
                                    };
                                    *sync_refresh.write() = next_refresh;
                                },
                            on_sync_now: move |_| {
                                *sync_action_message.write() = Some("Sync started...".into());
                                let c = ctx.clone();
                                spawn(async move {
                                    let project = c.project();
                                    match &project.config.sync {
                                        flynt_core::models::SyncConfig::Git { remote, branch, .. } => {
                                            let remote = remote.clone();
                                            let branch = branch.clone();
                                            let result = tokio::task::spawn_blocking(move || {
                                                let git = GitSync::new(project.root.clone(), remote, branch);
                                                git.auto_commit("[flynt] manual sync from activity panel")?;
                                                flynt_core::sync::SyncBackend::sync(&git)?;
                                                anyhow::Ok(())
                                            }).await;
                                            match result {
                                                    Ok(Ok(())) => {
                                                        *sync_action_message.write() = Some("Sync completed.".into());
                                                        let next_refresh = {
                                                            let current = *sync_refresh.peek();
                                                            current.wrapping_add(1)
                                                        };
                                                        *sync_refresh.write() = next_refresh;
                                                    }
                                                Ok(Err(e)) => *sync_action_message.write() = Some(format!("Sync failed: {e}")),
                                                Err(e) => *sync_action_message.write() = Some(format!("Sync interrupted: {e}")),
                                            }
                                        }
                                        _ => *sync_action_message.write() = Some("No Git sync backend is configured.".into()),
                                    }
                                });
                            },
                            on_open_conflict: move |path: String| {
                                let c = ctx.clone();
                                spawn(async move {
                                    let project = c.project();
                                    if let Ok(Some(doc)) = project.store.get_document_by_path(std::path::Path::new(&path)) {
                                        tab_state.write().open(doc.id, doc.title);
                                        *active_route.write() = Route::Notes;
                                    }
                                });
                            },
                        }
                    }
                }
                button {
                    class: if *show_agent.read() { "btn btn-ghost active" } else { "btn btn-ghost" },
                    title: "Toggle agent rail",
                    onclick: move |_| {
                        let opening = !*show_agent.read();
                        let omegon = omegon.clone();
                        let project_root = project_root.clone();
                        if opening {
                            let mut should_clear_child = false;
                            let mut child_check_error = None;
                            {
                                let mut child_slot = omegon_child.write();
                                if let Some(child) = child_slot.as_mut() {
                                    match child.try_wait() {
                                        Ok(Some(_status)) => should_clear_child = true,
                                        Ok(None) => {}
                                        Err(err) => {
                                            should_clear_child = true;
                                            child_check_error = Some(err.to_string());
                                        }
                                    }
                                }
                                if should_clear_child {
                                    *child_slot = None;
                                }
                            }
                            if should_clear_child {
                                *omegon_pid.write() = None;
                            }
                            if let Some(err) = child_check_error {
                                *omegon_launch_error.write() = Some(err);
                            }

                            if omegon_child.read().is_none() {
                                spawn(async move {
                                    match omegon.spawn_background_host(&project_root).await {
                                        Ok(child) => {
                                            let pid = child.id();
                                            *omegon_child.write() = Some(child);
                                            *omegon_pid.write() = pid;
                                            *omegon_launch_error.write() = None;
                                        }
                                        Err(err) => {
                                            *omegon_child.write() = None;
                                            *omegon_pid.write() = None;
                                            *omegon_launch_error.write() = Some(err.to_string());
                                        }
                                    }
                                });
                            }
                        } else if let Some(mut child) = omegon_child.write().take() {
                            spawn(async move {
                                let _ = child.kill().await;
                                let _ = child.wait().await;
                            });
                            *omegon_pid.write() = None;
                            *omegon_launch_error.write() = None;
                        }
                        *show_agent.write() = opening;
                    },
                    span { class: "nav-icon", dangerous_inner_html: crate::icons::ICON_OMEGON }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{cycle_active_index, flatten_grouped_results, group_results};
    use flynt_core::models::{DocumentId, SearchResult};
    use std::path::PathBuf;
    use uuid::Uuid;

    fn result(path: &str, score: f32) -> SearchResult {
        SearchResult {
            document_id: DocumentId(Uuid::nil()),
            path: PathBuf::from(path),
            title: path.to_string(),
            excerpt: String::new(),
            score,
        }
    }

    #[test]
    fn quick_results_follow_full_search_ranking() {
        let groups = group_results(&[
            result("ideas/low.md", 0.2),
            result("notes/high.md", 0.9),
            result("notes/mid.md", 0.5),
            result("ideas/top.md", 1.2),
        ]);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].folder, "ideas");
        assert_eq!(groups[0].items[0].path, PathBuf::from("ideas/top.md"));
        assert_eq!(groups[1].folder, "notes");
        assert_eq!(groups[1].items[0].path, PathBuf::from("notes/high.md"));
    }

    #[test]
    fn flattened_results_preserve_render_order() {
        let groups = group_results(&[
            result("ideas/top.md", 1.2),
            result("ideas/low.md", 0.2),
            result("notes/high.md", 0.9),
        ]);

        let flattened = flatten_grouped_results(&groups);
        assert_eq!(flattened[0].path, PathBuf::from("ideas/top.md"));
        assert_eq!(flattened[1].path, PathBuf::from("ideas/low.md"));
        assert_eq!(flattened[2].path, PathBuf::from("notes/high.md"));
    }

    #[test]
    fn keyboard_selection_wraps() {
        assert_eq!(cycle_active_index(None, 3, 1), Some(0));
        assert_eq!(cycle_active_index(Some(0), 3, -1), Some(2));
        assert_eq!(cycle_active_index(Some(2), 3, 1), Some(0));
        assert_eq!(cycle_active_index(None, 3, -1), Some(2));
        assert_eq!(cycle_active_index(Some(1), 0, 1), None);
    }
}
