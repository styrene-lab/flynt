use codex_core::store::VaultStore;
use comrak::{Options, markdown_to_html};
use dioxus::prelude::*;
use crate::{bootstrap::AppContext, state::{Route, TabState}};

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
    postprocess_html(markdown_to_html(&preprocess(content), &opts))
}

/// Replace `href="codex-note://slug"` with a data attribute so the WebView
/// never navigates externally — our JS click listener handles it instead.
fn postprocess_html(html: String) -> String {
    let pattern = "href=\"codex-note://";
    let mut result = String::with_capacity(html.len());
    let mut rest   = html.as_str();
    while let Some(idx) = rest.find(pattern) {
        result.push_str(&rest[..idx]);
        let after = &rest[idx + pattern.len()..];
        if let Some(end) = after.find('"') {
            let slug = &after[..end];
            result.push_str("href=\"#\" data-codex-note=\"");
            result.push_str(slug);
            result.push('"');
            rest = &after[end + 1..];
        } else {
            result.push_str(&rest[idx..]);
            break;
        }
    }
    result.push_str(rest);
    result
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

// ── Notes view ────────────────────────────────────────────────────────────────

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

    // Wire wikilink navigation: JS sends "codex-note://slug" via dioxus.send()
    let ctx_link     = ctx.clone();
    let mut ts_link  = tab_state;
    let mut ar_link  = use_context::<Signal<Route>>();
    use_effect(move || {
        let mut eval = document::eval("dioxus.recv().then(function(msg){ dioxus.send(msg); });");
        let c  = ctx_link.clone();
        spawn(async move {
            loop {
                if let Ok(val) = eval.recv::<String>().await {
                    let slug = val
                        .trim_start_matches("codex-note://")
                        .replace("%20", " ")
                        .to_lowercase();
                    let c2 = c.clone();
                    if let Ok(Some(meta)) = tokio::task::spawn_blocking(move || {
                        c2.vault.store.find_document_by_slug(&slug)
                    }).await.unwrap_or(Ok(None)) {
                        ts_link.write().open(meta.id.clone(), meta.title.clone());
                        *ar_link.write() = Route::Notes;
                    }
                } else {
                    break;
                }
            }
        });
    });

    // No tab open → prompt
    if !has_active {
        return rsx! {
            div { class: "notes-empty",
                p { class: "muted", "Select a note from the sidebar." }
            }
        };
    }

    // Tab open but content not yet loaded
    let Some(data) = &*rendered.read() else {
        return rsx! {
            div { class: "notes-loading muted", "Loading…" }
        };
    };

    // Tab open but document not found in store
    let Some((rel_path, title, _body, html)) = data else {
        return rsx! {
            div { class: "notes-empty",
                p { class: "muted", "Document not found." }
            }
        };
    };

    let title = title.clone();
    let html  = html.clone();
    let path  = rel_path.clone();

    rsx! {
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
                                    let content = edit_body.read().clone();
                                    let p       = path.clone();
                                    let c       = ctx_save1.clone();
                                    let mut re  = rendered;
                                    spawn(async move {
                                        match tokio::task::spawn_blocking(move || {
                                            c.vault.save_document_content(&p, &content)
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
                    { document::eval(r#"
                        // Syntax highlight
                        document.querySelectorAll('.markdown-body pre code:not([data-highlighted])').forEach(b => typeof hljs !== 'undefined' && hljs.highlightElement(b));
                        // Wire internal wikilinks (data-codex-note) — never triggers navigation
                        document.querySelectorAll('.markdown-body [data-codex-note]').forEach(function(a) {
                            if (a._codex_wired) return;
                            a._codex_wired = true;
                            a.addEventListener('click', function(e) {
                                e.preventDefault();
                                dioxus.send('codex-note://' + decodeURIComponent(a.dataset.codexNote));
                            });
                        });
                        // Hover preview in footer
                        const footer = document.getElementById('codex-link-preview');
                        if (footer) {
                            document.querySelectorAll('.markdown-body a').forEach(function(a) {
                                if (a._preview_wired) return;
                                a._preview_wired = true;
                                a.addEventListener('mouseenter', function() {
                                    const note = a.dataset.codexNote;
                                    footer.textContent = note ? '→ ' + decodeURIComponent(note) : a.href;
                                    footer.classList.add('visible');
                                });
                                a.addEventListener('mouseleave', function() {
                                    footer.classList.remove('visible');
                                    footer.textContent = '';
                                });
                            });
                        }
                    "#); }
                    div { class: "markdown-body", dangerous_inner_html: "{html}" }
                    div { id: "codex-link-preview", class: "link-preview-bar" }
                },
                EditMode::Edit => {
                    let path_save = rel_path.clone();
                    rsx! {
                        { document::eval(r#"(function(){
                            const ed=document.getElementById('codex-editor');
                            const pr=document.getElementById('codex-preview');
                            if(typeof hljs!=='undefined') pr&&pr.querySelectorAll('pre code:not([data-highlighted])').forEach(b=>hljs.highlightElement(b));
                            if(!ed||!pr||ed._codex_bound)return;
                            ed._codex_bound=true;
                            let busy=false;
                            ed.addEventListener('scroll',function(){if(busy)return;busy=true;const p=ed.scrollTop/Math.max(1,ed.scrollHeight-ed.clientHeight);pr.scrollTop=p*(pr.scrollHeight-pr.clientHeight);requestAnimationFrame(()=>busy=false);});
                            pr.addEventListener('scroll',function(){if(busy)return;busy=true;const p=pr.scrollTop/Math.max(1,pr.scrollHeight-pr.clientHeight);ed.scrollTop=p*(ed.scrollHeight-ed.clientHeight);requestAnimationFrame(()=>busy=false);});
                        })();"#); }
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
                                            let content = edit_body.read().clone();
                                            let p       = path_save.clone();
                                            let c       = ctx_save2.clone();
                                            let mut re  = rendered;
                                            spawn(async move {
                                                match tokio::task::spawn_blocking(move || {
                                                    c.vault.save_document_content(&p, &content)
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
                    }
                },
            }
        }
    }
}
