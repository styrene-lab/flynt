use crate::models::{Frontmatter, WikiLink};
use comrak::{Arena, Options, parse_document};
use comrak::nodes::NodeValue;

/// Extract frontmatter + wikilinks from raw markdown source.
/// Returns `(body_without_frontmatter, frontmatter, links)`.
pub fn parse_document_source(raw: &str) -> (String, Frontmatter, Vec<WikiLink>) {
    let (frontmatter, body) = split_frontmatter(raw);
    let links = extract_wikilinks(&body);
    (body, frontmatter, links)
}

/// Split TOML frontmatter delimited by `+++` or YAML by `---`.
/// Returns (frontmatter, body). Both fields may be empty strings.
fn split_frontmatter(raw: &str) -> (Frontmatter, String) {
    // Try TOML frontmatter: +++\n...\n+++
    if let Some(rest) = raw.strip_prefix("+++\n") {
        if let Some(end) = rest.find("\n+++") {
            let fm_str = &rest[..end];
            let body = rest[end + 4..].trim_start_matches('\n').to_string();
            let fm: Frontmatter = toml::from_str(fm_str).unwrap_or_default();
            return (fm, body);
        }
    }
    // Try YAML frontmatter: ---\n...\n---  (stored as TOML-compatible struct via serde)
    if let Some(rest) = raw.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            // We accept basic YAML-looking TOML-compatible values (tags, aliases, status)
            let fm_str = &rest[..end];
            let body = rest[end + 4..].trim_start_matches('\n').to_string();
            // Best-effort: parse as TOML (most frontmatter keys are compatible)
            let fm: Frontmatter = toml::from_str(fm_str).unwrap_or_default();
            return (fm, body);
        }
    }
    (Frontmatter::default(), raw.to_string())
}

/// Extract all `[[wikilink]]` patterns and local markdown links from content.
/// Handles `[[target]]`, `[[target|display]]`, `[[target#anchor]]`,
/// and standard `[text](path.md)` links to local files.
fn extract_wikilinks(body: &str) -> Vec<WikiLink> {
    let arena = Arena::new();
    let opts = Options::default();
    let root = parse_document(&arena, body, &opts);

    let mut links = Vec::new();

    for node in root.descendants() {
        match &node.data.borrow().value {
            // Scan text nodes for [[wikilinks]]
            NodeValue::Text(text) => {
                links.extend(scan_wikilinks(text));
            }
            // Extract local markdown links: [text](path.md)
            NodeValue::Link(link) => {
                let url = &link.url;
                // Skip external URLs and anchors-only
                if url.starts_with("http://") || url.starts_with("https://")
                    || url.starts_with("mailto:") || url.starts_with('#')
                    || url.is_empty()
                {
                    continue;
                }
                // Resolve relative paths — strip leading ../ and extract the filename
                let path = std::path::Path::new(url.split('#').next().unwrap_or(url));
                // Only include links to markdown files or extensionless refs
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if !ext.is_empty() && ext != "md" { continue; }
                // Use the file stem as the target slug (like wikilinks do)
                let target = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                if target.is_empty() { continue; }
                let anchor = url.find('#').map(|i| url[i + 1..].to_string());
                // Extract display text from child text nodes
                let display: String = node.descendants()
                    .filter_map(|child| {
                        if let NodeValue::Text(ref t) = child.data.borrow().value {
                            Some(t.clone())
                        } else { None }
                    })
                    .collect::<Vec<_>>()
                    .join("");
                links.push(WikiLink {
                    target,
                    display: if display.is_empty() { None } else { Some(display) },
                    anchor,
                });
            }
            _ => {}
        }
    }

    links
}

fn scan_wikilinks(text: &str) -> Vec<WikiLink> {
    let mut links = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find("]]") else { break };
        let inner = &rest[..end];
        rest = &rest[end + 2..];

        // Split display: [[target|display]]
        let (target_part, display) = if let Some(pipe) = inner.find('|') {
            (&inner[..pipe], Some(inner[pipe + 1..].to_string()))
        } else {
            (inner, None)
        };

        // Split anchor: [[target#heading]]
        let (target, anchor) = if let Some(hash) = target_part.find('#') {
            (target_part[..hash].to_string(), Some(target_part[hash + 1..].to_string()))
        } else {
            (target_part.to_string(), None)
        };

        if !target.is_empty() {
            links.push(WikiLink { target, display, anchor });
        }
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_simple_wikilink() {
        let links = scan_wikilinks("See [[some-note]] for details.");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "some-note");
        assert!(links[0].display.is_none());
    }

    #[test]
    fn extracts_piped_display_wikilink() {
        let links = scan_wikilinks("See [[some-note|My Note]] here.");
        assert_eq!(links[0].target, "some-note");
        assert_eq!(links[0].display.as_deref(), Some("My Note"));
    }

    #[test]
    fn extracts_anchor_wikilink() {
        let links = scan_wikilinks("Jump to [[design#architecture]].");
        assert_eq!(links[0].target, "design");
        assert_eq!(links[0].anchor.as_deref(), Some("architecture"));
    }

    #[test]
    fn splits_toml_frontmatter() {
        let raw = "+++\ntags = [\"rust\", \"design\"]\n+++\n\nBody here.";
        let (fm, body) = split_frontmatter(raw);
        assert_eq!(fm.tags, vec!["rust", "design"]);
        assert_eq!(body, "Body here.");
    }

    // ── Additional parser edge cases ────────────────────────────────

    #[test]
    fn no_frontmatter_returns_defaults() {
        let raw = "Just body content.";
        let (body, fm, links) = parse_document_source(raw);
        assert_eq!(body, "Just body content.");
        assert!(fm.tags.is_empty());
        assert!(links.is_empty());
    }

    #[test]
    fn malformed_toml_frontmatter_returns_defaults() {
        let raw = "+++\nthis is not valid toml {{{\n+++\n\nBody.";
        let (body, fm, _) = parse_document_source(raw);
        assert_eq!(body, "Body.");
        assert!(fm.tags.is_empty()); // parse failed, got default
    }

    #[test]
    fn frontmatter_with_title() {
        let raw = "+++\ntitle = \"My Note\"\ntags = [\"test\"]\n+++\n\nContent.";
        let (_, fm, _) = parse_document_source(raw);
        assert_eq!(fm.title.as_deref(), Some("My Note"));
        assert_eq!(fm.tags, vec!["test"]);
    }

    #[test]
    fn body_containing_triple_plus_not_treated_as_frontmatter() {
        let raw = "+++\ntitle = \"Real\"\n+++\n\nSome text\n\n+++\nthis is body not frontmatter\n+++\n";
        let (body, fm, _) = parse_document_source(raw);
        assert_eq!(fm.title.as_deref(), Some("Real"));
        assert!(body.contains("this is body not frontmatter"));
    }

    #[test]
    fn multiple_wikilinks_in_one_line() {
        let (_, _, links) = parse_document_source("See [[alpha]] and [[beta]] and [[gamma]].");
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].target, "alpha");
        assert_eq!(links[1].target, "beta");
        assert_eq!(links[2].target, "gamma");
    }

    #[test]
    fn empty_wikilink_ignored() {
        let links = scan_wikilinks("See [[]] nothing.");
        assert!(links.is_empty());
    }

    #[test]
    fn wikilink_with_spaces() {
        let links = scan_wikilinks("See [[My Long Note Title]].");
        assert_eq!(links[0].target, "My Long Note Title");
    }

    #[test]
    fn unclosed_wikilink_ignored() {
        let links = scan_wikilinks("See [[unclosed and never closed.");
        assert!(links.is_empty());
    }

    #[test]
    fn yaml_frontmatter_parsed() {
        let raw = "---\ntitle = \"YAML-ish\"\ntags = [\"yaml\"]\n---\n\nBody.";
        let (body, fm, _) = parse_document_source(raw);
        assert_eq!(fm.title.as_deref(), Some("YAML-ish"));
        assert_eq!(body, "Body.");
    }

    #[test]
    fn empty_document() {
        let (body, fm, links) = parse_document_source("");
        assert_eq!(body, "");
        assert!(fm.tags.is_empty());
        assert!(links.is_empty());
    }

    #[test]
    fn frontmatter_only_no_body() {
        let raw = "+++\ntitle = \"Just FM\"\n+++\n";
        let (body, fm, _) = parse_document_source(raw);
        assert_eq!(fm.title.as_deref(), Some("Just FM"));
        assert!(body.is_empty() || body.trim().is_empty());
    }

    // ── Markdown reference link extraction ─────────────────────────

    #[test]
    fn extracts_local_markdown_link() {
        let (_, _, links) = parse_document_source("See [design doc](../docs/provider-landscape.md) for details.");
        assert!(links.iter().any(|l| l.target == "provider-landscape"), "links: {links:?}");
    }

    #[test]
    fn extracts_local_link_with_anchor() {
        let (_, _, links) = parse_document_source("See [arch](design.md#overview) section.");
        let link = links.iter().find(|l| l.target == "design").unwrap();
        assert_eq!(link.anchor.as_deref(), Some("overview"));
    }

    #[test]
    fn ignores_external_http_links() {
        let (_, _, links) = parse_document_source("See [docs](https://example.com/design.md).");
        assert!(links.is_empty(), "should not extract http links: {links:?}");
    }

    #[test]
    fn ignores_image_links() {
        let (_, _, links) = parse_document_source("See [photo](image.png).");
        assert!(links.is_empty(), "should not extract image links: {links:?}");
    }

    #[test]
    fn extracts_extensionless_local_link() {
        let (_, _, links) = parse_document_source("See [roadmap](../roadmap) here.");
        assert!(links.iter().any(|l| l.target == "roadmap"), "links: {links:?}");
    }

    #[test]
    fn extracts_mixed_wikilinks_and_md_links() {
        let (_, _, links) = parse_document_source(
            "See [[alpha]] and [beta doc](beta.md) and [[gamma]]."
        );
        assert_eq!(links.len(), 3);
        assert!(links.iter().any(|l| l.target == "alpha"));
        assert!(links.iter().any(|l| l.target == "beta"));
        assert!(links.iter().any(|l| l.target == "gamma"));
    }

    #[test]
    fn local_link_preserves_display_text() {
        let (_, _, links) = parse_document_source("See [the design](design.md).");
        let link = links.iter().find(|l| l.target == "design").unwrap();
        assert_eq!(link.display.as_deref(), Some("the design"));
    }
}
