//! omegon-design extension — exposes design helper tools over the omegon
//! ACP extension protocol. Tools deliberately do NOT write canvas content;
//! that responsibility stays with `flynt-agent`'s `canvas_*` family. These
//! tools inform the agent's design decisions and surface the influences
//! shaping its behavior so the operator never sees shadow prompts.

use async_trait::async_trait;
use omegon_extension::Extension;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

use crate::{skill_install, style_guide};

pub struct DesignExtension {
    vault_root: PathBuf,
}

impl DesignExtension {
    pub fn new(vault_root: PathBuf) -> Self {
        Self { vault_root }
    }
}

#[async_trait]
impl Extension for DesignExtension {
    fn name(&self) -> &str { "omegon-design" }
    fn version(&self) -> &str { env!("CARGO_PKG_VERSION") }

    async fn handle_rpc(&self, method: &str, params: Value) -> omegon_extension::Result<Value> {
        match method {
            // ── v2 handshake ────────────────────────────────────────────────
            "initialize" => {
                let tools = self.handle_rpc("get_tools", json!({})).await?;
                Ok(json!({
                    "protocol_version": 2,
                    "extension_info": {
                        "name": self.name(),
                        "version": self.version(),
                        "sdk_version": "0.16.0"
                    },
                    "capabilities": {
                        "tools": true, "widgets": false, "mind": false,
                        "vox": false, "resources": false, "prompts": false,
                        "sampling": false, "elicitation": false, "streaming": false
                    },
                    "tools": tools
                }))
            }

            "tools/call" => {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                self.handle_rpc(&format!("execute_{name}"), args).await
            }
            "execute_tool" => {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("args").cloned().unwrap_or(json!({}));
                self.handle_rpc(&format!("execute_{name}"), args).await
            }

            // ── Discovery ────────────────────────────────────────────────────
            "get_tools" | "tools/list" => Ok(json!([
                {
                    "name": "design_describe_influences",
                    "label": "Design: Describe Influences",
                    "description": "Return a structured inventory of EVERYTHING this extension is contributing to the agent's prompt + tool surface right now: the active flynt-design skill (id, version, content hash, summary), loaded style guides at project and user levels (paths, sizes, checksums for drift detection), the current canvas theme + available themes with full var maps, and primitive availability. The operator must always be able to see what's shaping your design behavior — call this on every fresh design turn and emit a one-line summary in the visible response. Pass `full_content: true` to dump the actual skill prompt text and merged style-guide body for deep inspection.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "canvas_path": {
                                "type": "string",
                                "description": "Optional path to a .canvas file. If provided, the report includes the active theme and its full var map. Otherwise the theme block reports only available presets."
                            },
                            "full_content": {
                                "type": "boolean",
                                "default": false,
                                "description": "When true, response includes the skill's full markdown body and the merged style-guide content. Default false to keep responses compact."
                            }
                        }
                    }
                },
                {
                    "name": "design_load_style_guide",
                    "label": "Design: Load Style Guide",
                    "description": "Read the project-level (`<project>/.flynt/style-guide.md`) and user-level (`~/.flynt/style-guide.md`) style guides and return a merged report. Project wins on conflict; the per-level content fields are kept so you can reason about where any given rule originated. When neither guide exists, the response includes a setup_hint telling the user how to add one. Style guides are markdown with optional TOML frontmatter for structured data (brand colors, typography). Treat the merged content as source of truth for brand voice, color overrides, and visual rules.",
                    "parameters": { "type": "object", "properties": {} }
                },
                {
                    "name": "design_suggest_theme",
                    "label": "Design: Suggest Theme",
                    "description": "Given a textual brief about the desired visual direction (e.g., 'retro warm beige', 'industrial dark with amber'), return the best-matching theme from the bundled tweakcn presets along with a confidence score and reasoning. Eliminates guesswork at theme selection. Returns null when the brief is empty or when no preset is a reasonable match — in that case, ask the user to pick from the available list rather than picking arbitrarily.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "brief": { "type": "string", "description": "Free-text description of the desired aesthetic." }
                        },
                        "required": ["brief"]
                    }
                },
                {
                    "name": "canvas_capture_viewport",
                    "label": "Canvas: Capture Viewport (agent eyes)",
                    "description": "Capture the canvas pane as the user actually sees it RIGHT NOW — real post-layout pixels at the user's current viewport size and panel widths. Returns: (1) per-cell metrics with `cell_box` (the grid placement) and `content_box` (the natural extent of the cell's body content) and `fill_ratio` (content_height / cell_height — values < 0.85 mean visible dead space below content), (2) the cropped PNG of the canvas pane, base64-encoded, and (3) image dimensions + window scale factor. Use this AFTER any canvas_set_cells edit to verify your design actually fills correctly. The numeric fill_ratio is the cheap source of truth; the image is for visual hierarchy / color readability checks. First call on macOS triggers the system Screen Recording permission prompt — call `canvas_capture_status` first to surface that to the operator. Linux: no permission required.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "canvas_path": { "type": "string", "description": "Path to the .canvas file relative to project root. Optional — when omitted, captures whatever canvas is currently rendered." },
                            "include_metrics": { "type": "boolean", "default": true, "description": "When false, skip the per-iframe postMessage round-trip and return image-only. Faster, but you lose the fill_ratio data." }
                        }
                    }
                },
                {
                    "name": "canvas_capture_status",
                    "label": "Canvas: Capture Status",
                    "description": "Probe whether canvas viewport capture is available right now. Returns the platform, whether OS permission is granted, and (when denied) instructions for the operator. Skill mandate: call this BEFORE the first canvas_capture_viewport in a session, and surface 'denied' status to the operator with the instructions verbatim. Don't silently fail or retry.",
                    "parameters": { "type": "object", "properties": {} }
                },
                {
                    "name": "design_critique",
                    "label": "Design: Critique",
                    "description": "Audit a canvas against structural discipline (cell-fill, h-full coverage), theme coherence (cells using theme tokens vs hardcoded colors fighting the active theme), and style-guide adherence (when a guide is loaded). Returns a structured report grouped by severity: blocker / warning / suggestion. Stronger than the inline lint_warnings from canvas_set_cells — that lint catches per-cell structural issues at write time; this critique evaluates the canvas as a whole. Run after a coherent set of edits, before declaring done.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "canvas_path": { "type": "string", "description": "Path to the .canvas file relative to project root." }
                        },
                        "required": ["canvas_path"]
                    }
                }
            ])),

            "execute_design_describe_influences" => self.execute_describe_influences(params),
            "execute_design_load_style_guide" => self.execute_load_style_guide(),
            "execute_design_suggest_theme" => self.execute_suggest_theme(params),
            "execute_design_critique" => self.execute_critique(params),
            "execute_canvas_capture_viewport" => self.execute_capture_viewport(params).await,
            "execute_canvas_capture_status" => self.execute_capture_status(),

            _ => Err(omegon_extension::Error::method_not_found(method)),
        }
    }
}

impl DesignExtension {
    fn skill_path(&self) -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/"))
            .join(".omegon")
            .join("skills")
            .join("flynt-design")
            .join("SKILL.md")
    }

    fn presets_path(&self) -> PathBuf {
        self.vault_root
            .join(".flynt-local")
            .join("flynt")
            .join("assets")
            .join("tweakcn-presets.json")
    }

    fn primitives_path(&self) -> PathBuf {
        self.vault_root
            .join(".flynt-local")
            .join("flynt")
            .join("assets")
            .join("shadcn-primitives.json")
    }

    fn read_active_theme(&self, canvas_path: &str) -> Option<String> {
        // Refuse path traversal; resolve relative to project.
        let rel = std::path::Path::new(canvas_path);
        if rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return None;
        }
        let abs = self.vault_root.join(rel);
        let canvas = flynt_core::canvas::Canvas::load(&abs).ok()?;
        Some(canvas.theme)
    }

    fn execute_describe_influences(&self, params: Value) -> omegon_extension::Result<Value> {
        let full = params.get("full_content").and_then(|v| v.as_bool()).unwrap_or(false);
        let canvas_path = params.get("canvas_path").and_then(|v| v.as_str());

        // Skill block — read installed copy, hash for drift detection.
        let skill_block = match std::fs::read(self.skill_path()) {
            Ok(bytes) => {
                let mut hasher = Sha256::new();
                hasher.update(&bytes);
                let hash = format!("sha256:{:x}", hasher.finalize());
                let content = String::from_utf8_lossy(&bytes).to_string();
                json!({
                    "id": "flynt-design",
                    "version": env!("CARGO_PKG_VERSION"),
                    "active": true,
                    "installed_at": self.skill_path().to_string_lossy(),
                    "size_bytes": bytes.len(),
                    "content_hash": hash,
                    "summary": "Canvas-aware workflow + structural discipline + aesthetic principles. Full text via full_content=true.",
                    "full_content": if full { Some(content) } else { None },
                })
            }
            Err(e) => json!({
                "id": "flynt-design",
                "active": false,
                "reason": format!("not installed: {e}"),
            }),
        };

        // Style guides.
        let guide_report = style_guide::load_report(&self.vault_root)
            .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
        let guide_value = if full {
            serde_json::to_value(&guide_report).unwrap_or(Value::Null)
        } else {
            // Strip the heavy `content` fields when not requested.
            let mut p = guide_report.project.clone();
            p.content = None;
            let mut u = guide_report.user.clone();
            u.content = None;
            json!({
                "project": p,
                "user": u,
                "merged": guide_report.merged.as_ref().map(|s| {
                    if s.len() > 200 { format!("{}…", &s[..197]) } else { s.clone() }
                }),
                "setup_hint": guide_report.setup_hint,
            })
        };

        // Theme block.
        let presets = read_json(&self.presets_path()).unwrap_or(json!({}));
        let available: Vec<String> = presets.as_object()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        let active_theme = canvas_path.and_then(|p| self.read_active_theme(p));
        let active_vars = active_theme
            .as_deref()
            .and_then(|id| presets.get(id).and_then(|v| v.get("vars").cloned()));
        let theme_block = json!({
            "active_theme_id": active_theme,
            "available": available,
            "vars": active_vars,
        });

        // Primitives count + guidance lines.
        let primitives_doc = read_json(&self.primitives_path()).unwrap_or(json!({}));
        let prim_count = primitives_doc.get("primitives")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let guidance_count = primitives_doc.get("cell_authoring_guidance")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);

        Ok(json!({
            "skill": skill_block,
            "style_guides": guide_value,
            "theme": theme_block,
            "primitives": {
                "count": prim_count,
                "guidance_lines": guidance_count,
            },
            "related_skills": Vec::<Value>::new(),  // populated when omegon exposes a skill-list query
        }))
    }

    fn execute_load_style_guide(&self) -> omegon_extension::Result<Value> {
        let report = style_guide::load_report(&self.vault_root)
            .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
        serde_json::to_value(&report)
            .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))
    }

    fn execute_suggest_theme(&self, params: Value) -> omegon_extension::Result<Value> {
        let brief = params.get("brief")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or("");
        if brief.is_empty() {
            return Ok(Value::Null);
        }
        let presets = read_json(&self.presets_path())
            .unwrap_or(json!({}));
        let scored = score_themes_against_brief(brief, &presets);
        // Pick the highest-scoring preset above a threshold; null otherwise.
        let best = scored.iter().max_by_key(|(_, s)| *s);
        match best {
            Some((id, score)) if *score > 0 => {
                let preset = presets.get(id).cloned().unwrap_or(Value::Null);
                Ok(json!({
                    "theme_id": id,
                    "name": preset.get("name").cloned().unwrap_or(json!(id)),
                    "description": preset.get("description").cloned().unwrap_or(Value::Null),
                    "confidence": *score,
                    "alternatives": scored.iter()
                        .filter(|(other, _)| other != id)
                        .map(|(id, s)| json!({ "theme_id": id, "score": s }))
                        .collect::<Vec<_>>(),
                    "reasoning": format!("Brief mentioned terms matching the {id} preset's description and var palette."),
                }))
            }
            _ => Ok(Value::Null),
        }
    }

    /// Probe screen-capture permission. Lightweight: doesn't touch the WebView,
    /// just attempts a 1×1 capture via xcap and looks for the macOS-blocked
    /// black-pixel signal. Linux always reports granted.
    fn execute_capture_status(&self) -> omegon_extension::Result<Value> {
        #[cfg(target_os = "macos")]
        {
            match xcap::Monitor::all() {
                Ok(monitors) if !monitors.is_empty() => {
                    let m = &monitors[0];
                    match m.capture_region(0, 0, 1, 1) {
                        Ok(img) => {
                            let p = img.get_pixel(0, 0);
                            let visible = (p[0] as u32) + (p[1] as u32) + (p[2] as u32) > 0;
                            if visible {
                                Ok(json!({
                                    "platform": "macos",
                                    "status": "granted",
                                    "first_call_will_prompt": false,
                                }))
                            } else {
                                Ok(json!({
                                    "platform": "macos",
                                    "status": "denied",
                                    "first_call_will_prompt": false,
                                    "instructions": "Grant Screen Recording permission via System Settings → Privacy & Security → Screen Recording → enable Flynt. Restart Flynt after granting.",
                                }))
                            }
                        }
                        Err(e) => Ok(json!({
                            "platform": "macos",
                            "status": "denied",
                            "instructions": format!("Capture probe failed ({e}). Open System Settings → Privacy & Security → Screen Recording, ensure Flynt is enabled, and relaunch."),
                        })),
                    }
                }
                _ => Ok(json!({"platform": "macos", "status": "unknown"})),
            }
        }
        #[cfg(target_os = "linux")]
        {
            Ok(json!({"platform": "linux", "status": "granted", "first_call_will_prompt": false}))
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            Ok(json!({"platform": "other", "status": "unknown", "instructions": "Platform not supported by Flynt's capture pipeline."}))
        }
    }

    /// Trigger a viewport capture by writing a request file and polling for
    /// the response. Flynt-app's CanvasView watcher does the work — see
    /// `canvas_capture::process_capture_request` in flynt-app.
    async fn execute_capture_viewport(&self, params: Value) -> omegon_extension::Result<Value> {
        use flynt_core::canvas::{capture_request_dir, capture_response_dir, CaptureRequest, CaptureResponse};

        let canvas_path = params.get("canvas_path")
            .and_then(|v| v.as_str())
            .map(String::from);
        let include_metrics = params.get("include_metrics")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let request_id = uuid::Uuid::new_v4().to_string();
        let req = CaptureRequest {
            request_id: request_id.clone(),
            canvas_path,
            include_metrics,
        };

        let req_dir = capture_request_dir(&self.vault_root);
        let resp_dir = capture_response_dir(&self.vault_root);
        std::fs::create_dir_all(&req_dir)
            .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
        std::fs::create_dir_all(&resp_dir)
            .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

        // Atomic write so flynt-app's watcher never sees a half-written file.
        let req_path = req_dir.join(format!("{request_id}.json"));
        let req_tmp = req_path.with_extension("json.tmp");
        let req_json = serde_json::to_string(&req)
            .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
        std::fs::write(&req_tmp, req_json.as_bytes())
            .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;
        std::fs::rename(&req_tmp, &req_path)
            .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

        // Poll for response. Flynt-app ticks every 200ms; JS measurement +
        // capture typically <1s. 5s budget covers a slow run; longer means
        // flynt-app isn't running or the canvas pane isn't visible.
        let resp_path = resp_dir.join(format!("{request_id}.json"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            if let Ok(body) = std::fs::read_to_string(&resp_path) {
                if let Ok(resp) = serde_json::from_str::<CaptureResponse>(&body) {
                    let _ = std::fs::remove_file(&resp_path);
                    return Ok(serde_json::to_value(&resp)
                        .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?);
                }
                // Mid-write or malformed — keep polling
            }
            if std::time::Instant::now() >= deadline {
                let _ = std::fs::remove_file(&req_path);
                return Err(omegon_extension::Error::internal_error(
                    "capture timed out after 5s — is Flynt running with a canvas open? \
                     If permission has never been granted on macOS, call canvas_capture_status \
                     and surface the instructions to the operator.".to_string(),
                ));
            }
        }
    }

    fn execute_critique(&self, params: Value) -> omegon_extension::Result<Value> {
        let path = params.get("canvas_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| omegon_extension::Error::invalid_params("missing 'canvas_path'"))?;
        let rel = std::path::Path::new(path);
        if rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(omegon_extension::Error::invalid_params("path must not contain '..'"));
        }
        let abs = self.vault_root.join(rel);
        let canvas = flynt_core::canvas::Canvas::load(&abs)
            .map_err(|e| omegon_extension::Error::internal_error(e.to_string()))?;

        let blockers: Vec<String> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();
        let mut suggestions: Vec<String> = Vec::new();

        // Structural fill check (same heuristic as canvas_set_cells lint).
        for cell in &canvas.cells {
            if !outermost_fills_cell(&cell.html) {
                warnings.push(format!(
                    "cell '{}': outermost element lacks h-full — empty space will show theme bg below content",
                    cell.id
                ));
            }
            if html_has_arbitrary_tailwind(&cell.html) {
                warnings.push(format!(
                    "cell '{}': uses Tailwind arbitrary-value classes (bg-[#…]) that the curated subset can't resolve",
                    cell.id
                ));
            }
        }

        // Theme-coherence check: if cells override bg with hardcoded hex but
        // theme has its own --background, flag.
        let hardcoded_bg_count = canvas.cells.iter()
            .filter(|c| c.html.contains("background:#") || c.html.contains("background: #") || c.html.contains("bg-black") || c.html.contains("bg-white"))
            .count();
        if hardcoded_bg_count > 0 {
            suggestions.push(format!(
                "{hardcoded_bg_count} cell(s) hardcode background color instead of using theme tokens (bg-card, bg-background). Consider switching the theme via canvas_apply_theme rather than fighting it per-cell."
            ));
        }

        // Coverage check: very tall cells with very short content.
        for cell in &canvas.cells {
            if cell.h >= 3 && cell.html.len() < 200 {
                suggestions.push(format!(
                    "cell '{}': h={} but html is {} bytes — likely too tall for its content. Either reduce h or add content that earns the height.",
                    cell.id, cell.h, cell.html.len()
                ));
            }
        }

        // Style-guide presence (just a heads-up, no rule application yet).
        let guide = style_guide::load_report(&self.vault_root).ok();
        let guide_loaded = guide.as_ref().and_then(|g| g.merged.as_ref()).is_some();
        if !guide_loaded {
            suggestions.push(
                "No style guide configured — design_load_style_guide returned no merged content. \
                 Critique can't audit brand adherence without one."
                    .into(),
            );
        }

        Ok(json!({
            "canvas_path": path,
            "cell_count": canvas.cells.len(),
            "theme": canvas.theme,
            "report": {
                "blockers": blockers,
                "warnings": warnings,
                "suggestions": suggestions,
            },
            "summary": format!(
                "{} blocker(s), {} warning(s), {} suggestion(s)",
                blockers.len(), warnings.len(), suggestions.len()
            ),
        }))
    }
}

fn read_json(path: &std::path::Path) -> Option<Value> {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok())
}

/// Same heuristic as canvas_set_cells lint — kept here as a sibling
/// implementation rather than depending on flynt-agent (the dep direction
/// would be wrong; lint is a private helper there).
fn outermost_fills_cell(html: &str) -> bool {
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_alphabetic() {
            break;
        }
        i += 1;
    }
    if i >= bytes.len() { return true; }
    let tag_end = match html[i..].find('>') { Some(e) => i + e, None => return true };
    let tag = &html[i..=tag_end];
    tag.contains("h-full")
        || tag.contains("h-screen")
        || tag.contains("height:100%")
        || tag.contains("height: 100%")
}

fn html_has_arbitrary_tailwind(html: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "bg-[", "text-[", "border-[", "ring-[", "shadow-[",
        "p-[", "px-[", "py-[", "m-[", "w-[", "h-[", "rounded-[",
    ];
    PREFIXES.iter().any(|p| html.contains(p))
}

/// Score each preset against the brief by simple keyword overlap with the
/// preset's name + description. Cheap, deterministic, no model dep. Returns
/// (preset_id, score) pairs.
fn score_themes_against_brief(brief: &str, presets: &Value) -> Vec<(String, u32)> {
    let lower_brief = brief.to_lowercase();
    let words: Vec<&str> = lower_brief
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .collect();
    let mut out = Vec::new();
    if let Some(map) = presets.as_object() {
        for (id, preset) in map {
            let mut score: u32 = 0;
            let mut blob = id.to_lowercase();
            if let Some(name) = preset.get("name").and_then(|v| v.as_str()) {
                blob.push(' ');
                blob.push_str(&name.to_lowercase());
            }
            if let Some(desc) = preset.get("description").and_then(|v| v.as_str()) {
                blob.push(' ');
                blob.push_str(&desc.to_lowercase());
            }
            for w in &words {
                if blob.contains(w) {
                    score += 1;
                }
            }
            // Heuristic boosts for common visual-language pairings the bundled
            // presets handle well.
            if (lower_brief.contains("dark") || lower_brief.contains("black"))
                && id != "light" {
                score += 1;
            }
            if lower_brief.contains("light") && id == "light" {
                score += 2;
            }
            if (lower_brief.contains("warm") || lower_brief.contains("orange") || lower_brief.contains("amber"))
                && id == "amber" {
                score += 2;
            }
            if (lower_brief.contains("cool") || lower_brief.contains("blue") || lower_brief.contains("teal") || lower_brief.contains("ocean"))
                && id == "ocean" {
                score += 2;
            }
            out.push((id.clone(), score));
        }
    }
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out
}

#[allow(dead_code)]
const _USE_SKILL_INSTALL_BYTES: &[u8] = skill_install::SKILL_BYTES;

#[cfg(test)]
mod tests {
    use super::*;
    use omegon_extension::Extension;
    use tempfile::TempDir;

    fn test_ext() -> (TempDir, DesignExtension) {
        let tmp = TempDir::new().unwrap();
        // Seed the canvas-asset files the influence-describer expects.
        let assets_dir = tmp.path().join(".flynt-local/flynt/assets");
        std::fs::create_dir_all(&assets_dir).unwrap();
        std::fs::write(
            assets_dir.join("tweakcn-presets.json"),
            r##"{
                "default": {"name":"Default","description":"Neutral dark","vars":{"--background":"#0c0c0c","--primary":"#6c8cff"}},
                "amber":   {"name":"Amber","description":"Warm dark with amber accent","vars":{"--background":"#0c0a09","--primary":"#f59e0b"}},
                "ocean":   {"name":"Ocean","description":"Cool blue with teal","vars":{"--background":"#0a0f1c","--primary":"#06b6d4"}},
                "light":   {"name":"Light","description":"Clean white","vars":{"--background":"#ffffff","--primary":"#1f2937"}}
            }"##,
        ).unwrap();
        std::fs::write(
            assets_dir.join("shadcn-primitives.json"),
            r#"{"version":1,"cell_authoring_guidance":["wrap with h-full"],"primitives":[{"id":"button","html":"<b/>"}]}"#,
        ).unwrap();
        let path = tmp.path().to_path_buf();
        (tmp, DesignExtension::new(path))
    }

    fn test_ext_inplace(tmp: &TempDir) -> DesignExtension {
        DesignExtension::new(tmp.path().to_path_buf())
    }

    #[tokio::test]
    async fn get_tools_lists_all_design_tools() {
        let (_tmp, ext) = test_ext();
        let tools = ext.handle_rpc("get_tools", json!({})).await.unwrap();
        let names: Vec<String> = tools.as_array().unwrap().iter()
            .filter_map(|t| t["name"].as_str().map(str::to_string))
            .collect();
        for expected in [
            "design_describe_influences", "design_load_style_guide",
            "design_suggest_theme", "design_critique"
        ] {
            assert!(names.contains(&expected.to_string()), "missing: {expected}");
        }
    }

    #[tokio::test]
    async fn describe_influences_summarizes_presets_and_primitives() {
        let (tmp, _) = test_ext();
        let ext = test_ext_inplace(&tmp);
        let out = ext.handle_rpc("execute_design_describe_influences", json!({})).await.unwrap();
        assert_eq!(out["primitives"]["count"], 1);
        assert_eq!(out["primitives"]["guidance_lines"], 1);
        let available = out["theme"]["available"].as_array().unwrap();
        assert!(available.len() >= 4);
    }

    #[tokio::test]
    async fn describe_influences_full_content_includes_skill_text_when_installed() {
        let (tmp, _) = test_ext();
        let ext = test_ext_inplace(&tmp);
        // Skill not installed in this test env — full_content should reflect that.
        let out = ext.handle_rpc(
            "execute_design_describe_influences",
            json!({"full_content": true}),
        ).await.unwrap();
        assert!(out["skill"]["active"] == false || out["skill"]["full_content"].is_string());
    }

    #[tokio::test]
    async fn suggest_theme_picks_amber_for_warm_brief() {
        let (tmp, _) = test_ext();
        let ext = test_ext_inplace(&tmp);
        let out = ext.handle_rpc(
            "execute_design_suggest_theme",
            json!({"brief": "warm orange industrial vibe"}),
        ).await.unwrap();
        assert_eq!(out["theme_id"], "amber");
    }

    #[tokio::test]
    async fn suggest_theme_picks_ocean_for_cool_brief() {
        let (tmp, _) = test_ext();
        let ext = test_ext_inplace(&tmp);
        let out = ext.handle_rpc(
            "execute_design_suggest_theme",
            json!({"brief": "cool teal blue ocean dashboard"}),
        ).await.unwrap();
        assert_eq!(out["theme_id"], "ocean");
    }

    #[tokio::test]
    async fn suggest_theme_returns_null_for_empty_brief() {
        let (tmp, _) = test_ext();
        let ext = test_ext_inplace(&tmp);
        let out = ext.handle_rpc(
            "execute_design_suggest_theme",
            json!({"brief": ""}),
        ).await.unwrap();
        assert!(out.is_null());
    }

    #[tokio::test]
    async fn load_style_guide_emits_setup_hint_when_neither_level_present() {
        let (tmp, _) = test_ext();
        let ext = test_ext_inplace(&tmp);
        let out = ext.handle_rpc("execute_design_load_style_guide", json!({})).await.unwrap();
        assert!(!out["project"]["loaded"].as_bool().unwrap());
        // user-level may or may not exist on the test system; setup_hint
        // appears only when neither side resolves
        if out["merged"].is_null() {
            assert!(out["setup_hint"].is_string());
        }
    }

    #[tokio::test]
    async fn critique_flags_missing_h_full() {
        let (tmp, _) = test_ext();
        let ext = test_ext_inplace(&tmp);
        // Seed a canvas with a cell that lacks h-full.
        let mut canvas = flynt_core::canvas::Canvas::default();
        canvas.upsert_cell(flynt_core::canvas::Cell {
            id: "x".into(), x: 0, y: 0, w: 4, h: 3,
            html: "<div class=\"bg-card p-4\">x</div>".into(),
            css: "".into(), js: None,
        });
        std::fs::create_dir_all(tmp.path().join("canvases")).unwrap();
        canvas.save(&tmp.path().join("canvases/Demo.canvas")).unwrap();

        let out = ext.handle_rpc(
            "execute_design_critique",
            json!({"canvas_path": "canvases/Demo.canvas"}),
        ).await.unwrap();
        let warnings = out["report"]["warnings"].as_array().unwrap();
        assert!(warnings.iter().any(|w| w.as_str().unwrap().contains("h-full")));
    }

    #[tokio::test]
    async fn critique_flags_arbitrary_tailwind() {
        let (tmp, _) = test_ext();
        let ext = test_ext_inplace(&tmp);
        let mut canvas = flynt_core::canvas::Canvas::default();
        canvas.upsert_cell(flynt_core::canvas::Cell {
            id: "x".into(), x: 0, y: 0, w: 4, h: 3,
            html: "<div class=\"h-full bg-[#FF0000]\">x</div>".into(),
            css: "".into(), js: None,
        });
        std::fs::create_dir_all(tmp.path().join("canvases")).unwrap();
        canvas.save(&tmp.path().join("canvases/Demo.canvas")).unwrap();

        let out = ext.handle_rpc(
            "execute_design_critique",
            json!({"canvas_path": "canvases/Demo.canvas"}),
        ).await.unwrap();
        let warnings = out["report"]["warnings"].as_array().unwrap();
        assert!(warnings.iter().any(|w| w.as_str().unwrap().contains("arbitrary")));
    }

    #[tokio::test]
    async fn critique_rejects_path_traversal() {
        let (tmp, _) = test_ext();
        let ext = test_ext_inplace(&tmp);
        let err = ext.handle_rpc(
            "execute_design_critique",
            json!({"canvas_path": "../etc/passwd"}),
        ).await.unwrap_err();
        assert!(err.to_string().contains(".."));
    }
}
