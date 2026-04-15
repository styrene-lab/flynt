use codex_core::{models::DocumentId, store::VaultStore};
use comrak::{Options, markdown_to_html};
use dioxus::prelude::*;
use crate::bootstrap::AppContext;

#[derive(Clone, PartialEq)]
enum EditMode { Preview, Edit }

fn render_html(content: &str) -> String {
    let mut opts = Options::default();
    opts.extension.table                      = true;
    opts.extension.strikethrough              = true;
    opts.extension.tasklist                   = true;
    opts.extension.autolink                   = true;
    opts.extension.footnotes                  = true;
    opts.extension.wikilinks_title_after_pipe = true;
    opts.render.unsafe_                       = true;
    let processed = preprocess(content);
    markdown_to_html(&processed, &opts)
}

/// Convert Obsidian-style embeds/links before comrak sees the source.
fn preprocess(src: &str) -> String {
    let mut out = String::with_capacity(src.len() + 64);
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '!' && chars.peek() == Some(&'[') {
            chars.next(); // consume first [
            if chars.peek() == Some(&'[') {
                chars.next(); // consume second [
                let inner: String = chars.by_ref().take_while(|&ch| ch != ']').collect();
                // consume trailing ]
                if chars.peek() == Some(&']') { chars.next(); }
                // Image embed: ![[file.png]] → ![file](vault://localhost/file.png)
                let ext = std::path::Path::new(&inner)
                    .extension().and_then(|e| e.to_str()).unwrap_or("");
                if matches!(ext, "png"|"jpg"|"jpeg"|"gif"|"svg"|"webp") {
                    let encoded = inner.replace(' ', "%20");
                    out.push_str(&format!("![{inner}](vault://localhost/{encoded})"));
                } else {
                    // Non-image embed — inline as a link for now
                    let encoded = inner.replace(' ', "%20");
                    out.push_str(&format!("[{inner}](vault://localhost/{encoded})"));
                }
                continue;
            } else {
                out.push('!'); out.push('[');
            }
        } else if c == '[' && chars.peek() == Some(&'[') {
            chars.next();
            let inner: String = chars.by_ref().take_while(|&ch| ch != ']').collect();
            if chars.peek() == Some(&']') { chars.next(); }
            // [[target|display]] or [[target]]
            let (target, display) = inner.split_once('|')
                .map(|(t, d)| (t, d))
                .unwrap_or((&inner, &inner as &str));
            let encoded = target.replace(' ', "%20");
            out.push_str(&format!("[{display}](codex-note://{encoded})"));
            continue;
        }
        out.push(c);
    }
    out
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
                                div { class: "editor-split",
                                    // ── Left: raw markdown ───────────────────────────────
                                    div { class: "editor-pane",
                                        textarea {
                                            class: "editor-textarea",
                                            value: "{edit_body}",
                                            oninput: move |e| *edit_body.write() = e.value(),
                                            onkeydown: move |e| {
                                                let save_key = e.modifiers().meta() || e.modifiers().ctrl();
                                                if save_key && e.key() == Key::Character("s".to_string()) {
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
                                    }
                                    // ── Divider ─────────────────────────────────────
                                    div { class: "editor-divider" }
                                    // ── Right: live preview ──────────────────────────
                                    div { class: "preview-pane",
                                        div {
                                            class: "markdown-body",
                                            dangerous_inner_html: "{render_html(&edit_body.read())}",
                                        }
                                    }
                                }
                            },
                        }
                    }
                },
            }
        }
    }
}
