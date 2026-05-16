//! Semantic drawing specs for agent-authored Excalidraw scenes.
//!
//! Excalidraw remains the editable/rendered file format. `DrawingSpec` is a
//! stable authoring layer for agents: components, connections, and layout
//! constraints are converted deterministically into Excalidraw JSON.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingSpec {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub layout: DrawingLayout,
    #[serde(default)]
    pub style: DrawingStyle,
    #[serde(default)]
    pub components: Vec<DrawingComponent>,
    #[serde(default)]
    pub connections: Vec<DrawingConnection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingLayout {
    #[serde(default)]
    pub direction: LayoutDirection,
    #[serde(default = "default_spacing_x")]
    pub spacing_x: f64,
    #[serde(default = "default_spacing_y")]
    pub spacing_y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutDirection {
    LeftToRight,
    TopDown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingStyle {
    #[serde(default = "default_theme")]
    pub theme: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingComponent {
    pub id: String,
    pub kind: String,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub rank: Option<i32>,
    #[serde(default)]
    pub lane: Option<i32>,
    #[serde(default)]
    pub emphasis: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingConnection {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub style: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingRender {
    pub scene: Value,
    pub validation: DrawingValidation,
    pub element_map: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DrawingValidation {
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct BoxGeom {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Default for DrawingSpec {
    fn default() -> Self {
        Self {
            title: None,
            subtitle: None,
            layout: DrawingLayout::default(),
            style: DrawingStyle::default(),
            components: Vec::new(),
            connections: Vec::new(),
        }
    }
}

impl Default for DrawingLayout {
    fn default() -> Self {
        Self {
            direction: LayoutDirection::LeftToRight,
            spacing_x: default_spacing_x(),
            spacing_y: default_spacing_y(),
        }
    }
}

impl Default for LayoutDirection {
    fn default() -> Self {
        Self::LeftToRight
    }
}

impl Default for DrawingStyle {
    fn default() -> Self {
        Self {
            theme: default_theme(),
        }
    }
}

fn default_spacing_x() -> f64 {
    96.0
}

fn default_spacing_y() -> f64 {
    72.0
}

fn default_theme() -> String {
    "dark".into()
}

pub fn render_excalidraw(spec: &DrawingSpec) -> DrawingRender {
    let mut validation = validate(spec);
    let mut elements = Vec::new();
    let mut element_map = BTreeMap::<String, Vec<String>>::new();
    let component_boxes = layout_components(spec);
    let boundary_boxes = layout_boundaries(spec, &component_boxes);

    if let Some(title) = spec.title.as_deref() {
        let id = stable_id("title", title);
        elements.push(text_element(&id, 40.0, 32.0, title, 28, "#1c7ed6"));
        element_map.insert("title".into(), vec![id]);
    }
    if let Some(subtitle) = spec.subtitle.as_deref() {
        let id = stable_id("subtitle", subtitle);
        elements.push(text_element(&id, 40.0, 70.0, subtitle, 16, "#868e96"));
        element_map.insert("subtitle".into(), vec![id]);
    }

    for component in spec.components.iter().filter(|c| c.kind == "boundary") {
        if let Some(geom) = boundary_boxes.get(&component.id) {
            let rect_id = element_id(&component.id, "boundary");
            let label_id = element_id(&component.id, "label");
            elements.push(rect_element(
                &rect_id,
                *geom,
                "#adb5bd",
                "transparent",
                2,
                "dashed",
            ));
            elements.push(text_element(
                &label_id,
                geom.x + 16.0,
                geom.y + 12.0,
                &component.label,
                16,
                "#15aabf",
            ));
            element_map.insert(component.id.clone(), vec![rect_id, label_id]);
        } else {
            validation.warnings.push(format!(
                "boundary `{}` has no child components",
                component.id
            ));
        }
    }

    for component in spec.components.iter().filter(|c| c.kind != "boundary") {
        if let Some(geom) = component_boxes.get(&component.id) {
            let rect_id = element_id(&component.id, "box");
            let label_id = element_id(&component.id, "label");
            let palette = palette_for(&component.kind, component.emphasis.as_deref());
            elements.push(rect_element(
                &rect_id,
                *geom,
                palette.stroke,
                palette.fill,
                2,
                "solid",
            ));
            elements.push(text_element(
                &label_id,
                geom.x + 14.0,
                geom.y + 16.0,
                &component.label,
                16,
                palette.text,
            ));
            if let Some(description) = component.description.as_deref() {
                let desc_id = element_id(&component.id, "description");
                elements.push(text_element(
                    &desc_id,
                    geom.x + 14.0,
                    geom.y + 44.0,
                    description,
                    12,
                    "#495057",
                ));
                element_map.insert(component.id.clone(), vec![rect_id, label_id, desc_id]);
            } else {
                element_map.insert(component.id.clone(), vec![rect_id, label_id]);
            }
        }
    }

    for connection in &spec.connections {
        let Some(from) = component_boxes.get(&connection.from) else {
            continue;
        };
        let Some(to) = component_boxes.get(&connection.to) else {
            continue;
        };
        let arrow_id = element_id(&connection.id, "arrow");
        let mut ids = vec![arrow_id.clone()];
        elements.push(arrow_element(
            &arrow_id,
            connection_points(*from, *to, spec.layout.direction),
            stroke_for_connection(connection.style.as_deref()),
        ));
        if let Some(label) = connection.label.as_deref() {
            let label_id = element_id(&connection.id, "label");
            let mid = midpoint(*from, *to);
            elements.push(text_element(
                &label_id,
                mid.0 - 40.0,
                mid.1 - 24.0,
                label,
                12,
                "#868e96",
            ));
            ids.push(label_id);
        }
        element_map.insert(connection.id.clone(), ids);
    }

    let scene = json!({
        "type": "excalidraw",
        "version": 2,
        "source": "flynt:drawing-spec",
        "elements": elements,
        "appState": {
            "theme": spec.style.theme,
            "viewBackgroundColor": "transparent",
            "gridSize": null
        },
        "files": {}
    });

    DrawingRender {
        scene,
        validation,
        element_map,
    }
}

pub fn validate(spec: &DrawingSpec) -> DrawingValidation {
    let mut out = DrawingValidation::default();
    let mut ids = BTreeSet::new();
    for component in &spec.components {
        if component.id.trim().is_empty() {
            out.warnings.push("component with empty id".into());
        }
        if !ids.insert(component.id.as_str()) {
            out.warnings
                .push(format!("duplicate component id `{}`", component.id));
        }
    }
    let component_ids: BTreeSet<&str> = spec.components.iter().map(|c| c.id.as_str()).collect();
    for connection in &spec.connections {
        if !component_ids.contains(connection.from.as_str()) {
            out.warnings.push(format!(
                "connection `{}` references missing from component `{}`",
                connection.id, connection.from
            ));
        }
        if !component_ids.contains(connection.to.as_str()) {
            out.warnings.push(format!(
                "connection `{}` references missing to component `{}`",
                connection.id, connection.to
            ));
        }
    }
    out
}

fn layout_components(spec: &DrawingSpec) -> BTreeMap<String, BoxGeom> {
    let mut lanes: BTreeMap<i32, Vec<&DrawingComponent>> = BTreeMap::new();
    for component in spec.components.iter().filter(|c| c.kind != "boundary") {
        lanes
            .entry(component.lane.unwrap_or(0))
            .or_default()
            .push(component);
    }

    let mut out = BTreeMap::new();
    let origin_x = 80.0;
    let origin_y = if spec.title.is_some() { 140.0 } else { 80.0 };
    for (lane_index, (_lane, mut components)) in lanes.into_iter().enumerate() {
        components.sort_by_key(|c| (c.rank.unwrap_or(i32::MAX), c.id.clone()));
        for (rank_index, component) in components.into_iter().enumerate() {
            let (w, h) = size_for(
                &component.kind,
                &component.label,
                component.description.as_deref(),
            );
            let rank = component.rank.unwrap_or(rank_index as i32).max(0) as f64;
            let lane = lane_index as f64;
            let (x, y) = match spec.layout.direction {
                LayoutDirection::LeftToRight => (
                    origin_x + rank * (w + spec.layout.spacing_x),
                    origin_y + lane * (h + spec.layout.spacing_y),
                ),
                LayoutDirection::TopDown => (
                    origin_x + lane * (w + spec.layout.spacing_x),
                    origin_y + rank * (h + spec.layout.spacing_y),
                ),
            };
            out.insert(component.id.clone(), BoxGeom { x, y, w, h });
        }
    }
    out
}

fn layout_boundaries(
    spec: &DrawingSpec,
    component_boxes: &BTreeMap<String, BoxGeom>,
) -> BTreeMap<String, BoxGeom> {
    let mut out = BTreeMap::new();
    for boundary in spec.components.iter().filter(|c| c.kind == "boundary") {
        let children: Vec<BoxGeom> = spec
            .components
            .iter()
            .filter(|c| c.group.as_deref() == Some(boundary.id.as_str()))
            .filter_map(|c| component_boxes.get(&c.id).copied())
            .collect();
        if children.is_empty() {
            continue;
        }
        let min_x = children.iter().map(|g| g.x).fold(f64::INFINITY, f64::min);
        let min_y = children.iter().map(|g| g.y).fold(f64::INFINITY, f64::min);
        let max_x = children
            .iter()
            .map(|g| g.x + g.w)
            .fold(f64::NEG_INFINITY, f64::max);
        let max_y = children
            .iter()
            .map(|g| g.y + g.h)
            .fold(f64::NEG_INFINITY, f64::max);
        out.insert(
            boundary.id.clone(),
            BoxGeom {
                x: min_x - 32.0,
                y: min_y - 56.0,
                w: max_x - min_x + 64.0,
                h: max_y - min_y + 88.0,
            },
        );
    }
    out
}

fn size_for(kind: &str, label: &str, description: Option<&str>) -> (f64, f64) {
    let label_w = (label.chars().count() as f64 * 8.0).clamp(120.0, 300.0);
    let desc_h = description
        .map(|d| 22.0 + (d.chars().count() as f64 / 32.0).ceil() * 16.0)
        .unwrap_or(0.0);
    let base = match kind {
        "database" | "cache" | "queue" => (220.0, 88.0),
        "actor" => (180.0, 64.0),
        "worker_pool" => (260.0, 82.0),
        "note" | "callout" => (240.0, 96.0),
        _ => (label_w + 36.0, 76.0),
    };
    (base.0, base.1 + desc_h)
}

struct Palette {
    stroke: &'static str,
    fill: &'static str,
    text: &'static str,
}

fn palette_for(kind: &str, emphasis: Option<&str>) -> Palette {
    if emphasis == Some("primary") {
        return Palette {
            stroke: "#1971c2",
            fill: "#d0ebff",
            text: "#0b1f33",
        };
    }
    match kind {
        "actor" => Palette {
            stroke: "#5c7cfa",
            fill: "#edf2ff",
            text: "#1f2a44",
        },
        "database" => Palette {
            stroke: "#7950f2",
            fill: "#e5dbff",
            text: "#2b1a55",
        },
        "queue" | "cache" => Palette {
            stroke: "#f08c00",
            fill: "#fff3bf",
            text: "#3d2b00",
        },
        "worker_pool" => Palette {
            stroke: "#1098ad",
            fill: "#c5f6fa",
            text: "#0b3338",
        },
        "note" | "callout" => Palette {
            stroke: "#868e96",
            fill: "#f1f3f5",
            text: "#343a40",
        },
        _ => Palette {
            stroke: "#0c8599",
            fill: "#d0ebff",
            text: "#12343b",
        },
    }
}

fn stroke_for_connection(style: Option<&str>) -> &'static str {
    match style {
        Some("warning") => "#f08c00",
        Some("success") => "#2b8a3e",
        Some("danger") => "#c92a2a",
        _ => "#adb5bd",
    }
}

fn rect_element(
    id: &str,
    geom: BoxGeom,
    stroke: &str,
    background: &str,
    stroke_width: i32,
    stroke_style: &str,
) -> Value {
    base_element(
        id,
        "rectangle",
        geom.x,
        geom.y,
        geom.w,
        geom.h,
        json!({
            "strokeColor": stroke,
            "backgroundColor": background,
            "fillStyle": "solid",
            "strokeWidth": stroke_width,
            "strokeStyle": stroke_style,
            "roundness": { "type": 3 },
        }),
    )
}

fn text_element(id: &str, x: f64, y: f64, text: &str, font_size: i32, color: &str) -> Value {
    let width = (text.chars().count() as f64 * font_size as f64 * 0.58).clamp(80.0, 520.0);
    let height = (font_size as f64 * 1.35).max(20.0);
    base_element(
        id,
        "text",
        x,
        y,
        width,
        height,
        json!({
            "strokeColor": color,
            "backgroundColor": "transparent",
            "fillStyle": "solid",
            "strokeWidth": 1,
            "strokeStyle": "solid",
            "text": text,
            "fontSize": font_size,
            "fontFamily": 1,
            "textAlign": "left",
            "verticalAlign": "top",
            "containerId": null,
            "originalText": text,
            "lineHeight": 1.25,
        }),
    )
}

fn arrow_element(id: &str, points: ((f64, f64), (f64, f64)), stroke: &str) -> Value {
    let ((x1, y1), (x2, y2)) = points;
    let x = x1.min(x2);
    let y = y1.min(y2);
    let p1 = [x1 - x, y1 - y];
    let p2 = [x2 - x, y2 - y];
    base_element(
        id,
        "arrow",
        x,
        y,
        (x2 - x1).abs().max(1.0),
        (y2 - y1).abs().max(1.0),
        json!({
            "strokeColor": stroke,
            "backgroundColor": "transparent",
            "fillStyle": "solid",
            "strokeWidth": 2,
            "strokeStyle": "solid",
            "points": [p1, p2],
            "startBinding": null,
            "endBinding": null,
            "startArrowhead": null,
            "endArrowhead": "arrow",
            "elbowed": false,
        }),
    )
}

fn base_element(
    id: &str,
    ty: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    extra: Value,
) -> Value {
    let mut value = json!({
        "id": id,
        "type": ty,
        "x": round(x),
        "y": round(y),
        "width": round(width),
        "height": round(height),
        "angle": 0,
        "seed": seed_for(id),
        "version": 1,
        "versionNonce": seed_for(&format!("{id}:nonce")),
        "isDeleted": false,
        "groupIds": [],
        "frameId": null,
        "boundElements": null,
        "link": null,
        "locked": false,
        "updated": 1,
        "roughness": 1,
        "opacity": 100,
    });
    if let (Some(obj), Some(extra_obj)) = (value.as_object_mut(), extra.as_object()) {
        for (k, v) in extra_obj {
            obj.insert(k.clone(), v.clone());
        }
    }
    value
}

fn connection_points(
    from: BoxGeom,
    to: BoxGeom,
    direction: LayoutDirection,
) -> ((f64, f64), (f64, f64)) {
    match direction {
        LayoutDirection::LeftToRight => {
            if from.x <= to.x {
                (
                    (from.x + from.w, from.y + from.h / 2.0),
                    (to.x, to.y + to.h / 2.0),
                )
            } else {
                (
                    (from.x, from.y + from.h / 2.0),
                    (to.x + to.w, to.y + to.h / 2.0),
                )
            }
        }
        LayoutDirection::TopDown => {
            if from.y <= to.y {
                (
                    (from.x + from.w / 2.0, from.y + from.h),
                    (to.x + to.w / 2.0, to.y),
                )
            } else {
                (
                    (from.x + from.w / 2.0, from.y),
                    (to.x + to.w / 2.0, to.y + to.h),
                )
            }
        }
    }
}

fn midpoint(from: BoxGeom, to: BoxGeom) -> (f64, f64) {
    (
        (from.x + from.w / 2.0 + to.x + to.w / 2.0) / 2.0,
        (from.y + from.h / 2.0 + to.y + to.h / 2.0) / 2.0,
    )
}

fn element_id(id: &str, suffix: &str) -> String {
    stable_id("flynt", &format!("{id}:{suffix}"))
}

fn stable_id(prefix: &str, input: &str) -> String {
    format!("{prefix}-{:016x}", hash64(input))
}

fn seed_for(input: &str) -> i64 {
    (hash64(input) % 2_000_000_000) as i64 + 1
}

fn hash64(input: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_spec() -> DrawingSpec {
        DrawingSpec {
            title: Some("Architecture".into()),
            subtitle: None,
            layout: DrawingLayout::default(),
            style: DrawingStyle::default(),
            components: vec![
                DrawingComponent {
                    id: "ui".into(),
                    kind: "actor".into(),
                    label: "Operator / UI".into(),
                    description: None,
                    group: None,
                    rank: Some(0),
                    lane: Some(0),
                    emphasis: None,
                },
                DrawingComponent {
                    id: "api".into(),
                    kind: "service".into(),
                    label: "Control API".into(),
                    description: Some("status + enroll".into()),
                    group: None,
                    rank: Some(1),
                    lane: Some(0),
                    emphasis: Some("primary".into()),
                },
            ],
            connections: vec![DrawingConnection {
                id: "ui-api".into(),
                from: "ui".into(),
                to: "api".into(),
                label: Some("HTTPS 443".into()),
                style: None,
            }],
        }
    }

    #[test]
    fn renders_deterministic_scene() {
        let first = render_excalidraw(&sample_spec());
        let second = render_excalidraw(&sample_spec());
        assert_eq!(first.scene, second.scene);
        assert_eq!(first.validation.warnings, Vec::<String>::new());
        assert_eq!(first.scene["type"], "excalidraw");
        assert!(first.scene["elements"].as_array().unwrap().len() >= 5);
        assert!(first.element_map.contains_key("ui"));
    }

    #[test]
    fn validation_reports_dangling_connection() {
        let mut spec = sample_spec();
        spec.connections[0].to = "missing".into();
        let validation = validate(&spec);
        assert!(
            validation
                .warnings
                .iter()
                .any(|w| w.contains("missing to component"))
        );
    }
}
