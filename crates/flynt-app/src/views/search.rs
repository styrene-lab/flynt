use flynt_core::{models::SearchResult, store::VaultStore};
use dioxus::prelude::*;
use crate::{bootstrap::AppContext, state::{Route, TabState}};

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
pub fn SearchView(mut search_query: Signal<String>) -> Element {
    let ctx              = use_context::<AppContext>();
    let mut tab_state    = use_context::<Signal<TabState>>();
    let mut active_route = use_context::<Signal<Route>>();

    let results = use_resource(move || {
        let q = search_query.read().clone();
        let vault = ctx.vault();
        async move {
            if q.trim().is_empty() { return vec![]; }
            tokio::task::spawn_blocking(move || {
                vault.store.search_documents(&q).unwrap_or_default()
            })
            .await
            .unwrap_or_default()
        }
    });

    let q_val   = search_query.read().clone();
    let q_match = q_val.clone();
    let q_disp  = q_val.clone();

    rsx! {
        div { class: "search-view",
            div { class: "search-view-bar",
                span { class: "search-view-icon", "⌕" }
                input {
                    class: "search-view-input",
                    r#type: "text",
                    placeholder: "Search all notes…",
                    autofocus: true,
                    value: "{q_val}",
                    oninput: move |e| *search_query.write() = e.value(),
                    onkeydown: move |e| {
                        if e.key() == Key::Escape {
                            *active_route.write() = Route::Notes;
                        }
                    },
                }
            }

            div { class: "search-content",
                match &*results.read() {
                    None => rsx! {
                        div { class: "search-status muted", "Searching…" }
                    },

                    Some(list) if list.is_empty() && q_match.trim().is_empty() => rsx! {
                        div { class: "search-empty-state",
                            div { class: "search-empty-icon", "⌕" }
                            p { class: "search-empty-msg", "Type to search across all notes." }
                        }
                    },

                    Some(list) if list.is_empty() => rsx! {
                        div { class: "search-empty-state",
                            div { class: "search-empty-icon", "◎" }
                            p { class: "search-empty-msg",
                                "No results for \"{q_disp}\""
                            }
                            p { class: "muted search-empty-hint",
                                "Try different keywords or check the spelling."
                            }
                        }
                    },

                    Some(list) => {
                        let groups = group_results(list);
                        let total = list.len();
                        let file_count = list.iter()
                            .map(|item| item.document_id.clone())
                            .collect::<std::collections::HashSet<_>>()
                            .len();
                        let res_word = if total == 1 { "result" } else { "results" };
                        let file_word = if file_count == 1 { "file" } else { "files" };

                        rsx! {
                            div { class: "search-stats-bar",
                                span { class: "search-stats-count", "{total}" }
                                span { class: "muted", " {res_word} in " }
                                span { class: "search-stats-count", "{file_count}" }
                                span { class: "muted", " {file_word}" }
                            }

                            div { class: "search-results-list",
                                for group in groups {
                                    div { class: "search-group",
                                        if !group.folder.is_empty() {
                                            div { class: "search-group-header",
                                                span { class: "search-folder-icon", "▶" }
                                                span { class: "search-group-name", "{group.folder}" }
                                                span { class: "search-group-badge", "{group.items.len()}" }
                                            }
                                        }

                                        for item in group.items {
                                            {
                                                let doc_id  = item.document_id.clone();
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
                                                        class: "search-result-card",
                                                        onclick: move |_| {
                                                            tab_state.write().open(doc_id.clone(), t2.clone());
                                                            *active_route.write() = Route::Notes;
                                                        },
                                                        div { class: "src-header",
                                                            span { class: "src-file-icon nav-icon", dangerous_inner_html: crate::icons::ICON_SCROLL }
                                                            div { class: "src-meta",
                                                                span { class: "src-title", "{title}" }
                                                                if !breadcrumb.is_empty() {
                                                                    span { class: "src-path muted", "{breadcrumb}" }
                                                                }
                                                            }
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
                        }
                    },
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{group_results, top_level_folder};
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
    fn top_level_folder_only_for_nested_paths() {
        assert_eq!(top_level_folder(PathBuf::from("notes/alpha.md").as_path()), "notes");
        assert_eq!(top_level_folder(PathBuf::from("alpha.md").as_path()), "");
    }

    #[test]
    fn groups_and_items_follow_best_score() {
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
