//! Agent tools for semantic Excalidraw drawings.
//!
//! These tools sit above raw `.excalidraw` JSON. Agents author a
//! `DrawingSpec` in terms of components and connections; Flynt renders that
//! spec deterministically into Excalidraw elements and stores the spec beside
//! the drawing as `<name>.drawing.json` for future patching.

use flynt_core::drawing::{
    DrawingComponent, DrawingConnection, DrawingSpec, render_excalidraw, validate,
};
use flynt_store::project::Project;
use omegon_extension::{Error as ExtError, Result as ExtResult};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "drawing_create_spec",
            "label": "Drawing: Create from Spec",
            "description": "Create a Flynt Excalidraw drawing from a semantic DrawingSpec. Use this instead of generating raw Excalidraw JSON. Writes drawings/<name>.excalidraw, drawings/<name>.drawing.json, and drawings/<name>.md. The spec describes components and connections; Flynt assigns deterministic positions and Excalidraw element IDs.",
            "parameters": {
                "type": "object",
                "required": ["name", "spec"],
                "properties": {
                    "name": { "type": "string", "description": "Drawing name, used as filename stem under drawings/." },
                    "spec": drawing_spec_schema()
                }
            }
        }),
        json!({
            "name": "drawing_get_spec",
            "label": "Drawing: Get Spec",
            "description": "Read the semantic DrawingSpec sidecar for a drawing. Pass path/drawing_path to the .excalidraw file. Returns null spec if the drawing was hand-authored without a sidecar.",
            "parameters": {
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": { "type": "string", "description": "Project-relative .excalidraw path." }
                }
            }
        }),
        json!({
            "name": "drawing_render_spec",
            "label": "Drawing: Render Spec",
            "description": "Replace an existing .excalidraw scene from a complete semantic DrawingSpec and update the .drawing.json sidecar. Use for deterministic full redraws.",
            "parameters": {
                "type": "object",
                "required": ["path", "spec"],
                "properties": {
                    "path": { "type": "string", "description": "Project-relative .excalidraw path." },
                    "spec": drawing_spec_schema()
                }
            }
        }),
        json!({
            "name": "drawing_patch_spec",
            "label": "Drawing: Patch Spec",
            "description": "Patch a semantic DrawingSpec sidecar and re-render the .excalidraw scene. Upserts components/connections by id and removes requested ids. Use this for incremental edits to an existing agent-authored drawing.",
            "parameters": {
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": { "type": "string", "description": "Project-relative .excalidraw path." },
                    "title": { "type": "string" },
                    "subtitle": { "type": "string" },
                    "layout": drawing_layout_schema(),
                    "style": drawing_style_schema(),
                    "upsert_components": { "type": "array", "items": drawing_component_schema() },
                    "remove_components": { "type": "array", "items": { "type": "string" } },
                    "upsert_connections": { "type": "array", "items": drawing_connection_schema() },
                    "remove_connections": { "type": "array", "items": { "type": "string" } }
                }
            }
        }),
        json!({
            "name": "drawing_validate_spec",
            "label": "Drawing: Validate Spec",
            "description": "Validate a semantic DrawingSpec without writing files. Returns warnings for duplicate ids and dangling connections.",
            "parameters": {
                "type": "object",
                "required": ["spec"],
                "properties": { "spec": drawing_spec_schema() }
            }
        }),
    ]
}

pub fn drawing_create_spec(project: &Project, params: Value) -> ExtResult<Value> {
    let name = params["name"]
        .as_str()
        .ok_or_else(|| ExtError::invalid_params("missing 'name'"))?;
    validate_file_stem(name)?;
    let spec = parse_spec_arg(&params)?;

    let drawings_dir = project.root.join("drawings");
    std::fs::create_dir_all(&drawings_dir).map_err(|e| ExtError::internal_error(e.to_string()))?;

    let drawing_rel = PathBuf::from("drawings").join(format!("{name}.excalidraw"));
    let spec_rel = PathBuf::from("drawings").join(format!("{name}.drawing.json"));
    let wrapper_rel = PathBuf::from("drawings").join(format!("{name}.md"));
    for rel in [&drawing_rel, &spec_rel, &wrapper_rel] {
        if project.root.join(rel).exists() {
            return Err(ExtError::invalid_params(format!(
                "{} already exists",
                rel.display()
            )));
        }
    }

    write_spec_and_scene(project, &drawing_rel, &spec)?;

    let excalidraw_file = drawing_rel
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let md_content = format!(
        "+++\ntitle = \"{}\"\ntags = [\"drawing\"]\n+++\n\n![[{excalidraw_file}]]\n",
        name.replace('"', "\\\"")
    );
    project
        .save_document_content(&wrapper_rel, &md_content)
        .map_err(|e| ExtError::internal_error(e.to_string()))?;

    let render = render_excalidraw(&spec);
    Ok(json!({
        "wrapper_path": wrapper_rel.to_string_lossy(),
        "drawing_path": drawing_rel.to_string_lossy(),
        "spec_path": spec_rel.to_string_lossy(),
        "component_count": spec.components.len(),
        "connection_count": spec.connections.len(),
        "validation": render.validation,
        "element_map": render.element_map,
    }))
}

pub fn drawing_get_spec(project: &Project, params: Value) -> ExtResult<Value> {
    let drawing_rel =
        parse_drawing_path(params.get("path").or_else(|| params.get("drawing_path")))?;
    let spec_rel = spec_path_for(&drawing_rel);
    let spec_abs = project.root.join(&spec_rel);
    let spec = if spec_abs.exists() {
        Some(load_spec(&spec_abs)?)
    } else {
        None
    };
    Ok(json!({
        "drawing_path": drawing_rel.to_string_lossy(),
        "spec_path": spec_rel.to_string_lossy(),
        "spec": spec,
    }))
}

pub fn drawing_render_spec(project: &Project, params: Value) -> ExtResult<Value> {
    let drawing_rel =
        parse_drawing_path(params.get("path").or_else(|| params.get("drawing_path")))?;
    if !project.root.join(&drawing_rel).exists() {
        return Err(ExtError::invalid_params(format!(
            "no such drawing: {} — call drawing_create_spec first",
            drawing_rel.display()
        )));
    }
    let spec = parse_spec_arg(&params)?;
    write_spec_and_scene(project, &drawing_rel, &spec)?;
    let render = render_excalidraw(&spec);
    Ok(json!({
        "drawing_path": drawing_rel.to_string_lossy(),
        "spec_path": spec_path_for(&drawing_rel).to_string_lossy(),
        "updated": true,
        "validation": render.validation,
        "element_map": render.element_map,
    }))
}

pub fn drawing_patch_spec(project: &Project, params: Value) -> ExtResult<Value> {
    let drawing_rel =
        parse_drawing_path(params.get("path").or_else(|| params.get("drawing_path")))?;
    let spec_abs = project.root.join(spec_path_for(&drawing_rel));
    if !spec_abs.exists() {
        return Err(ExtError::invalid_params(format!(
            "no semantic sidecar for {}; call drawing_render_spec with a complete spec first",
            drawing_rel.display()
        )));
    }
    let mut spec = load_spec(&spec_abs)?;

    if let Some(title) = params.get("title").and_then(|v| v.as_str()) {
        spec.title = Some(title.to_string());
    }
    if let Some(subtitle) = params.get("subtitle").and_then(|v| v.as_str()) {
        spec.subtitle = Some(subtitle.to_string());
    }
    if let Some(layout) = params.get("layout") {
        spec.layout = serde_json::from_value(coerce_value(layout.clone(), "layout")?)
            .map_err(|e| ExtError::invalid_params(format!("layout: {e}")))?;
    }
    if let Some(style) = params.get("style") {
        spec.style = serde_json::from_value(coerce_value(style.clone(), "style")?)
            .map_err(|e| ExtError::invalid_params(format!("style: {e}")))?;
    }
    if let Some(ids) = params.get("remove_components") {
        let ids = string_array(ids.clone(), "remove_components")?;
        spec.components.retain(|c| !ids.contains(&c.id));
        spec.connections
            .retain(|c| !ids.contains(&c.from) && !ids.contains(&c.to));
    }
    if let Some(ids) = params.get("remove_connections") {
        let ids = string_array(ids.clone(), "remove_connections")?;
        spec.connections.retain(|c| !ids.contains(&c.id));
    }
    if let Some(components) = params.get("upsert_components") {
        for component in component_array(components.clone())? {
            upsert_by_id(&mut spec.components, component, |c| &c.id);
        }
    }
    if let Some(connections) = params.get("upsert_connections") {
        for connection in connection_array(connections.clone())? {
            upsert_by_id(&mut spec.connections, connection, |c| &c.id);
        }
    }

    write_spec_and_scene(project, &drawing_rel, &spec)?;
    let render = render_excalidraw(&spec);
    Ok(json!({
        "drawing_path": drawing_rel.to_string_lossy(),
        "spec_path": spec_path_for(&drawing_rel).to_string_lossy(),
        "component_count": spec.components.len(),
        "connection_count": spec.connections.len(),
        "validation": render.validation,
        "element_map": render.element_map,
    }))
}

pub fn drawing_validate_spec(_project: &Project, params: Value) -> ExtResult<Value> {
    let spec = parse_spec_arg(&params)?;
    Ok(json!({
        "validation": validate(&spec),
    }))
}

fn write_spec_and_scene(
    project: &Project,
    drawing_rel: &Path,
    spec: &DrawingSpec,
) -> ExtResult<()> {
    let drawing_abs = project.root.join(drawing_rel);
    if let Some(parent) = drawing_abs.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ExtError::internal_error(e.to_string()))?;
    }
    let render = render_excalidraw(spec);
    let scene = serde_json::to_string_pretty(&render.scene)
        .map_err(|e| ExtError::internal_error(e.to_string()))?;
    std::fs::write(&drawing_abs, scene).map_err(|e| ExtError::internal_error(e.to_string()))?;

    let spec_abs = project.root.join(spec_path_for(drawing_rel));
    let body =
        serde_json::to_string_pretty(spec).map_err(|e| ExtError::internal_error(e.to_string()))?;
    std::fs::write(&spec_abs, body).map_err(|e| ExtError::internal_error(e.to_string()))?;
    Ok(())
}

fn load_spec(path: &Path) -> ExtResult<DrawingSpec> {
    let body =
        std::fs::read_to_string(path).map_err(|e| ExtError::internal_error(e.to_string()))?;
    serde_json::from_str(&body).map_err(|e| ExtError::internal_error(format!("parse spec: {e}")))
}

fn parse_spec_arg(params: &Value) -> ExtResult<DrawingSpec> {
    let value = params
        .get("spec")
        .ok_or_else(|| ExtError::invalid_params("missing 'spec'"))?;
    serde_json::from_value(coerce_value(value.clone(), "spec")?)
        .map_err(|e| ExtError::invalid_params(format!("spec: {e}")))
}

fn parse_drawing_path(value: Option<&Value>) -> ExtResult<PathBuf> {
    let path = value
        .and_then(|v| v.as_str())
        .ok_or_else(|| ExtError::invalid_params("missing 'path' (or 'drawing_path')"))?;
    let rel = Path::new(path);
    if rel
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(ExtError::invalid_params("path must not contain '..'"));
    }
    if rel.extension().and_then(|e| e.to_str()) != Some("excalidraw") {
        return Err(ExtError::invalid_params("path must end in .excalidraw"));
    }
    Ok(rel.to_path_buf())
}

fn spec_path_for(drawing_rel: &Path) -> PathBuf {
    drawing_rel.with_extension("drawing.json")
}

fn validate_file_stem(name: &str) -> ExtResult<()> {
    if name.trim().is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(ExtError::invalid_params(
            "name must not be empty or contain path separators",
        ));
    }
    Ok(())
}

fn coerce_value(value: Value, field: &str) -> ExtResult<Value> {
    match value {
        Value::String(s) => serde_json::from_str(&s).map_err(|e| {
            ExtError::invalid_params(format!("{field}: stringified JSON did not parse: {e}"))
        }),
        other => Ok(other),
    }
}

fn string_array(value: Value, field: &str) -> ExtResult<Vec<String>> {
    let value = coerce_value(value, field)?;
    let arr = value
        .as_array()
        .ok_or_else(|| ExtError::invalid_params(format!("{field}: expected array")))?;
    Ok(arr
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect())
}

fn component_array(value: Value) -> ExtResult<Vec<DrawingComponent>> {
    serde_json::from_value(coerce_value(value, "upsert_components")?)
        .map_err(|e| ExtError::invalid_params(format!("upsert_components: {e}")))
}

fn connection_array(value: Value) -> ExtResult<Vec<DrawingConnection>> {
    serde_json::from_value(coerce_value(value, "upsert_connections")?)
        .map_err(|e| ExtError::invalid_params(format!("upsert_connections: {e}")))
}

fn upsert_by_id<T, F>(items: &mut Vec<T>, item: T, id: F)
where
    F: Fn(&T) -> &str,
{
    let needle = id(&item).to_string();
    if let Some(existing) = items.iter_mut().find(|candidate| id(candidate) == needle) {
        *existing = item;
    } else {
        items.push(item);
    }
}

fn drawing_spec_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "subtitle": { "type": "string" },
            "layout": drawing_layout_schema(),
            "style": drawing_style_schema(),
            "components": { "type": "array", "items": drawing_component_schema() },
            "connections": { "type": "array", "items": drawing_connection_schema() }
        }
    })
}

fn drawing_layout_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "direction": { "type": "string", "enum": ["left_to_right", "top_down"], "default": "left_to_right" },
            "spacing_x": { "type": "number", "default": 96 },
            "spacing_y": { "type": "number", "default": 72 }
        }
    })
}

fn drawing_style_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "theme": { "type": "string", "default": "dark" }
        }
    })
}

fn drawing_component_schema() -> Value {
    json!({
        "type": "object",
        "required": ["id", "kind", "label"],
        "properties": {
            "id": { "type": "string", "description": "Stable semantic id." },
            "kind": { "type": "string", "description": "actor | service | database | queue | cache | api | worker_pool | boundary | note | callout | <custom>" },
            "label": { "type": "string" },
            "description": { "type": "string" },
            "group": { "type": "string", "description": "Boundary component id to wrap this component." },
            "rank": { "type": "integer", "description": "Primary layout order." },
            "lane": { "type": "integer", "description": "Secondary layout row/column." },
            "emphasis": { "type": "string", "description": "primary or omitted." }
        }
    })
}

fn drawing_connection_schema() -> Value {
    json!({
        "type": "object",
        "required": ["id", "from", "to"],
        "properties": {
            "id": { "type": "string", "description": "Stable semantic id." },
            "from": { "type": "string", "description": "Source component id." },
            "to": { "type": "string", "description": "Target component id." },
            "label": { "type": "string" },
            "style": { "type": "string", "description": "warning | success | danger | omitted" }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn project(tmp: &TempDir) -> Project {
        Project::open(tmp.path()).unwrap()
    }

    fn spec() -> Value {
        json!({
            "title": "Qrypt",
            "components": [
                {"id": "ui", "kind": "actor", "label": "Operator", "rank": 0, "lane": 0},
                {"id": "api", "kind": "service", "label": "Control API", "rank": 1, "lane": 0}
            ],
            "connections": [
                {"id": "ui-api", "from": "ui", "to": "api", "label": "HTTPS"}
            ]
        })
    }

    #[test]
    fn create_spec_writes_wrapper_scene_and_sidecar() {
        let tmp = TempDir::new().unwrap();
        let project = project(&tmp);
        let out = drawing_create_spec(&project, json!({"name": "Arch", "spec": spec()})).unwrap();
        assert_eq!(out["drawing_path"], "drawings/Arch.excalidraw");
        assert!(tmp.path().join("drawings/Arch.md").exists());
        assert!(tmp.path().join("drawings/Arch.excalidraw").exists());
        assert!(tmp.path().join("drawings/Arch.drawing.json").exists());
    }

    #[test]
    fn patch_spec_upserts_and_rerenders() {
        let tmp = TempDir::new().unwrap();
        let project = project(&tmp);
        drawing_create_spec(&project, json!({"name": "Arch", "spec": spec()})).unwrap();
        let out = drawing_patch_spec(
            &project,
            json!({
                "path": "drawings/Arch.excalidraw",
                "upsert_components": [
                    {"id": "db", "kind": "database", "label": "SQLite", "rank": 2, "lane": 0}
                ],
                "upsert_connections": [
                    {"id": "api-db", "from": "api", "to": "db", "label": "read/write"}
                ]
            }),
        )
        .unwrap();
        assert_eq!(out["component_count"], 3);
        let got = drawing_get_spec(&project, json!({"path": "drawings/Arch.excalidraw"})).unwrap();
        assert_eq!(got["spec"]["components"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn render_spec_rejects_missing_drawing() {
        let tmp = TempDir::new().unwrap();
        let project = project(&tmp);
        let err = drawing_render_spec(
            &project,
            json!({"path": "drawings/Missing.excalidraw", "spec": spec()}),
        )
        .unwrap_err();
        assert!(err.to_string().contains("drawing_create_spec"));
    }
}
