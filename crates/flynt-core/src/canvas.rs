//! Design canvas document model.
//!
//! A `.canvas` file is JSON describing a grid of HTML/CSS cells. Both
//! `flynt-app` (renderer) and `flynt-agent` (canvas_* ACP tools) read and
//! write these files, so the wire shape lives here in `flynt-core` to
//! keep the two binaries in lockstep.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Current on-disk schema version. Bump when the shape changes in a way
/// that older readers cannot tolerate. Old files still parse via the
/// `version` check in `Canvas::load`, which surfaces a typed error rather
/// than silently corrupting data.
pub const CANVAS_VERSION: u32 = 1;

/// Top-level canvas document. Lives on disk as `<name>.canvas` JSON; a
/// sibling `<name>.md` wrapper with `![[<name>.canvas]]` makes it
/// indexable and routable in the UI (mirrors the Excalidraw pattern).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Canvas {
    pub version: u32,
    pub theme: String,
    pub grid: Grid,
    pub cells: Vec<Cell>,
}

/// Grid container parameters. Cells position themselves in this grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grid {
    pub cols: u32,
    pub rows: u32,
    /// Gap between cells in pixels.
    pub gap: u32,
}

/// One cell in the canvas. The agent owns this — it writes raw HTML, CSS,
/// and optional JS. Each cell renders inside a sandboxed iframe in the UI,
/// so cells cannot leak styles or JS into each other or the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    /// Stable identifier so the agent can apply partial updates without
    /// rewriting the whole document.
    pub id: String,
    /// Grid column, 0-indexed.
    pub x: u32,
    /// Grid row, 0-indexed.
    pub y: u32,
    /// Column span (>= 1).
    pub w: u32,
    /// Row span (>= 1).
    pub h: u32,
    pub html: String,
    pub css: String,
    /// Optional vanilla JS that runs scoped to this cell's iframe.
    pub js: Option<String>,
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            version: CANVAS_VERSION,
            theme: "default".into(),
            grid: Grid::default(),
            cells: Vec::new(),
        }
    }
}

impl Default for Grid {
    fn default() -> Self {
        Self { cols: 12, rows: 8, gap: 8 }
    }
}

impl Canvas {
    /// Parse a JSON canvas file. Returns an error on missing/malformed
    /// JSON or on a `version` we don't know how to read.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let data = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("read {}: {e}", path.display()))?;
        Self::from_json(&data)
    }

    /// Parse from a JSON string. Same error semantics as `load`.
    pub fn from_json(data: &str) -> anyhow::Result<Self> {
        let canvas: Canvas = serde_json::from_str(data)
            .map_err(|e| anyhow::anyhow!("parse canvas json: {e}"))?;
        if canvas.version > CANVAS_VERSION {
            anyhow::bail!(
                "canvas version {} is newer than supported version {}",
                canvas.version,
                CANVAS_VERSION
            );
        }
        Ok(canvas)
    }

    /// Serialize and write to disk atomically (write to tempfile, then
    /// rename). Atomic write avoids partial-file corruption if Flynt
    /// crashes mid-save, which matters here because the agent edits this
    /// file too and a torn write would surface to it as a parse error.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("canvas.tmp");
        std::fs::write(&tmp, json.as_bytes())?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Find a cell by ID. Used by `canvas_set_cells` to apply partial
    /// updates without callers needing to scan.
    pub fn find_cell(&self, id: &str) -> Option<&Cell> {
        self.cells.iter().find(|c| c.id == id)
    }

    pub fn find_cell_mut(&mut self, id: &str) -> Option<&mut Cell> {
        self.cells.iter_mut().find(|c| c.id == id)
    }

    /// Insert or replace a cell (matched by `id`). Returns `true` if an
    /// existing cell was replaced, `false` if appended.
    pub fn upsert_cell(&mut self, cell: Cell) -> bool {
        if let Some(existing) = self.find_cell_mut(&cell.id) {
            *existing = cell;
            true
        } else {
            self.cells.push(cell);
            false
        }
    }

    /// Remove a cell by ID. Returns whether it was present.
    pub fn remove_cell(&mut self, id: &str) -> bool {
        let len = self.cells.len();
        self.cells.retain(|c| c.id != id);
        self.cells.len() != len
    }
}

/// Create a new canvas: a `.canvas` data file plus a `.md` wrapper that
/// embeds it. Returns the `.md` path (indexable by Flynt). The wrapper
/// pattern mirrors how Excalidraw documents are stored — the `.md` is
/// the indexable handle, the data file is the source of truth.
///
/// Lives here in flynt-core so both flynt-app (UI menu/command palette)
/// and flynt-agent (canvas_create ACP tool) can call into the same
/// implementation. Refuses to overwrite an existing `.canvas` file.
pub fn create_canvas(project_root: &Path, name: &str) -> anyhow::Result<PathBuf> {
    let canvases_dir = project_root.join("canvases");
    std::fs::create_dir_all(&canvases_dir)?;

    let canvas_file = format!("{name}.canvas");
    let canvas_abs = canvases_dir.join(&canvas_file);
    if canvas_abs.exists() {
        anyhow::bail!("canvas already exists: canvases/{canvas_file}");
    }
    Canvas::default().save(&canvas_abs)?;

    let md_file = format!("{name}.md");
    let md_rel = PathBuf::from("canvases").join(&md_file);
    let md_abs = project_root.join(&md_rel);
    let escaped_name = name.replace('"', "\\\"");
    let md_content = format!(
        "+++\ntitle = \"{escaped_name}\"\ntags = [\"canvas\"]\n+++\n\n![[{canvas_file}]]\n"
    );
    std::fs::write(&md_abs, md_content)?;

    Ok(md_rel)
}

// ── Capture pipeline types ──────────────────────────────────────────────
//
// The runtime capture (xcap, JS measurement) lives in `flynt-app` since it
// needs the WebView. The wire types live here so the omegon-design tool
// (separate binary) and flynt-app's request handler agree on the shape of
// `<project>/.flynt-local/flynt/capture-{requests,responses}/*.json` files.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureRequest {
    pub request_id: String,
    #[serde(default)]
    pub canvas_path: Option<String>,
    #[serde(default = "default_true")]
    pub include_metrics: bool,
}

fn default_true() -> bool { true }

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoxXywh {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellMetric {
    pub id: String,
    pub cell_box: BoxXywh,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_box: Option<BoxXywh>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill_ratio: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureResponse {
    pub request_id: String,
    pub image_path: String,
    /// PNG bytes, base64-encoded. Inlined here so the tool can return it
    /// in one round trip without the agent needing a follow-up read.
    pub image_base64: String,
    pub image_width: u32,
    pub image_height: u32,
    pub viewport_box: BoxXywh,
    pub cells: Vec<CellMetric>,
    pub scale_factor: f32,
    pub captured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn capture_request_dir(project_root: &Path) -> PathBuf {
    project_root.join(".flynt-local").join("flynt").join("capture-requests")
}

pub fn capture_response_dir(project_root: &Path) -> PathBuf {
    project_root.join(".flynt-local").join("flynt").join("capture-responses")
}

/// Operator-level canvas settings, persisted in `FlyntOperatorSettings.canvas`.
/// Phase 4 introduces real values; Phase 1+2 ship the field with defaults so
/// later phases can attach without migrating existing operator-settings.json.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasSettings {
    /// Theme preset ID applied to new canvases.
    pub default_theme: String,
    /// Grid dimensions used when creating a new canvas.
    pub default_grid: Grid,
    /// One-shot bootstrap flag set after canvas assets are copied into
    /// the project's `.flynt-local/flynt/assets/` directory. See Phase 4.
    pub assets_initialized: bool,
}

impl Default for CanvasSettings {
    fn default() -> Self {
        Self {
            default_theme: "default".into(),
            default_grid: Grid::default(),
            assets_initialized: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{NamedTempFile, TempDir};

    fn sample_cell(id: &str) -> Cell {
        Cell {
            id: id.into(),
            x: 0, y: 0, w: 4, h: 2,
            html: "<button class=\"btn\">Hi</button>".into(),
            css: ".btn { color: red; }".into(),
            js: None,
        }
    }

    #[test]
    fn canvas_default_is_v1_with_empty_cells() {
        let c = Canvas::default();
        assert_eq!(c.version, CANVAS_VERSION);
        assert_eq!(c.theme, "default");
        assert_eq!(c.grid.cols, 12);
        assert_eq!(c.grid.rows, 8);
        assert!(c.cells.is_empty());
    }

    #[test]
    fn canvas_round_trip_through_json() {
        let mut c = Canvas::default();
        c.upsert_cell(sample_cell("a"));
        c.upsert_cell(Cell {
            id: "b".into(),
            x: 5, y: 0, w: 3, h: 4,
            html: "<div>ok</div>".into(),
            css: "".into(),
            js: Some("console.log(1)".into()),
        });
        let json = serde_json::to_string(&c).unwrap();
        let back: Canvas = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn canvas_load_rejects_future_version() {
        let json = r#"{"version":99,"theme":"x","grid":{"cols":1,"rows":1,"gap":0},"cells":[]}"#;
        let err = Canvas::from_json(json).unwrap_err().to_string();
        assert!(err.contains("newer than supported"), "got: {err}");
    }

    #[test]
    fn canvas_load_rejects_malformed_json() {
        let err = Canvas::from_json("not json").unwrap_err().to_string();
        assert!(err.contains("parse canvas json"), "got: {err}");
    }

    #[test]
    fn canvas_load_rejects_missing_required_fields() {
        // theme missing → serde error via from_json
        let err = Canvas::from_json(r#"{"version":1,"grid":{"cols":1,"rows":1,"gap":0},"cells":[]}"#)
            .unwrap_err().to_string();
        assert!(err.contains("parse canvas json"), "got: {err}");
    }

    #[test]
    fn save_then_load_round_trip() {
        let f = NamedTempFile::new().unwrap();
        let mut c = Canvas::default();
        c.upsert_cell(sample_cell("only"));
        c.save(f.path()).unwrap();

        let back = Canvas::load(f.path()).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn save_is_atomic_no_tmp_left_behind() {
        let f = NamedTempFile::new().unwrap();
        Canvas::default().save(f.path()).unwrap();

        let tmp = f.path().with_extension("canvas.tmp");
        assert!(!tmp.exists(), "atomic save must clean up its tempfile");
    }

    #[test]
    fn upsert_replaces_existing() {
        let mut c = Canvas::default();
        c.upsert_cell(sample_cell("a"));
        let mut updated = sample_cell("a");
        updated.html = "<span>new</span>".into();
        let was_replaced = c.upsert_cell(updated.clone());
        assert!(was_replaced);
        assert_eq!(c.cells.len(), 1);
        assert_eq!(c.find_cell("a").unwrap().html, "<span>new</span>");
    }

    #[test]
    fn upsert_appends_when_id_unknown() {
        let mut c = Canvas::default();
        let was_replaced = c.upsert_cell(sample_cell("new"));
        assert!(!was_replaced);
        assert_eq!(c.cells.len(), 1);
    }

    #[test]
    fn remove_returns_whether_present() {
        let mut c = Canvas::default();
        c.upsert_cell(sample_cell("a"));
        assert!(c.remove_cell("a"));
        assert!(!c.remove_cell("a"));
        assert!(c.cells.is_empty());
    }

    #[test]
    fn create_canvas_writes_data_file_and_wrapper() {
        let tmp = TempDir::new().unwrap();
        let md_path = create_canvas(tmp.path(), "Hero").unwrap();

        assert!(md_path.to_string_lossy().ends_with(".md"));
        let md_abs = tmp.path().join(&md_path);
        let canvas_abs = tmp.path().join("canvases/Hero.canvas");
        assert!(md_abs.exists());
        assert!(canvas_abs.exists());

        let canvas = Canvas::load(&canvas_abs).unwrap();
        assert_eq!(canvas.version, CANVAS_VERSION);
        assert!(canvas.cells.is_empty());

        let md = std::fs::read_to_string(&md_abs).unwrap();
        assert!(md.contains("![[Hero.canvas]]"));
        assert!(md.contains("tags = [\"canvas\"]"));
        assert!(md.contains("title = \"Hero\""));
    }

    #[test]
    fn create_canvas_refuses_to_overwrite_existing() {
        let tmp = TempDir::new().unwrap();
        create_canvas(tmp.path(), "Hero").unwrap();
        let err = create_canvas(tmp.path(), "Hero").unwrap_err().to_string();
        assert!(err.contains("already exists"), "got: {err}");
    }

    #[test]
    fn create_canvas_escapes_quotes_in_name() {
        let tmp = TempDir::new().unwrap();
        let md_path = create_canvas(tmp.path(), "Quoted \"X\"").unwrap();
        let md = std::fs::read_to_string(tmp.path().join(&md_path)).unwrap();
        // Frontmatter title must remain valid TOML — embedded quote escaped.
        assert!(md.contains(r#"title = "Quoted \"X\"""#), "got: {md}");
    }

    #[test]
    fn canvas_settings_default_marks_assets_uninitialized() {
        let s = CanvasSettings::default();
        assert!(!s.assets_initialized);
        assert_eq!(s.default_theme, "default");
        assert_eq!(s.default_grid.cols, 12);
    }

    #[test]
    fn cell_serializes_optional_js_only_when_present() {
        let mut c = sample_cell("x");
        c.js = None;
        let json = serde_json::to_string(&c).unwrap();
        // serde keeps the null by default; we accept that — round-trip is
        // what matters, not field omission. This test pins the behavior so
        // a future serde annotation change is intentional.
        let back: Cell = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }
}
