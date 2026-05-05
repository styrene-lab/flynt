//! Convert HTML (as produced by comrak) back to Markdown.
//!
//! This is intentionally a lightweight, dependency-free converter that handles
//! the subset of HTML that comrak produces. It does not attempt to be a
//! general-purpose HTML-to-markdown engine.

/// Convert an HTML string (from contenteditable editing of comrak output)
/// back to canonical markdown.
pub fn html_to_markdown(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut chars = html.chars().peekable();

    while chars.peek().is_some() {
        if chars.peek() == Some(&'<') {
            let tag = read_tag(&mut chars);
            process_tag(&tag, &mut chars, &mut out, 0);
        } else {
            // Text outside any tag — shouldn't happen in well-formed HTML,
            // but handle gracefully
            let ch = chars.next().unwrap();
            out.push(decode_entity_char(ch));
        }
    }

    // Clean up excessive blank lines
    let cleaned = collapse_blank_lines(&out);
    cleaned.trim().to_string() + "\n"
}

// ── Tag processing ──────────────────────────────────────────────────────────

fn process_tag(
    tag: &str,
    chars: &mut std::iter::Peekable<std::str::Chars>,
    out: &mut String,
    depth: usize,
) {
    let tag_lower = tag.to_lowercase();

    // Self-closing tags
    if tag_lower.starts_with("br") || tag_lower.starts_with("br/")
        || tag_lower.starts_with("hr") || tag_lower.starts_with("hr/")
    {
        if tag_lower.starts_with("hr") {
            out.push_str("\n---\n\n");
        } else {
            out.push('\n');
        }
        return;
    }

    // Input (task list checkboxes)
    if tag_lower.starts_with("input") {
        let checked = tag_lower.contains("checked");
        if checked {
            out.push_str("[x] ");
        } else {
            out.push_str("[ ] ");
        }
        return;
    }

    // Image
    if tag_lower.starts_with("img") {
        let alt = extract_attr(&tag_lower, "alt").unwrap_or_default();
        let src = extract_attr(&tag_lower, "src").unwrap_or_default();
        // Reverse vault:// protocol to ![[wikilink]] syntax
        if let Some(filename) = src.strip_prefix("vault://localhost/") {
            let decoded = filename.replace("%20", " ");
            out.push_str(&format!("![[{decoded}]]"));
        } else {
            out.push_str(&format!("![{alt}]({src})"));
        }
        return;
    }

    // Headings
    if let Some(level) = heading_level(&tag_lower) {
        let content = collect_inner_markdown(chars, &format!("h{level}"), depth);
        let prefix = "#".repeat(level);
        out.push_str(&format!("\n{prefix} {}\n\n", content.trim()));
        return;
    }

    // Paragraph
    if tag_lower == "p" {
        let content = collect_inner_markdown(chars, "p", depth);
        out.push_str(&format!("{}\n\n", content.trim()));
        return;
    }

    // Bold
    if tag_lower == "strong" || tag_lower == "b" {
        let close = if tag_lower == "strong" { "strong" } else { "b" };
        let content = collect_inner_markdown(chars, close, depth);
        out.push_str(&format!("**{}**", content.trim()));
        return;
    }

    // Italic
    if tag_lower == "em" || tag_lower == "i" {
        let close = if tag_lower == "em" { "em" } else { "i" };
        let content = collect_inner_markdown(chars, close, depth);
        out.push_str(&format!("*{}*", content.trim()));
        return;
    }

    // Strikethrough
    if tag_lower == "del" {
        let content = collect_inner_markdown(chars, "del", depth);
        out.push_str(&format!("~~{}~~", content.trim()));
        return;
    }

    // Inline code
    if tag_lower == "code" && depth > 0 {
        // Inside a <pre>, this is a code block — handled by <pre>
        let content = collect_text_until_close(chars, "code");
        out.push_str(&format!("`{content}`"));
        return;
    }

    // Code block
    if tag_lower == "pre" {
        let inner = collect_text_until_close(chars, "pre");
        // Inner typically contains <code class="language-xxx">...</code>
        let (lang, code) = parse_code_block(&inner);
        out.push_str(&format!("\n```{lang}\n{code}\n```\n\n"));
        return;
    }

    // Links
    if tag_lower.starts_with("a ") || tag_lower == "a" {
        // Check for wikilink (data-flynt-note attribute)
        if let Some(slug) = extract_attr(&tag_lower, "data-flynt-note") {
            let content = collect_inner_markdown(chars, "a", depth);
            let decoded = slug.replace("%20", " ");
            if content.trim() == decoded {
                out.push_str(&format!("[[{decoded}]]"));
            } else {
                out.push_str(&format!("[[{}|{}]]", decoded, content.trim()));
            }
            return;
        }
        let href = extract_attr(&tag_lower, "href").unwrap_or_default();
        let content = collect_inner_markdown(chars, "a", depth);
        if href == "#" || href.is_empty() {
            out.push_str(&content);
        } else {
            out.push_str(&format!("[{}]({href})", content.trim()));
        }
        return;
    }

    // Unordered list
    if tag_lower == "ul" {
        let items = collect_list_items(chars, "ul", depth, false);
        for item in &items {
            out.push_str(&format!("- {}\n", item.trim()));
        }
        out.push('\n');
        return;
    }

    // Ordered list
    if tag_lower == "ol" {
        let items = collect_list_items(chars, "ol", depth, true);
        for (i, item) in items.iter().enumerate() {
            out.push_str(&format!("{}. {}\n", i + 1, item.trim()));
        }
        out.push('\n');
        return;
    }

    // Blockquote
    if tag_lower == "blockquote" {
        let content = collect_inner_markdown(chars, "blockquote", depth);
        for line in content.trim().lines() {
            out.push_str(&format!("> {line}\n"));
        }
        out.push('\n');
        return;
    }

    // Table
    if tag_lower == "table" {
        let table_html = collect_text_until_close(chars, "table");
        let table_md = convert_table(&table_html);
        out.push_str(&table_md);
        out.push('\n');
        return;
    }

    // Div, span, section — pass through contents
    if matches!(tag_lower.as_str(), "div" | "span" | "section" | "main" | "article") {
        let content = collect_inner_markdown(chars, &tag_lower, depth);
        out.push_str(&content);
        return;
    }

    // Unknown tags — collect and pass through content
    let close_tag = tag_lower.split_whitespace().next().unwrap_or(&tag_lower).to_string();
    let content = collect_inner_markdown(chars, &close_tag, depth);
    out.push_str(&content);
}

// ── HTML parsing helpers ────────────────────────────────────────────────────

fn read_tag(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    // Skip the opening '<'
    chars.next();
    let mut tag = String::new();
    for ch in chars.by_ref() {
        if ch == '>' {
            break;
        }
        tag.push(ch);
    }
    tag
}

/// Collect inner content as markdown until we hit the closing tag.
fn collect_inner_markdown(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    close_tag: &str,
    depth: usize,
) -> String {
    let mut inner = String::new();
    let _closing = format!("/{close_tag}");

    loop {
        match chars.peek() {
            None => break,
            Some(&'<') => {
                let tag = read_tag(chars);
                let tag_lower = tag.to_lowercase();
                let tag_name = tag_lower.split_whitespace().next().unwrap_or("").trim_start_matches('/');

                if tag_lower.starts_with('/') && tag_name == close_tag {
                    break;
                }
                process_tag(&tag, chars, &mut inner, depth + 1);
            }
            Some(_) => {
                let ch = chars.next().unwrap();
                inner.push(decode_entity_char(ch));
            }
        }
    }
    decode_entities(&inner)
}

/// Collect raw text (no markdown conversion) until closing tag.
fn collect_text_until_close(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    close_tag: &str,
) -> String {
    let mut text = String::new();
    let mut nesting = 1;

    loop {
        match chars.peek() {
            None => break,
            Some(&'<') => {
                let tag = read_tag(chars);
                let tag_lower = tag.to_lowercase();
                let tag_name = tag_lower.split_whitespace().next().unwrap_or("").trim_start_matches('/');

                if tag_lower.starts_with('/') && tag_name == close_tag {
                    nesting -= 1;
                    if nesting == 0 {
                        break;
                    }
                    text.push('<');
                    text.push_str(&tag);
                    text.push('>');
                } else if tag_name == close_tag && !tag_lower.ends_with('/') {
                    nesting += 1;
                    text.push('<');
                    text.push_str(&tag);
                    text.push('>');
                } else {
                    text.push('<');
                    text.push_str(&tag);
                    text.push('>');
                }
            }
            Some(_) => {
                text.push(chars.next().unwrap());
            }
        }
    }
    decode_entities(&text)
}

fn collect_list_items(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    close_tag: &str,
    depth: usize,
    _ordered: bool,
) -> Vec<String> {
    let mut items = Vec::new();

    loop {
        match chars.peek() {
            None => break,
            Some(&'<') => {
                let tag = read_tag(chars);
                let tag_lower = tag.to_lowercase();
                let tag_name = tag_lower.split_whitespace().next().unwrap_or("").trim_start_matches('/');

                if tag_lower.starts_with('/') && tag_name == close_tag {
                    break;
                }
                if tag_name == "li" && !tag_lower.starts_with('/') {
                    let content = collect_inner_markdown(chars, "li", depth + 1);
                    items.push(content.trim().to_string());
                }
                // Skip other tags within the list
            }
            Some(_) => {
                chars.next(); // skip whitespace between items
            }
        }
    }
    items
}

fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    let pattern = format!("{attr}=\"");
    if let Some(start) = tag.find(&pattern) {
        let after = &tag[start + pattern.len()..];
        if let Some(end) = after.find('"') {
            return Some(after[..end].to_string());
        }
    }
    // Also try single quotes
    let pattern_sq = format!("{attr}='");
    if let Some(start) = tag.find(&pattern_sq) {
        let after = &tag[start + pattern_sq.len()..];
        if let Some(end) = after.find('\'') {
            return Some(after[..end].to_string());
        }
    }
    // Check for bare attribute (e.g., "checked")
    if tag.contains(attr) && !tag.contains(&format!("{attr}=")) {
        return Some(String::new());
    }
    None
}

fn heading_level(tag: &str) -> Option<usize> {
    let tag = tag.split_whitespace().next().unwrap_or(tag);
    match tag {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

fn parse_code_block(inner: &str) -> (String, String) {
    // Inner is typically: <code class="language-rust">code here</code>
    if let Some(rest) = inner.strip_prefix("<code") {
        let lang = extract_attr(&rest.to_lowercase(), "class")
            .and_then(|c| c.strip_prefix("language-").map(String::from))
            .unwrap_or_default();
        // Find the end of the opening tag
        if let Some(gt) = rest.find('>') {
            let after_tag = &rest[gt + 1..];
            let code = if let Some(end) = after_tag.rfind("</code>") {
                &after_tag[..end]
            } else {
                after_tag
            };
            return (lang, decode_entities(code));
        }
    }
    // Fallback: treat entire inner as code
    (String::new(), decode_entities(inner))
}

fn convert_table(html: &str) -> String {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut is_header = false;
    let mut current_row: Vec<String> = Vec::new();

    let mut chars = html.chars().peekable();
    loop {
        match chars.peek() {
            None => break,
            Some(&'<') => {
                let tag = read_tag(&mut chars);
                let tag_lower = tag.to_lowercase();
                let tag_name = tag_lower.split_whitespace().next().unwrap_or("").trim_start_matches('/');

                match tag_name {
                    "thead" => is_header = true,
                    "tbody" => is_header = false,
                    "tr" if !tag_lower.starts_with('/') => {
                        current_row = Vec::new();
                    }
                    "tr" => {
                        if !current_row.is_empty() {
                            rows.push(current_row.clone());
                            if is_header {
                                // Add separator row after header
                                let sep: Vec<String> = current_row.iter().map(|_| "---".to_string()).collect();
                                rows.push(sep);
                            }
                        }
                    }
                    "th" | "td" if !tag_lower.starts_with('/') => {
                        let cell_tag = if tag_name == "th" { "th" } else { "td" };
                        let content = collect_inner_markdown(&mut chars, cell_tag, 1);
                        current_row.push(content.trim().to_string());
                    }
                    _ => {}
                }
            }
            Some(_) => { chars.next(); }
        }
    }

    let mut md = String::from("\n");
    for row in &rows {
        md.push_str("| ");
        md.push_str(&row.join(" | "));
        md.push_str(" |\n");
    }
    md
}

fn decode_entity_char(ch: char) -> char {
    ch // individual chars don't need decoding
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

fn collapse_blank_lines(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut blank_count = 0;
    for line in s.lines() {
        if line.trim().is_empty() {
            blank_count += 1;
            if blank_count <= 1 {
                result.push('\n');
            }
        } else {
            blank_count = 0;
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings() {
        assert_eq!(html_to_markdown("<h1>Title</h1>"), "# Title\n");
        assert_eq!(html_to_markdown("<h3>Sub</h3>"), "### Sub\n");
    }

    #[test]
    fn paragraphs_and_inline() {
        let html = "<p>Hello <strong>bold</strong> and <em>italic</em> world.</p>";
        assert_eq!(html_to_markdown(html), "Hello **bold** and *italic* world.\n");
    }

    #[test]
    fn links() {
        let html = r#"<p>See <a href="https://example.com">here</a>.</p>"#;
        assert_eq!(html_to_markdown(html), "See [here](https://example.com).\n");
    }

    #[test]
    fn wikilinks() {
        let html = r##"<p>See <a href="#" data-flynt-note="design">design</a>.</p>"##;
        assert_eq!(html_to_markdown(html), "See [[design]].\n");
    }

    #[test]
    fn wikilink_with_display() {
        let html = r##"<p><a href="#" data-flynt-note="some%20note">display text</a></p>"##;
        assert_eq!(html_to_markdown(html), "[[some note|display text]]\n");
    }

    #[test]
    fn unordered_list() {
        let html = "<ul><li>one</li><li>two</li><li>three</li></ul>";
        let result = html_to_markdown(html);
        assert!(result.contains("- one\n- two\n- three"));
    }

    #[test]
    fn ordered_list() {
        let html = "<ol><li>first</li><li>second</li></ol>";
        let result = html_to_markdown(html);
        assert!(result.contains("1. first\n2. second"));
    }

    #[test]
    fn code_block() {
        let html = r#"<pre><code class="language-rust">fn main() {}</code></pre>"#;
        let result = html_to_markdown(html);
        assert!(result.contains("```rust"));
        assert!(result.contains("fn main() {}"));
        assert!(result.contains("```"));
    }

    #[test]
    fn inline_code() {
        let html = "<p>Use <code>cargo test</code> to run.</p>";
        assert_eq!(html_to_markdown(html), "Use `cargo test` to run.\n");
    }

    #[test]
    fn blockquote() {
        let html = "<blockquote><p>quoted text</p></blockquote>";
        let result = html_to_markdown(html);
        assert!(result.contains("> quoted text"));
    }

    #[test]
    fn strikethrough() {
        let html = "<p>Hello <del>removed</del> world.</p>";
        assert_eq!(html_to_markdown(html), "Hello ~~removed~~ world.\n");
    }

    #[test]
    fn image_vault_protocol() {
        let html = r#"<img src="vault://localhost/photo%20name.png" alt="photo name">"#;
        assert_eq!(html_to_markdown(html), "![[photo name.png]]\n");
    }

    #[test]
    fn image_external() {
        let html = r#"<img src="https://example.com/img.png" alt="example">"#;
        assert_eq!(html_to_markdown(html), "![example](https://example.com/img.png)\n");
    }

    #[test]
    fn entities_decoded() {
        let html = "<p>A &amp; B &lt; C &gt; D</p>";
        assert_eq!(html_to_markdown(html), "A & B < C > D\n");
    }

    #[test]
    #[test]
    fn task_list() {
        let html = r#"<ul><li><input type="checkbox" checked disabled> Done</li><li><input type="checkbox" disabled> Todo</li></ul>"#;
        let result = html_to_markdown(html);
        assert!(result.contains("[x]") && result.contains("Done"), "got: {result}");
        assert!(result.contains("[ ]") && result.contains("Todo"), "got: {result}");
    }

    #[test]
    fn horizontal_rule() {
        let html = "<p>Above</p><hr><p>Below</p>";
        let result = html_to_markdown(html);
        assert!(result.contains("---"));
        assert!(result.contains("Above"));
        assert!(result.contains("Below"));
    }

    #[test]
    fn roundtrip_basic() {
        // Markdown → HTML → Markdown should produce equivalent output
        let original = "# Hello\n\nSome **bold** and *italic* text.\n\n- item one\n- item two\n";
        let html = {
            let mut opts = comrak::Options::default();
            opts.extension.strikethrough = true;
            opts.extension.tasklist = true;
            opts.render.unsafe_ = true;
            comrak::markdown_to_html(original, &opts)
        };
        let back = html_to_markdown(&html);
        assert!(back.contains("# Hello"));
        assert!(back.contains("**bold**"));
        assert!(back.contains("*italic*"));
        assert!(back.contains("- item one"));
        assert!(back.contains("- item two"));
    }
}
