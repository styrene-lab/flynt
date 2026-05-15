use crate::{
    bootstrap::{AppContext, OmegonRuntimeContext},
    state::{Route, SyncStatus, TabState},
};
use dioxus::prelude::*;
use flynt_core::{models::SearchResult, store::ProjectStore};
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
    let mut results: Signal<Vec<SearchResult>> = use_signal(Vec::new);
    let mut focused = use_signal(|| false);
    let mut active_index = use_signal(|| None::<usize>);
    let mut update_action = use_signal(|| None::<String>);
    let update_state =
        use_resource(|| async move { crate::self_update::check_latest_release().await });

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
                    span { class: "{sync_class}", title: "{sync_title}", "{sync_label}" }
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
