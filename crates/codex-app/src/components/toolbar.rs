use codex_core::{models::SearchResult, store::VaultStore};
use dioxus::prelude::*;
use crate::{bootstrap::AppContext, state::{Route, SyncStatus, TabState}};

#[derive(Clone)]
struct SearchGroup {
    folder: String,
    items:  Vec<SearchResult>,
}

fn top_level_folder(path: &std::path::Path) -> String {
    let mut comps = path.components();
    let Some(first) = comps.next() else { return String::new(); };
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
            groups.push(SearchGroup { folder, items: vec![item] });
        }
    }

    for group in &mut groups {
        group.items.sort_by(|a, b| b.score.total_cmp(&a.score));
    }

    groups.sort_by(|a, b| {
        let a_score = a.items.first().map(|item| item.score).unwrap_or(f32::NEG_INFINITY);
        let b_score = b.items.first().map(|item| item.score).unwrap_or(f32::NEG_INFINITY);
        b_score.total_cmp(&a_score).then_with(|| a.folder.cmp(&b.folder))
    });

    groups
}

#[component]
pub fn Toolbar(
    sync_status:      Signal<SyncStatus>,
    mut show_agent:   Signal<bool>,
    mut active_route: Signal<Route>,
    mut search_query: Signal<String>,
) -> Element {
    let ctx           = use_context::<AppContext>();
    let mut tab_state = use_context::<Signal<TabState>>();
    let mut results:  Signal<Vec<SearchResult>> = use_signal(Vec::new);
    let mut focused = use_signal(|| false);

    let ctx_search = ctx.clone();
    let on_input = move |e: Event<FormData>| {
        let q = e.value();
        *search_query.write() = q.clone();
        if q.trim().is_empty() { *results.write() = Vec::new(); return; }
        let c = ctx_search.clone();
        spawn(async move {
            let hits = tokio::task::spawn_blocking(move || {
                c.vault.store.search_documents(&q).unwrap_or_default()
            }).await.unwrap_or_default();
            *results.write() = hits;
        });
    };

    let sync_label = match *sync_status.read() {
        SyncStatus::Idle        => "",
        SyncStatus::Syncing     => "⟳",
        SyncStatus::Conflict(_) => "⚠",
    };

    rsx! {
        div { class: "toolbar",
            span { class: "toolbar-vault-name", "{ctx.vault.config.vault_name}" }

            div { class: "toolbar-search-wrap",
                input {
                    class: "toolbar-search",
                    r#type: "text",
                    placeholder: "Search notes…  ↵ for full results",
                    value: "{search_query}",
                    oninput:  on_input,
                    onfocus:  move |_| *focused.write() = true,
                    onkeydown: move |e| {
                        if e.key() == Key::Enter {
                            *active_route.write() = Route::Search;
                            *focused.write()  = false;
                            *results.write()  = Vec::new();
                        }
                        if e.key() == Key::Escape {
                            *focused.write()  = false;
                            *results.write()  = Vec::new();
                        }
                    },
                    onblur: move |_| {
                        spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                            *focused.write() = false;
                            *results.write() = Vec::new();
                        });
                    },
                }

                if *focused.read() && !results.read().is_empty() {
                    div { class: "search-overlay",
                        for group in group_results(&results.read()) {
                            if !group.folder.is_empty() {
                                div { class: "search-group-header",
                                    span { class: "search-folder-icon", "▶" }
                                    span { class: "search-group-name", "{group.folder}" }
                                    span { class: "search-group-badge", "{group.items.len()}" }
                                }
                            }

                            for item in group.items {
                                {
                                    let id      = item.document_id.clone();
                                    let title   = item.title.clone();
                                    let t2      = title.clone();
                                    let path    = item.path.to_string_lossy().to_string();
                                    let excerpt = item.excerpt.clone();
                                    let breadcrumb: String = {
                                        let mut parts: Vec<&str> = path.split('/').collect();
                                        if parts.len() > 1 { parts.pop(); }
                                        parts.join(" › ")
                                    };
                                    rsx! {
                                        button {
                                            class: "search-overlay-item",
                                            onmousedown: move |_| {
                                                tab_state.write().open(id.clone(), t2.clone());
                                                *active_route.write() = Route::Notes;
                                                *focused.write() = false;
                                                *results.write() = Vec::new();
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
                        div { class: "search-overlay-enter",
                            "Press ↵ to see all results"
                        }
                    }
                }
            }

            div { class: "toolbar-right",
                if !sync_label.is_empty() {
                    span { class: "sync-badge", "{sync_label}" }
                }
                button {
                    class: if *show_agent.read() { "btn btn-ghost active" } else { "btn btn-ghost" },
                    title: "Toggle agent rail",
                    onclick: move |_| { let v = *show_agent.read(); *show_agent.write() = !v; },
                    "✦"
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::group_results;
    use codex_core::models::{DocumentId, SearchResult};
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
}
