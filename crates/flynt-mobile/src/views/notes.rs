use flynt_core::store::ProjectStore;
use comrak::{Options, markdown_to_html};
use dioxus::prelude::*;
use crate::bootstrap::MobileRuntime;

fn render_md(content: &str) -> String {
    let mut opts = Options::default();
    opts.extension.table = true;
    opts.extension.strikethrough = true;
    opts.extension.tasklist = true;
    opts.extension.autolink = true;
    opts.render.unsafe_ = true;
    markdown_to_html(content, &opts)
}

#[component]
pub fn NotesList(
    on_select: EventHandler<String>,
) -> Element {
    let rt = use_context::<Signal<MobileRuntime>>();
    let docs = use_memo(move || {
        rt.read()
            .project
            .store
            .list_documents()
            .unwrap_or_default()
    });

    rsx! {
        div { class: "notes-list",
            div { class: "notes-list-header",
                h2 { "Notes" }
                span { class: "notes-count", "{docs.read().len()}" }
            }
            div { class: "notes-list-items",
                for doc in docs.read().iter() {
                    {
                        let doc_id = doc.id.0.to_string();
                        let title = doc.title.clone();
                        let path = doc.path.display().to_string();
                        rsx! {
                            button {
                                key: "{doc_id}",
                                class: "note-item",
                                onclick: move |_| on_select.call(doc_id.clone()),
                                div { class: "note-item-title", "{title}" }
                                div { class: "note-item-path", "{path}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn NoteDetail(
    doc_id: String,
    on_back: EventHandler<()>,
) -> Element {
    let rt = use_context::<Signal<MobileRuntime>>();

    let content = use_memo(move || {
        let id = flynt_core::models::DocumentId(doc_id.parse().unwrap_or_default());
        rt.read()
            .project
            .store
            .get_document(&id)
            .ok()
            .flatten()
            .map(|doc| (doc.title.clone(), render_md(&doc.content)))
    });

    match &*content.read() {
        Some((title, html)) => rsx! {
            div { class: "note-detail",
                div { class: "note-detail-header",
                    button { class: "back-btn", onclick: move |_| on_back.call(()), "← Back" }
                    h2 { "{title}" }
                }
                div { class: "note-detail-body markdown-body", dangerous_inner_html: "{html}" }
            }
        },
        None => rsx! {
            div { class: "note-detail",
                div { class: "note-detail-header",
                    button { class: "back-btn", onclick: move |_| on_back.call(()), "← Back" }
                }
                p { class: "muted", "Document not found." }
            }
        },
    }
}
