use codex_core::{models::DocumentId, store::VaultStore};
use comrak::{Options, markdown_to_html};
use dioxus::prelude::*;
use once_cell::sync::Lazy;
use syntect::{
    highlighting::{Theme, ThemeSet},
    html::highlighted_html_for_string,
    parsing::SyntaxSet,
};
use crate::bootstrap::AppContext;

// ── Syntax highlighting globals (loaded once, reused) ─────────────────────
static SS: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);
static THEME: Lazy<Theme> = Lazy::new(|| {
    ThemeSet::load_defaults().themes["base16-ocean.dark"].clone()
});

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
    let raw_html  = markdown_to_html(&processed, &opts);
    highlight_code_blocks(&raw_html)
}

/// Post-process comrak's HTML output: replace <pre><code class="language-X">…</code></pre>
/// with syntect-highlighted HTML.
fn highlight_code_blocks(html: &str) -> String {
    // Simple tag-based scanner — avoids pulling in a full HTML parser.
    const OPEN: &str  = "<pre><code";
    const CLOSE: &str = "</code></pre>";
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find(OPEN) {
        out.push_str(&rest[..start]);
        rest = &rest[start + OPEN.len()..];
        // Extract lang from class="language-X"
        let lang = if rest.starts_with(" class=\"language-") {
            let after = &rest[" class=\"language-".len()..];
            let end   = after.find('"').unwrap_or(0);
            &after[..end]
        } else { "" };
        // Advance past the > that closes the opening <code> tag
        let gt = rest.find('>').unwrap_or(0);
        rest = &rest[gt + 1..];
        let end = rest.find(CLOSE).unwrap_or(rest.len());
        let code_html_encoded = &rest[..end];
        rest = &rest[end + CLOSE.len()..];
        // Decode HTML entities (&amp; &lt; &gt; &quot;)
        let code = code_html_encoded
            .replace("&lt;",  "<")
            .replace("&gt;",  ">")
            .replace("&amp;", "&")
            .replace("&quot;", "\"");
        let syntax = SS.find_syntax_by_token(lang)
            .or_else(|| SS.find_syntax_by_first_line(&code))
            .unwrap_or_else(|| SS.find_syntax_plain_text());
        match highlighted_html_for_string(&code, &SS, syntax, &THEME) {
            Ok(highlighted) => out.push_str(&highlighted),
            Err(_)          => {
                out.push_str(OPEN);
                out.push_str(code_html_encoded);
                out.push_str(CLOSE);
            }
        }
    }
    out.push_str(rest);
    out
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
