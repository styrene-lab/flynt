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
/// stay liberal (parse higher versions) but the `FlowDocument` returned
/// carries the observed value so callers can decide whether to refuse to
/// save back.
pub const SCHEMA_VERSION: u32 = 1;

/// Parsed `.flow` file: graph payload plus frontmatter metadata the
/// caller needs to make decisions (document identity, version
/// compatibility).
///
/// Returned by `parse_flow` / `load_flow`. Holds everything callers might
/// reasonably need so they don't have to re-parse the frontmatter
/// separately. `schema_version` lets a caller detect a future-version
/// file and refuse to overwrite it (the indexer should still surface it
/// in the sidebar — the right policy is "show, but make the operator
/// upgrade before editing").
#[derive(Debug, Clone, PartialEq)]
pub struct FlowDocument {
    pub id: Uuid,
    /// Schema version observed in the file. Compare to [`SCHEMA_VERSION`]
    /// to decide if this loader fully understands the body.
    pub schema_version: u32,
    pub flow: Flow,
}

impl FlowDocument {
    /// True when this loader's `SCHEMA_VERSION` matches what was on disk.
    /// Future versions return false; callers typically refuse to save.
    pub fn schema_matches(&self) -> bool {
        self.schema_version == SCHEMA_VERSION
    }
}

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

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

// ── Serialization ───────────────────────────────────────────────────────────

/// Render a `Flow` to its on-disk representation. `id` is the document id
/// stamped into frontmatter — caller passes a stable uuid (typically the
/// document id from the indexer; for fresh flows, `Uuid::new_v4()`).
///
/// The JSON body is pretty-printed with 2-space indent so git diffs are
/// useful; this trades a few KB on disk for human-readable history.
pub fn serialize_flow(flow: &Flow, id: Uuid) -> String {
    // Hand-build the frontmatter so the [data] sub-table renders on its
    // own line (toml::to_string flattens nested tables inline, which makes
    // diffs noisy). Mirrors the task-file approach.
    let mut out = String::from("+++\n");
    out.push_str(&format!("id = \"{}\"\n", id));
    out.push_str("kind = \"flow\"\n\n");
    out.push_str("[data]\n");
    if let Some(title) = &flow.meta.title {
        out.push_str(&format!("title = {}\n", toml_basic_string(title)));
    }
    out.push_str(&format!("schema_version = {}\n", SCHEMA_VERSION));
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
    std::fs::write(path, raw).with_context(|| format!("write flow file {}", path.display()))?;
    Ok(())
}

// ── Parsing ─────────────────────────────────────────────────────────────────

/// Parse a `.flow` file's raw contents into a [`FlowDocument`].
///
/// Errors only on structural problems (missing/unclosed frontmatter,
/// non-flow `kind`, malformed TOML/JSON). Higher schema versions parse
/// successfully — callers inspect [`FlowDocument::schema_matches`].
/// Per-graph integrity (dangling edges, duplicate ids) is reported via
/// [`Flow::validate`] separately, never as a parse error.
pub fn parse_flow(raw: &str) -> Result<FlowDocument> {
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

    let fm: Frontmatter =
        toml::from_str(fm_toml).context("invalid TOML frontmatter in flow file")?;

    if fm.kind != "flow" {
        anyhow::bail!("expected kind = \"flow\", got kind = \"{}\"", fm.kind);
    }

    let body = body.trim();
    let flow: Flow = if body.is_empty() {
        Flow::default()
    } else {
        serde_json::from_str(body).context("invalid JSON body in flow file")?
    };

    Ok(FlowDocument {
        id: fm.id,
        schema_version: fm.data.schema_version,
        flow,
    })
}

/// Read and parse a `.flow` file from disk.
pub fn load_flow(path: &Path) -> Result<FlowDocument> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("read flow file {}", path.display()))?;
    parse_flow(&raw)
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Quote a string as a TOML basic string (`"…"`), escaping the characters
/// that would break the format.
///
/// TOML basic strings forbid raw control characters except `\t`. Without
/// escaping, an agent-generated multiline title (e.g. "Line 1\nLine 2")
/// produces invalid TOML at the next save. The wider codebase has the
/// same bug in `flynt_models::task_file::toml_quote`; harmonizing those
/// is a separate cross-crate cleanup.
fn toml_basic_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            // Other C0 controls — TOML allows them only via \uXXXX escape.
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
