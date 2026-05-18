use crate::{
    bootstrap::AppContext,
    state::{NoteHistoryCommand, NoteInspectorCommand, NoteInspectorTarget, Route, TabState},
};
use comrak::{Options, markdown_to_html};
use dioxus::prelude::*;
use flynt_core::models::{DocumentMeta, Frontmatter};
use flynt_core::parser::parse_document_source;
use flynt_core::store::ProjectStore;
use flynt_store::sync::git::{FileHistoryEntry, FileSnapshot, GitSync};
use std::time::Duration;

#[derive(Clone, PartialEq)]
enum EditMode {
    Live,
    Source,
}

#[derive(Clone, PartialEq)]
#[allow(dead_code)] // Dirty is set via JS DOM manipulation, not Rust
enum SaveState {
    Clean,
    Dirty,
    Saved,
}

#[derive(Clone, Copy, PartialEq)]
enum InspectorTab {
    Links,
    Outline,
    Properties,
}

#[derive(Clone, PartialEq, Debug)]
struct NoteHeading {
    level: usize,
    title: String,
    anchor: String,
    line: usize,
}

#[derive(Clone, PartialEq, Debug, Default)]
struct LinkContext {
    backlinks: Vec<DocumentMeta>,
    outgoing: Vec<OutgoingLinkContext>,
    aliases: Vec<String>,
    resolved_count: usize,
    missing_count: usize,
}

#[derive(Clone, PartialEq, Debug)]
struct OutgoingLinkContext {
    target: String,
    display: Option<String>,
    anchor: Option<String>,
    resolved: Option<DocumentMeta>,
    count: usize,
}

#[derive(Clone, PartialEq, Debug)]
struct HistoryPanelState {
    entries: Vec<FileHistoryEntry>,
    error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HistoryDiffKind {
    Context,
    Added,
    Removed,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct HistoryDiffLine {
    kind: HistoryDiffKind,
    old_line: Option<usize>,
    new_line: Option<usize>,
    text: String,
}

fn build_line_diff(old: &str, new: &str) -> Vec<HistoryDiffLine> {
    let old_lines = old.lines().collect::<Vec<_>>();
    let new_lines = new.lines().collect::<Vec<_>>();
    let mut lcs = vec![vec![0usize; new_lines.len() + 1]; old_lines.len() + 1];

    for i in (0..old_lines.len()).rev() {
        for j in (0..new_lines.len()).rev() {
            lcs[i][j] = if old_lines[i] == new_lines[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut out = Vec::new();
    let mut i = 0usize;
    let mut j = 0usize;
    while i < old_lines.len() || j < new_lines.len() {
        if i < old_lines.len() && j < new_lines.len() && old_lines[i] == new_lines[j] {
            out.push(HistoryDiffLine {
                kind: HistoryDiffKind::Context,
                old_line: Some(i + 1),
                new_line: Some(j + 1),
                text: old_lines[i].to_string(),
            });
            i += 1;
            j += 1;
        } else if j < new_lines.len() && (i == old_lines.len() || lcs[i][j + 1] >= lcs[i + 1][j]) {
            out.push(HistoryDiffLine {
                kind: HistoryDiffKind::Added,
                old_line: None,
                new_line: Some(j + 1),
                text: new_lines[j].to_string(),
            });
            j += 1;
        } else if i < old_lines.len() {
            out.push(HistoryDiffLine {
                kind: HistoryDiffKind::Removed,
                old_line: Some(i + 1),
                new_line: None,
                text: old_lines[i].to_string(),
            });
            i += 1;
        }
    }

    out
}

fn build_link_context(
    backlinks: Vec<DocumentMeta>,
    body: &str,
    frontmatter: &Frontmatter,
    mut resolve: impl FnMut(&str) -> Option<DocumentMeta>,
) -> LinkContext {
    let (_, _, links) = parse_document_source(body);
    let mut outgoing = Vec::<OutgoingLinkContext>::new();

    for link in links {
        if let Some(existing) = outgoing.iter_mut().find(|existing| {
            existing.target.eq_ignore_ascii_case(&link.target)
                && existing.anchor == link.anchor
                && existing.display == link.display
        }) {
            existing.count += 1;
            continue;
        }

        let resolved = resolve(&link.target);
        outgoing.push(OutgoingLinkContext {
            target: link.target,
            display: link.display,
            anchor: link.anchor,
            resolved,
            count: 1,
        });
    }

    let resolved_count = outgoing
        .iter()
        .filter(|link| link.resolved.is_some())
        .count();
    let missing_count = outgoing.len().saturating_sub(resolved_count);

    LinkContext {
        backlinks,
        outgoing,
        aliases: frontmatter.aliases.clone(),
        resolved_count,
        missing_count,
    }
}

fn extract_headings(markdown: &str) -> Vec<NoteHeading> {
    let mut headings = Vec::new();
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut in_fence = false;

    for (idx, line) in markdown.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || !trimmed.starts_with('#') {
            continue;
        }

        let marker_len = trimmed.chars().take_while(|c| *c == '#').count();
        if marker_len == 0 || marker_len > 6 {
            continue;
        }
        let after = &trimmed[marker_len..];
        if !after.starts_with(' ') {
            continue;
        }
        let title = after.trim().trim_end_matches('#').trim().to_string();
        if title.is_empty() {
            continue;
        }

        let base = heading_anchor(&title);
        let count = seen.entry(base.clone()).or_insert(0);
        let anchor = if *count == 0 {
            base
        } else {
            format!("{base}-{}", *count + 1)
        };
        *count += 1;
        headings.push(NoteHeading {
            level: marker_len,
            title,
            anchor,
            line: idx + 1,
        });
    }

    headings
}

fn heading_anchor(title: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in title.chars().flat_map(|c| c.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if ch.is_whitespace() || ch == '-' {
            if !last_dash && !out.is_empty() {
                out.push('-');
                last_dash = true;
            }
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "section".into()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_headings_skipping_fenced_code() {
        let headings = extract_headings(
            r#"# Alpha

```md
## Ignored
```

## Beta!
### Beta!
"#,
        );

        assert_eq!(
            headings,
            vec![
                NoteHeading {
                    level: 1,
                    title: "Alpha".into(),
                    anchor: "alpha".into(),
                    line: 1,
                },
                NoteHeading {
                    level: 2,
                    title: "Beta!".into(),
                    anchor: "beta".into(),
                    line: 7,
                },
                NoteHeading {
                    level: 3,
                    title: "Beta!".into(),
                    anchor: "beta-2".into(),
                    line: 8,
                },
            ]
        );
    }

    #[test]
    fn heading_anchor_normalizes_punctuation_and_whitespace() {
        assert_eq!(
            heading_anchor(" v1.1 Failover & Redundancy "),
            "v11-failover-redundancy"
        );
        assert_eq!(heading_anchor("!!!"), "section");
    }

    #[test]
    fn link_context_preserves_aliases_and_marks_missing_links() {
        let mut frontmatter = Frontmatter::default();
        frontmatter.aliases = vec!["Alpha Prime".into()];
        let resolved = DocumentMeta {
            id: flynt_core::models::DocumentId(uuid::Uuid::new_v4()),
            path: "beta.md".into(),
            title: "Beta".into(),
            tags: vec![],
            metadata: Default::default(),
            entity_kind: None,
            updated_at: chrono::Utc::now(),
        };

        let context = build_link_context(
            vec![],
            "See [[beta|Beta Display]], [[missing]], and [[beta|Beta Display]].",
            &frontmatter,
            |target| {
                target
                    .eq_ignore_ascii_case("beta")
                    .then(|| resolved.clone())
            },
        );

        assert_eq!(context.aliases, vec!["Alpha Prime"]);
        assert_eq!(context.resolved_count, 1);
        assert_eq!(context.missing_count, 1);
        assert_eq!(context.outgoing.len(), 2);
        assert_eq!(context.outgoing[0].count, 2);
        assert!(context.outgoing[0].resolved.is_some());
        assert!(context.outgoing[1].resolved.is_none());
    }

    #[test]
    fn line_diff_marks_added_removed_and_context_lines() {
        let diff = build_line_diff(
            "alpha\nbeta\ngamma\n",
            "alpha\nbeta changed\ngamma\ndelta\n",
        );

        assert_eq!(
            diff.iter().map(|line| line.kind).collect::<Vec<_>>(),
            vec![
                HistoryDiffKind::Context,
                HistoryDiffKind::Added,
                HistoryDiffKind::Removed,
                HistoryDiffKind::Context,
                HistoryDiffKind::Added,
            ]
        );
        assert_eq!(diff[0].old_line, Some(1));
        assert_eq!(diff[0].new_line, Some(1));
        assert_eq!(diff[1].old_line, None);
        assert_eq!(diff[1].new_line, Some(2));
        assert_eq!(diff[2].old_line, Some(2));
        assert_eq!(diff[2].new_line, None);
    }
}

fn render_html(content: &str) -> String {
    render_html_with_store(content, None, None)
}

fn render_html_with_store(
    content: &str,
    store: Option<&dyn flynt_core::store::ProjectStore>,
    project_root: Option<&std::path::Path>,
) -> String {
    let mut opts = Options::default();
    opts.extension.table = true;
    opts.extension.strikethrough = true;
    opts.extension.tasklist = true;
    opts.extension.autolink = true;
    opts.extension.footnotes = true;
    opts.extension.wikilinks_title_after_pipe = true;
    opts.render.unsafe_ = true;
    let mut html = postprocess_html(markdown_to_html(&preprocess(content), &opts));

    // Execute inline query blocks: <pre><code class="language-query">...</code></pre>
    if let Some(store) = store {
        while let Some(start) = html.find("<code class=\"language-query\">") {
            let code_start = start + "<code class=\"language-query\">".len();
            let Some(code_end) = html[code_start..].find("</code>") else {
                break;
            };
            let code_end = code_start + code_end;

            // Find the wrapping <pre>
            let pre_start = html[..start].rfind("<pre>").unwrap_or(start);
            let pre_end = html[code_end..]
                .find("</pre>")
                .map(|p| code_end + p + 6)
                .unwrap_or(code_end + 7);

            let query_source = html_unescape(&html[code_start..code_end]);
            let result = match flynt_core::query::execute_query(&query_source, store) {
                Ok(rendered) => format!("<div class=\"query-result\">{rendered}</div>"),
                Err(e) => format!(
                    "<div class=\"query-error\">This query could not run: {e}<br><small>Syntax: <code>TABLE title, tags FROM \"\" WHERE tags = \"#tag\" SORT title</code></small></div>"
                ),
            };

            html = format!("{}{}{}", &html[..pre_start], result, &html[pre_end..]);
        }
    }

    // Embed Excalidraw drawings: ![[file.excalidraw]] → inline SVG
    // Also handles image embeds: ![[image.png]] → <img src="project://...">
    if let Some(root) = project_root {
        // Pattern: ![[something.excalidraw]] (may appear as text or inside <p> tags)
        while let Some(start) = html.find("![[") {
            let Some(end) = html[start..].find("]]") else {
                break;
            };
            let end = start + end;
            let ref_name = &html[start + 3..end];

            if ref_name.contains(".excalidraw") {
                // Parse optional width: ![[drawing.excalidraw|400]]
                let (file_ref, width) = if let Some(pipe) = ref_name.find('|') {
                    (&ref_name[..pipe], Some(&ref_name[pipe + 1..]))
                } else {
                    (ref_name, None)
                };

                // Search for the .excalidraw file in common locations
                let candidates = [root.join(file_ref), root.join("drawings").join(file_ref)];
                let excalidraw_path = candidates
                    .iter()
                    .find(|p| p.exists())
                    .cloned()
                    .unwrap_or_else(|| root.join(file_ref));
                let svg_path = excalidraw_path.with_extension("svg");
                let style = width
                    .map(|w| format!(" style=\"max-width:{w}px\""))
                    .unwrap_or_default();
                let escaped_ref = file_ref.replace('"', "&quot;");

                let replacement = if svg_path.exists() {
                    match std::fs::read_to_string(&svg_path) {
                        Ok(svg) => format!(
                            "<div class=\"excalidraw-embed\" data-drawing=\"{escaped_ref}\"{style}>{svg}</div>"
                        ),
                        Err(_) => format!(
                            "<div class=\"excalidraw-embed-placeholder\">[Drawing: {file_ref}]</div>"
                        ),
                    }
                } else if excalidraw_path.exists() {
                    format!(
                        "<div class=\"excalidraw-embed-placeholder\" data-drawing=\"{escaped_ref}\">[Drawing: {file_ref} — open to render]</div>"
                    )
                } else {
                    format!(
                        "<span class=\"broken-embed\">Embedded file not found: {file_ref}</span>"
                    )
                };

                html = format!("{}{}{}", &html[..start], replacement, &html[end + 2..]);
            } else if ref_name.contains(".d2") {
                // D2 diagram embed — same pattern as Excalidraw: look for .svg sidecar
                let (file_ref, width) = if let Some(pipe) = ref_name.find('|') {
                    (&ref_name[..pipe], Some(&ref_name[pipe + 1..]))
                } else {
                    (ref_name, None)
                };

                let candidates = [
                    root.join(file_ref),
                    root.join("diagrams").join(file_ref),
                    root.join("drawings").join(file_ref),
                ];
                let d2_path = candidates
                    .iter()
                    .find(|p| p.exists())
                    .cloned()
                    .unwrap_or_else(|| root.join(file_ref));
                let svg_path = d2_path.with_extension("svg");
                let style = width
                    .map(|w| format!(" style=\"max-width:{w}px\""))
                    .unwrap_or_default();

                let replacement = if svg_path.exists() {
                    match std::fs::read_to_string(&svg_path) {
                        Ok(svg) => format!("<div class=\"d2-embed\"{style}>{svg}</div>"),
                        Err(_) => format!(
                            "<div class=\"d2-embed-placeholder\">[Diagram: {file_ref}]</div>"
                        ),
                    }
                } else if d2_path.exists() {
                    format!(
                        "<div class=\"d2-embed-placeholder\">[Diagram: {file_ref} — rendering not available]</div>"
                    )
                } else {
                    format!(
                        "<span class=\"broken-embed\">Diagram file not found: {file_ref}</span>"
                    )
                };

                html = format!("{}{}{}", &html[..start], replacement, &html[end + 2..]);
            } else if ref_name.ends_with(".png")
                || ref_name.ends_with(".jpg")
                || ref_name.ends_with(".jpeg")
                || ref_name.ends_with(".gif")
                || ref_name.ends_with(".svg")
                || ref_name.ends_with(".webp")
            {
                // Image embed — resolve path, searching common locations
                let image_candidates = [
                    ref_name.to_string(),
                    format!("assets/{ref_name}"),
                    format!("images/{ref_name}"),
                    format!("drawings/{ref_name}"),
                ];
                let resolved = image_candidates
                    .iter()
                    .find(|p| root.join(p).exists())
                    .cloned()
                    .unwrap_or_else(|| ref_name.to_string());
                let encoded = resolved.replace(' ', "%20");
                let replacement = format!(
                    "<img class=\"embedded-image\" src=\"project://localhost/{encoded}\" alt=\"{ref_name}\" />"
                );
                html = format!("{}{}{}", &html[..start], replacement, &html[end + 2..]);
            } else {
                break; // not an embed we handle — avoid infinite loop
            }
        }
    }

    // Replace bare external URLs with smart badges
    // Match <a href="https://...">https://...</a> (autolinked URLs where text == href)
    let mut out = String::with_capacity(html.len());
    let mut search_from = 0;
    while let Some(start) = html[search_from..].find("<a href=\"http") {
        let abs_start = search_from + start;
        if let Some(close) = html[abs_start..].find("</a>") {
            let tag_end = abs_start + close + 4;
            let tag = &html[abs_start..tag_end];
            // Extract href
            if let (Some(href_start), Some(href_end)) = (tag.find("href=\""), tag.find("\">")) {
                let href = &tag[href_start + 6..href_end];
                // Extract link text
                let text_start = href_end + 2;
                let text_end = tag.len() - 4; // before </a>
                let text = &tag[text_start..text_end];
                // Only replace if link text IS the URL (autolinked) or starts with http
                if text.starts_with("http") || text == href {
                    let ext_ref = flynt_core::external_ref::parse_ref(href);
                    if ext_ref.provider != flynt_core::external_ref::Provider::Generic {
                        let badge = flynt_core::external_ref::render_html(&ext_ref);
                        out.push_str(&html[search_from..abs_start]);
                        out.push_str(&badge);
                        search_from = tag_end;
                        continue;
                    }
                }
            }
            out.push_str(&html[search_from..tag_end]);
            search_from = tag_end;
        } else {
            break;
        }
    }
    out.push_str(&html[search_from..]);
    let html = out;

    html
}

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
        .replace("&#34;", "\"")
}

fn postprocess_html(html: String) -> String {
    let pattern = "href=\"flynt-note://";
    let mut result = String::with_capacity(html.len());
    let mut rest = html.as_str();
    while let Some(idx) = rest.find(pattern) {
        result.push_str(&rest[..idx]);
        let after = &rest[idx + pattern.len()..];
        if let Some(end) = after.find('"') {
            let slug = &after[..end];
            result.push_str("href=\"#\" data-flynt-note=\"");
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
                if chars.peek() == Some(&']') {
                    chars.next();
                }
                let ext = std::path::Path::new(&inner)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                if matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp") {
                    let encoded = inner.replace(' ', "%20");
                    out.push_str(&format!("![{inner}](project://localhost/{encoded})"));
                } else {
                    let encoded = inner.replace(' ', "%20");
                    out.push_str(&format!("[{inner}](project://localhost/{encoded})"));
                }
                continue;
            } else {
                out.push('!');
                out.push('[');
            }
        } else if c == '[' && chars.peek() == Some(&'[') {
            chars.next();
            let inner: String = chars.by_ref().take_while(|&ch| ch != ']').collect();
            if chars.peek() == Some(&']') {
                chars.next();
            }
            let (target, display) = inner
                .split_once('|')
                .map(|(t, d)| (t, d))
                .unwrap_or((&inner, &inner as &str));
            let encoded = target.replace(' ', "%20");
            out.push_str(&format!("[{display}](flynt-note://{encoded})"));
            continue;
        }
        out.push(c);
    }
    out
}

// ── CM6 init JS ─────────────────────────────────────────────────────────────

pub(crate) fn cm6_fast_swap_js(content: &str) -> String {
    let escaped = serde_json::to_string(content).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"
(function() {{
    const container = document.getElementById('flynt-cm-editor');
    const cm = window._flyntCM;
    if (!container || !cm || !cm.dom || !container.contains(cm.dom)) return false;
    const next = {escaped};
    cm.dispatch({{ changes: {{ from: 0, to: cm.state.doc.length, insert: next }} }});
    cm.scrollDOM.scrollTop = 0;
    return true;
}})();
"#
    )
}

fn cm6_init_js(content: &str) -> String {
    let escaped = serde_json::to_string(content).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"
(function() {{
    function _initCM() {{
    const container = document.getElementById('flynt-cm-editor');
    if (!container) {{ setTimeout(_initCM, 16); return; }}

    console.time('cm6-total');
    // Fast path: if CM6 already exists AND its DOM is still attached
    // to the current container, swap the document content in place.
    //
    // Why the attachment check: when the operator toggles Source then
    // back to Live, Dioxus unmounts the editor div and mounts a fresh
    // one (same id, different DOM node). `window._flyntCM` still
    // references the OLD editor whose root DOM is now detached.
    // Dispatching content to that editor draws nothing — the new div
    // stays blank until a re-init. So if attachment is broken, fall
    // through to the full init path which rebuilds the editor under
    // the fresh container.
    if (window._flyntCM) {{
        const cm = window._flyntCM;
        const stillAttached = cm.dom && container.contains(cm.dom);
        if (stillAttached) {{
            console.time('cm6-swap');
            const newContent = {escaped};
            cm.dispatch({{
                changes: {{ from: 0, to: cm.state.doc.length, insert: newContent }}
            }});
            cm.scrollDOM.scrollTop = 0;
            console.timeEnd('cm6-swap');
            console.timeEnd('cm6-total');
            return;
        }} else {{
            try {{ cm.destroy(); }} catch(e) {{}}
            window._flyntCM = null;
            // fall through to full init
        }}
    }}
    console.time('cm6-init');
    container.innerHTML = '';

    const {{
        EditorView, Decoration, WidgetType, keymap, drawSelection, highlightActiveLine,
        highlightSpecialChars,
        EditorState,
        defaultKeymap, history, historyKeymap, indentWithTab,
        markdown, markdownLanguage, GFM,
        languages,
        syntaxHighlighting, defaultHighlightStyle, bracketMatching,
        oneDark,
        closeBrackets,
        searchKeymap, highlightSelectionMatches,
        HighlightStyle, tags,
        createLivePreview, createBlockRender, createFrontmatterHider,
    }} = CM;

    const livePreview = createLivePreview();
    const blockRenderPlugin = createBlockRender();
    const frontmatterPlugin = createFrontmatterHider();

    class TableWidget extends WidgetType {{
        constructor(html) {{ super(); this._html = html; }}
        toDOM() {{
            const d = document.createElement('div');
            d.className = 'cm-table-widget';
            d.innerHTML = this._html;
            return d;
        }}
        ignoreEvent() {{ return false; }}
        eq(o) {{ return this._html === o._html; }}
    }}

    const flyntTheme = EditorView.theme({{
        '&': {{
            backgroundColor: 'var(--background)',
            color: 'var(--prose-body, #d7e0ea)',
            fontSize: 'var(--font-size-md, 15px)',
        }},
        '.cm-content': {{
            caretColor: 'var(--ring, #2ab4c8)',
            padding: '0',
            fontFamily: 'var(--font-sans)',
            lineHeight: 'var(--line-height, 1.7)',
        }},
        '.cm-cursor': {{
            borderLeftColor: 'var(--ring, #2ab4c8)',
            borderLeftWidth: '2px',
        }},
        '.cm-activeLine': {{
            backgroundColor: 'rgba(255,255,255,0.03)',
        }},
        '.cm-selectionBackground, ::selection': {{
            backgroundColor: 'rgba(42, 180, 200, 0.2) !important',
        }},
        '.cm-gutters': {{ display: 'none' }},
        '.cm-scroller': {{
            overflow: 'auto',
            padding: 'var(--space-8, 32px) var(--space-10, 40px)',
        }},
        '.cm-line': {{ padding: '0 4px' }},
        '.cm-codeblock-line': {{
            backgroundColor: 'var(--prose-pre-bg, rgba(15, 23, 42, 0.8))',
            fontFamily: 'var(--font-mono)',
            fontSize: 'var(--font-size-sm, 13px)',
            lineHeight: '1.5',
            borderLeft: '3px solid var(--prose-pre-border, #1e293b)',
            paddingLeft: '12px !important',
        }},
        '.cm-codeblock-fence': {{
            backgroundColor: 'var(--prose-pre-bg, rgba(15, 23, 42, 0.8))',
            fontFamily: 'var(--font-mono)',
            fontSize: 'var(--font-size-xs, 11px)',
            color: 'var(--muted-foreground, #475569)',
            borderLeft: '3px solid var(--prose-pre-border, #1e293b)',
            paddingLeft: '12px !important',
        }},
        '.cm-codeblock-first': {{
            borderTopLeftRadius: '6px', borderTopRightRadius: '6px',
            paddingTop: '8px !important',
        }},
        '.cm-codeblock-last': {{
            borderBottomLeftRadius: '6px', borderBottomRightRadius: '6px',
            paddingBottom: '8px !important',
        }},
    }}, {{ dark: true }});

    const flyntHighlight = HighlightStyle.define([
        {{ tag: tags.heading1, fontSize: '1.8em', fontWeight: '700', color: 'var(--prose-heading, #f1f5f9)' }},
        {{ tag: tags.heading2, fontSize: '1.5em', fontWeight: '600', color: 'var(--prose-heading, #f1f5f9)' }},
        {{ tag: tags.heading3, fontSize: '1.25em', fontWeight: '600', color: 'var(--prose-heading, #f1f5f9)' }},
        {{ tag: tags.heading4, fontSize: '1.1em', fontWeight: '600', color: 'var(--prose-heading, #f1f5f9)' }},
        {{ tag: tags.heading5, fontSize: '1.05em', fontWeight: '600', color: 'var(--prose-heading, #f1f5f9)' }},
        {{ tag: tags.heading6, fontSize: '1em', fontWeight: '600', color: 'var(--prose-heading, #f1f5f9)' }},
        {{ tag: tags.processingInstruction, color: 'var(--muted-foreground, #475569)', fontSize: '0.85em' }},
        {{ tag: tags.strong, fontWeight: '700', color: 'var(--prose-heading, #f1f5f9)' }},
        {{ tag: tags.emphasis, fontStyle: 'italic' }},
        {{ tag: tags.strikethrough, textDecoration: 'line-through', color: 'var(--muted-foreground, #64748b)' }},
        {{ tag: tags.url, color: 'var(--prose-link, #4cc9f0)' }},
        {{ tag: tags.link, color: 'var(--prose-link, #4cc9f0)', textDecoration: 'underline' }},
        {{ tag: tags.monospace, fontFamily: 'var(--font-mono)', color: 'var(--prose-code-fg, #e2e8f0)', backgroundColor: 'var(--prose-code-bg, rgba(30,41,59,0.7))', borderRadius: '3px', padding: '1px 4px' }},
        {{ tag: tags.list, color: 'var(--ring, #2ab4c8)' }},
        {{ tag: tags.quote, color: 'var(--muted-foreground, #94a3b8)', fontStyle: 'italic' }},
        {{ tag: tags.meta, color: 'var(--muted-foreground, #475569)' }},
        {{ tag: tags.content, color: 'var(--prose-body, #d7e0ea)' }},
    ]);

    // ── Live preview: hide markdown punctuation on non-active lines ──
    const hideMarkupPlugin = EditorView.decorations.compute(['doc'], (state) => {{ try {{
        const decs = [];
        const doc = state.doc;

        // Performance: only hide markup on small documents
        if (doc.lines > 150) return Decoration.none;

        // Hide TOML frontmatter (+++ ... +++)
        let fmStart = -1, fmEnd = -1;
        if (doc.lines >= 1 && doc.line(1).text.trim() === '+++') {{
            fmStart = 1;
            for (let j = 2; j <= doc.lines; j++) {{
                if (doc.line(j).text.trim() === '+++') {{ fmEnd = j; break; }}
            }}
        }}
        if (fmStart > 0 && fmEnd > 0) {{
            for (let fl = fmStart; fl <= fmEnd; fl++) {{
                const fline = doc.line(fl);
                if (fline.length > 0) {{
                    decs.push(Decoration.replace({{}}).range(fline.from, fline.to));
                }}
            }}
        }}

        for (let i = 1; i <= doc.lines; i++) {{
            if (fmStart > 0 && fmEnd > 0 && i >= fmStart && i <= fmEnd) continue; // skip frontmatter lines
            const line = doc.line(i);
            const text = line.text;

            // Horizontal rule: --- or *** or ___ → styled line
            if (text.trim() === '---' || text.trim() === '***' || text.trim() === '___') {{
                decs.push(Decoration.replace({{}}).range(line.from, line.to));
                decs.push(Decoration.line({{ class: 'cm-hr-line' }}).range(line.from));
                continue;
            }}

            // Hide heading markers
            if (text.match(/^#/) && text.indexOf(' ') > 0 && text.indexOf(' ') <= 7) {{
                const spaceIdx = text.indexOf(' ');
                decs.push(Decoration.replace({{}}).range(line.from, line.from + spaceIdx + 1));
                continue;
            }}

            // Hide bold markers: **text** (max 50 iterations per line)
            let idx = 0, safety = 0;
            while ((idx = text.indexOf('**', idx)) !== -1 && safety++ < 50) {{
                const end = text.indexOf('**', idx + 2);
                if (end > idx) {{
                    decs.push(Decoration.replace({{}}).range(line.from + idx, line.from + idx + 2));
                    decs.push(Decoration.replace({{}}).range(line.from + end, line.from + end + 2));
                    idx = end + 2;
                }} else break;
            }}

            // Hide wikilink brackets: [[target]] or [[target|display]]
            idx = 0; safety = 0;
            while ((idx = text.indexOf('[[', idx)) !== -1 && safety++ < 50) {{
                const end = text.indexOf(']]', idx + 2);
                if (end > idx) {{
                    const inner = text.substring(idx + 2, end);
                    const pipe = inner.indexOf('|');
                    if (pipe >= 0) {{
                        decs.push(Decoration.replace({{}}).range(line.from + idx, line.from + idx + 2 + pipe + 1));
                        decs.push(Decoration.replace({{}}).range(line.from + end, line.from + end + 2));
                    }} else {{
                        decs.push(Decoration.replace({{}}).range(line.from + idx, line.from + idx + 2));
                        decs.push(Decoration.replace({{}}).range(line.from + end, line.from + end + 2));
                    }}
                    idx = end + 2;
                }} else break;
            }}

            // Hide inline code backticks
            idx = 0;
            safety = 0;
            while ((idx = text.indexOf('`', idx)) !== -1 && safety++ < 50) {{
                if (text.charAt(idx + 1) === '`') {{ idx += 2; continue; }}
                const end = text.indexOf('`', idx + 1);
                if (end > idx) {{
                    decs.push(Decoration.replace({{}}).range(line.from + idx, line.from + idx + 1));
                    decs.push(Decoration.replace({{}}).range(line.from + end, line.from + end + 1));
                    idx = end + 1;
                }} else break;
            }}

            // Hide underscore italic/bold: _text_ and __text__
            idx = 0; safety = 0;
            while ((idx = text.indexOf('__', idx)) !== -1 && safety++ < 50) {{
                const end = text.indexOf('__', idx + 2);
                if (end > idx) {{
                    decs.push(Decoration.replace({{}}).range(line.from + idx, line.from + idx + 2));
                    decs.push(Decoration.replace({{}}).range(line.from + end, line.from + end + 2));
                    idx = end + 2;
                }} else break;
            }}
            idx = 0;
            while (idx < text.length) {{
                idx = text.indexOf('_', idx);
                if (idx === -1) break;
                if (text.charAt(idx - 1) === '_' || text.charAt(idx + 1) === '_') {{ idx++; continue; }} // skip __
                if (idx > 0 && text.charAt(idx - 1).match(/[a-zA-Z0-9]/)) {{ idx++; continue; }} // mid-word
                const end = text.indexOf('_', idx + 1);
                if (end > idx && !(text.charAt(end - 1) === '_' || text.charAt(end + 1) === '_')) {{
                    decs.push(Decoration.replace({{}}).range(line.from + idx, line.from + idx + 1));
                    decs.push(Decoration.replace({{}}).range(line.from + end, line.from + end + 1));
                    idx = end + 1;
                }} else {{ idx++; }}
            }}

            // Tables — find full table block and replace with rendered widget
            if (text.indexOf('|') >= 0 && text.trim().charAt(0) === '|') {{
                // Find table extent
                let ts = i, te = i;
                while (ts > 1 && doc.line(ts-1).text.trim().startsWith('|')) ts--;
                while (te < doc.lines && doc.line(te+1).text.trim().startsWith('|')) te++;

                if (i === ts) {{
                    const tFrom = doc.line(ts).from;
                    const tTo = doc.line(te).to;
                    if (sel.head < tFrom || sel.head > tTo) {{
                        // Parse table
                        let rows = [], hasSep = false;
                        for (let r = ts; r <= te; r++) {{
                            const rt = doc.line(r).text.trim();
                            let allSep = true;
                            for (let c = 0; c < rt.length; c++) {{
                                if ('|-: '.indexOf(rt.charAt(c)) < 0) {{ allSep = false; break; }}
                            }}
                            if (allSep) {{ hasSep = true; continue; }}
                            rows.push(rt.split('|').slice(1,-1).map(s => s.trim()));
                        }}
                        if (rows.length > 0) {{
                            let h = '<table class="cm-rendered-table">';
                            rows.forEach((cells, ri) => {{
                                h += '<tr>';
                                const t = ri === 0 ? 'th' : 'td';
                                cells.forEach(c => {{
                                    let v = c.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
                                    h += '<'+t+'>'+v+'</'+t+'>';
                                }});
                                h += '</tr>';
                            }});
                            h += '</table>';
                            const to = Math.min(tTo + 1, doc.length);
                            decs.push(Decoration.replace({{ widget: new TableWidget(h) }}).range(tFrom, to));
                        }}
                    }}
                }}
                i = te;
                continue;
            }}

            // Hide unordered list markers: "- " or "* " or "+ " at line start
            const trimmed = text.trimStart();
            const indent = text.length - trimmed.length;
            if ((trimmed.startsWith('- ') || trimmed.startsWith('* ') || trimmed.startsWith('+ ')) && trimmed.length > 2) {{
                decs.push(Decoration.replace({{}}).range(line.from + indent, line.from + indent + 2));
            }}

            // Hide strikethrough: ~~text~~
            idx = 0;
            while ((idx = text.indexOf('~~', idx)) !== -1) {{
                const end = text.indexOf('~~', idx + 2);
                if (end > idx) {{
                    decs.push(Decoration.replace({{}}).range(line.from + idx, line.from + idx + 2));
                    decs.push(Decoration.replace({{}}).range(line.from + end, line.from + end + 2));
                    idx = end + 2;
                }} else break;
            }}
        }}

        // Hide frontmatter block (+++...+++)
        let fmState = 0; // 0=before, 1=inside, 2=done
        for (let i = 1; i <= doc.lines && fmState < 2; i++) {{
            const line = doc.line(i);
            if (line.text.trim() === '+++') {{
                if (fmState === 0) {{ fmState = 1; }}
                else {{ fmState = 2; }}
                if (i !== activeLine) {{
                    decs.push(Decoration.replace({{}}).range(line.from, Math.min(line.to + 1, doc.length)));
                }}
            }} else if (fmState === 1 && i !== activeLine) {{
                decs.push(Decoration.replace({{}}).range(line.from, Math.min(line.to + 1, doc.length)));
            }}
        }}

        // CM6 requires decorations sorted by from position
        return Decoration.set(decs, true);
    }} catch(e) {{ window._hideErr = e.message; return Decoration.none; }}
    }});

    // ── Table styling: add CSS classes to table lines ──
    // Combined decoration plugin — single pass over all lines for tables, code blocks, tasks, embeds
    const combinedPlugin = EditorView.decorations.compute(['doc'], (state) => {{
        const decs = [];
        const doc = state.doc;
        let inTable = false, isHeader = true, inCodeBlock = false;

        for (let i = 1; i <= doc.lines; i++) {{
            const line = doc.line(i);
            const text = line.text;
            const trimmed = text.trim();

            // Code blocks
            if (!inCodeBlock && trimmed.startsWith('```')) {{
                inCodeBlock = true;
                decs.push(Decoration.line({{ class: 'cm-codeblock-fence cm-codeblock-first' }}).range(line.from));
                continue;
            }} else if (inCodeBlock && trimmed.startsWith('```')) {{
                inCodeBlock = false;
                decs.push(Decoration.line({{ class: 'cm-codeblock-fence cm-codeblock-last' }}).range(line.from));
                continue;
            }} else if (inCodeBlock) {{
                decs.push(Decoration.line({{ class: 'cm-codeblock-line' }}).range(line.from));
                continue;
            }}

            // Tables
            if (trimmed.startsWith('|') && trimmed.endsWith('|')) {{
                if (!inTable) {{ inTable = true; isHeader = true; }}
                if (trimmed.match(/^\|[\s\-:|]+\|$/)) {{
                    decs.push(Decoration.line({{ class: 'cm-table-separator' }}).range(line.from));
                    isHeader = false;
                }} else if (isHeader) {{
                    decs.push(Decoration.line({{ class: 'cm-table-header' }}).range(line.from));
                }} else {{
                    decs.push(Decoration.line({{ class: 'cm-table-row' }}).range(line.from));
                }}
                continue;
            }} else {{
                inTable = false; isHeader = true;
            }}

            // Task checkboxes: - [ ] or - [x]
            const taskMatch = text.match(/^(\s*[-*]\s*)\[([ xX])\]\s/);
            if (taskMatch) {{
                const prefixLen = taskMatch[1].length;
                const checked = taskMatch[2] !== ' ';
                const replaceFrom = line.from + prefixLen;
                const replaceTo = line.from + prefixLen + 3;
                decs.push(Decoration.replace({{
                    widget: new TaskCheckWidget(checked, line.from),
                }}).range(replaceFrom, replaceTo));
                if (prefixLen > 0) {{
                    decs.push(Decoration.replace({{}}).range(line.from, line.from + prefixLen));
                }}
                continue;
            }}

            // Wikilinks: [[target]] or [[target|display]] — add link styling
            let wlIdx = 0, wlSafety = 0;
            while ((wlIdx = text.indexOf('[[', wlIdx)) !== -1 && wlSafety++ < 20) {{
                // Skip embed syntax ![[
                if (wlIdx > 0 && text[wlIdx - 1] === '!') {{ wlIdx += 2; continue; }}
                const wlEnd = text.indexOf(']]', wlIdx + 2);
                if (wlEnd > wlIdx) {{
                    decs.push(Decoration.mark({{ class: 'cm-wikilink' }}).range(line.from + wlIdx, line.from + wlEnd + 2));
                    wlIdx = wlEnd + 2;
                }} else break;
            }}

            // Embed: ![[file.excalidraw]] or ![[image.png]]
            const embedMatch = trimmed.match(/^!\[\[(.+?)\]\]$/);
            if (embedMatch) {{
                const ref = embedMatch[1];
                let type = 'other';
                if (ref.endsWith('.excalidraw')) type = 'drawing';
                else if (/\.(png|jpg|jpeg|gif|svg|webp)$/i.test(ref)) type = 'image';
                if (type !== 'other') {{
                    decs.push(Decoration.replace({{
                        widget: new EmbedWidget(ref, type),
                    }}).range(line.from, line.to));
                }}
            }}
        }}
        return Decoration.set(decs, true);
    }});

    // ── Wikilink bracket hiding — lightweight, selection-aware ──
    // Separate from combinedPlugin so structural decorations don't recompute on cursor move.
    const wikilinkHidePlugin = EditorView.decorations.compute(['doc', 'selection'], (state) => {{
        const decs = [];
        const doc = state.doc;
        const sel = state.selection.main;
        const activeLine = doc.lineAt(sel.head).number;

        for (let i = 1; i <= doc.lines; i++) {{
            if (i === activeLine) continue; // show raw syntax on active line
            const line = doc.line(i);
            const text = line.text;
            let idx = 0, safety = 0;
            while ((idx = text.indexOf('[[', idx)) !== -1 && safety++ < 20) {{
                // Skip embed syntax ![[
                if (idx > 0 && text[idx - 1] === '!') {{ idx += 2; continue; }}
                const end = text.indexOf(']]', idx + 2);
                if (end > idx) {{
                    const inner = text.substring(idx + 2, end);
                    const pipe = inner.indexOf('|');
                    // Hide opening [[
                    decs.push(Decoration.replace({{}}).range(line.from + idx, line.from + idx + 2));
                    // For [[target|display]], also hide target and pipe
                    if (pipe >= 0) {{
                        decs.push(Decoration.replace({{}}).range(line.from + idx + 2, line.from + idx + 2 + pipe + 1));
                    }}
                    // Hide closing ]]
                    decs.push(Decoration.replace({{}}).range(line.from + end, line.from + end + 2));
                    idx = end + 2;
                }} else break;
            }}
        }}
        return Decoration.set(decs, true);
    }});

    // Legacy — kept for reference but NOT used (replaced by combinedPlugin)
    const tablePlugin_unused = EditorView.decorations.compute(['doc'], (state) => {{
        const decs = [];
        const doc = state.doc;
        let inTable = false;
        let isHeader = true;
        for (let i = 1; i <= doc.lines; i++) {{
            const line = doc.line(i);
            const t = line.text.trim();
            if (t.startsWith('|') && t.endsWith('|')) {{
                if (!inTable) {{ inTable = true; isHeader = true; }}
                // Separator line (|---|---|)
                if (t.match(/^\|[\s\-:|]+\|$/)) {{
                    decs.push(Decoration.line({{ class: 'cm-table-sep' }}).range(line.from));
                    isHeader = false;
                }} else if (isHeader) {{
                    decs.push(Decoration.line({{ class: 'cm-table-header' }}).range(line.from));
                }} else {{
                    decs.push(Decoration.line({{ class: 'cm-table-row' }}).range(line.from));
                }}
            }} else {{
                inTable = false;
                isHeader = true;
            }}
        }}
        return Decoration.set(decs);
    }});

    const codeBlockPlugin = EditorView.decorations.compute(['doc'], (state) => {{
        const decorations = [];
        const doc = state.doc;
        let inBlock = false;
        for (let i = 1; i <= doc.lines; i++) {{
            const line = doc.line(i);
            const text = line.text.trimStart();
            if (!inBlock && text.startsWith('```')) {{
                inBlock = true;
                decorations.push(Decoration.line({{ class: 'cm-codeblock-fence cm-codeblock-first' }}).range(line.from));
            }} else if (inBlock && text.startsWith('```')) {{
                inBlock = false;
                decorations.push(Decoration.line({{ class: 'cm-codeblock-fence cm-codeblock-last' }}).range(line.from));
            }} else if (inBlock) {{
                decorations.push(Decoration.line({{ class: 'cm-codeblock-line' }}).range(line.from));
            }}
        }}
        return Decoration.set(decorations);
    }});

    // Task list plugin: render - [ ] and - [x] as checkboxes
    class TaskCheckWidget extends WidgetType {{
        constructor(checked, lineFrom) {{ super(); this._checked = checked; this._lineFrom = lineFrom; }}
        eq(o) {{ return this._checked === o._checked; }}
        toDOM() {{
            const cb = document.createElement('input');
            cb.type = 'checkbox';
            cb.checked = this._checked;
            cb.className = 'cm-task-checkbox';
            cb.onclick = (e) => {{
                e.preventDefault();
                const view = window._flyntCM;
                if (!view) return;
                const line = view.state.doc.lineAt(this._lineFrom);
                const text = line.text;
                const newText = this._checked
                    ? text.replace('[x]', '[ ]').replace('[X]', '[ ]')
                    : text.replace('[ ]', '[x]');
                view.dispatch({{ changes: {{ from: line.from, to: line.to, insert: newText }} }});
                window._flyntNotify('edit', view.state.doc.toString());
            }};
            return cb;
        }}
    }}
    const taskListPlugin = EditorView.decorations.compute(['doc'], (state) => {{
        const decs = [];
        for (let i = 1; i <= state.doc.lines; i++) {{
            const line = state.doc.line(i);
            const text = line.text;
            // Skip if cursor is on this line
            const m = text.match(/^(\s*[-*]\s*)\[([ xX])\]\s/);
            if (m) {{
                const prefixLen = m[1].length;
                const checked = m[2] !== ' ';
                // Replace "- [ ] " or "- [x] " with checkbox widget
                const replaceFrom = line.from + prefixLen;
                const replaceTo = line.from + prefixLen + 3; // [x] or [ ]
                decs.push(Decoration.replace({{
                    widget: new TaskCheckWidget(checked, line.from),
                }}).range(replaceFrom, replaceTo));
                // Also hide the leading "- "
                if (prefixLen > 0) {{
                    decs.push(Decoration.replace({{}}).range(line.from, line.from + prefixLen));
                }}
            }}
        }}
        return Decoration.set(decs.sort((a, b) => a.from - b.from));
    }});

    // Embed plugin: render ![[file.excalidraw]] and ![[image.png]] as widgets
    class EmbedWidget extends WidgetType {{
        constructor(ref, type) {{ super(); this._ref = ref; this._type = type; }}
        eq(o) {{ return this._ref === o._ref; }}
        toDOM() {{
            const d = document.createElement('span');
            if (this._type === 'drawing') {{
                d.className = 'cm-embed-chip cm-embed-drawing';
                d.textContent = '\u{{1f4d0}} ' + this._ref.replace('.excalidraw', '');
                d.title = 'Click to open drawing';
                d.onclick = () => window._flyntNotify('open-drawing', this._ref);
            }} else {{
                // Image — try to render inline
                const img = document.createElement('img');
                img.className = 'cm-embed-image';
                img.src = 'project://localhost/' + encodeURIComponent(this._ref).replace(/%2F/g, '/');
                img.alt = this._ref;
                img.onerror = () => {{
                    // Try common subdirs
                    const dirs = ['assets/', 'images/', 'drawings/'];
                    let tried = 0;
                    function tryNext() {{
                        if (tried >= dirs.length) {{ img.replaceWith(document.createTextNode('[Image: ' + img.alt + ']')); return; }}
                        img.src = 'project://localhost/' + dirs[tried++] + encodeURIComponent(img.alt).replace(/%2F/g, '/');
                    }}
                    img.onerror = tryNext;
                    tryNext();
                }};
                d.appendChild(img);
            }}
            return d;
        }}
    }}
    const embedPlugin = EditorView.decorations.compute(['doc'], (state) => {{
        const decs = [];
        for (let i = 1; i <= state.doc.lines; i++) {{
            const line = state.doc.line(i);
            const text = line.text.trim();
            // Skip if cursor is on this line (let user edit the raw text)
            const m = text.match(/^!\[\[(.+?)\]\]$/);
            if (m) {{
                const ref = m[1];
                let type = 'other';
                if (ref.endsWith('.excalidraw')) type = 'drawing';
                else if (/\.(png|jpg|jpeg|gif|svg|webp)$/i.test(ref)) type = 'image';
                if (type !== 'other') {{
                    decs.push(Decoration.replace({{
                        widget: new EmbedWidget(ref, type),
                    }}).range(line.from, line.to));
                }}
            }}
        }}
        return Decoration.set(decs);
    }});

    // Save on blur / visibility change — never lose content
    document.addEventListener('visibilitychange', () => {{
        if (document.hidden && window._flyntCM) {{
            window._flyntNotify('autosave', window._flyntCM.state.doc.toString());
        }}
    }});
    window.addEventListener('blur', () => {{
        if (window._flyntCM) {{
            window._flyntNotify('autosave', window._flyntCM.state.doc.toString());
        }}
    }});

    // Formatting shortcuts: wrap selection with markdown syntax
    function wrapSelection(view, before, after) {{
        const sel = view.state.selection.main;
        const selected = view.state.sliceDoc(sel.from, sel.to);
        // If already wrapped, unwrap
        if (selected.startsWith(before) && selected.endsWith(after)) {{
            view.dispatch({{ changes: {{ from: sel.from, to: sel.to, insert: selected.slice(before.length, -after.length) }} }});
        }} else {{
            view.dispatch({{ changes: {{ from: sel.from, to: sel.to, insert: before + selected + after }} }});
        }}
        return true;
    }}
    const formatKeymap = keymap.of([
        {{ key: 'Mod-b', run: (v) => wrapSelection(v, '**', '**') }},
        {{ key: 'Mod-i', run: (v) => wrapSelection(v, '*', '*') }},
        {{ key: 'Mod-k', run: (v) => {{
            const sel = v.state.selection.main;
            const selected = v.state.sliceDoc(sel.from, sel.to);
            v.dispatch({{ changes: {{ from: sel.from, to: sel.to, insert: '[' + selected + '](url)' }} }});
            return true;
        }} }},
    ]);

    let saveTimer = null;
    let editTimer = null;
    const changeHandler = EditorView.updateListener.of((update) => {{
        if (update.docChanged) {{
            clearTimeout(saveTimer);
            clearTimeout(editTimer);
            // Defer toString() into the timeout — avoid blocking on large pastes
            editTimer = setTimeout(() => {{
                if (window._flyntCM) window._flyntNotify('edit', window._flyntCM.state.doc.toString());
            }}, 300);
            saveTimer = setTimeout(() => {{
                if (window._flyntCM) window._flyntNotify('autosave', window._flyntCM.state.doc.toString());
            }}, 1500);
        }}
    }});

    const saveKeymap = keymap.of([{{
        key: 'Mod-s',
        run: (view) => {{
            window._flyntNotify('save', view.state.doc.toString());
            return true;
        }},
    }}, {{
        key: 'Mod-e',
        run: () => {{
            window._flyntNotify('mode', 'source');
            return true;
        }},
    }}]);

    // Context menu action dispatcher
    window._flyntCtxAction = function(id, view) {{
        const sel = view.state.selection.main;
        const selected = view.state.sliceDoc(sel.from, sel.to);
        const line = view.state.doc.lineAt(sel.head);

        function wrap(before, after) {{
            if (selected.startsWith(before) && selected.endsWith(after)) {{
                view.dispatch({{ changes: {{ from: sel.from, to: sel.to, insert: selected.slice(before.length, -after.length) }} }});
            }} else {{
                view.dispatch({{ changes: {{ from: sel.from, to: sel.to, insert: before + selected + after }} }});
            }}
        }}

        function insertAtLineStart(prefix) {{
            const text = line.text;
            // If line already has this prefix, remove it
            if (text.startsWith(prefix)) {{
                view.dispatch({{ changes: {{ from: line.from, to: line.from + prefix.length, insert: '' }} }});
            }} else {{
                // Remove any existing heading prefix first
                const hm = text.match(/^#{{1,6}}\s/);
                const remove = hm ? hm[0].length : 0;
                view.dispatch({{ changes: {{ from: line.from, to: line.from + remove, insert: prefix }} }});
            }}
        }}

        function insertBlock(text) {{
            // Insert at end of current line with newlines
            const pos = line.to;
            view.dispatch({{ changes: {{ from: pos, insert: '\n' + text + '\n' }}, selection: {{ anchor: pos + 1 + text.length }} }});
        }}

        switch (id) {{
            case 'bold':      wrap('**', '**'); break;
            case 'italic':    wrap('*', '*'); break;
            case 'code':      wrap('`', '`'); break;
            case 'strike':    wrap('~~', '~~'); break;
            case 'link':      view.dispatch({{ changes: {{ from: sel.from, to: sel.to, insert: '[' + selected + '](url)' }} }}); break;
            case 'wikilink':  wrap('[[', ']]'); break;
            case 'h1':        insertAtLineStart('# '); break;
            case 'h2':        insertAtLineStart('## '); break;
            case 'h3':        insertAtLineStart('### '); break;
            case 'bullet':    insertAtLineStart('- '); break;
            case 'task':      insertAtLineStart('- [ ] '); break;
            case 'quote':     insertAtLineStart('> '); break;
            case 'codeblock': insertBlock('```\n\n```'); break;
            case 'table':     insertBlock('| Column 1 | Column 2 | Column 3 |\n| --- | --- | --- |\n|  |  |  |'); break;
            case 'hr':        insertBlock('---'); break;
        }}
        view.focus();
        // Notify edit
        clearTimeout(saveTimer);
        clearTimeout(editTimer);
        editTimer = setTimeout(() => {{
            if (window._flyntCM) window._flyntNotify('edit', window._flyntCM.state.doc.toString());
        }}, 300);
        saveTimer = setTimeout(() => {{
            if (window._flyntCM) window._flyntNotify('autosave', window._flyntCM.state.doc.toString());
        }}, 1500);
    }};

    const docText = {escaped};
    // Place cursor after frontmatter (first blank line after +++ closing)
    let cursorPos = docText.length;
    const fmMatch = docText.match(/^\+\+\+\n[\s\S]*?\n\+\+\+\n/);
    if (fmMatch) {{
        cursorPos = fmMatch[0].length;
        // Skip any blank lines after frontmatter
        while (cursorPos < docText.length && docText[cursorPos] === '\n') cursorPos++;
    }}
    const state = EditorState.create({{
        doc: docText,
        selection: {{ anchor: cursorPos }},
        extensions: [
            flyntTheme,
            syntaxHighlighting(flyntHighlight),
            oneDark,
            syntaxHighlighting(defaultHighlightStyle, {{ fallback: true }}),
            markdown({{ base: markdownLanguage, codeLanguages: languages, extensions: GFM }}),
            history(),
            drawSelection(),
            highlightActiveLine(),
            highlightSpecialChars(),
            highlightSelectionMatches(),
            bracketMatching(),
            closeBrackets(),
            keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap, indentWithTab]),
            saveKeymap,
            formatKeymap,
            changeHandler,
            livePreview,
            blockRenderPlugin,
            frontmatterPlugin,
            // Click wikilink to navigate; uses document text at click position
            EditorView.domEventHandlers({{
                click(event, view) {{
                    const old = document.getElementById('flynt-ctx-menu');
                    if (old) old.remove();

                    // Get position from click coordinates (works regardless of decorations)
                    const pos = view.posAtCoords({{ x: event.clientX, y: event.clientY }});
                    if (pos === null) return;
                    const line = view.state.doc.lineAt(pos);
                    const text = line.text;
                    const colInLine = pos - line.from;
                    // Check if click landed inside a [[wikilink]]
                    let idx = 0;
                    let found = false;
                    while ((idx = text.indexOf('[[', idx)) !== -1) {{
                        const end = text.indexOf(']]', idx + 2);
                        if (end > idx) {{
                            const absFrom = line.from + idx;
                            const absTo = line.from + end + 2;
                            if (pos >= absFrom && pos <= absTo) {{
                                const inner = text.substring(idx + 2, end);
                                const pipe = inner.indexOf('|');
                                const linkTarget = pipe >= 0 ? inner.substring(0, pipe) : inner;
                                window._flyntNotify('nav', linkTarget.trim());
                                found = true;
                                break;
                            }}
                            idx = end + 2;
                        }} else break;
                    }}
                    if (found) return true;
                }},
                contextmenu(event) {{
                    event.preventDefault();
                    // Remove old menu
                    const old = document.getElementById('flynt-ctx-menu');
                    if (old) old.remove();

                    const view = window._flyntCM;
                    if (!view) return true;

                    const sel = view.state.selection.main;
                    const hasSelection = sel.from !== sel.to;

                    const menu = document.createElement('div');
                    menu.id = 'flynt-ctx-menu';
                    menu.className = 'ctx-menu';
                    menu.style.cssText = `left:${{event.clientX}}px;top:${{event.clientY}}px;position:fixed;z-index:1000;`;

                    const items = [
                        ...(hasSelection ? [
                            {{ id: 'bold',      label: 'Bold',           key: '\u{{2318}}B' }},
                            {{ id: 'italic',    label: 'Italic',         key: '\u{{2318}}I' }},
                            {{ id: 'code',      label: 'Inline Code',    key: '' }},
                            {{ id: 'strike',    label: 'Strikethrough',  key: '' }},
                            {{ id: 'link',      label: 'Link',           key: '\u{{2318}}K' }},
                            {{ id: 'wikilink',  label: 'Wikilink',       key: '' }},
                            {{ id: 'sep' }},
                        ] : []),
                        {{ id: 'h1',        label: 'Heading 1',      key: '' }},
                        {{ id: 'h2',        label: 'Heading 2',      key: '' }},
                        {{ id: 'h3',        label: 'Heading 3',      key: '' }},
                        {{ id: 'sep' }},
                        {{ id: 'bullet',    label: 'Bullet List',    key: '' }},
                        {{ id: 'task',      label: 'Task List',      key: '' }},
                        {{ id: 'quote',     label: 'Blockquote',     key: '' }},
                        {{ id: 'codeblock', label: 'Code Block',     key: '' }},
                        {{ id: 'table',     label: 'Table',          key: '' }},
                        {{ id: 'hr',        label: 'Horizontal Rule', key: '' }},
                    ];

                    items.forEach(it => {{
                        if (it.id === 'sep') {{
                            const sep = document.createElement('div');
                            sep.className = 'ctx-menu-sep';
                            menu.appendChild(sep);
                            return;
                        }}
                        const btn = document.createElement('button');
                        btn.className = 'ctx-menu-item';
                        btn.innerHTML = it.key ? `<span>${{it.label}}</span><span class="ctx-menu-key">${{it.key}}</span>` : it.label;
                        btn.onclick = () => {{
                            menu.remove();
                            overlay.remove();
                            _flyntCtxAction(it.id, view);
                        }};
                        menu.appendChild(btn);
                    }});

                    const overlay = document.createElement('div');
                    overlay.className = 'ctx-menu-overlay';
                    overlay.onclick = () => {{ menu.remove(); overlay.remove(); }};
                    document.body.appendChild(overlay);
                    document.body.appendChild(menu);

                    // Clamp to viewport
                    requestAnimationFrame(() => {{
                        const r = menu.getBoundingClientRect();
                        if (r.right > window.innerWidth) menu.style.left = Math.max(8, window.innerWidth - r.width - 8) + 'px';
                        if (r.bottom > window.innerHeight) menu.style.top = Math.max(8, window.innerHeight - r.height - 8) + 'px';
                    }});
                    return true;
                }}
            }}),
            EditorView.lineWrapping,
        ],
    }});

    window._flyntCM = new EditorView({{ state, parent: container }});
    window._flyntCM.focus();
    console.timeEnd('cm6-init');
    console.timeEnd('cm6-total');
    }} // end _initCM
    try {{ _initCM(); }} catch(e) {{
        const c = document.getElementById('flynt-cm-editor');
        if (c) {{
            c.innerHTML = '<pre style="color:#ef4444;padding:20px;font-size:12px;white-space:pre-wrap;">CM6 error: ' + e.message + '\n\n' + (e.stack || '') + '</pre>';
        }}
        if (window._flyntNotify) window._flyntNotify('debug', 'CM6_ERROR: ' + e.message);
    }}
}})();
"#
    )
}

// ── Notification bridge JS ──────────────────────────────────────────────────
// Uses a global function + polling eval to decouple CM6 lifecycle from
// the Dioxus eval channel. CM6 calls window._flyntNotify(type, data),
// which queues messages. A persistent eval loop drains the queue.

const BRIDGE_JS: &str = r#"
if (!window._flyntQueue) {
    window._flyntQueue = [];
    window._flyntNotify = function(type, data) {
        window._flyntQueue.push(JSON.stringify({type: type, data: data}));
    };
}

// Drain loop — sends queued messages to Rust via this eval's channel
async function _flyntDrain() {
    while (true) {
        if (window._flyntQueue.length > 0) {
            const msg = window._flyntQueue.shift();
            dioxus.send(msg);
        } else {
            await new Promise(r => setTimeout(r, 50));
        }
    }
}
_flyntDrain();

// Click-to-edit for Excalidraw embeds
document.addEventListener('click', function(e) {
    const embed = e.target.closest('.excalidraw-embed[data-drawing]');
    if (embed) {
        const drawing = embed.getAttribute('data-drawing');
        if (drawing) {
            window._flyntNotify('open-drawing', drawing);
        }
    }
});
"#;

#[component]
fn NoteInspector(
    tab: Signal<InspectorTab>,
    body: String,
    frontmatter: Frontmatter,
    link_context: Option<LinkContext>,
    on_open_doc: EventHandler<DocumentMeta>,
    on_jump_line: EventHandler<usize>,
    on_close: EventHandler<()>,
) -> Element {
    let headings = extract_headings(&body);
    rsx! {
        aside { class: "note-inspector",
            div { class: "note-inspector-header",
                div { class: "note-inspector-title", "Context" }
                button {
                    class: "note-inspector-close",
                    title: "Close inspector",
                    onclick: move |_| on_close.call(()),
                    "\u{00D7}"
                }
            }
            div { class: "note-inspector-tabs",
                InspectorTabButton { tab, value: InspectorTab::Links, label: "Links" }
                InspectorTabButton { tab, value: InspectorTab::Outline, label: "Outline" }
                InspectorTabButton { tab, value: InspectorTab::Properties, label: "Properties" }
            }
            div { class: "note-inspector-body",
                match *tab.read() {
                    InspectorTab::Links => rsx! {
                        NoteLinksPanel {
                            link_context,
                            on_open_doc,
                        }
                    },
                    InspectorTab::Outline => rsx! {
                        NoteOutlinePanel {
                            headings,
                            on_jump_line,
                        }
                    },
                    InspectorTab::Properties => rsx! {
                        NotePropertiesPanel {
                            frontmatter,
                        }
                    },
                }
            }
        }
    }
}

#[component]
fn InspectorTabButton(
    tab: Signal<InspectorTab>,
    value: InspectorTab,
    label: &'static str,
) -> Element {
    let active = *tab.read() == value;
    rsx! {
        button {
            class: if active { "note-inspector-tab active" } else { "note-inspector-tab" },
            onclick: move |_| *tab.write() = value,
            "{label}"
        }
    }
}

#[component]
fn NoteLinksPanel(
    link_context: Option<LinkContext>,
    on_open_doc: EventHandler<DocumentMeta>,
) -> Element {
    match link_context {
        None => rsx! { div { class: "note-inspector-empty", "Loading links..." } },
        Some(ctx) => rsx! {
            div { class: "note-link-summary",
                div { class: "note-link-stat",
                    span { class: "note-link-stat-value", "{ctx.backlinks.len()}" }
                    span { class: "note-link-stat-label", "backlinks" }
                }
                div { class: "note-link-stat",
                    span { class: "note-link-stat-value", "{ctx.resolved_count}" }
                    span { class: "note-link-stat-label", "resolved" }
                }
                div { class: "note-link-stat missing",
                    span { class: "note-link-stat-value", "{ctx.missing_count}" }
                    span { class: "note-link-stat-label", "missing" }
                }
            }
            if !ctx.aliases.is_empty() {
                div { class: "note-inspector-section compact",
                    div { class: "note-inspector-section-title", "Accepted aliases" }
                    div { class: "note-property-chips",
                        for alias in ctx.aliases {
                            span { class: "note-property-chip", "{alias}" }
                        }
                    }
                }
            }
            div { class: "note-inspector-section",
                div { class: "note-inspector-section-title",
                    "Backlinks"
                    span { class: "note-inspector-count", "{ctx.backlinks.len()}" }
                }
                if ctx.backlinks.is_empty() {
                    div { class: "note-inspector-empty", "No backlinks" }
                } else {
                    div { class: "note-inspector-list",
                        for doc in ctx.backlinks {
                            {
                                let meta = doc.clone();
                                rsx! {
                                    button {
                                        class: "note-inspector-item",
                                        onclick: move |_| on_open_doc.call(meta.clone()),
                                        span { class: "note-inspector-item-title", "{doc.title}" }
                                        span { class: "note-inspector-item-meta", "{doc.path.display()}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "note-inspector-section",
                div { class: "note-inspector-section-title",
                    "Outgoing"
                    span { class: "note-inspector-count", "{ctx.outgoing.len()}" }
                }
                if ctx.outgoing.is_empty() {
                    div { class: "note-inspector-empty", "No outgoing links" }
                } else {
                    div { class: "note-inspector-list",
                        for link in ctx.outgoing {
                            {
                                let label = link.display.clone().unwrap_or_else(|| link.target.clone());
                                let anchor = link.anchor.clone();
                                let meta = link.resolved.clone();
                                let disabled = meta.is_none();
                                let meta_for_open = meta.clone();
                                let status = if link.resolved.is_some() { "resolved" } else { "missing" };
                                let mut classes = "note-inspector-item link-target".to_string();
                                if disabled {
                                    classes.push_str(" missing");
                                }
                                rsx! {
                                    button {
                                        class: "{classes}",
                                        disabled,
                                        onclick: move |_| {
                                            if let Some(doc) = meta_for_open.clone() {
                                                on_open_doc.call(doc);
                                            }
                                        },
                                        span { class: "note-inspector-item-title", "{label}" }
                                        span { class: "note-inspector-item-meta",
                                            "{link.target}"
                                            if let Some(anchor) = anchor {
                                                " #{anchor}"
                                            }
                                            if link.count > 1 {
                                                " x{link.count}"
                                            }
                                        }
                                        span { class: "note-link-status {status}", "{status}" }
                                        if disabled {
                                            span { class: "note-inspector-item-meta", "No matching note yet" }
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

#[component]
fn NoteOutlinePanel(headings: Vec<NoteHeading>, on_jump_line: EventHandler<usize>) -> Element {
    if headings.is_empty() {
        return rsx! { div { class: "note-inspector-empty", "No headings" } };
    }
    rsx! {
        div { class: "note-inspector-list",
            for heading in headings {
                {
                    let line = heading.line;
                    let indent = ((heading.level.saturating_sub(1)) * 12).min(60);
                    rsx! {
                        button {
                            class: "note-inspector-item outline-item",
                            style: "padding-left: calc(var(--space-2) + {indent}px);",
                            onclick: move |_| on_jump_line.call(line),
                            span { class: "note-inspector-item-title", "{heading.title}" }
                            span { class: "note-inspector-item-meta", "line {heading.line} · #{heading.anchor}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn NotePropertiesPanel(frontmatter: Frontmatter) -> Element {
    let kind = frontmatter
        .kind
        .clone()
        .unwrap_or_else(|| "document".into());
    let status = frontmatter.status.clone().unwrap_or_else(|| "none".into());
    let id = frontmatter
        .id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "unmanaged".into());
    let publication = if frontmatter.publication.enabled {
        format!("{:?}", frontmatter.publication.visibility).to_lowercase()
    } else {
        "disabled".into()
    };
    let data_rows = frontmatter_data_rows(&frontmatter);

    rsx! {
        div { class: "note-properties",
            PropertyRow { label: "Kind", value: kind }
            PropertyRow { label: "Status", value: status }
            PropertyRow { label: "ID", value: id }
            PropertyRow { label: "Publication", value: publication }
            div { class: "note-property-block",
                div { class: "note-property-label", "Tags" }
                if frontmatter.tags.is_empty() {
                    div { class: "note-property-empty", "none" }
                } else {
                    div { class: "note-property-chips",
                        for tag in frontmatter.tags {
                            span { class: "note-property-chip", "#{tag}" }
                        }
                    }
                }
            }
            div { class: "note-property-block",
                div { class: "note-property-label", "Aliases" }
                if frontmatter.aliases.is_empty() {
                    div { class: "note-property-empty", "none" }
                } else {
                    div { class: "note-property-chips",
                        for alias in frontmatter.aliases {
                            span { class: "note-property-chip", "{alias}" }
                        }
                    }
                }
            }
            if !data_rows.is_empty() {
                div { class: "note-property-block",
                    div { class: "note-property-label", "[data]" }
                    div { class: "note-property-data",
                        for (key, value) in data_rows {
                            div { class: "note-property-data-row",
                                span { class: "note-property-data-key", "{key}" }
                                span { class: "note-property-data-value", "{value}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn PropertyRow(label: &'static str, value: String) -> Element {
    rsx! {
        div { class: "note-property-row",
            span { class: "note-property-label", "{label}" }
            span { class: "note-property-value", "{value}" }
        }
    }
}

fn frontmatter_data_rows(frontmatter: &Frontmatter) -> Vec<(String, String)> {
    let Some(data) = frontmatter.data.as_ref().and_then(|v| v.as_table()) else {
        return vec![];
    };
    data.iter()
        .map(|(key, value)| (key.clone(), compact_toml_value(value)))
        .collect()
}

fn compact_toml_value(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(v) => v.to_string(),
        toml::Value::Float(v) => v.to_string(),
        toml::Value::Boolean(v) => v.to_string(),
        toml::Value::Datetime(v) => v.to_string(),
        toml::Value::Array(values) => {
            let items = values.iter().map(compact_toml_value).collect::<Vec<_>>();
            format!("[{}]", items.join(", "))
        }
        toml::Value::Table(_) => "{...}".into(),
    }
}

#[component]
fn NoteHistoryModal(
    path: std::path::PathBuf,
    state: Option<HistoryPanelState>,
    snapshot: Option<FileSnapshot>,
    current_body: String,
    snapshot_error: Option<String>,
    restore_message: Option<String>,
    on_close: EventHandler<()>,
    on_select_commit: EventHandler<String>,
    on_restore_snapshot: EventHandler<FileSnapshot>,
) -> Element {
    let diff_lines = snapshot
        .as_ref()
        .map(|snapshot| build_line_diff(&snapshot.content, &current_body))
        .unwrap_or_default();

    rsx! {
        div { class: "history-overlay", onclick: move |_| on_close.call(()) }
        div { class: "history-modal", onclick: move |e| e.stop_propagation(),
            div { class: "history-header",
                div {
                    div { class: "history-title", "Note History" }
                    div { class: "history-path", "{path.display()}" }
                }
                button {
                    class: "note-inspector-close",
                    title: "Close history",
                    onclick: move |_| on_close.call(()),
                    "\u{00D7}"
                }
            }
            div { class: "history-body",
                div { class: "history-list",
                    match state {
                        None => rsx! { div { class: "note-inspector-empty", "Loading history..." } },
                        Some(HistoryPanelState { error: Some(error), .. }) => rsx! {
                            div { class: "history-error", "{error}" }
                        },
                        Some(HistoryPanelState { entries, error: None }) => rsx! {
                            if entries.is_empty() {
                                div { class: "note-inspector-empty", "No commits found for this note" }
                            } else {
                                for entry in entries {
                                    {
                                        let commit = entry.commit.clone();
                                        let timestamp = entry.timestamp.format("%Y-%m-%d %H:%M").to_string();
                                        rsx! {
                                            button {
                                                class: "history-entry",
                                                onclick: move |_| on_select_commit.call(commit.clone()),
                                                span { class: "history-entry-summary", "{entry.summary}" }
                                                span { class: "history-entry-meta",
                                                    "{entry.short_commit} · {entry.author} · {timestamp}"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        },
                    }
                }
                div { class: "history-preview",
                    if let Some(message) = restore_message {
                        div { class: "history-restore-message", "{message}" }
                    }
                    if let Some(error) = snapshot_error {
                        div { class: "history-error", "{error}" }
                    }
                    if let Some(snapshot) = snapshot {
                        div { class: "history-preview-toolbar",
                            div {
                                div { class: "history-entry-meta", "{snapshot.commit.chars().take(7).collect::<String>()}" }
                                div { class: "history-diff-caption", "Selected commit compared to current note" }
                            }
                            {
                                let restore_snapshot = snapshot.clone();
                                rsx! {
                                    button {
                                        class: "btn btn-primary btn-sm",
                                        onclick: move |_| on_restore_snapshot.call(restore_snapshot.clone()),
                                        "Restore as copy"
                                    }
                                }
                            }
                        }
                        div { class: "history-diff-content",
                            if diff_lines.is_empty() {
                                div { class: "history-preview-empty", "Snapshot and current note are identical." }
                            } else {
                                for line in diff_lines {
                                    {
                                        let class = match line.kind {
                                            HistoryDiffKind::Context => "context",
                                            HistoryDiffKind::Added => "added",
                                            HistoryDiffKind::Removed => "removed",
                                        };
                                        let marker = match line.kind {
                                            HistoryDiffKind::Context => " ",
                                            HistoryDiffKind::Added => "+",
                                            HistoryDiffKind::Removed => "-",
                                        };
                                        let old_line = line.old_line.map(|n| n.to_string()).unwrap_or_default();
                                        let new_line = line.new_line.map(|n| n.to_string()).unwrap_or_default();
                                        rsx! {
                                            div { class: "history-diff-line {class}",
                                                span { class: "history-diff-gutter old", "{old_line}" }
                                                span { class: "history-diff-gutter new", "{new_line}" }
                                                span { class: "history-diff-marker", "{marker}" }
                                                code { class: "history-diff-text", "{line.text}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        div { class: "history-preview-empty", "Select a commit to preview the note body at that point." }
                    }
                }
            }
        }
    }
}

// ── Notes view ──────────────────────────────────────────────────────────────

#[component]
pub fn NotesView() -> Element {
    let ctx = use_context::<AppContext>();
    let mut tab_state = use_context::<Signal<TabState>>();
    let mut is_drawing = use_context::<Signal<bool>>();
    let ctx_res = ctx.clone();
    let ctx_save2 = ctx.clone();

    let mut mode = use_signal(|| EditMode::Live);
    let mut edit_body = use_signal(String::new);
    let mut save_err = use_signal(|| Option::<String>::None);
    let mut save_state = use_signal(|| SaveState::Clean);
    let mut render_ver = use_signal(|| 0u32);
    let mut conflict_detected = use_signal(|| false);
    let mut inspector_open = use_signal(|| true);
    let mut inspector_tab = use_signal(|| InspectorTab::Links);
    let mut history_open = use_signal(|| false);
    let mut history_snapshot: Signal<Option<FileSnapshot>> = use_signal(|| None);
    let mut history_snapshot_error: Signal<Option<String>> = use_signal(|| None);
    let mut history_restore_message: Signal<Option<String>> = use_signal(|| None);
    let inspector_command = use_context::<Signal<NoteInspectorCommand>>();
    let mut last_inspector_command = use_signal(|| 0u64);
    let history_command = use_context::<Signal<NoteHistoryCommand>>();
    let mut last_history_command = use_signal(|| 0u64);

    use_effect(move || {
        let command = *inspector_command.read();
        if command.version == *last_inspector_command.peek() {
            return;
        }
        *last_inspector_command.write() = command.version;
        match command.target {
            NoteInspectorTarget::Toggle => {
                let open = *inspector_open.peek();
                *inspector_open.write() = !open;
            }
            NoteInspectorTarget::Links => {
                *inspector_open.write() = true;
                *inspector_tab.write() = InspectorTab::Links;
            }
            NoteInspectorTarget::Outline => {
                *inspector_open.write() = true;
                *inspector_tab.write() = InspectorTab::Outline;
            }
            NoteInspectorTarget::Properties => {
                *inspector_open.write() = true;
                *inspector_tab.write() = InspectorTab::Properties;
            }
        }
    });

    use_effect(move || {
        let command = *history_command.read();
        if command.version == *last_history_command.peek() {
            return;
        }
        *last_history_command.write() = command.version;
        *history_snapshot.write() = None;
        *history_snapshot_error.write() = None;
        *history_restore_message.write() = None;
        *history_open.write() = true;
    });

    // ── Two-phase rendering ───────────────────────────────────────────
    // Phase 1 (instant): read document from SQLite synchronously — <1ms.
    //   Sets edit_body and raw content immediately so the editor is responsive.
    // Phase 2 (background): render HTML via comrak + query execution.
    //   Swaps in when ready. Cached for instant tab switching.

    // Render cache: doc_id → (path, title, body, html, has_conflicts)
    let mut render_cache: Signal<
        std::collections::HashMap<
            flynt_core::models::DocumentId,
            (std::path::PathBuf, String, String, String, bool),
        >,
    > = use_signal(std::collections::HashMap::new);

    // Invalidate cache on save
    use_effect(move || {
        let _ver = *render_ver.read();
        if _ver > 0 {
            if let Some(id) = tab_state.read().active_id().cloned() {
                render_cache.write().remove(&id);
            }
        }
    });

    // Phase 1: synchronous document read — no spawn_blocking, no async overhead.
    //
    // Tuple holds (id, path, title, body, frontmatter). Carrying the id
    // alongside the rest is what the sync effect below uses to detect
    // "is this still the doc we just asked for?" — without it, a stale
    // doc_data value (from a previous tab) could be propagated to the
    // editor when the sync effect fires before doc_data has refreshed
    // for the newly active tab.
    let mut doc_data: Signal<
        Option<(
            flynt_core::models::DocumentId,
            std::path::PathBuf,
            String,
            String,
            flynt_core::models::Frontmatter,
        )>,
    > = use_signal(|| None);
    use_effect(move || {
        let _ver = *render_ver.read();
        let selected_id = tab_state.read().active_id().cloned();
        let Some(doc_id) = selected_id else {
            *doc_data.write() = None;
            return;
        };
        // Synchronous SQLite read — <1ms for any document
        let project = ctx_res.project();
        match project.store.get_document(&doc_id) {
            Ok(Some(doc)) => {
                *doc_data.write() = Some((
                    doc.id.clone(),
                    doc.path.clone(),
                    doc.title.clone(),
                    doc.content.clone(),
                    doc.frontmatter.clone(),
                ));
            }
            Ok(None) => {
                tracing::warn!("doc_data_effect: doc {:?} not found in store", doc_id);
            }
            Err(e) => {
                tracing::warn!("doc_data_effect: store error for {:?}: {e}", doc_id);
            }
        }
    });

    // Boards + engagements caches for the metadata strip's pickers.
    // Refreshed by the existing `refresh` signal (sidebar bumps it on
    // any project event), so a kanban board rename or a new engagement
    // shows up in the picker without a tab toggle.
    let mut boards_cache: Signal<Vec<flynt_core::models::Board>> = use_signal(Vec::new);
    let mut engagements_cache: Signal<Vec<flynt_core::models::Engagement>> = use_signal(Vec::new);
    use_effect(move || {
        let _ver = *render_ver.read();
        let project = ctx_res.project();
        if let Ok(b) = project.store.list_boards() {
            *boards_cache.write() = b;
        }
        if let Ok(e) = project.store.list_engagements() {
            *engagements_cache.write() = e;
        }
    });

    // Install the dispatcher once at mount. Picker `on_change` events
    // flow into this channel; the spawned receiver translates them into
    // `Project::set_data_field` calls. Going through a channel rather
    // than direct ctx access keeps the strip's component scope free of
    // AppContext (Dioxus contexts are scope-bound; the picker is in a
    // different scope path than the apply site).
    use_effect(move || {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<
            crate::components::task_metadata_strip::FieldChangeRequest,
        >();
        crate::components::task_metadata_strip::install_dispatcher(tx);

        let project = ctx_res.project();
        let mut bump = render_ver;
        spawn(async move {
            while let Some(req) = rx.recv().await {
                let key_for_log = req.key.clone();
                let key = req.key;
                let value = req.value;
                let path = req.path;
                let project_for_blocking = project.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let toml_value =
                        crate::components::task_metadata_strip::translate_value(&key, &value);
                    project_for_blocking.set_data_field(&path, &key, toml_value)
                })
                .await;
                match result {
                    Ok(Ok(())) => {
                        // Bump render_ver so doc_data + rendered re-fetch
                        // and the strip re-renders with the new value.
                        *bump.write() += 1;
                    }
                    Ok(Err(e)) => {
                        tracing::error!(error = %e, "set_data_field failed for {key_for_log}");
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "set_data_field task panicked for {key_for_log}");
                    }
                }
            }
        });
    });

    // Phase 2: background HTML rendering — fires after doc_data is set
    let rendered: Resource<Option<(std::path::PathBuf, String, String, String, bool)>> =
        use_resource(move || {
            let _ver = *render_ver.read();
            let selected_id = tab_state.read().active_id().cloned();
            let project = ctx_res.project();
            async move {
                let Some(doc_id) = selected_id else {
                    return None;
                };

                // Cache hit — instant
                if let Some(cached) = render_cache.read().get(&doc_id) {
                    return Some(cached.clone());
                }

                // Background render — won't block the UI
                let cache_id = doc_id.clone();
                let result = tokio::task::spawn_blocking(move || {
                    project
                        .store
                        .get_document(&doc_id)
                        .ok()
                        .flatten()
                        .map(|doc| {
                            let html = render_html_with_store(
                                &doc.content,
                                Some(&*project.store),
                                Some(&project.root),
                            );
                            let has_conflicts =
                                flynt_core::conflict::has_conflict_markers(&doc.content);
                            (
                                doc.path.clone(),
                                doc.title.clone(),
                                doc.content.clone(),
                                html,
                                has_conflicts,
                            )
                        })
                })
                .await
                .ok()
                .flatten();

                if let Some(ref r) = result {
                    *conflict_detected.write() = r.4;
                    render_cache.write().insert(cache_id, r.clone());
                }
                result
            }
        });

    let link_ctx = ctx.clone();
    let link_context: Resource<Option<LinkContext>> = use_resource(move || {
        let _ver = *render_ver.read();
        let selected_id = tab_state.read().active_id().cloned();
        let project = link_ctx.project();
        async move {
            let Some(doc_id) = selected_id else {
                return None;
            };
            tokio::task::spawn_blocking(move || {
                let Some(doc) = project.store.get_document(&doc_id)? else {
                    return Ok(None);
                };
                let backlinks = project.store.get_backlinks(&doc_id).unwrap_or_default();
                let context =
                    build_link_context(backlinks, &doc.content, &doc.frontmatter, |target| {
                        project
                            .store
                            .find_document_by_slug(&target.to_lowercase())
                            .ok()
                            .flatten()
                    });
                Ok::<_, anyhow::Error>(Some(context))
            })
            .await
            .ok()
            .and_then(Result::ok)
            .flatten()
        }
    });

    let history_ctx = ctx.clone();
    let history_state: Resource<Option<HistoryPanelState>> = use_resource(move || {
        let is_open = *history_open.read();
        let selected = doc_data
            .read()
            .as_ref()
            .map(|(_, path, _, _, _)| path.clone());
        let project = history_ctx.project();
        async move {
            if !is_open {
                return None;
            }
            let Some(path) = selected else {
                return Some(HistoryPanelState {
                    entries: vec![],
                    error: Some("No active note selected".into()),
                });
            };
            tokio::task::spawn_blocking(move || {
                let (remote, branch) = match &project.config.sync {
                    flynt_core::models::SyncConfig::Git { remote, branch, .. } => {
                        (remote.clone(), branch.clone())
                    }
                    _ => ("origin".into(), "main".into()),
                };
                let git = GitSync::new(project.root.clone(), remote, branch);
                match git.list_file_history(&path, 40) {
                    Ok(entries) => HistoryPanelState {
                        entries,
                        error: None,
                    },
                    Err(e) => HistoryPanelState {
                        entries: vec![],
                        error: Some(format!("Could not read git history: {e}")),
                    },
                }
            })
            .await
            .ok()
        }
    });

    // The signal that drives CM6 loading. Its (id, body) value is the
    // "last content we asked CM6 to display." The CM6 init effect
    // subscribes to it; whenever it changes, CM6 swaps to the new body.
    //
    // Single source of truth for de-dup — no separate `synced_doc_id`.
    // The previous shape gated on synced_doc_id alone, which prevented
    // post-save propagation: after a save, doc_data refreshed with new
    // content but `already_synced` was true so cm6_load_source was
    // never updated. CM6 kept the pre-save body; the operator's saved
    // edits never appeared in Live mode.
    let mut cm6_load_source: Signal<Option<(flynt_core::models::DocumentId, String)>> =
        use_signal(|| None);
    use_effect(move || {
        let current_id = tab_state.read().active_id().cloned();
        let Some(active_id) = current_id else { return };
        // Confirm doc_data is for this tab (not a stale value from
        // the previous tab's load). If the ids mismatch, bail and
        // wait for doc_data to refresh — the effect subscribes to
        // doc_data so it will re-fire.
        let body = match &*doc_data.read() {
            Some((doc_id, _, _, body, _)) if doc_id == &active_id => body.clone(),
            _ => return,
        };
        // De-dup. Three cases trigger a load:
        //   1. First load (no prior cm6_load_source) — propagate.
        //   2. Tab switch (id changed) — propagate.
        //   3. Same tab, body on disk changed AND operator hasn't
        //      typed anything we'd be clobbering. We detect the
        //      no-clobber condition by checking edit_body against
        //      the previously-loaded body — if they match, the user
        //      has no unsaved divergence and it's safe to refresh.
        let action = match &*cm6_load_source.peek() {
            None => Some("first-load"),
            Some((prev_id, _)) if prev_id != &active_id => Some("tab-switch"),
            Some((_, prev_body)) => {
                if body != *prev_body {
                    // Body on disk changed since last load. Safe to
                    // overwrite only if edit_body matches what we last
                    // loaded (no unsaved divergence in CM6 / textarea).
                    let eb = edit_body.peek().clone();
                    if eb == *prev_body {
                        Some("disk-changed-no-divergence")
                    } else if eb == body {
                        // edit_body matches new body — operator just
                        // saved their own edits. Propagate so CM6
                        // catches up to the saved content.
                        Some("save-propagation")
                    } else {
                        // edit_body has uncommitted divergence AND
                        // disk changed externally. Keep their work;
                        // they'll have to resolve manually.
                        None
                    }
                } else {
                    None
                }
            }
        };
        let Some(reason) = action else { return };
        tracing::debug!(
            "sync_effect: propagating ({}) active_id={:?} body_len={}",
            reason,
            active_id,
            body.len()
        );
        *edit_body.write() = body.clone();
        *save_state.write() = SaveState::Clean;
        *cm6_load_source.write() = Some((active_id, body));
    });

    let has_active = tab_state.read().active_id().is_some();

    // Initialize CM6 when: new document loaded OR mode switched back to Live.
    //
    // Subscribes to `cm6_load_source` so the effect runs with the exact
    // body that came from the doc load — no race against edit_body.
    // Also subscribes to `mode` so toggling Source → Live re-fires the
    // init (calling cm6_init_js's swap path with the latest content).
    let is_drawing_mode = use_context::<Signal<bool>>();
    use_effect(move || {
        let source = cm6_load_source.read().clone();
        let Some((doc_id, body)) = source else { return };
        if *is_drawing_mode.read() {
            return;
        }
        if !matches!(&*mode.read(), EditMode::Live) {
            return;
        }
        tracing::info!(
            "CM6 init effect triggered for doc_id={:?} body_len={}",
            doc_id,
            body.len()
        );
        document::eval(&cm6_init_js(&body));
    });

    // Autosave for Source mode (textarea path). CM6 already has its own
    // autosave wired through the bridge; this gives Source-mode editing
    // the same behavior so operators don't have to ⌘S manually after
    // typing in the textarea.
    //
    // Debounced: each edit_body change resets a 1.5s timer; the save
    // fires only after the operator has been quiet that long. Skips
    // when edit_body matches what's already on disk (no actual diff).
    let mut autosave_token = use_signal(|| 0u64);
    let autosave_ctx = ctx.clone();
    use_effect(move || {
        let body = edit_body.read().clone();
        if !matches!(&*mode.read(), EditMode::Source) {
            return;
        }
        // Resolve the path from doc_data — we need the relative path
        // to save to. If doc_data isn't loaded yet, skip.
        let (disk_body, path) = match &*doc_data.peek() {
            Some((_, p, _, b, _)) => (b.clone(), p.clone()),
            None => return,
        };
        if body == disk_body {
            return;
        } // no diff vs. disk
        let token = autosave_token.peek().wrapping_add(1);
        *autosave_token.write() = token;
        let mut bump = render_ver;
        let mut state = save_state;
        let mut err = save_err;
        let c = autosave_ctx.clone();
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            // Newer edit superseded this one — bail.
            if *autosave_token.peek() != token {
                return;
            }
            let project = c.project();
            let path_for_save = path.clone();
            let body_for_save = body.clone();
            match tokio::task::spawn_blocking(move || {
                project.save_document_content(&path_for_save, &body_for_save)
            })
            .await
            {
                Ok(Ok(())) => {
                    *bump.write() += 1;
                    *err.write() = None;
                    *state.write() = SaveState::Saved;
                }
                Ok(Err(e)) => *err.write() = Some(format!("Autosave failed — {e}")),
                Err(e) => *err.write() = Some(format!("Autosave interrupted — {e}")),
            }
        });
    });

    // Persistent message bridge — one eval that polls a global queue.
    // CM6 pushes messages to the queue; this loop drains them to Rust.
    let ctx_link = ctx.clone();
    let mut ts_link = tab_state;
    let mut ar_link = use_context::<Signal<Route>>();
    use_effect(move || {
        let mut eval = document::eval(BRIDGE_JS);
        let c = ctx_link.clone();

        spawn(async move {
            loop {
                let Ok(val) = eval.recv::<String>().await else {
                    break;
                };

                let Ok(msg) = serde_json::from_str::<serde_json::Value>(&val) else {
                    continue;
                };
                let msg_type = msg["type"].as_str().unwrap_or("");
                let data = msg["data"].as_str().unwrap_or("");

                match msg_type {
                    "edit" => {
                        // Keep edit_body in sync with CM6's live content.
                        // The CM6 div has a stable id (`flynt-cm-editor`), so
                        // Dioxus reconciles it as the same element on re-render
                        // — the editor instance is not torn down. The earlier
                        // shape avoided this write and relied on a CM6 read at
                        // toggle time, which raced against post-save CM6
                        // re-init: a stale CM6 (still showing N-1 before its
                        // re-init ran) was adopted into edit_body, reverting
                        // the operator's saved edits when they toggled back
                        // to Source. With this write, edit_body is the single
                        // source of truth for "the current document contents."
                        *edit_body.write() = data.to_string();
                        *save_state.write() = SaveState::Dirty;
                    }
                    "save" | "autosave" => {
                        let content = data.to_string();
                        // peek — do NOT subscribe reactively
                        if let Some(Some((p, _, _, _, _))) = &*rendered.peek() {
                            let path = p.clone();
                            let project = c.project();
                            match tokio::task::spawn_blocking(move || {
                                project.save_document_content(&path, &content)
                            })
                            .await
                            {
                                Ok(Ok(())) => {
                                    // Update save indicator via DOM — no signal write
                                    document::eval(
                                        "document.querySelectorAll('.save-status').forEach(e => {{ e.textContent = 'saved'; e.className = 'save-status saved'; }});",
                                    );
                                }
                                Ok(Err(e)) => {
                                    *save_err.write() = Some(format!("Could not save — {e}"))
                                }
                                Err(e) => {
                                    *save_err.write() = Some(format!("Save interrupted — {e}"))
                                }
                            }
                        }
                    }
                    "mode" => {
                        if data == "source" {
                            // edit_body is already the source of truth —
                            // the "edit" message handler above keeps it in
                            // sync with CM6. No read-CM6 dance needed.
                            *mode.write() = EditMode::Source;
                        }
                    }
                    "open-drawing" => {
                        // Open the excalidraw wrapper .md in a tab — NotesView
                        // detects the embed and renders ExcalidrawView automatically
                        let drawing_file = data.to_string();
                        let slug = drawing_file.replace(".excalidraw", "").to_lowercase();
                        let project = c.project();
                        if let Ok(Some(meta)) = tokio::task::spawn_blocking(move || {
                            project.store.find_document_by_slug(&slug)
                        })
                        .await
                        .unwrap_or(Ok(None))
                        {
                            ts_link.write().open(meta.id.clone(), meta.title.clone());
                            *ar_link.write() = Route::Notes;
                        }
                    }
                    "nav" => {
                        let slug = data.to_lowercase();
                        let project = c.project();
                        if let Ok(Some(meta)) = tokio::task::spawn_blocking(move || {
                            project.store.find_document_by_slug(&slug)
                        })
                        .await
                        .unwrap_or(Ok(None))
                        {
                            ts_link.write().open(meta.id.clone(), meta.title.clone());
                            *ar_link.write() = Route::Notes;
                        }
                    }
                    _ => {}
                }
            }
        });
    });

    // No tab open
    if !has_active {
        return rsx! {
            crate::components::TabBar {}
            div { class: "notes-empty",
                div { class: "notes-empty-content",
                    div { class: "notes-empty-icon", dangerous_inner_html: crate::icons::ICON_SCROLL }
                    p { "Select a note from the sidebar" }
                    p { class: "notes-empty-hint", "or press + to create a new one" }
                }
            }
        };
    }

    // Gate on doc_data (synchronous, instant) not rendered (async, slow).
    // The editor gets raw content immediately; HTML preview swaps in when ready.
    let Some((_doc_id, rel_path, title, body, frontmatter)) = doc_data.read().clone() else {
        return rsx! {
            crate::components::TabBar {}
            if has_active {
                div { class: "notes-loading muted", "Loading…" }
            }
        };
    };

    // If this document is an excalidraw wrapper, render ExcalidrawView directly.
    // Two acceptance paths:
    //  (1) body is exactly `![[X.excalidraw]]` (the normal wrapper shape), or
    //  (2) frontmatter has `tags = [..., "drawing", ...]` AND a sibling
    //      `<stem>.excalidraw` file exists. (2) recovers from wrapper body
    //      corruption without hiding the user's actual drawing data.
    let excalidraw_file_from_body = crate::views::excalidraw::excalidraw_embed_path(&body);
    let excalidraw_file_from_recovery = if excalidraw_file_from_body.is_none()
        && frontmatter.tags.iter().any(|tag| tag == "drawing")
    {
        rel_path
            .file_stem()
            .map(|s| format!("{}.excalidraw", s.to_string_lossy()))
    } else {
        None
    };
    if let Some(excalidraw_file) = excalidraw_file_from_body.or(excalidraw_file_from_recovery) {
        let project_root = ctx.project_root();
        // Resolve the .excalidraw file relative to the document's directory
        let doc_dir = rel_path.parent().unwrap_or(std::path::Path::new(""));
        let excalidraw_path = doc_dir.join(&excalidraw_file);
        let abs = project_root.join(&excalidraw_path);
        if abs.exists() {
            is_drawing.set(true);
            return rsx! {
                div {
                    style: "display:flex;flex-direction:column;flex:1;overflow:hidden;padding:0;min-height:0;height:100%;",
                    crate::views::ExcalidrawView { path: excalidraw_path }
                }
            };
        }
    }

    // If this document is a canvas wrapper, render CanvasView directly.
    // Two acceptance paths:
    //  (1) body is exactly `![[X.canvas]]` (the normal wrapper shape), or
    //  (2) frontmatter has `tags = [..., "canvas", ...]` AND a sibling
    //      `<stem>.canvas` file exists. (1) is the happy path; (2) is a
    //      recovery for cases where another bug has stomped the body but
    //      the user's design data is still intact on disk — better to
    //      surface the canvas than dump them into an empty note view.
    let canvas_file_from_body = crate::views::canvas::canvas_embed_path(&body);
    let canvas_file_from_recovery = if canvas_file_from_body.is_none()
        && crate::views::canvas::frontmatter_has_canvas_tag(&body)
    {
        rel_path
            .file_stem()
            .map(|s| format!("{}.canvas", s.to_string_lossy()))
    } else {
        None
    };
    if let Some(canvas_file) = canvas_file_from_body.or(canvas_file_from_recovery) {
        let project_root = ctx.project_root();
        let doc_dir = rel_path.parent().unwrap_or(std::path::Path::new(""));
        let canvas_path = doc_dir.join(&canvas_file);
        let abs = project_root.join(&canvas_path);
        if abs.exists() {
            is_drawing.set(true);
            return rsx! {
                div {
                    style: "display:flex;flex-direction:column;flex:1;overflow:hidden;padding:0;min-height:0;height:100%;",
                    crate::views::CanvasView { path: canvas_path }
                }
            };
        }
    }

    // Clear drawing mode flag — but ONLY if it was set. Dioxus signals
    // notify subscribers on every `set`, even when the value didn't
    // change. The CM6 init effect subscribes to is_drawing_mode, so an
    // unconditional `set(false)` here fires that effect twice per
    // tab-switch (once for the cm6_load_source write, once for this
    // no-op is_drawing toggle) — wasted work and log noise.
    if *is_drawing.peek() {
        is_drawing.set(false);
    }

    // edit_body is seeded by the use_effect that watches rendered,
    // and synced from CM6 on mode switch. No eager write here.

    let title = title.clone();
    let _body = body.clone();
    let path = rel_path.clone();

    let mut renaming = use_signal(|| false);
    let mut rename_input = use_signal(|| title.clone());

    // Watch for rename trigger from sidebar context menu
    let rename_trigger = use_context::<Signal<crate::state::RenameTrigger>>();
    let mut last_rename_ver = use_signal(|| 0u64);
    if rename_trigger.read().0 > *last_rename_ver.peek() {
        *last_rename_ver.write() = rename_trigger.read().0;
        *rename_input.write() = title.clone();
        *renaming.write() = true;
    }
    let mut rename_msg: Signal<Option<String>> = use_signal(|| None);
    let path_for_rename = path.clone();
    let ctx_rename = ctx.clone();
    let history_modal_path = path.clone();
    let history_select_path = path.clone();
    let history_restore_path = path.clone();

    rsx! {
        crate::components::TabBar {}
        div { class: "notes-workspace",
        div { class: "notes-pane",
            // Conflict resolution banner
            if *conflict_detected.read() {
                div { class: "conflict-banner",
                    span { class: "conflict-icon", "\u{26A0}" }
                    span { "This file has merge conflicts." }
                    div { class: "conflict-actions",
                        button {
                            class: "btn btn-sm btn-ghost",
                            onclick: move |_| {
                                let content = edit_body.read().clone();
                                let resolved = flynt_core::conflict::resolve_ours(&content);
                                *edit_body.write() = resolved.clone();
                                // Auto-save
                                let p = rendered.read().as_ref().and_then(|r| r.as_ref().map(|t| t.0.clone()));
                                let c = ctx.clone();
                                if let Some(path) = p {
                                    spawn(async move {
                                        let project = c.project();
                                        let _ = project.save_document_content(&path, &resolved);
                                        *render_ver.write() += 1;
                                    });
                                }
                            },
                            "Keep mine"
                        }
                        button {
                            class: "btn btn-sm btn-ghost",
                            onclick: move |_| {
                                let content = edit_body.read().clone();
                                let resolved = flynt_core::conflict::resolve_theirs(&content);
                                *edit_body.write() = resolved.clone();
                                let p = rendered.read().as_ref().and_then(|r| r.as_ref().map(|t| t.0.clone()));
                                let c = ctx.clone();
                                if let Some(path) = p {
                                    spawn(async move {
                                        let project = c.project();
                                        let _ = project.save_document_content(&path, &resolved);
                                        *render_ver.write() += 1;
                                    });
                                }
                            },
                            "Keep theirs"
                        }
                        button {
                            class: "btn btn-sm btn-primary",
                            onclick: move |_| {
                                *mode.write() = EditMode::Source;
                            },
                            "Edit manually"
                        }
                    }
                }
            }
            div { class: "notes-topbar",
                if *renaming.read() {
                    div { class: "rename-inline",
                        input {
                            autofocus: true,
                            class: "rename-input",
                            value: "{rename_input}",
                            oninput: move |e| *rename_input.write() = e.value(),
                            onkeydown: move |e| {
                                if e.key() == Key::Escape {
                                    *renaming.write() = false;
                                }
                                if e.key() == Key::Enter {
                                    let new_title = rename_input.read().trim().to_string();
                                    if new_title.is_empty() || new_title == title { *renaming.write() = false; return; }
                                    let p = path_for_rename.clone();
                                    let c = ctx_rename.clone();
                                    spawn(async move {
                                        let project = c.project();
                                        match tokio::task::spawn_blocking(move || {
                                            project.rename_document(&p, &new_title)
                                        }).await {
                                            Ok(Ok(n)) => {
                                                *rename_msg.write() = Some(format!("Renamed, {n} link(s) updated"));
                                                render_ver += 1;
                                            }
                                            Ok(Err(e)) => *rename_msg.write() = Some(format!("Rename failed — {e}")),
                                            Err(e) => *rename_msg.write() = Some(format!("Rename interrupted — {e}")),
                                        }
                                        *renaming.write() = false;
                                    });
                                }
                            },
                        }
                        {
                            let title = title.clone();
                            let path_for_rename = path_for_rename.clone();
                            let ctx_rename = ctx_rename.clone();
                            rsx! { button {
                            class: "btn btn-primary btn-xs",
                            onclick: move |_| {
                                let new_title = rename_input.read().trim().to_string();
                                if new_title.is_empty() || new_title == title { *renaming.write() = false; return; }
                                let p = path_for_rename.clone();
                                let c = ctx_rename.clone();
                                spawn(async move {
                                    let project = c.project();
                                    match tokio::task::spawn_blocking(move || {
                                        project.rename_document(&p, &new_title)
                                    }).await {
                                        Ok(Ok(n)) => {
                                            *rename_msg.write() = Some(format!("Renamed, {n} link(s) updated"));
                                            render_ver += 1;
                                        }
                                        Ok(Err(e)) => *rename_msg.write() = Some(format!("Rename failed — {e}")),
                                        Err(e) => *rename_msg.write() = Some(format!("Rename interrupted — {e}")),
                                    }
                                    *renaming.write() = false;
                                });
                            },
                            "Save"
                        } }
                        }
                        button { class: "btn btn-ghost btn-xs", onclick: move |_| *renaming.write() = false, "Cancel" }
                    }
                } else {
                    h1 {
                        class: "doc-title",
                        ondoubleclick: move |_| {
                            *rename_input.write() = title.clone();
                            *renaming.write() = true;
                        },
                        "{title}"
                    }
                }
                if let Some(ref msg) = *rename_msg.read() {
                    span { class: "rename-msg", "{msg}" }
                }
                div { class: "notes-actions",
                    // Save status updated via JS to avoid Dioxus re-render
                    span { class: "save-status" }
                    button {
                        class: if *inspector_open.read() { "btn btn-ghost active" } else { "btn btn-ghost" },
                        title: "Toggle note context",
                        onclick: move |_| {
                            let open = *inspector_open.read();
                            *inspector_open.write() = !open;
                        },
                        "Context"
                    }
                    button {
                        class: "btn btn-ghost",
                        title: "Open note history",
                        onclick: move |_| {
                            *history_snapshot.write() = None;
                            *history_snapshot_error.write() = None;
                            *history_restore_message.write() = None;
                            *history_open.write() = true;
                        },
                        "History"
                    }
                    match *mode.read() {
                        EditMode::Live => rsx! {
                            span { class: "mode-hint", "⌘E source" }
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| {
                                    spawn(async move {
                                        let mut eval = document::eval("if(window._flyntCM){dioxus.send(window._flyntCM.state.doc.toString())}else{dioxus.send('')}");
                                        if let Ok(content) = eval.recv::<String>().await {
                                            if !content.is_empty() {
                                                *edit_body.write() = content;
                                            }
                                        }
                                        *mode.write() = EditMode::Source;
                                    });
                                },
                                "Source"
                            }
                        },
                        EditMode::Source => rsx! {
                            button {
                                class: "btn btn-primary",
                                onclick: move |_| {
                                    let content = edit_body.read().clone();
                                    let p       = path.clone();
                                    let c       = ctx.clone();

                                    spawn(async move {
                                        let project = c.project();
                                        match tokio::task::spawn_blocking(move || {
                                            project.save_document_content(&p, &content)
                                        }).await {
                                            Ok(Ok(())) => {
                                                render_ver += 1;
                                                *save_err.write() = None;
                                                *save_state.write() = SaveState::Saved;
                                            }
                                            Ok(Err(e)) => *save_err.write() = Some(format!("Could not save — {e}")),
                                            Err(e)     => *save_err.write() = Some(format!("Save interrupted — {e}")),
                                        }
                                    });
                                    // Stay in Source mode — operator hits
                                    // "Live" explicitly when ready to review.
                                    // Auto-flipping caused a race where CM6
                                    // re-init lagged behind the mode change,
                                    // and the subsequent Source-toggle would
                                    // adopt CM6's stale content into edit_body.
                                },
                                "Save"
                            }
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| *mode.write() = EditMode::Live,
                                "Live"
                            }
                        },
                    }
                }
            }

            // Task metadata strip — between title bar and editor body.
            // Renders only when the doc is `kind = "task"`. Pills are
            // editable inline; changes flow through the dispatcher
            // channel installed above.
            if frontmatter.kind.as_deref() == Some("task")
                && !crate::views::excalidraw::is_excalidraw(&rel_path)
                && !crate::views::flow::is_flow(&rel_path)
            {
                crate::components::TaskMetadataStrip {
                    path: rel_path.clone(),
                    frontmatter: frontmatter.clone(),
                    boards: ReadSignal::<Vec<flynt_core::models::Board>>::from(boards_cache),
                    engagements: ReadSignal::<Vec<flynt_core::models::Engagement>>::from(engagements_cache),
                }
            }

            div { class: "notes-scroll",
            // Excalidraw and .flow files get their own editor; everything
            // else goes through the markdown editor.
            {
            let check_path = rel_path.clone();
            let is_special =
                crate::views::excalidraw::is_excalidraw(&check_path)
                || crate::views::flow::is_flow(&check_path);
            rsx! {
            if crate::views::excalidraw::is_excalidraw(&check_path) {
                crate::views::ExcalidrawView { path: rel_path.clone() }
            } else if crate::views::flow::is_flow(&check_path) {
                crate::views::FlowView { path: rel_path.clone() }
            }

            match *mode.read() {
                EditMode::Live if !is_special => {
                    rsx! {
                        div {
                            id: "flynt-cm-editor",
                            class: "cm-editor-container",
                        }
                    }
                },
                EditMode::Live => rsx! {},
                // Source mode is disabled for special files — the markdown
                // editor would rewrite a .flow JSON body (or .excalidraw
                // scene) as plain text on save, corrupting the file. The
                // mode toggle is still rendered (`EditMode::Source` is the
                // user's stated intent) but the source-editor body
                // short-circuits to empty for these kinds.
                EditMode::Source if is_special => rsx! {},
                EditMode::Source => {
                    let path_save = rel_path.clone();
                    rsx! {
                        { document::eval(r#"(function(){
                            const ed=document.getElementById('flynt-editor');
                            const pr=document.getElementById('flynt-preview');
                            if(typeof hljs!=='undefined') pr&&pr.querySelectorAll('pre code:not([data-highlighted])').forEach(b=>hljs.highlightElement(b));
                            if(!ed||!pr||ed._flynt_bound)return;
                            ed._flynt_bound=true;
                            let busy=false;
                            ed.addEventListener('scroll',function(){if(busy)return;busy=true;const p=ed.scrollTop/Math.max(1,ed.scrollHeight-ed.clientHeight);pr.scrollTop=p*(pr.scrollHeight-pr.clientHeight);requestAnimationFrame(()=>busy=false);});
                            pr.addEventListener('scroll',function(){if(busy)return;busy=true;const p=pr.scrollTop/Math.max(1,pr.scrollHeight-pr.clientHeight);ed.scrollTop=p*(ed.scrollHeight-ed.clientHeight);requestAnimationFrame(()=>busy=false);});
                        })();"#); }
                        div { class: "editor-split",
                            div { class: "editor-pane",
                                textarea {
                                    id: "flynt-editor",
                                    class: "editor-textarea",
                                    value: "{edit_body}",
                                    oninput: move |e| *edit_body.write() = e.value(),
                                    onkeydown: move |e| {
                                        let save_key = e.modifiers().meta() || e.modifiers().ctrl();
                                        if save_key && e.key() == Key::Character("s".to_string()) {
                                            let content = edit_body.read().clone();
                                            let p       = path_save.clone();
                                            let c       = ctx_save2.clone();

                                            spawn(async move {
                                                let project = c.project();
                                                match tokio::task::spawn_blocking(move || {
                                                    project.save_document_content(&p, &content)
                                                }).await {
                                                    Ok(Ok(())) => {
                                                        render_ver += 1;
                                                        *save_err.write() = None;
                                                        *save_state.write() = SaveState::Saved;
                                                    }
                                                    Ok(Err(e)) => *save_err.write() = Some(format!("Could not save — {e}")),
                                                    Err(e)     => *save_err.write() = Some(format!("Save interrupted — {e}")),
                                                }
                                            });
                                            // ⌘S stays in Source mode; the
                                            // operator clicks Live explicitly.
                                        }
                                    },
                                }
                            }
                            div { class: "editor-divider" }
                            div {
                                id: "flynt-preview",
                                class: "preview-pane",
                                div {
                                    class: "markdown-body",
                                    // Source mode renders directly from edit_body
                                    // so the preview tracks keystrokes live. The
                                    // cached `rendered_html` is for Live mode's
                                    // post-save HTML — using it here would mean
                                    // the preview lags behind by a save cycle and
                                    // operators see no update while typing.
                                    dangerous_inner_html: "{render_html(&edit_body.read())}",
                                }
                            }
                        }
                    }
                },
            }
            } // rsx block
            } // check_path scope
            } // notes-scroll
        }
        if *inspector_open.read() {
            NoteInspector {
                tab: inspector_tab,
                body: edit_body.read().clone(),
                frontmatter: frontmatter.clone(),
                link_context: link_context.read().clone().flatten(),
                on_close: move |_| *inspector_open.write() = false,
                on_open_doc: move |doc: DocumentMeta| {
                    tab_state.write().open(doc.id.clone(), doc.title.clone());
                },
                on_jump_line: move |line: usize| {
                    let js = format!(
                        r#"(function(){{
                            if(window._flyntCM){{
                                const line = window._flyntCM.state.doc.line(Math.max(1, {line}));
                                window._flyntCM.dispatch({{selection: {{anchor: line.from}}, effects: window.CM.EditorView.scrollIntoView(line.from, {{y: "start", yMargin: 24}})}});
                                window._flyntCM.focus();
                                return;
                            }}
                            const ed = document.getElementById('flynt-editor');
                            if(ed){{
                                const lines = ed.value.split('\n');
                                let pos = 0;
                                for(let i = 0; i < Math.max(0, {line} - 1) && i < lines.length; i++) pos += lines[i].length + 1;
                                ed.focus();
                                ed.setSelectionRange(pos, pos);
                                ed.scrollTop = Math.max(0, ({line} - 1) * 24);
                            }}
                        }})();"#
                    );
                    document::eval(&js);
                },
            }
        }
        if *history_open.read() {
            NoteHistoryModal {
                path: history_modal_path.clone(),
                state: history_state.read().clone().flatten(),
                snapshot: history_snapshot.read().clone(),
                current_body: edit_body.read().clone(),
                snapshot_error: history_snapshot_error.read().clone(),
                restore_message: history_restore_message.read().clone(),
                on_close: move |_| *history_open.write() = false,
                on_select_commit: move |commit: String| {
                    *history_snapshot.write() = None;
                    *history_snapshot_error.write() = None;
                    *history_restore_message.write() = None;
                    let c = ctx.clone();
                    let p = history_select_path.clone();
                    spawn(async move {
                        let project = c.project();
                        let (remote, branch) = match &project.config.sync {
                            flynt_core::models::SyncConfig::Git { remote, branch, .. } => {
                                (remote.clone(), branch.clone())
                            }
                            _ => ("origin".into(), "main".into()),
                        };
                        let result = tokio::task::spawn_blocking(move || {
                            let git = GitSync::new(project.root.clone(), remote, branch);
                            git.read_file_at_commit(&p, &commit)
                        })
                        .await;
                        match result {
                            Ok(Ok(snapshot)) => *history_snapshot.write() = Some(snapshot),
                            Ok(Err(e)) => *history_snapshot_error.write() = Some(format!("Could not load snapshot: {e}")),
                            Err(e) => *history_snapshot_error.write() = Some(format!("Snapshot load interrupted: {e}")),
                        }
                    });
                },
                on_restore_snapshot: move |snapshot: FileSnapshot| {
                    *history_restore_message.write() = None;
                    let c = ctx.clone();
                    let original_path = history_restore_path.clone();
                    spawn(async move {
                        let project = c.project();
                        let short = snapshot.commit.chars().take(7).collect::<String>();
                        let stem = original_path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("note");
                        let recovered = std::path::PathBuf::from("Recovered")
                            .join(format!("{stem} {short}.md"));
                        let save_result = project.save_document_content(&recovered, &snapshot.content);
                        match save_result {
                            Ok(()) => {
                                let _ = project.reindex();
                                *render_ver.write() += 1;
                                if let Ok(Some(meta)) = project
                                    .store
                                    .get_document_by_path(&recovered)
                                    .map(|doc| doc.map(|doc| DocumentMeta {
                                        id: doc.id,
                                        path: doc.path,
                                        title: doc.title,
                                        tags: doc.frontmatter.tags,
                                        metadata: Default::default(),
                                        entity_kind: doc.entity.map(|entity| entity.kind),
                                        updated_at: doc.updated_at,
                                    }))
                                {
                                    tab_state.write().open(meta.id.clone(), meta.title.clone());
                                }
                                *history_restore_message.write() = Some(format!("Restored copy to {}", recovered.display()));
                            }
                            Err(e) => *history_snapshot_error.write() = Some(format!("Restore failed: {e}")),
                        }
                    });
                },
            }
        }
        }
    }
}
