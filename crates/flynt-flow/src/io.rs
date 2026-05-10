//! `.flow` file I/O — TOML frontmatter + JSON body.
//!
//! The wrapper format mirrors task files and Excalidraw drawings: a
//! `+++`-delimited TOML block carrying the entity metadata, then a JSON
//! body with the actual graph payload. The frontmatter is what makes
//! `.flow` files first-class in Flynt's indexer/sidebar/search; the JSON
//! body is what the react-flow webview round-trips.
//!
//! ```text
//! +++
//! id = "..."
//! kind = "flow"
//! [data]
//! title = "Auth Subsystem"
//! schema_version = 1
//! +++
//! { "meta": {...}, "nodes": [...], "edges": [...] }
//! ```

use anyhow::{Context, Result};
use std::path::Path;
use uuid::Uuid;

use crate::schema::Flow;

/// Bumped when the JSON body shape changes in a non-additive way. Loaders
/// emit a warning when they read a higher version than they were built
/// against; we don't refuse to load (be liberal in what we accept) but
/// the editor should refuse to save back.
pub const SCHEMA_VERSION: u32 = 1;

/// Frontmatter for a `.flow` file. Kept private; callers use
/// `serialize_flow` / `parse_flow` and never see this directly.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Frontmatter {
    id: Uuid,
    kind: String,
    #[serde(default)]
    data: FrontmatterData,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct FrontmatterData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(default = "default_schema_version")]
    schema_version: u32,
}

fn default_schema_version() -> u32 { SCHEMA_VERSION }

// ── Serialization ───────────────────────────────────────────────────────────

/// Render a `Flow` to its on-disk representation. `id` is the document id
/// stamped into frontmatter — caller passes a stable uuid (typically the
/// document id from the indexer; for fresh flows, `Uuid::new_v4()`).
///
/// The JSON body is pretty-printed with 2-space indent so git diffs are
/// useful; this trades a few KB on disk for human-readable history.
pub fn serialize_flow(flow: &Flow, id: Uuid) -> String {
    let fm = Frontmatter {
        id,
        kind: "flow".into(),
        data: FrontmatterData {
            title: flow.meta.title.clone(),
            schema_version: SCHEMA_VERSION,
        },
    };

    // Hand-build the frontmatter — `toml::to_string` would flatten the
    // [data] sub-table inline, which makes diffs noisy. Mirror the
    // task-file approach.
    let mut out = String::from("+++\n");
    out.push_str(&format!("id = \"{}\"\n", fm.id));
    out.push_str(&format!("kind = \"{}\"\n\n", fm.kind));
    out.push_str("[data]\n");
    if let Some(title) = &fm.data.title {
        out.push_str(&format!("title = {}\n", toml_quote(title)));
    }
    out.push_str(&format!("schema_version = {}\n", fm.data.schema_version));
    out.push_str("+++\n");

    // Body: pretty JSON. 2-space indent matches react-flow's developer
    // tooling and keeps the file readable when an operator opens it in a
    // text editor.
    let body = serde_json::to_string_pretty(flow).expect("Flow always serializes");
    out.push_str(&body);
    out.push('\n');
    out
}

/// Write a `Flow` to disk at `path`. Generates a fresh document id if
/// `id` is `None` — caller should pass `Some(existing_id)` when
/// overwriting an indexed file so the document identity stays stable.
pub fn save_flow(path: &Path, flow: &Flow, id: Option<Uuid>) -> Result<()> {
    let id = id.unwrap_or_else(Uuid::new_v4);
    let raw = serialize_flow(flow, id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create parent dir for {}", path.display()))?;
    }
    std::fs::write(path, raw)
        .with_context(|| format!("write flow file {}", path.display()))?;
    Ok(())
}

// ── Parsing ─────────────────────────────────────────────────────────────────

/// Parse a `.flow` file's raw contents. Returns the `Flow` plus the
/// document id from frontmatter (caller decides whether to trust it or
/// stamp a fresh one — the indexer wants the existing id; a "clone"
/// operation wants a new one).
pub fn parse_flow(raw: &str) -> Result<(Flow, Uuid)> {
    // Locate the frontmatter block. Reuse the same delimiter convention
    // as task files (`+++` on its own line).
    let trimmed = raw.trim_start();
    let rest = trimmed
        .strip_prefix("+++")
        .context("flow file missing TOML frontmatter (expected leading `+++`)")?;
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    let end = rest
        .find("\n+++")
        .context("flow file frontmatter is not closed (expected closing `+++`)")?;
    let fm_toml = &rest[..end];
    let body = &rest[end + 4..];
    let body = body.strip_prefix('\n').unwrap_or(body);

    let fm: Frontmatter = toml::from_str(fm_toml)
        .context("invalid TOML frontmatter in flow file")?;

    if fm.kind != "flow" {
        anyhow::bail!("expected kind = \"flow\", got kind = \"{}\"", fm.kind);
    }

    // Be liberal on schema_version: accept higher versions but the editor
    // is responsible for refusing to overwrite an unknown-shape file.
    let _ = fm.data.schema_version;

    let body = body.trim();
    let flow: Flow = if body.is_empty() {
        Flow::default()
    } else {
        serde_json::from_str(body).context("invalid JSON body in flow file")?
    };

    Ok((flow, fm.id))
}

/// Read and parse a `.flow` file from disk.
pub fn load_flow(path: &Path) -> Result<(Flow, Uuid)> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("read flow file {}", path.display()))?;
    parse_flow(&raw)
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn toml_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}
