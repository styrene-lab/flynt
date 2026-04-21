use codex_core::store::VaultStore;
use comrak::{Options, markdown_to_html};
use dioxus::prelude::*;
use crate::{bootstrap::AppContext, state::{Route, TabState}};

#[derive(Clone, PartialEq)]
enum EditMode { Live, Source }

#[derive(Clone, PartialEq)]
enum SaveState { Clean, Dirty, Saved }

fn render_html(content: &str) -> String {
    render_html_with_store(content, None, None)
}

fn render_html_with_store(content: &str, store: Option<&dyn codex_core::store::VaultStore>, vault_root: Option<&std::path::Path>) -> String {
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
            let result = match codex_core::query::execute_query(&query_source, store) {
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

                let excalidraw_path = root.join(file_ref);
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
                    format!("<div class=\"excalidraw-embed-placeholder\" data-drawing=\"{escaped_ref}\">[Drawing: {file_ref} — save to auto-export SVG]</div>")
                } else {
                    format!("<span class=\"broken-embed\">Embedded file not found: {file_ref}</span>")
                };

                html = format!("{}{}{}", &html[..start], replacement, &html[end + 2..]);
            } else if ref_name.ends_with(".png") || ref_name.ends_with(".jpg") || ref_name.ends_with(".jpeg") || ref_name.ends_with(".gif") || ref_name.ends_with(".svg") || ref_name.ends_with(".webp") {
                // Image embed
                let replacement = format!("<img class=\"embedded-image\" src=\"vault://localhost/{ref_name}\" alt=\"{ref_name}\" />");
                html = format!("{}{}{}", &html[..start], replacement, &html[end + 2..]);
            } else {
                break; // not an embed we handle — avoid infinite loop
            }
        }
    }

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

// ── CM6 init JS ─────────────────────────────────────────────────────────────

fn cm6_init_js(content: &str) -> String {
    let escaped = serde_json::to_string(content).unwrap_or_else(|_| "\"\"".into());
    format!(r#"
(function() {{
    function _initCM() {{
    const container = document.getElementById('codex-cm-editor');
    if (!container) {{ setTimeout(_initCM, 16); return; }}

    if (window._codexCM) {{
        window._codexCM.destroy();
        window._codexCM = null;
    }}
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
    }} = CM;

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

    const codexTheme = EditorView.theme({{
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

    const codexHighlight = HighlightStyle.define([
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
    const hideMarkupPlugin = EditorView.decorations.compute(['doc', 'selection'], (state) => {{ try {{
        const decs = [];
        const sel = state.selection.main;
        const activeLine = state.doc.lineAt(sel.head).number;
        const doc = state.doc;

        for (let i = 1; i <= doc.lines; i++) {{
            if (i === activeLine) continue; // show markup on cursor line
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

            // Hide bold markers: **text**
            let idx = 0;
            while ((idx = text.indexOf('**', idx)) !== -1) {{
                const end = text.indexOf('**', idx + 2);
                if (end > idx) {{
                    decs.push(Decoration.replace({{}}).range(line.from + idx, line.from + idx + 2));
                    decs.push(Decoration.replace({{}}).range(line.from + end, line.from + end + 2));
                    idx = end + 2;
                }} else break;
            }}

            // Hide wikilink brackets: [[target]] or [[target|display]]
            idx = 0;
            while ((idx = text.indexOf('[[', idx)) !== -1) {{
                const end = text.indexOf(']]', idx + 2);
                if (end > idx) {{
                    const inner = text.substring(idx + 2, end);
                    const pipe = inner.indexOf('|');
                    if (pipe >= 0) {{
                        // [[target|display]] → hide [[ + target + | and ]]
                        decs.push(Decoration.replace({{}}).range(line.from + idx, line.from + idx + 2 + pipe + 1));
                        decs.push(Decoration.replace({{}}).range(line.from + end, line.from + end + 2));
                    }} else {{
                        // [[target]] → hide [[ and ]]
                        decs.push(Decoration.replace({{}}).range(line.from + idx, line.from + idx + 2));
                        decs.push(Decoration.replace({{}}).range(line.from + end, line.from + end + 2));
                    }}
                    idx = end + 2;
                }} else break;
            }}

            // Hide inline code backticks
            idx = 0;
            while ((idx = text.indexOf('`', idx)) !== -1) {{
                if (text.charAt(idx + 1) === '`') {{ idx += 2; continue; }} // skip ``
                const end = text.indexOf('`', idx + 1);
                if (end > idx) {{
                    decs.push(Decoration.replace({{}}).range(line.from + idx, line.from + idx + 1));
                    decs.push(Decoration.replace({{}}).range(line.from + end, line.from + end + 1));
                    idx = end + 1;
                }} else break;
            }}

            // Hide underscore italic/bold: _text_ and __text__
            idx = 0;
            while ((idx = text.indexOf('__', idx)) !== -1) {{
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

        // Sort by position (required by CM6)
        decs.sort((a, b) => a.from - b.from || a.startSide - b.startSide);
        return Decoration.set(decs);
    }} catch(e) {{ console.error('hideMarkup error:', e); return Decoration.none; }}
    }});

    // ── Table styling: add CSS classes to table lines ──
    const tablePlugin = EditorView.decorations.compute(['doc'], (state) => {{
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

    let saveTimer = null;
    const changeHandler = EditorView.updateListener.of((update) => {{
        if (update.docChanged) {{
            clearTimeout(saveTimer);
            const doc = update.state.doc.toString();
            // Immediately sync to Rust state
            window._codexNotify('edit', doc);
            // Debounced auto-save
            saveTimer = setTimeout(() => window._codexNotify('autosave', doc), 2000);
        }}
    }});

    const saveKeymap = keymap.of([{{
        key: 'Mod-s',
        run: (view) => {{
            window._codexNotify('save', view.state.doc.toString());
            return true;
        }},
    }}, {{
        key: 'Mod-e',
        run: () => {{
            window._codexNotify('mode', 'source');
            return true;
        }},
    }}]);

    const docText = {escaped};
    const state = EditorState.create({{
        doc: docText,
        selection: {{ anchor: docText.length }},
        extensions: [
            codexTheme,
            syntaxHighlighting(codexHighlight),
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
            changeHandler,
            hideMarkupPlugin,
            tablePlugin,
            codeBlockPlugin,
            EditorView.lineWrapping,
        ],
    }});

    window._codexCM = new EditorView({{ state, parent: container }});
    window._codexCM.focus();
    }} // end _initCM
    _initCM();
}})();
"#)
}

// ── Notification bridge JS ──────────────────────────────────────────────────
// Uses a global function + polling eval to decouple CM6 lifecycle from
// the Dioxus eval channel. CM6 calls window._codexNotify(type, data),
// which queues messages. A persistent eval loop drains the queue.

const BRIDGE_JS: &str = r#"
if (!window._codexQueue) {
    window._codexQueue = [];
    window._codexNotify = function(type, data) {
        window._codexQueue.push(JSON.stringify({type: type, data: data}));
    };
}

// Drain loop — sends queued messages to Rust via this eval's channel
async function _codexDrain() {
    while (true) {
        if (window._codexQueue.length > 0) {
            const msg = window._codexQueue.shift();
            dioxus.send(msg);
        } else {
            await new Promise(r => setTimeout(r, 50));
        }
    }
}
_codexDrain();

// Click-to-edit for Excalidraw embeds
document.addEventListener('click', function(e) {
    const embed = e.target.closest('.excalidraw-embed[data-drawing]');
    if (embed) {
        const drawing = embed.getAttribute('data-drawing');
        if (drawing) {
            window._codexNotify('open-drawing', drawing);
        }
    }
});
"#;

// ── Notes view ──────────────────────────────────────────────────────────────

#[component]
pub fn NotesView() -> Element {
    let ctx       = use_context::<AppContext>();
    let tab_state = use_context::<Signal<TabState>>();
    let ctx_res   = ctx.clone();
    let ctx_save2 = ctx.clone();

    let mut mode       = use_signal(|| EditMode::Live);
    let mut edit_body  = use_signal(String::new);
    let mut save_err   = use_signal(|| Option::<String>::None);
    let mut save_state = use_signal(|| SaveState::Clean);
    let mut render_ver = use_signal(|| 0u32);

    let rendered: Resource<Option<(std::path::PathBuf, String, String, String)>> = use_resource(move || {
        let _ver = *render_ver.read();
        let selected_id = tab_state.read().active_id().cloned();
        let vault = ctx_res.vault();
        async move {
            let Some(doc_id) = selected_id else { return None; };
            tokio::task::spawn_blocking(move || {
                vault.store.get_document(&doc_id).ok().flatten().map(|doc| {
                    let html = render_html_with_store(&doc.content, Some(&*vault.store), Some(&vault.root));
                    (doc.path.clone(), doc.title.clone(), doc.content.clone(), html)
                })
            })
            .await.ok().flatten()
        }
    });

    // Sync edit_body when a new document loads
    use_effect(move || {
        if let Some(Some((_, _, body, _))) = &*rendered.read() {
            *edit_body.write() = body.clone();
            *save_state.write() = SaveState::Clean;
        }
    });

    let has_active = tab_state.read().active_id().is_some();

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
                        *edit_body.write() = data.to_string();
                        *save_state.write() = SaveState::Dirty;
                    }
                    "save" | "autosave" => {
                        *edit_body.write() = data.to_string();
                        let content = data.to_string();
                        if let Some(Some((p, _, _, _))) = &*rendered.read() {
                            let path = p.clone();
                            let vault = c.vault();
                            match tokio::task::spawn_blocking(move || {
                                vault.save_document_content(&path, &content)
                            }).await {
                                Ok(Ok(())) => {
                                    *save_state.write() = SaveState::Saved;
                                    *save_err.write() = None;
                                }
                                Ok(Err(e)) => *save_err.write() = Some(format!("Could not save — {e}")),
                                Err(e) => *save_err.write() = Some(format!("Save interrupted — {e}")),
                            }
                        }
                    }
                    "mode" => {
                        if data == "source" {
                            // Sync edit_body from CM6 before switching
                            if let Some(cm) = None::<()> { let _ = cm; } // placeholder
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
            div { class: "notes-empty",
                div { class: "notes-empty-content",
                    div { class: "notes-empty-icon", dangerous_inner_html: crate::icons::ICON_SCROLL }
                    p { "Select a note from the sidebar" }
                    p { class: "notes-empty-hint", "or press + to create a new one" }
                }
            }
        };
    }

    let Some(data) = &*rendered.read() else {
        return rsx! {
            div { class: "notes-loading muted", "Loading…" }
        };
    };
    let Some((rel_path, title, body, _html)) = data else {
        return rsx! {
            div { class: "notes-empty",
                p { class: "muted", "This note may have been moved or deleted." }
                p { class: "muted", style: "font-size: 12px; margin-top: 8px;",
                    "Close this tab and select another note from the sidebar, or press \u{2318}N to create a new one."
                }
            }
        };
    };

    // If this document is an excalidraw wrapper, render ExcalidrawView directly
    if let Some(excalidraw_file) = crate::views::excalidraw::excalidraw_embed_path(body) {
        let vault_root = ctx.vault_root();
        // Resolve the .excalidraw file relative to the document's directory
        let doc_dir = rel_path.parent().unwrap_or(std::path::Path::new(""));
        let excalidraw_path = doc_dir.join(&excalidraw_file);
        let abs = vault_root.join(&excalidraw_path);
        if abs.exists() {
            return rsx! {
                div {
                    style: "display:flex;flex-direction:column;flex:1;overflow:hidden;padding:0;min-height:0;height:100%;",
                    crate::views::ExcalidrawView { path: excalidraw_path }
                }
            };
        }
    }

    // Eagerly seed edit_body if it's empty and we have content —
    // ensures CM6 has content even before use_effect fires.
    if edit_body.read().is_empty() && !body.is_empty() {
        *edit_body.write() = body.clone();
    }

    let title = title.clone();
    let _body  = body.clone();
    let path  = rel_path.clone();

    let mut renaming = use_signal(|| false);
    let mut rename_input = use_signal(|| title.clone());
    let mut rename_msg: Signal<Option<String>> = use_signal(|| None);
    let path_for_rename = path.clone();
    let ctx_rename = ctx.clone();

    rsx! {
        div { class: "notes-pane",
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
                    match *save_state.read() {
                        SaveState::Dirty => rsx! { span { class: "save-status dirty", title: "Unsaved changes", "unsaved" } },
                        SaveState::Saved => rsx! { span { class: "save-status saved", "saved" } },
                        SaveState::Clean => rsx! {},
                    }
                    if let Some(ref err) = *save_err.read() {
                        span { class: "save-msg err", "{err}" }
                    }
                    match *mode.read() {
                        EditMode::Live => rsx! {
                            span { class: "mode-hint", "⌘E source" }
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| *mode.write() = EditMode::Source,
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

            // Excalidraw files get their own editor
            if crate::views::excalidraw::is_excalidraw(rel_path) {
                crate::views::ExcalidrawView { path: rel_path.clone() }
            }

            match *mode.read() {
                EditMode::Live if !crate::views::excalidraw::is_excalidraw(rel_path) => {
                    let cm_content = edit_body.read().clone();
                    rsx! {
                        { document::eval(&cm6_init_js(&cm_content)); }
                        div {
                            id: "codex-cm-editor",
                            class: "cm-editor-container",
                        }
                    }
                },
                EditMode::Live => rsx! {},
                EditMode::Source => {
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
