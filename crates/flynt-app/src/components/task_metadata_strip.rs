//! Task metadata strip — pills + pickers above the editor body.
//!
//! Renders below the title bar in the notes view when the document's
//! frontmatter has `kind = "task"`. Pills are clickable; clicking opens
//! a popover with the right input variant for the field. Changes flow
//! through `Project::set_data_field`, the watcher fires, and both the
//! notes view and the kanban update — same path the agent tools use.
//!
//! ## Visual hierarchy
//!
//! Status pill is the primary control (color-coded by status, leftmost
//! after the kind badge). Column pill is de-emphasized (column = where
//! the operator put the card; status = what it's actually doing). Other
//! pills come after in this order: priority · column · board · due_date
//! · tags · engagement · ↑ design_node.
//!
//! ## Decoupling
//!
//! Status changes do not touch column. Column changes do not touch
//! status. Sentry reads status; the kanban + this strip render column
//! as placement metadata. Coupling them was considered and explicitly
//! rejected — operator might keep a `done`-status task in `Active` for
//! visibility, or move an `in_progress` task to `Archive` because they
//! lost interest.

use crate::components::field_schema::{
    priority_int_for_label, priority_label_for_int, task_field_schemas, FieldDescriptor, FieldKind,
    LookupSource,
};
use dioxus::prelude::*;
use flynt_core::models::{Board, Engagement, Frontmatter};
use std::path::PathBuf;

// ── Top-level strip ─────────────────────────────────────────────────────────

#[component]
pub fn TaskMetadataStrip(
    path: PathBuf,
    frontmatter: Frontmatter,
    boards: ReadSignal<Vec<Board>>,
    engagements: ReadSignal<Vec<Engagement>>,
) -> Element {
    // Renders only for tasks. Other entity kinds get nothing in v1 —
    // a generic `EntityMetadataStrip` per kind lands later.
    let is_task = frontmatter.kind.as_deref() == Some("task");
    if !is_task {
        return rsx! {};
    }

    let data = frontmatter.data.as_ref().and_then(|v| v.as_table());

    // Helpers to pull individual fields out of the [data] table. Defaults
    // mirror what `Task::new` produces for a fresh kanban-created task.
    let get_str = |key: &str| -> Option<String> {
        data.and_then(|t| t.get(key))
            .and_then(|v| v.as_str())
            .map(String::from)
    };
    let get_int = |key: &str| -> Option<i64> {
        data.and_then(|t| t.get(key)).and_then(|v| v.as_integer())
    };
    let get_str_list = |key: &str| -> Vec<String> {
        data.and_then(|t| t.get(key))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };

    let status = get_str("status").unwrap_or_else(|| "todo".into());
    let priority_int = get_int("priority").unwrap_or(2);
    let priority = priority_label_for_int(priority_int).to_string();
    let column = get_str("column").unwrap_or_else(|| "Active".into());
    let board_id = get_str("board")
        .and_then(|s| uuid::Uuid::parse_str(&s).ok());
    let due_date = get_str("due_date");
    let tags = get_str_list("tags");
    let engagement_id = get_str("engagement");
    let design_node_id = get_str("design_node");

    // Resolve board name via the cached lookup. Falls back to the raw
    // UUID (truncated) if the board isn't in the cache (deleted? not
    // yet loaded?) so we never render an empty-looking pill.
    let board_name = board_id
        .and_then(|id| {
            boards.read().iter().find(|b| b.id.0 == id).map(|b| b.name.clone())
        })
        .unwrap_or_else(|| {
            board_id
                .map(|id| format!("(missing board {})", &id.to_string()[..8]))
                .unwrap_or_else(|| "(no board)".into())
        });

    let engagement_name = engagement_id
        .as_ref()
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .and_then(|id| {
            engagements
                .read()
                .iter()
                .find(|e| e.id.0 == id)
                .map(|e| e.name.clone())
        });

    let active_board = board_id;
    let schemas = task_field_schemas(active_board);

    let path_for_handlers = path.clone();

    rsx! {
        div { class: "task-metadata-strip",
            span { class: "kind-badge", "task" }

            // Status — the primary lifecycle pill.
            EditablePill {
                class: format!("pill pill-status pill-status-{status}"),
                label: status.clone(),
                field: schemas["status"].clone(),
                current: Some(status.clone()),
                path: path_for_handlers.clone(),
                boards,
                engagements,
            }

            // Priority — also primary, color-coded by level.
            EditablePill {
                class: format!("pill pill-priority pill-priority-{priority}"),
                label: priority.clone(),
                field: schemas["priority"].clone(),
                current: Some(priority.clone()),
                path: path_for_handlers.clone(),
                boards,
                engagements,
            }

            // Column — de-emphasized; this is where the card sits.
            EditablePill {
                class: "pill pill-column".to_string(),
                label: column.clone(),
                field: schemas["column"].clone(),
                current: Some(column.clone()),
                path: path_for_handlers.clone(),
                boards,
                engagements,
            }

            EditablePill {
                class: "pill pill-board".to_string(),
                label: board_name.clone(),
                field: schemas["board"].clone(),
                current: board_id.map(|id| id.to_string()),
                path: path_for_handlers.clone(),
                boards,
                engagements,
            }

            if let Some(due) = due_date.clone() {
                EditablePill {
                    class: "pill pill-due".to_string(),
                    label: format!("Due {due}"),
                    field: schemas["due_date"].clone(),
                    current: Some(due),
                    path: path_for_handlers.clone(),
                    boards,
                    engagements,
                }
            }

            EditablePill {
                class: "pill pill-tags".to_string(),
                label: if tags.is_empty() {
                    "+ tags".into()
                } else {
                    format!("#{}", tags.join(" #"))
                },
                field: schemas["tags"].clone(),
                current: Some(tags.join(", ")),
                path: path_for_handlers.clone(),
                boards,
                engagements,
            }

            if let Some(name) = engagement_name {
                EditablePill {
                    class: "pill pill-engagement".to_string(),
                    label: name,
                    field: schemas["engagement"].clone(),
                    current: engagement_id.clone(),
                    path: path_for_handlers.clone(),
                    boards,
                    engagements,
                }
            }

            // Design node — link UP only, no editor. Clicking navigates
            // to the parent design node document.
            if let Some(_dn_id) = design_node_id {
                // Resolution to a name lookup is a Phase 2 concern — for
                // now show the icon + a placeholder; click handler is
                // deferred until DesignNode resolution lands.
                span {
                    class: "pill pill-design-node",
                    title: "Parent design node",
                    "↑ design"
                }
            }
        }
    }
}

// ── Editable pill ───────────────────────────────────────────────────────────

/// A clickable pill that opens a picker popover anchored to itself.
///
/// One component, four picker variants — keeps the rsx in the parent
/// strip flat and lets the per-pill className carry styling.
#[component]
fn EditablePill(
    class: String,
    label: String,
    field: FieldDescriptor,
    current: Option<String>,
    path: PathBuf,
    boards: ReadSignal<Vec<Board>>,
    engagements: ReadSignal<Vec<Engagement>>,
) -> Element {
    let mut open = use_signal(|| false);
    let class_for_button = class.clone();
    let label_for_button = label.clone();

    rsx! {
        div { class: "pill-anchor",
            button {
                class: class_for_button,
                onclick: move |_| { let v = *open.read(); *open.write() = !v; },
                "{label_for_button}"
            }
            if *open.read() {
                FieldPicker {
                    field: field.clone(),
                    current: current.clone(),
                    path: path.clone(),
                    boards,
                    engagements,
                    on_close: move |_| *open.write() = false,
                }
            }
        }
    }
}

// ── Picker popover ──────────────────────────────────────────────────────────

#[component]
fn FieldPicker(
    field: FieldDescriptor,
    current: Option<String>,
    path: PathBuf,
    boards: ReadSignal<Vec<Board>>,
    engagements: ReadSignal<Vec<Engagement>>,
    on_close: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "field-picker-popover",
            // Esc anywhere inside cancels.
            onkeydown: move |e| {
                if e.key() == Key::Escape { on_close.call(()); }
            },
            match &field.kind {
                FieldKind::Enum { values } => rsx! {
                    EnumPicker {
                        field: field.clone(),
                        values: values.clone(),
                        current: current.clone(),
                        path: path.clone(),
                        on_close,
                    }
                },
                FieldKind::Lookup { source, .. } => rsx! {
                    LookupPicker {
                        field: field.clone(),
                        source: source.clone(),
                        current: current.clone(),
                        path: path.clone(),
                        boards,
                        engagements,
                        on_close,
                    }
                },
                FieldKind::FreeText { .. } => rsx! {
                    TextPicker {
                        field: field.clone(),
                        current: current.clone(),
                        path: path.clone(),
                        on_close,
                    }
                },
                FieldKind::Date => rsx! {
                    DatePicker {
                        field: field.clone(),
                        current: current.clone(),
                        path: path.clone(),
                        on_close,
                    }
                },
            }
        }
    }
}

// ── Picker variants ─────────────────────────────────────────────────────────

#[component]
fn EnumPicker(
    field: FieldDescriptor,
    values: Vec<String>,
    current: Option<String>,
    path: PathBuf,
    on_close: EventHandler<()>,
) -> Element {
    rsx! {
        ul { class: "field-picker-list",
            for value in values {
                {
                    let is_current = current.as_deref() == Some(value.as_str());
                    let val_for_click = value.clone();
                    let path_for_click = path.clone();
                    let key_for_click = field.key.clone();
                    rsx! {
                        li {
                            class: if is_current { "field-picker-item current" } else { "field-picker-item" },
                            onclick: move |_| {
                                let key = key_for_click.clone();
                                let value = val_for_click.clone();
                                let path = path_for_click.clone();
                                spawn(async move {
                                    apply_field_change(&key, &value, &path).await;
                                });
                                on_close.call(());
                            },
                            "{value}"
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn LookupPicker(
    field: FieldDescriptor,
    source: LookupSource,
    current: Option<String>,
    path: PathBuf,
    boards: ReadSignal<Vec<Board>>,
    engagements: ReadSignal<Vec<Engagement>>,
    on_close: EventHandler<()>,
) -> Element {
    // Resolve the value set from the source. Cheap — both `boards` and
    // `engagements` are pre-loaded and cached in the parent. Columns
    // pull from the active board's column list.
    let entries: Vec<(String, String)> = match &source {
        LookupSource::Boards => boards
            .read()
            .iter()
            .map(|b| (b.id.0.to_string(), b.name.clone()))
            .collect(),
        LookupSource::Columns(board_id) => boards
            .read()
            .iter()
            .find(|b| b.id.0 == *board_id)
            .map(|b| {
                b.columns
                    .iter()
                    .map(|c| (c.name.clone(), c.name.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        LookupSource::Engagements => engagements
            .read()
            .iter()
            .map(|e| (e.id.0.to_string(), e.name.clone()))
            .collect(),
        LookupSource::DesignNodes => Vec::new(), // resolution lands later
    };

    rsx! {
        ul { class: "field-picker-list",
            if entries.is_empty() {
                li { class: "field-picker-item muted", "(no values)" }
            }
            for (id, display) in entries {
                {
                    let is_current = current.as_deref() == Some(id.as_str());
                    let id_for_click = id.clone();
                    let path_for_click = path.clone();
                    let key_for_click = field.key.clone();
                    rsx! {
                        li {
                            class: if is_current { "field-picker-item current" } else { "field-picker-item" },
                            onclick: move |_| {
                                let key = key_for_click.clone();
                                let value = id_for_click.clone();
                                let path = path_for_click.clone();
                                spawn(async move {
                                    apply_field_change(&key, &value, &path).await;
                                });
                                on_close.call(());
                            },
                            "{display}"
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TextPicker(
    field: FieldDescriptor,
    current: Option<String>,
    path: PathBuf,
    on_close: EventHandler<()>,
) -> Element {
    let mut text = use_signal(|| current.clone().unwrap_or_default());

    rsx! {
        div { class: "field-picker-text",
            input {
                class: "field-picker-input",
                r#type: "text",
                value: "{text}",
                autofocus: true,
                oninput: move |e| *text.write() = e.value(),
                onkeydown: move |e| {
                    if e.key() == Key::Enter {
                        let value = text.read().clone();
                        let path = path.clone();
                        let key = field.key.clone();
                        spawn(async move {
                            apply_field_change(&key, &value, &path).await;
                        });
                        on_close.call(());
                    }
                },
            }
        }
    }
}

#[component]
fn DatePicker(
    field: FieldDescriptor,
    current: Option<String>,
    path: PathBuf,
    on_close: EventHandler<()>,
) -> Element {
    let initial = current.clone().unwrap_or_default();

    rsx! {
        div { class: "field-picker-date",
            input {
                class: "field-picker-input",
                r#type: "date",
                value: "{initial}",
                autofocus: true,
                onchange: move |e| {
                    let value = e.value();
                    let path = path.clone();
                    let key = field.key.clone();
                    spawn(async move {
                        apply_field_change(&key, &value, &path).await;
                    });
                    on_close.call(());
                },
            }
        }
    }
}

// ── Apply a change ──────────────────────────────────────────────────────────

/// Persist a field change. Type-translates per field key (priority is
/// stored as int; tags as array; everything else as string).
///
/// Lives outside the components so each picker can call it without
/// pulling in the surrounding ctx — the callsites read AppContext from
/// the function's body, keeping the picker variants pure UI.
async fn apply_field_change(key: &str, value: &str, rel_path: &std::path::Path) {
    // We need AppContext but components can't read context from
    // outside their render scope. Use the global ctx via the spawned
    // task pattern — caller component already passed `path`, but the
    // project comes from the runtime context. dioxus::core::ScopeId
    // doesn't help here cross-component, so use the "snapshot ctx and
    // pass-through" pattern: callers spawn this with a captured ctx.
    //
    // Practical implementation: read context inline via a helper that
    // grabs the current scope. For now we use a dispatcher set up in
    // the notes-view wiring (next commit).
    crate::components::task_metadata_strip::dispatch_field_change(key, value, rel_path).await;
}

// ── Dispatcher (set by the notes view at mount-time) ────────────────────────

use std::sync::OnceLock;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct FieldChangeRequest {
    pub key: String,
    pub value: String,
    pub path: PathBuf,
}

static DISPATCH_TX: OnceLock<mpsc::UnboundedSender<FieldChangeRequest>> = OnceLock::new();

/// Set by the notes view once at mount. The strip's pickers fire change
/// requests through this channel; the notes view's spawned receiver
/// translates them into `Project::set_data_field` calls. Going through a
/// channel rather than a direct call lets the strip stay free of
/// `AppContext` reads (Dioxus contexts are scope-bound; the picker is
/// rendered in a different scope path than the apply site).
pub fn install_dispatcher(tx: mpsc::UnboundedSender<FieldChangeRequest>) {
    let _ = DISPATCH_TX.set(tx);
}

async fn dispatch_field_change(key: &str, value: &str, rel_path: &std::path::Path) {
    let Some(tx) = DISPATCH_TX.get() else {
        tracing::warn!("TaskMetadataStrip: dispatcher not installed; change dropped");
        return;
    };
    let _ = tx.send(FieldChangeRequest {
        key: key.to_string(),
        value: value.to_string(),
        path: rel_path.to_path_buf(),
    });
}

/// Translate a wire-format string from the picker into a `toml_edit::Value`
/// shaped for the field. Public so the notes-view receiver can call it.
pub fn translate_value(key: &str, value: &str) -> toml_edit::Value {
    match key {
        "priority" => match priority_int_for_label(value) {
            Some(n) => toml_edit::Value::from(n),
            None => toml_edit::Value::from(value),
        },
        "tags" => {
            // CSV in, array out. Operator types "infra, pipeline" → ["infra", "pipeline"].
            let mut arr = toml_edit::Array::new();
            for tag in value.split(',') {
                let t = tag.trim();
                if !t.is_empty() {
                    arr.push(t);
                }
            }
            toml_edit::Value::Array(arr)
        }
        _ => toml_edit::Value::from(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translate_priority_label_to_int() {
        let v = translate_value("priority", "high");
        let i = v.as_integer().expect("high → int");
        assert_eq!(i, 3);
    }

    #[test]
    fn translate_priority_unknown_falls_through_to_string() {
        // Defensive: if a future picker emits a value the schema
        // doesn't know, we don't silently land int 0 — we land the
        // string and let the indexer surface it.
        let v = translate_value("priority", "weird");
        assert_eq!(v.as_str(), Some("weird"));
    }

    #[test]
    fn translate_tags_csv_to_array() {
        let v = translate_value("tags", "infra, pipeline,  ops  ");
        let arr = v.as_array().expect("tags → array");
        let names: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(names, vec!["infra", "pipeline", "ops"]);
    }

    #[test]
    fn translate_status_passes_through_string() {
        let v = translate_value("status", "in_progress");
        assert_eq!(v.as_str(), Some("in_progress"));
    }
}
