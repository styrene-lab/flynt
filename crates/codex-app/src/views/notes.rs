use codex_core::store::VaultStore;
use comrak::{Options, markdown_to_html};
use dioxus::prelude::*;
use crate::{bootstrap::AppContext, state::TabState};

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

fn preprocess(src: &str) -> String {
    let mut out = String::with_capacity(src.len() + 64);
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '!' && chars.peek() == Some(&'[') {
            chars.next();
            if chars.peek() == Some(&'[') {
                chars.next();
                let inner: String = chars.by_ref().take_while(|&ch| ch != ']').collect();
                if chars.peek() == Some(&']') { chars.next(); }
                let ext = std::path::Path::new(&inner)
                    .extension().and_then(|e| e.to_str()).unwrap_or("");
                if matches!(ext, "png"|"jpg"|"jpeg"|"gif"|"svg"|"webp") {
                    let encoded = inner.replace(' ', "%20");
                    out.push_str(&format!("![{inner}](vault://localhost/{encoded})"));
                } else {
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
pub fn NotesView() -> Element {
    let ctx       = use_context::<AppContext>();
    let tab_state = use_context::<Signal<TabState>>();

    let ctx_res   = ctx.clone();
    let ctx_save1 = ctx.clone();
    let ctx_save2 = ctx.clone();

    let mut mode      = use_signal(|| EditMode::Preview);
    let mut edit_body = use_signal(String::new);
    let mut save_err  = use_signal(|| Option::<String>::None);

    let rendered = use_resource(move || {
        let selected_id = tab_state.read().active_id().cloned();
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

    use_effect(move || {
        if let Some(Some((_, _, body, _))) = &*rendered.read() {
            *edit_body.write() = body.clone();
            *mode.write() = EditMode::Preview;
        }
    });

    let has_active = tab_state.read().active_id().is_some();

    rsx! {
        div { class: "view-notes",
            match &*rendered.read() {
                None if has_active => rsx! {
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
                                { document::eval("document.querySelectorAll('.markdown-body pre code:not([data-highlighted])').forEach(b => typeof hljs !== 'undefined' && hljs.highlightElement(b))"); }
                                div {
                                    class: "markdown-body",
                                    dangerous_inner_html: "{html}",
                                }
                            },
                            EditMode::Edit => rsx! {
                                { document::eval(r#"
                                    (function() {
                                        const ed = document.getElementById('codex-editor');
                                        const pr = document.getElementById('codex-preview');
                                        if (typeof hljs !== 'undefined') {
                                            pr && pr.querySelectorAll('pre code:not([data-highlighted])').forEach(b => hljs.highlightElement(b));
                                        }
                                        if (!ed || !pr || ed._codex_bound) return;
                                        ed._codex_bound = true;
                                        let busy = false;
                                        ed.addEventListener('scroll', function() {
                                            if (busy) return; busy = true;
                                            const pct = ed.scrollTop / Math.max(1, ed.scrollHeight - ed.clientHeight);
                                            pr.scrollTop = pct * (pr.scrollHeight - pr.clientHeight);
                                            requestAnimationFrame(() => busy = false);
                                        });
                                        pr.addEventListener('scroll', function() {
                                            if (busy) return; busy = true;
                                            const pct = pr.scrollTop / Math.max(1, pr.scrollHeight - pr.clientHeight);
                                            ed.scrollTop = pct * (ed.scrollHeight - ed.clientHeight);
                                            requestAnimationFrame(() => busy = false);
                                        });
                                    })();
                                "#); }
                                div { class: "editor-split",
                                    div { class: "editor-pane",
                                        textarea {
                                            id: "codex-editor",
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
                                    div { class: "editor-divider" }
                                    div {
                                        id: "codex-preview",
                                        class: "preview-pane",
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
