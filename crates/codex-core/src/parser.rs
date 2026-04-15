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

/// Extract all `[[wikilink]]` patterns from markdown content.
/// Handles `[[target]]` and `[[target|display]]` and `[[target#anchor]]`.
fn extract_wikilinks(body: &str) -> Vec<WikiLink> {
    let arena = Arena::new();
    let opts = Options::default();
    let root = parse_document(&arena, body, &opts);

    let mut links = Vec::new();

    // Walk every text node and scan for [[...]]
    for node in root.descendants() {
        if let NodeValue::Text(ref text) = node.data.borrow().value {
            links.extend(scan_wikilinks(text));
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
}
