use codex_core::{models::DocumentId, store::VaultStore};
use comrak::{Options, markdown_to_html};
use dioxus::prelude::*;
use crate::bootstrap::AppContext;

#[derive(Clone, PartialEq)]
enum EditMode { Preview, Edit }

fn render_html(content: &str) -> String {
    let mut opts = Options::default();
    opts.extension.table        = true;
    opts.extension.strikethrough = true;
    opts.extension.tasklist     = true;
    opts.extension.autolink     = true;
    opts.extension.footnotes    = true;
    opts.render.unsafe_         = false;
    markdown_to_html(content, &opts)
}

#[component]
pub fn NotesView(selected_doc: Signal<Option<DocumentId>>) -> Element {
    let ctx = use_context::<AppContext>();

    // Separate clones: one per closure that captures ctx.
    let ctx_res   = ctx.clone(); // → use_resource
    let ctx_save1 = ctx.clone(); // → Save button onclick
    let ctx_save2 = ctx.clone(); // → editor Cmd+S onkeydown

    let mut mode      = use_signal(|| EditMode::Preview);
    let mut edit_body = use_signal(String::new);
    let mut save_err  = use_signal(|| Option::<String>::None);

    // Resource: (rel_path, title, raw_content, rendered_html)
    // Resource<T> is Copy in Dioxus 0.7 — safe to capture in multiple closures below.
    let rendered = use_resource(move || {
        let selected_id = selected_doc.read().clone();
        let c = ctx_res.clone();
        async move {
            let Some(doc_id) = selected_id else { return None; };
            tokio::task::spawn_blocking(move || {
                c.vault.store.get_document(&doc_id).ok().flatten().map(|doc| {
                    let html = render_html(&doc.content);
                    (doc.path.clone(), doc.title.clone(), doc.content.clone(), html)
                })
            })
            .await.ok().flatten()
        }
    });

    // Populate edit buffer when a new doc loads; reset mode to preview.
    use_effect(move || {
        if let Some(Some((_, _, body, _))) = &*rendered.read() {
            *edit_body.write() = body.clone();
            *mode.write() = EditMode::Preview;
        }
    });

    rsx! {
        div { class: "view-notes",
            match &*rendered.read() {
                None if selected_doc.read().is_some() => rsx! {
                    div { class: "notes-loading muted", "Loading…" }
                },
                None => rsx! {
                    div { class: "notes-empty",
                        p { class: "muted", "Select a note from the sidebar." }
                    }
                },
                Some(None) => rsx! {
                    div { class: "notes-empty",
                        p { class: "muted", "Document not found." }
                    }
                },
                Some(Some((_, title, _, html))) => rsx! {
                    div { class: "notes-pane",
                        div { class: "notes-topbar",
                            h1 { class: "doc-title", "{title}" }
                            div { class: "notes-actions",
                                if let Some(ref err) = *save_err.read() {
                                    span { class: "save-msg err", "{err}" }
                                }
                                match *mode.read() {
                                    EditMode::Preview => rsx! {
                                        button {
                                            class: "btn btn-ghost",
                                            onclick: move |_| *mode.write() = EditMode::Edit,
                                            "Edit"
                                        }
                                    },
                                    EditMode::Edit => rsx! {
                                        button {
                                            class: "btn btn-primary",
                                            onclick: move |_| {
                                                let Some(Some((rel_path, _, _, _))) = &*rendered.read() else { return };
                                                let path    = rel_path.clone();
                                                let content = edit_body.read().clone();
                                                let c       = ctx_save1.clone();
                                                let mut re  = rendered;
                                                spawn(async move {
                                                    match tokio::task::spawn_blocking(move || {
                                                        c.vault.save_document_content(&path, &content)
                                                    }).await {
                                                        Ok(Ok(())) => { re.restart(); *save_err.write() = None; }
                                                        Ok(Err(e)) => *save_err.write() = Some(e.to_string()),
                                                        Err(e)     => *save_err.write() = Some(e.to_string()),
                                                    }
                                                });
                                                *mode.write() = EditMode::Preview;
                                            },
                                            "Save"
                                        }
                                        button {
                                            class: "btn btn-ghost",
                                            onclick: move |_| *mode.write() = EditMode::Preview,
                                            "Cancel"
                                        }
                                    },
                                }
                            }
                        }

                        match *mode.read() {
                            EditMode::Preview => rsx! {
                                div {
                                    class: "markdown-body",
                                    dangerous_inner_html: "{html}",
                                }
                            },
                            EditMode::Edit => rsx! {
                                textarea {
                                    class: "editor-textarea",
                                    value: "{edit_body}",
                                    oninput: move |e| *edit_body.write() = e.value(),
                                    onkeydown: move |e| {
                                        let is_save = e.modifiers().meta() || e.modifiers().ctrl();
                                        if is_save && e.key() == Key::Character("s".to_string()) {
                                            let Some(Some((rel_path, _, _, _))) = &*rendered.read() else { return };
                                            let path    = rel_path.clone();
                                            let content = edit_body.read().clone();
                                            let c       = ctx_save2.clone();
                                            let mut re  = rendered;
                                            spawn(async move {
                                                match tokio::task::spawn_blocking(move || {
                                                    c.vault.save_document_content(&path, &content)
                                                }).await {
                                                    Ok(Ok(())) => { re.restart(); *save_err.write() = None; }
                                                    Ok(Err(e)) => *save_err.write() = Some(e.to_string()),
                                                    Err(e)     => *save_err.write() = Some(e.to_string()),
                                                }
                                            });
                                            *mode.write() = EditMode::Preview;
                                        }
                                    },
                                }
                            },
                        }
                    }
                },
            }
        }
    }
}
