use dioxus::prelude::*;
use flynt_core::models::{Document, DocumentId};
use flynt_core::store::ProjectStore;
use flynt_store::project::Project;

#[derive(Clone, Debug, PartialEq)]
pub struct NotePreview {
    pub title: String,
    pub path: String,
    pub excerpt: String,
}

impl NotePreview {
    pub fn from_document(doc: &Document) -> Self {
        Self {
            title: doc.title.clone(),
            path: doc.path.to_string_lossy().to_string(),
            excerpt: excerpt_from_markdown(&doc.content, 320),
        }
    }

    pub fn load_by_id(project: &Project, id: &DocumentId) -> Option<Self> {
        project
            .store
            .get_document(id)
            .ok()
            .flatten()
            .map(|doc| Self::from_document(&doc))
    }

    pub fn load_by_slug(project: &Project, slug: &str) -> Option<Self> {
        let meta = project.store.find_document_by_slug(slug).ok().flatten()?;
        Self::load_by_id(project, &meta.id)
    }
}

#[component]
pub fn NotePreviewCard(preview: NotePreview) -> Element {
    rsx! {
        div { class: "note-preview-card",
            div { class: "note-preview-title", "{preview.title}" }
            div { class: "note-preview-path", "{preview.path}" }
            if preview.excerpt.is_empty() {
                div { class: "note-preview-empty", "No preview text" }
            } else {
                div { class: "note-preview-excerpt", "{preview.excerpt}" }
            }
        }
    }
}

#[component]
pub fn FloatingNotePreview(preview: NotePreview, x: f64, y: f64) -> Element {
    let left = x + 14.0;
    let top = y + 14.0;
    rsx! {
        div {
            class: "note-preview-floating",
            style: "left: {left}px; top: {top}px;",
            NotePreviewCard { preview }
        }
    }
}

fn excerpt_from_markdown(markdown: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut in_frontmatter = false;
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed == "+++" || trimmed == "---" {
            in_frontmatter = !in_frontmatter;
            continue;
        }
        if in_frontmatter
            || trimmed.is_empty()
            || trimmed.starts_with("```")
            || trimmed.starts_with("~~~")
            || trimmed.starts_with("![")
        {
            continue;
        }
        let cleaned = trimmed
            .trim_start_matches('#')
            .trim_start_matches('>')
            .trim_start_matches('-')
            .trim_start_matches('*')
            .trim()
            .replace("[[", "")
            .replace("]]", "")
            .replace("**", "")
            .replace("__", "")
            .replace('`', "");
        if cleaned.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&cleaned);
        if out.chars().count() >= max_chars {
            break;
        }
    }
    truncate_chars(&out, max_chars)
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    let mut out: String = input.chars().take(max_chars).collect();
    if input.chars().count() > max_chars {
        out.push_str("...");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::excerpt_from_markdown;

    #[test]
    fn excerpt_skips_frontmatter_and_heavy_embeds() {
        let excerpt = excerpt_from_markdown(
            "+++\ntitle = \"A\"\n+++\n\n# Heading\n\n![[Drawing.excalidraw]]\n\nBody **text** with [[Link]].",
            80,
        );
        assert_eq!(excerpt, "Heading Body text with Link.");
    }

    #[test]
    fn excerpt_is_capped() {
        let excerpt = excerpt_from_markdown("abcdef", 3);
        assert_eq!(excerpt, "abc...");
    }
}
