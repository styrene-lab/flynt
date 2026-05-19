use crate::{
    bootstrap::AppContext,
    state::{Route, TabState},
};
use dioxus::prelude::*;
use flynt_core::{
    models::{LensLayout, ProjectLens},
    query::{LensResult, execute_lens},
    store::ProjectStore,
};
use std::path::PathBuf;

#[component]
pub fn LensesView() -> Element {
    let ctx = use_context::<AppContext>();
    let mut tab_state = use_context::<Signal<TabState>>();
    let mut active_route = use_context::<Signal<Route>>();
    let mut selected = use_signal(|| 0usize);
    let mut refresh = use_signal(|| 0_u64);

    let lenses = use_resource(move || {
        let project = ctx.project();
        let _ = refresh();
        async move {
            tokio::task::spawn_blocking(move || project.load_lenses())
                .await
                .unwrap_or_else(|e| Err(anyhow::anyhow!(e.to_string())))
        }
    });

    let loaded = lenses.read();
    let current = loaded
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .and_then(|items| items.get((*selected.read()).min(items.len().saturating_sub(1))))
        .cloned();

    let result = current.as_ref().map(|(_, lens)| {
        let project = ctx.project();
        execute_lens(lens, project.store.as_ref())
    });

    rsx! {
        div { class: "lenses-view",
            aside { class: "lenses-sidebar",
                div { class: "lenses-sidebar-header",
                    div {
                        div { class: "lenses-title", "Project Lenses" }
                        div { class: "lenses-subtitle", ".flynt/lenses/*.toml" }
                    }
                    button {
                        class: "btn btn-ghost btn-sm",
                        title: "Refresh lenses",
                        onclick: move |_| {
                            let next = refresh.read().wrapping_add(1);
                            refresh.set(next);
                        },
                        "Refresh"
                    }
                }
                match loaded.as_ref() {
                    None => rsx! { div { class: "lenses-empty", "Loading lenses..." } },
                    Some(Err(err)) => rsx! { div { class: "lenses-error", "{err}" } },
                    Some(Ok(items)) if items.is_empty() => rsx! {
                        div { class: "lenses-empty",
                            div { class: "lenses-empty-title", "No lenses yet" }
                            div { class: "lenses-empty-copy", "Save a search as a lens from the command palette, or add TOML files under .flynt/lenses/." }
                        }
                    },
                    Some(Ok(items)) => rsx! {
                        div { class: "lenses-list",
                            for (idx, (path, lens)) in items.iter().enumerate() {
                                {
                                    let is_active = idx == (*selected.read()).min(items.len().saturating_sub(1));
                                    let path_label = path.to_string_lossy().to_string();
                                    rsx! {
                                        button {
                                            key: "{path_label}",
                                            class: if is_active { "lens-list-item active" } else { "lens-list-item" },
                                            onclick: move |_| selected.set(idx),
                                            span { class: "lens-list-title", "{lens.title}" }
                                            span { class: "lens-list-meta", "{lens.source:?} · {lens.layout:?}" }
                                        }
                                    }
                                }
                            }
                        }
                    },
                }
            }

            main { class: "lenses-content",
                match (current, result) {
                    (Some((path, lens)), Some(Ok(result))) => rsx! {
                        LensDetail {
                            path,
                            lens,
                            result,
                            on_open_document: move |doc_id: flynt_core::models::DocumentId| {
                                let project = ctx.project();
                                if let Ok(Some(doc)) = project.store.get_document(&doc_id) {
                                    tab_state.write().open(doc.id, doc.title);
                                    *active_route.write() = Route::Notes;
                                }
                            }
                        }
                    },
                    (Some((_path, lens)), Some(Err(err))) => rsx! {
                        div { class: "lens-detail",
                            h1 { "{lens.title}" }
                            div { class: "lenses-error", "{err}" }
                        }
                    },
                    _ => rsx! {
                        div { class: "lens-placeholder",
                            div { class: "lens-placeholder-mark", dangerous_inner_html: crate::icons::ICON_LENS }
                            div { class: "lens-placeholder-title", "Select a lens" }
                        }
                    },
                }
            }
        }
    }
}

#[component]
fn LensDetail(
    path: PathBuf,
    lens: ProjectLens,
    result: LensResult,
    on_open_document: EventHandler<flynt_core::models::DocumentId>,
) -> Element {
    let path_label = path.to_string_lossy().to_string();
    rsx! {
        div { class: "lens-detail",
            header { class: "lens-detail-header",
                div {
                    h1 { "{lens.title}" }
                    div { class: "lens-detail-meta",
                        span { "{lens.source:?}" }
                        span { " · " }
                        span { "{result.rows.len()} rows" }
                        span { " · " }
                        span { "{path_label}" }
                    }
                }
            }
            match lens.layout {
                LensLayout::Table => rsx! {
                    div { class: "lens-table-wrap",
                        table { class: "lens-table",
                            thead {
                                tr {
                                    for column in result.columns.iter() {
                                        th { "{column.label.as_deref().unwrap_or(&column.field)}" }
                                    }
                                }
                            }
                            tbody {
                                if result.rows.is_empty() {
                                    tr {
                                        td {
                                            colspan: "{result.columns.len().max(1)}",
                                            class: "lens-empty-cell",
                                            "No matching rows"
                                        }
                                    }
                                } else {
                                    for row in result.rows.iter() {
                                        {
                                            let doc_id = row.document_id.clone();
                                            rsx! {
                                                tr {
                                                    for column in result.columns.iter() {
                                                        {
                                                            let value = row.values.get(&column.field).cloned().unwrap_or_default();
                                                            let is_title = column.field == "title" || column.field == "name";
                                                            rsx! {
                                                                td {
                                                                    if is_title {
                                                                        if let Some(id) = doc_id.clone() {
                                                                            button {
                                                                                class: "lens-row-link",
                                                                                onclick: move |_| on_open_document.call(id.clone()),
                                                                                "{value}"
                                                                            }
                                                                        } else {
                                                                            span { "{value}" }
                                                                        }
                                                                    } else {
                                                                        span { "{value}" }
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
                            }
                        }
                    }
                },
                LensLayout::List => rsx! {
                    div { class: "lens-list-layout",
                        for row in result.rows.iter() {
                            {
                                let doc_id = row.document_id.clone();
                                rsx! {
                                    button {
                                        class: "lens-list-row",
                                        disabled: doc_id.is_none(),
                                        onclick: move |_| {
                                            if let Some(id) = doc_id.clone() {
                                                on_open_document.call(id);
                                            }
                                        },
                                        "{row.title}"
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
