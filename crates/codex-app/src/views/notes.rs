use codex_core::{models::DocumentId, store::VaultStore};
use comrak::{Options, markdown_to_html};
use dioxus::prelude::*;
use crate::bootstrap::AppContext;

/// Obsidian-style markdown preview.
/// The sidebar controls which document is selected; this view renders it.
#[component]
pub fn NotesView(selected_doc: Signal<Option<DocumentId>>) -> Element {
    let ctx = use_context::<AppContext>();

    // Re-fetches whenever selected_doc changes (signal read in closure body = reactive dep).
    let rendered = use_resource(move || {
        let selected_id = selected_doc.read().clone();
        let vault = ctx.vault.clone();
        async move {
            let Some(doc_id) = selected_id else {
                return None;
            };
            tokio::task::spawn_blocking(move || {
                vault
                    .store
                    .get_document(&doc_id)
                    .ok()
                    .flatten()
                    .map(|doc| {
                        let mut opts = Options::default();
                        opts.extension.table = true;
                        opts.extension.strikethrough = true;
                        opts.extension.tasklist = true;
                        opts.extension.autolink = true;
                        opts.extension.footnotes = true;
                        opts.render.unsafe_ = false; // no raw HTML passthrough
                        (doc.title.clone(), markdown_to_html(&doc.content, &opts))
                    })
            })
            .await
            .ok()
            .flatten()
        }
    });

    rsx! {
        div { class: "view-notes",
            match &*rendered.read() {
                // Resource still loading
                None if selected_doc.read().is_some() => rsx! {
                    div { class: "notes-loading", "Loading…" }
                },
                // Nothing selected
                None => rsx! {
                    div { class: "notes-empty",
                        p { class: "muted", "Select a note from the sidebar." }
                    }
                },
                // Document ready
                Some(None) => rsx! {
                    div { class: "notes-empty",
                        p { class: "muted", "Document not found." }
                    }
                },
                Some(Some((title, html))) => rsx! {
                    div { class: "notes-pane",
                        h1 { class: "doc-title", "{title}" }
                        div {
                            class: "markdown-body",
                            dangerous_inner_html: "{html}",
                        }
                    }
                },
            }
        }
    }
}
