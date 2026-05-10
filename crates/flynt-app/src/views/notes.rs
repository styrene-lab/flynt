use flynt_core::store::VaultStore;
use comrak::{Options, markdown_to_html};
use dioxus::prelude::*;
use crate::{bootstrap::AppContext, state::{Route, TabState}};

#[derive(Clone, PartialEq)]
enum EditMode { Live, Source }

#[derive(Clone, PartialEq)]
#[allow(dead_code)] // Dirty is set via JS DOM manipulation, not Rust
enum SaveState { Clean, Dirty, Saved }

fn render_html(content: &str) -> String {
    render_html_with_store(content, None, None)
}

fn render_html_with_store(content: &str, store: Option<&dyn flynt_core::store::VaultStore>, vault_root: Option<&std::path::Path>) -> String {
    let mut opts = Options::default();
    opts.extension.table                      = true;
    opts.extension.strikethrough              = true;
    opts.extension.tasklist                   = true;
    opts.extension.autolink                   = true;
    opts.extension.footnotes                  = true;
    opts.extension.wikilinks_title_after_pipe = true;
    opts.render.unsafe_                       = true;
    let mut html = postprocess_html(markdown_to_html(&preprocess(content), &opts));

    // Execute inline query blocks: <pre><code class="language-query">...</code></pre>
    if let Some(store) = store {
        while let Some(start) = html.find("<code class=\"language-query\">") {
            let code_start = start + "<code class=\"language-query\">".len();
            let Some(code_end) = html[code_start..].find("</code>") else { break; };
            let code_end = code_start + code_end;

            // Find the wrapping <pre>
            let pre_start = html[..start].rfind("<pre>").unwrap_or(start);
            let pre_end = html[code_end..].find("</pre>").map(|p| code_end + p + 6).unwrap_or(code_end + 7);

            let query_source = html_unescape(&html[code_start..code_end]);
            let result = match flynt_core::query::execute_query(&query_source, store) {
                Ok(rendered) => format!("<div class=\"query-result\">{rendered}</div>"),
                Err(e) => format!("<div class=\"query-error\">This query could not run: {e}<br><small>Syntax: <code>TABLE title, tags FROM \"\" WHERE tags = \"#tag\" SORT title</code></small></div>"),
            };

            html = format!("{}{}{}", &html[..pre_start], result, &html[pre_end..]);
        }
    }

    // Embed Excalidraw drawings: ![[file.excalidraw]] → inline SVG
    // Also handles image embeds: ![[image.png]] → <img src="vault://...">
    if let Some(root) = vault_root {
        // Pattern: ![[something.excalidraw]] (may appear as text or inside <p> tags)
        while let Some(start) = html.find("![[") {
            let Some(end) = html[start..].find("]]") else { break; };
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
                let candidates = [
                    root.join(file_ref),
                    root.join("drawings").join(file_ref),
                ];
                let excalidraw_path = candidates.iter()
                    .find(|p| p.exists())
                    .cloned()
                    .unwrap_or_else(|| root.join(file_ref));
                let svg_path = excalidraw_path.with_extension("svg");
                let style = width.map(|w| format!(" style=\"max-width:{w}px\"")).unwrap_or_default();
                let escaped_ref = file_ref.replace('"', "&quot;");

                let replacement = if svg_path.exists() {
                    match std::fs::read_to_string(&svg_path) {
                        Ok(svg) => format!(
                            "<div class=\"excalidraw-embed\" data-drawing=\"{escaped_ref}\"{style}>{svg}</div>"
                        ),
                        Err(_) => format!("<div class=\"excalidraw-embed-placeholder\">[Drawing: {file_ref}]</div>"),
                    }
                } else if excalidraw_path.exists() {
                    format!("<div class=\"excalidraw-embed-placeholder\" data-drawing=\"{escaped_ref}\">[Drawing: {file_ref} — open to render]</div>")
                } else {
                    format!("<span class=\"broken-embed\">Embedded file not found: {file_ref}</span>")
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
                let d2_path = candidates.iter()
                    .find(|p| p.exists())
                    .cloned()
                    .unwrap_or_else(|| root.join(file_ref));
                let svg_path = d2_path.with_extension("svg");
                let style = width.map(|w| format!(" style=\"max-width:{w}px\"")).unwrap_or_default();

                let replacement = if svg_path.exists() {
                    match std::fs::read_to_string(&svg_path) {
                        Ok(svg) => format!(
                            "<div class=\"d2-embed\"{style}>{svg}</div>"
                        ),
                        Err(_) => format!("<div class=\"d2-embed-placeholder\">[Diagram: {file_ref}]</div>"),
                    }
                } else if d2_path.exists() {
                    format!("<div class=\"d2-embed-placeholder\">[Diagram: {file_ref} — rendering not available]</div>")
                } else {
                    format!("<span class=\"broken-embed\">Diagram file not found: {file_ref}</span>")
                };

                html = format!("{}{}{}", &html[..start], replacement, &html[end + 2..]);
            } else if ref_name.ends_with(".png") || ref_name.ends_with(".jpg") || ref_name.ends_with(".jpeg") || ref_name.ends_with(".gif") || ref_name.ends_with(".svg") || ref_name.ends_with(".webp") {
                // Image embed — resolve path, searching common locations
                let image_candidates = [
                    ref_name.to_string(),
                    format!("assets/{ref_name}"),
                    format!("images/{ref_name}"),
                    format!("drawings/{ref_name}"),
                ];
                let resolved = image_candidates.iter()
                    .find(|p| root.join(p).exists())
                    .cloned()
                    .unwrap_or_else(|| ref_name.to_string());
                let encoded = resolved.replace(' ', "%20");
                let replacement = format!("<img class=\"embedded-image\" src=\"vault://localhost/{encoded}\" alt=\"{ref_name}\" />");
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
    let mut rest   = html.as_str();
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
            out.push_str(&format!("[{display}](flynt-note://{encoded})"));
            continue;
        }
        out.push(c);
    }
    out
}

// ── CM6 init JS ─────────────────────────────────────────────────────────────

fn cm6_init_js(content: &str) -> String {
    let escaped = serde_json::to_string(content).unwrap_or_else(|_| "\"\"".into());
    format!(r#"
(function() {{
    function _initCM() {{
    const container = document.getElementById('flynt-cm-editor');
    if (!container) {{ setTimeout(_initCM, 16); return; }}

    console.time('cm6-total');
    // Fast path: if CM6 already exists, just swap the document content.
    if (window._flyntCM) {{
        console.time('cm6-swap');
        const newContent = {escaped};
        const cm = window._flyntCM;
        cm.dispatch({{
            changes: {{ from: 0, to: cm.state.doc.length, insert: newContent }}
        }});
        cm.scrollDOM.scrollTop = 0;
        console.timeEnd('cm6-swap');
        console.timeEnd('cm6-total');
        return;
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
                img.src = 'vault://localhost/' + encodeURIComponent(this._ref).replace(/%2F/g, '/');
                img.alt = this._ref;
                img.onerror = () => {{
                    // Try common subdirs
                    const dirs = ['assets/', 'images/', 'drawings/'];
                    let tried = 0;
                    function tryNext() {{
                        if (tried >= dirs.length) {{ img.replaceWith(document.createTextNode('[Image: ' + img.alt + ']')); return; }}
                        img.src = 'vault://localhost/' + dirs[tried++] + encodeURIComponent(img.alt).replace(/%2F/g, '/');
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
"#)
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

// ── Notes view ──────────────────────────────────────────────────────────────

#[component]
pub fn NotesView() -> Element {
    let ctx       = use_context::<AppContext>();
    let tab_state = use_context::<Signal<TabState>>();
    let mut is_drawing = use_context::<Signal<bool>>();
    let ctx_res   = ctx.clone();
    let ctx_save2 = ctx.clone();

    let mut mode       = use_signal(|| EditMode::Live);
    let mut edit_body  = use_signal(String::new);
    let mut save_err   = use_signal(|| Option::<String>::None);
    let mut save_state = use_signal(|| SaveState::Clean);
    let mut render_ver = use_signal(|| 0u32);
    let mut conflict_detected = use_signal(|| false);

    // Render cache — avoids re-rendering when switching between tabs
    let mut render_cache: Signal<std::collections::HashMap<
        flynt_core::models::DocumentId,
        (std::path::PathBuf, String, String, String, bool),
    >> = use_signal(std::collections::HashMap::new);

    // ── Two-phase rendering ───────────────────────────────────────────
    // Phase 1 (instant): read document from SQLite synchronously — <1ms.
    //   Sets edit_body and raw content immediately so the editor is responsive.
    // Phase 2 (background): render HTML via comrak + query execution.
    //   Swaps in when ready. Cached for instant tab switching.

    // Render cache: doc_id → (path, title, body, html, has_conflicts)
    let mut render_cache: Signal<std::collections::HashMap<
        flynt_core::models::DocumentId,
        (std::path::PathBuf, String, String, String, bool),
    >> = use_signal(std::collections::HashMap::new);

    // Invalidate cache on save
    use_effect(move || {
        let _ver = *render_ver.read();
        if _ver > 0 {
            if let Some(id) = tab_state.read().active_id().cloned() {
                render_cache.write().remove(&id);
            }
        }
    });

    // Phase 1: synchronous document read — no spawn_blocking, no async overhead
    let mut doc_data: Signal<Option<(std::path::PathBuf, String, String)>> = use_signal(|| None);
    use_effect(move || {
        let _ver = *render_ver.read();
        let selected_id = tab_state.read().active_id().cloned();
        let Some(doc_id) = selected_id else {
            *doc_data.write() = None;
            return;
        };
        // Synchronous SQLite read — <1ms for any document
        let vault = ctx_res.vault();
        if let Ok(Some(doc)) = vault.store.get_document(&doc_id) {
            *doc_data.write() = Some((doc.path.clone(), doc.title.clone(), doc.content.clone()));
        }
    });

    // Phase 2: background HTML rendering — fires after doc_data is set
    let rendered: Resource<Option<(std::path::PathBuf, String, String, String, bool)>> = use_resource(move || {
        let _ver = *render_ver.read();
        let selected_id = tab_state.read().active_id().cloned();
        let vault = ctx_res.vault();
        async move {
            let Some(doc_id) = selected_id else { return None; };

            // Cache hit — instant
            if let Some(cached) = render_cache.read().get(&doc_id) {
                return Some(cached.clone());
            }

            // Background render — won't block the UI
            let cache_id = doc_id.clone();
            let result = tokio::task::spawn_blocking(move || {
                vault.store.get_document(&doc_id).ok().flatten().map(|doc| {
                    let html = render_html_with_store(&doc.content, Some(&*vault.store), Some(&vault.root));
                    let has_conflicts = flynt_core::conflict::has_conflict_markers(&doc.content);
                    (doc.path.clone(), doc.title.clone(), doc.content.clone(), html, has_conflicts)
                })
            })
            .await.ok().flatten();

            if let Some(ref r) = result {
                *conflict_detected.write() = r.4;
                render_cache.write().insert(cache_id, r.clone());
            }
            result
        }
    });

    // Sync edit_body from phase 1 (instant) — don't wait for HTML render
    let mut synced_doc_id: Signal<Option<flynt_core::models::DocumentId>> = use_signal(|| None);
    use_effect(move || {
        let current_id = tab_state.read().active_id().cloned();
        if current_id == *synced_doc_id.peek() { return; }
        // Try phase 1 data first (immediate), fall back to rendered cache
        if let Some((_, _, body)) = &*doc_data.read() {
            *synced_doc_id.write() = current_id;
            *edit_body.write() = body.clone();
            *save_state.write() = SaveState::Clean;
        }
    });

    let has_active = tab_state.read().active_id().is_some();

    // Initialize CM6 when: new document loaded OR mode switched back to Live
    let is_drawing_mode = use_context::<Signal<bool>>();
    use_effect(move || {
        // Gate on synced_doc_id (not on a tab-change ver). synced_doc_id
        // is set AFTER doc_data → edit_body have populated, so by the
        // time this fires, edit_body has the body for the new tab.
        // Previously this gated on a manual ver counter that bumped on
        // tab change, racing the doc_data effect — empty edit_body
        // caused init to bail and CM6 stayed blank.
        let synced = synced_doc_id.read().clone();
        if synced.is_none() { return; }
        if *is_drawing_mode.read() { return; }
        if !matches!(&*mode.read(), EditMode::Live) { return; }
        tracing::info!("CM6 init effect triggered for synced_doc_id={:?}", synced);
        // edit_body is now the post-sync source of truth; rendered is a
        // fallback for documents loaded via the slow path.
        let content = {
            let eb = edit_body.peek().clone();
            if !eb.is_empty() {
                eb
            } else if let Some(Some((_, _, body, _, _))) = &*rendered.peek() {
                body.clone()
            } else {
                // synced is Some but edit_body is empty AND rendered
                // empty — genuinely empty document. Init CM6 with empty
                // string so the editor is ready for input.
                String::new()
            }
        };
        document::eval(&cm6_init_js(&content));
    });

    // Persistent message bridge — one eval that polls a global queue.
    // CM6 pushes messages to the queue; this loop drains them to Rust.
    let ctx_link = ctx.clone();
    let mut ts_link  = tab_state;
    let mut ar_link  = use_context::<Signal<Route>>();
    use_effect(move || {
        let mut eval = document::eval(BRIDGE_JS);
        let c = ctx_link.clone();

        spawn(async move {
            loop {
                let Ok(val) = eval.recv::<String>().await else { break; };

                let Ok(msg) = serde_json::from_str::<serde_json::Value>(&val) else {
                    continue;
                };
                let msg_type = msg["type"].as_str().unwrap_or("");
                let data = msg["data"].as_str().unwrap_or("");

                match msg_type {
                    "edit" => {
                        // Do NOT write to edit_body here — it triggers Dioxus re-render
                        // which destroys and recreates CM6. edit_body is synced from CM6
                        // directly when switching to source mode.
                        // Update save indicator via DOM — no Dioxus signal write
                        document::eval("document.querySelectorAll('.save-status').forEach(e => {{ e.textContent = 'unsaved'; e.className = 'save-status dirty'; }});");
                    }
                    "save" | "autosave" => {
                        let content = data.to_string();
                        // peek — do NOT subscribe reactively
                        if let Some(Some((p, _, _, _, _))) = &*rendered.peek() {
                            let path = p.clone();
                            let vault = c.vault();
                            match tokio::task::spawn_blocking(move || {
                                vault.save_document_content(&path, &content)
                            }).await {
                                Ok(Ok(())) => {
                                    // Update save indicator via DOM — no signal write
                                    document::eval("document.querySelectorAll('.save-status').forEach(e => {{ e.textContent = 'saved'; e.className = 'save-status saved'; }});");
                                }
                                Ok(Err(e)) => *save_err.write() = Some(format!("Could not save — {e}")),
                                Err(e) => *save_err.write() = Some(format!("Save interrupted — {e}")),
                            }
                        }
                    }
                    "mode" => {
                        if data == "source" {
                            // Sync edit_body from CM6 before switching
                            let mut sync_eval = document::eval("if(window._flyntCM){dioxus.send(window._flyntCM.state.doc.toString())}else{dioxus.send('')}");
                            if let Ok(content) = sync_eval.recv::<String>().await {
                                if !content.is_empty() {
                                    *edit_body.write() = content;
                                }
                            }
                            *mode.write() = EditMode::Source;
                        }
                    }
                    "open-drawing" => {
                        // Open the excalidraw wrapper .md in a tab — NotesView
                        // detects the embed and renders ExcalidrawView automatically
                        let drawing_file = data.to_string();
                        let slug = drawing_file.replace(".excalidraw", "").to_lowercase();
                        let vault = c.vault();
                        if let Ok(Some(meta)) = tokio::task::spawn_blocking(move || {
                            vault.store.find_document_by_slug(&slug)
                        }).await.unwrap_or(Ok(None)) {
                            ts_link.write().open(meta.id.clone(), meta.title.clone());
                            *ar_link.write() = Route::Notes;
                        }
                    }
                    "nav" => {
                        let slug = data.to_lowercase();
                        let vault = c.vault();
                        if let Ok(Some(meta)) = tokio::task::spawn_blocking(move || {
                            vault.store.find_document_by_slug(&slug)
                        }).await.unwrap_or(Ok(None)) {
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
    let Some((rel_path, title, body)) = doc_data.read().clone() else {
        return rsx! {
            crate::components::TabBar {}
            if has_active {
                div { class: "notes-loading muted", "Loading…" }
            }
        };
    };

    // HTML from background render (may not be ready yet)
    let rendered_html = rendered.read().as_ref()
        .and_then(|opt| opt.as_ref())
        .map(|(_, _, _, html, _)| html.clone());

    // If this document is an excalidraw wrapper, render ExcalidrawView directly
    if let Some(excalidraw_file) = crate::views::excalidraw::excalidraw_embed_path(&body) {
        let vault_root = ctx.vault_root();
        // Resolve the .excalidraw file relative to the document's directory
        let doc_dir = rel_path.parent().unwrap_or(std::path::Path::new(""));
        let excalidraw_path = doc_dir.join(&excalidraw_file);
        let abs = vault_root.join(&excalidraw_path);
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
        rel_path.file_stem()
            .map(|s| format!("{}.canvas", s.to_string_lossy()))
    } else {
        None
    };
    if let Some(canvas_file) = canvas_file_from_body.or(canvas_file_from_recovery) {
        let vault_root = ctx.vault_root();
        let doc_dir = rel_path.parent().unwrap_or(std::path::Path::new(""));
        let canvas_path = doc_dir.join(&canvas_file);
        let abs = vault_root.join(&canvas_path);
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

    // Clear drawing mode flag
    is_drawing.set(false);

    // edit_body is seeded by the use_effect that watches rendered,
    // and synced from CM6 on mode switch. No eager write here.

    let title = title.clone();
    let _body  = body.clone();
    let path  = rel_path.clone();

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

    rsx! {
        crate::components::TabBar {}
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
                                        let vault = c.vault();
                                        let _ = vault.save_document_content(&path, &resolved);
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
                                        let vault = c.vault();
                                        let _ = vault.save_document_content(&path, &resolved);
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
                                        let vault = c.vault();
                                        match tokio::task::spawn_blocking(move || {
                                            vault.rename_document(&p, &new_title)
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
                                    let vault = c.vault();
                                    match tokio::task::spawn_blocking(move || {
                                        vault.rename_document(&p, &new_title)
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
                                        let vault = c.vault();
                                        match tokio::task::spawn_blocking(move || {
                                            vault.save_document_content(&p, &content)
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
                                    *mode.write() = EditMode::Live;
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

            div { class: "notes-scroll",
            // Excalidraw files get their own editor
            {
            let check_path = rel_path.clone();
            let check_path2 = rel_path.clone();
            rsx! {
            if crate::views::excalidraw::is_excalidraw(&check_path) {
                crate::views::ExcalidrawView { path: rel_path.clone() }
            }

            match *mode.read() {
                EditMode::Live if !crate::views::excalidraw::is_excalidraw(&check_path2) => {
                    rsx! {
                        div {
                            id: "flynt-cm-editor",
                            class: "cm-editor-container",
                        }
                    }
                },
                EditMode::Live => rsx! {},
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
                                                let vault = c.vault();
                                                match tokio::task::spawn_blocking(move || {
                                                    vault.save_document_content(&p, &content)
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
                                            *mode.write() = EditMode::Live;
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
                                    dangerous_inner_html: if let Some(ref cached_html) = rendered_html {
                                        "{cached_html}"
                                    } else {
                                        "{render_html(&edit_body.read())}"
                                    },
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
    }
}
