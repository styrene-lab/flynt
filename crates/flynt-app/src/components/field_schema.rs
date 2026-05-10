//! Field schemas — what's a valid value for which entity field.
//!
//! The metadata strip's pickers ask this module "what kind of input
//! should I render for `status`?" and "give me the value set for
//! `priority`." For v1 the schemas are hardcoded here in code; v2
//! will read overrides from `<project>/.flynt/field-schemas.toml`
//! so operators can register custom fields and constrained value sets.
//! The trait/struct surface below is shaped to make that promotion
//! straightforward — no breaking change at the picker boundary.
//!
//! The framing locked in this session: certain fields shouldn't be
//! free-text. "Change engagement" pulls from a known-set lookup, not
//! an open string input. Status, priority, decay are constrained
//! enums. Tags are free-form (with autocomplete). Date pickers handle
//! due_date.

use std::collections::HashMap;

/// What input type a picker should render for this field.
///
/// `PartialEq` so Dioxus components carrying `FieldKind` props
/// short-circuit re-renders when the kind hasn't changed.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldKind {
    /// A fixed list of acceptable values. Status, priority, decay.
    Enum { values: Vec<String> },
    /// A value that must reference an existing entity. Board, column,
    /// engagement, design_node. The picker queries the lookup source
    /// at render time so the value set reflects what's currently in
    /// the project.
    Lookup {
        source: LookupSource,
        /// Field name on the looked-up entity to render in the picker
        /// list (e.g., `"name"` for boards). Empty string means "use
        /// the bare value."
        display: String,
    },
    /// Free-form string. Optional autocomplete pulls from prior
    /// values across the project (tag names, etc.).
    FreeText {
        autocomplete_from: Option<String>,
    },
    /// ISO date string. Picker uses native `<input type="date">` for
    /// v1; calendar widget can come later.
    Date,
}

/// Where a `Lookup` field's value set comes from.
///
/// `Columns(BoardId)` is dependent — the picker needs to know the
/// currently-selected board so it can show that board's columns. The
/// strip threads the active board id through when constructing the
/// descriptor for `column`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupSource {
    Boards,
    Columns(uuid::Uuid),
    Engagements,
    DesignNodes,
}

/// Per-field metadata: the kind, plus presentation hints. `PartialEq`
/// for the same component-prop reason as `FieldKind`.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDescriptor {
    /// The frontmatter key under `[data]`. Stable string — used as
    /// the dictionary key in `task_field_schemas` and as the argument
    /// to `Project::set_data_field`.
    pub key: String,
    /// Operator-facing label (used as the pill text when the value
    /// itself isn't surface-worthy, e.g. `"Due"` before the date).
    pub label: String,
    pub kind: FieldKind,
}

/// v1 hardcoded schemas for task fields.
///
/// Order is the rendering order in the strip: status leads (the
/// primary lifecycle pill), then priority, then organizational fields,
/// then optional fields, then tags. Design-node + engagement are at
/// the end because they're navigational rather than editorial.
pub fn task_field_schemas(active_board: Option<uuid::Uuid>) -> HashMap<&'static str, FieldDescriptor> {
    let mut schemas = HashMap::new();

    schemas.insert("status", FieldDescriptor {
        key: "status".into(),
        label: "Status".into(),
        kind: FieldKind::Enum {
            values: vec![
                "todo".into(),
                "in_progress".into(),
                "review".into(),
                "done".into(),
                "archived".into(),
            ],
        },
    });

    schemas.insert("priority", FieldDescriptor {
        key: "priority".into(),
        label: "Priority".into(),
        // Priority is stored as int (1-4) but the picker shows labels.
        // The picker's value-mapping layer translates "low" ↔ 1, etc.
        kind: FieldKind::Enum {
            values: vec![
                "low".into(),
                "medium".into(),
                "high".into(),
                "critical".into(),
            ],
        },
    });

    schemas.insert("column", FieldDescriptor {
        key: "column".into(),
        label: "Column".into(),
        kind: FieldKind::Lookup {
            // Falls back to the nil UUID when no board is active. The
            // picker handles this by showing an empty list — operator
            // sees "no columns yet" rather than a crash.
            source: LookupSource::Columns(active_board.unwrap_or(uuid::Uuid::nil())),
            // Columns are bare strings, not entities — the lookup
            // source synthesizes descriptors with `name == value`.
            display: String::new(),
        },
    });

    schemas.insert("board", FieldDescriptor {
        key: "board".into(),
        label: "Board".into(),
        kind: FieldKind::Lookup {
            source: LookupSource::Boards,
            display: "name".into(),
        },
    });

    schemas.insert("due_date", FieldDescriptor {
        key: "due_date".into(),
        label: "Due".into(),
        kind: FieldKind::Date,
    });

    schemas.insert("tags", FieldDescriptor {
        key: "tags".into(),
        label: "Tags".into(),
        kind: FieldKind::FreeText {
            autocomplete_from: Some("tags".into()),
        },
    });

    schemas.insert("engagement", FieldDescriptor {
        key: "engagement".into(),
        label: "Engagement".into(),
        kind: FieldKind::Lookup {
            source: LookupSource::Engagements,
            display: "name".into(),
        },
    });

    schemas.insert("decay", FieldDescriptor {
        key: "decay".into(),
        label: "Decay".into(),
        kind: FieldKind::Enum {
            values: vec![
                "none".into(),
                "slow".into(),
                "natural".into(),
                "fast".into(),
            ],
        },
    });

    schemas
}

/// Map a priority label ("low" / "medium" / "high" / "critical") to
/// the int the schema persists. Used by the priority picker; reverse
/// in `priority_label_for_int` for rendering the current value.
pub fn priority_int_for_label(label: &str) -> Option<i64> {
    match label {
        "low" => Some(1),
        "medium" => Some(2),
        "high" => Some(3),
        "critical" => Some(4),
        _ => None,
    }
}

pub fn priority_label_for_int(n: i64) -> &'static str {
    match n {
        1 => "low",
        3 => "high",
        4 => "critical",
        _ => "medium",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_schemas_cover_all_pillable_fields() {
        // Sanity check the schema list against the pills the strip
        // renders. If we add a new pill, add the schema entry first
        // — this test fires when the two drift.
        let schemas = task_field_schemas(None);
        for key in ["status", "priority", "column", "board", "due_date", "tags", "engagement", "decay"] {
            assert!(schemas.contains_key(key), "missing schema for {key}");
        }
    }

    #[test]
    fn priority_int_label_round_trips() {
        for label in ["low", "medium", "high", "critical"] {
            let n = priority_int_for_label(label).unwrap();
            assert_eq!(priority_label_for_int(n), label);
        }
    }

    #[test]
    fn priority_label_for_unknown_int_falls_back_to_medium() {
        // Defensive: a frontmatter that hand-edited an out-of-range
        // priority shouldn't crash the picker. Render as medium.
        assert_eq!(priority_label_for_int(0), "medium");
        assert_eq!(priority_label_for_int(99), "medium");
    }

    #[test]
    fn columns_lookup_uses_active_board() {
        // The column descriptor is parameterized on the active board
        // — picker renders that board's columns, not a static list.
        let board_id = uuid::Uuid::new_v4();
        let schemas = task_field_schemas(Some(board_id));
        let col = &schemas["column"];
        match &col.kind {
            FieldKind::Lookup { source: LookupSource::Columns(id), .. } => {
                assert_eq!(*id, board_id);
            }
            _ => panic!("column should be a Columns(_) lookup, got {:?}", col.kind),
        }
    }
}
