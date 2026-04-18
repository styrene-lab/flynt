//! Datum — the atomic typed value in Codex.
//!
//! Everything in the entity system reduces to Datums. A field is a named Datum.
//! An entity is an identified collection of fields. A document is an entity
//! with a markdown body and a file path. A project is a document whose `kind`
//! is "project" and whose fields satisfy the project schema.
//!
//! ```text
//! Datum          — single typed value (this module)
//!   ↓
//! Field          — named Datum with optional constraints
//!   ↓
//! Entity         — identified collection of Fields with a `kind`
//!   ↓
//! Document       — Entity + markdown body + file path
//!   ↓
//! Task, Project  — Documents with known field contracts
//! ```

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

// ── Datum ────────────────────────────────────────────────────────────────────

/// Atomic typed value — the irreducible unit of structured data in Codex.
///
/// Datums are recursive: `List` and `Map` contain other Datums, enabling
/// arbitrary nesting. The type set is deliberately small — it covers what
/// TOML frontmatter can express plus a `Ref` variant for entity relationships.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Datum {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Date(NaiveDate),
    Timestamp(DateTime<Utc>),
    Ref(DatumRef),
    List(Vec<Datum>),
    Map(BTreeMap<String, Datum>),
}

/// A typed reference to another entity by UUID.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DatumRef {
    pub id: Uuid,
    /// Optional kind hint for the target entity (e.g. "project", "task").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

impl Datum {
    pub fn is_null(&self) -> bool { matches!(self, Self::Null) }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Datum]> {
        match self {
            Self::List(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&BTreeMap<String, Datum>> {
        match self {
            Self::Map(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_ref(&self) -> Option<&DatumRef> {
        match self {
            Self::Ref(r) => Some(r),
            _ => None,
        }
    }

    /// Coerce a text Datum that looks like a UUID into a `Ref`.
    /// Returns the original Datum unchanged if it's not a UUID-shaped string.
    pub fn try_as_ref(&self) -> Option<DatumRef> {
        match self {
            Self::Text(s) => Uuid::parse_str(s).ok().map(|id| DatumRef { id, kind: None }),
            Self::Ref(r) => Some(r.clone()),
            _ => None,
        }
    }
}

impl Default for Datum {
    fn default() -> Self { Self::Null }
}

// ── Conversion from TOML values ──────────────────────────────────────────────

impl From<toml::Value> for Datum {
    fn from(v: toml::Value) -> Self {
        match v {
            toml::Value::String(s) => {
                // Try parsing as date, then datetime, then plain text
                if let Ok(d) = s.parse::<NaiveDate>() {
                    Self::Date(d)
                } else if let Ok(dt) = s.parse::<DateTime<Utc>>() {
                    Self::Timestamp(dt)
                } else {
                    Self::Text(s)
                }
            }
            toml::Value::Integer(n) => Self::Int(n),
            toml::Value::Float(f) => Self::Float(f),
            toml::Value::Boolean(b) => Self::Bool(b),
            toml::Value::Datetime(dt) => {
                let s = dt.to_string();
                if let Ok(d) = s.parse::<NaiveDate>() {
                    Self::Date(d)
                } else if let Ok(ts) = s.parse::<DateTime<Utc>>() {
                    Self::Timestamp(ts)
                } else {
                    Self::Text(s)
                }
            }
            toml::Value::Array(arr) => {
                Self::List(arr.into_iter().map(Datum::from).collect())
            }
            toml::Value::Table(tbl) => {
                Self::Map(tbl.into_iter().map(|(k, v)| (k, Datum::from(v))).collect())
            }
        }
    }
}

impl From<Datum> for toml::Value {
    fn from(d: Datum) -> Self {
        match d {
            Datum::Null => toml::Value::String(String::new()),
            Datum::Bool(b) => toml::Value::Boolean(b),
            Datum::Int(n) => toml::Value::Integer(n),
            Datum::Float(f) => toml::Value::Float(f),
            Datum::Text(s) => toml::Value::String(s),
            Datum::Date(d) => toml::Value::String(d.to_string()),
            Datum::Timestamp(ts) => toml::Value::String(ts.to_rfc3339()),
            Datum::Ref(r) => toml::Value::String(r.id.to_string()),
            Datum::List(v) => {
                toml::Value::Array(v.into_iter().map(toml::Value::from).collect())
            }
            Datum::Map(m) => {
                let tbl: toml::map::Map<String, toml::Value> =
                    m.into_iter().map(|(k, v)| (k, toml::Value::from(v))).collect();
                toml::Value::Table(tbl)
            }
        }
    }
}

/// Convert a serde_json::Value to Datum (for SQLite JSON round-tripping).
impl From<serde_json::Value> for Datum {
    fn from(v: serde_json::Value) -> Self {
        match v {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(b) => Self::Bool(b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Self::Int(i)
                } else {
                    Self::Float(n.as_f64().unwrap_or(0.0))
                }
            }
            serde_json::Value::String(s) => Self::Text(s),
            serde_json::Value::Array(arr) => {
                Self::List(arr.into_iter().map(Datum::from).collect())
            }
            serde_json::Value::Object(obj) => {
                Self::Map(obj.into_iter().map(|(k, v)| (k, Datum::from(v))).collect())
            }
        }
    }
}

impl From<Datum> for serde_json::Value {
    fn from(d: Datum) -> Self {
        match d {
            Datum::Null => serde_json::Value::Null,
            Datum::Bool(b) => serde_json::Value::Bool(b),
            Datum::Int(n) => serde_json::Value::Number(n.into()),
            Datum::Float(f) => {
                serde_json::Number::from_f64(f)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            }
            Datum::Text(s) => serde_json::Value::String(s),
            Datum::Date(d) => serde_json::Value::String(d.to_string()),
            Datum::Timestamp(ts) => serde_json::Value::String(ts.to_rfc3339()),
            Datum::Ref(r) => serde_json::Value::String(r.id.to_string()),
            Datum::List(v) => {
                serde_json::Value::Array(v.into_iter().map(serde_json::Value::from).collect())
            }
            Datum::Map(m) => {
                let obj: serde_json::Map<String, serde_json::Value> =
                    m.into_iter().map(|(k, v)| (k, serde_json::Value::from(v))).collect();
                serde_json::Value::Object(obj)
            }
        }
    }
}

// ── Entity ───────────────────────────────────────────────────────────────────

/// An identified collection of typed fields with a kind discriminator.
///
/// Every typed thing in Codex — documents, tasks, projects, contacts — is an
/// Entity under the hood. The `kind` field determines which schema applies.
/// Fields are stored as a flat `BTreeMap<String, Datum>`, not as a fixed struct,
/// so entities are schema-flexible by default and schema-validated when a Pkl
/// definition is available.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub id: Uuid,
    pub kind: EntityKind,
    pub fields: BTreeMap<String, Datum>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The kind of entity. Known kinds get first-class treatment in the UI;
/// `Custom` kinds are schema-driven and render generically.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Document,
    Project,
    Task,
    /// A git repository — local, remote, or both.
    Repo,
    /// An external resource reference (URL, dashboard, API, etc.)
    Link,
    /// User-defined entity type (e.g. "contact", "sprint", "milestone").
    #[serde(untagged)]
    Custom(String),
}

impl EntityKind {
    pub fn from_str(s: &str) -> Self {
        match s {
            "document" => Self::Document,
            "project" => Self::Project,
            "task" => Self::Task,
            "repo" => Self::Repo,
            "link" => Self::Link,
            other => Self::Custom(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Document => "document",
            Self::Project => "project",
            Self::Task => "task",
            Self::Repo => "repo",
            Self::Link => "link",
            Self::Custom(s) => s,
        }
    }

    /// Whether this kind represents a first-class entity with dedicated UI.
    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Custom(_))
    }
}

impl Default for EntityKind {
    fn default() -> Self { Self::Document }
}

impl Entity {
    pub fn new(kind: EntityKind) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            kind,
            fields: BTreeMap::new(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_field(mut self, key: impl Into<String>, value: Datum) -> Self {
        self.fields.insert(key.into(), value);
        self
    }

    pub fn get(&self, key: &str) -> Option<&Datum> {
        self.fields.get(key)
    }

    pub fn get_text(&self, key: &str) -> Option<&str> {
        self.fields.get(key).and_then(|d| d.as_text())
    }

    pub fn get_int(&self, key: &str) -> Option<i64> {
        self.fields.get(key).and_then(|d| d.as_int())
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.fields.get(key).and_then(|d| d.as_bool())
    }

    pub fn get_ref(&self, key: &str) -> Option<DatumRef> {
        self.fields.get(key).and_then(|d| d.try_as_ref())
    }

    /// Get a list of text values from a field (common for tags, columns, etc.)
    pub fn get_text_list(&self, key: &str) -> Vec<String> {
        self.fields
            .get(key)
            .and_then(|d| d.as_list())
            .map(|list| {
                list.iter()
                    .filter_map(|d| d.as_text().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn set(&mut self, key: impl Into<String>, value: Datum) {
        self.fields.insert(key.into(), value);
        self.updated_at = Utc::now();
    }
}

// ── Conversion: TOML frontmatter ↔ Entity ────────────────────────────────────

impl Entity {
    /// Build an Entity from parsed TOML frontmatter.
    ///
    /// Expects the frontmatter to contain `id` (UUID) and `kind` (string).
    /// Everything under the `[data]` table becomes the entity's fields.
    /// Top-level keys outside `[data]` are also preserved as fields with a
    /// `_fm_` prefix to avoid collisions with user-defined field names.
    pub fn from_frontmatter(fm: &toml::Value) -> Option<Self> {
        let table = fm.as_table()?;

        let kind_str = table.get("kind")?.as_str()?;
        let kind = EntityKind::from_str(kind_str);

        let id = table
            .get("id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
            .unwrap_or_else(Uuid::new_v4);

        let created_at = table
            .get("created_at")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
            .unwrap_or_else(Utc::now);

        let updated_at = table
            .get("updated_at")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
            .unwrap_or_else(Utc::now);

        // Entity fields come from the [data] table
        let mut fields = BTreeMap::new();
        if let Some(data) = table.get("data").and_then(|v| v.as_table()) {
            for (k, v) in data {
                fields.insert(k.clone(), Datum::from(v.clone()));
            }
        }

        Some(Self {
            id,
            kind,
            fields,
            created_at,
            updated_at,
        })
    }

    /// Serialize entity fields back to a TOML [data] table.
    /// The identity envelope (id, kind, timestamps) is separate.
    pub fn to_frontmatter_table(&self) -> toml::Value {
        let mut table = toml::map::Map::new();
        table.insert("id".into(), toml::Value::String(self.id.to_string()));
        table.insert("kind".into(), toml::Value::String(self.kind.as_str().to_string()));

        if !self.fields.is_empty() {
            let data: toml::map::Map<String, toml::Value> = self
                .fields
                .iter()
                .map(|(k, v)| (k.clone(), toml::Value::from(v.clone())))
                .collect();
            table.insert("data".into(), toml::Value::Table(data));
        }

        toml::Value::Table(table)
    }
}

// ── Typed projections ────────────────────────────────────────────────────────
//
// These structs are views over Entity fields for known kinds. They don't own
// the data — they're constructed on demand from an Entity and provide typed
// accessors. The Entity remains the canonical storage form.

/// A git repository — local checkout, remote, or both.
///
/// On disk as a markdown file with `kind = "repo"`:
/// ```toml
/// +++
/// id = "uuid"
/// kind = "repo"
///
/// [data]
/// name = "codex"
/// url = "https://github.com/black-meridian/codex"
/// provider = "github"
/// org = "black-meridian"
/// default_branch = "main"
/// local_path = "/Users/cwilson/workspace/black-meridian/codex"
/// +++
/// ```
#[derive(Debug, Clone)]
pub struct RepoView<'a> {
    pub entity: &'a Entity,
}

impl<'a> RepoView<'a> {
    pub fn from_entity(entity: &'a Entity) -> Option<Self> {
        if entity.kind == EntityKind::Repo {
            Some(Self { entity })
        } else {
            None
        }
    }

    pub fn name(&self) -> &str {
        self.entity.get_text("name").unwrap_or("unnamed")
    }

    pub fn url(&self) -> Option<&str> {
        self.entity.get_text("url")
    }

    pub fn provider(&self) -> Option<&str> {
        self.entity.get_text("provider")
    }

    pub fn org(&self) -> Option<&str> {
        self.entity.get_text("org")
    }

    pub fn default_branch(&self) -> &str {
        self.entity.get_text("default_branch").unwrap_or("main")
    }

    pub fn local_path(&self) -> Option<&str> {
        self.entity.get_text("local_path")
    }

    pub fn description(&self) -> Option<&str> {
        self.entity.get_text("description")
    }

    /// Project IDs that reference this repo.
    pub fn project_refs(&self) -> Vec<DatumRef> {
        self.entity
            .fields
            .get("projects")
            .and_then(|d| d.as_list())
            .map(|list| list.iter().filter_map(|d| d.try_as_ref()).collect())
            .unwrap_or_default()
    }
}

/// An external resource link — URL, dashboard, API endpoint, etc.
///
/// On disk as a markdown file with `kind = "link"`:
/// ```toml
/// +++
/// id = "uuid"
/// kind = "link"
///
/// [data]
/// title = "Grafana API Latency"
/// url = "https://grafana.internal/d/api-latency"
/// link_type = "dashboard"
/// +++
/// ```
#[derive(Debug, Clone)]
pub struct LinkView<'a> {
    pub entity: &'a Entity,
}

impl<'a> LinkView<'a> {
    pub fn from_entity(entity: &'a Entity) -> Option<Self> {
        if entity.kind == EntityKind::Link {
            Some(Self { entity })
        } else {
            None
        }
    }

    pub fn title(&self) -> &str {
        self.entity.get_text("title").unwrap_or("Untitled")
    }

    pub fn url(&self) -> Option<&str> {
        self.entity.get_text("url")
    }

    pub fn link_type(&self) -> Option<&str> {
        self.entity.get_text("link_type")
    }

    pub fn tags(&self) -> Vec<String> {
        self.entity.get_text_list("tags")
    }
}

/// A project view — typed accessors over project entity fields.
#[derive(Debug, Clone)]
pub struct ProjectView<'a> {
    pub entity: &'a Entity,
}

impl<'a> ProjectView<'a> {
    pub fn from_entity(entity: &'a Entity) -> Option<Self> {
        if entity.kind == EntityKind::Project {
            Some(Self { entity })
        } else {
            None
        }
    }

    pub fn title(&self) -> &str {
        self.entity.get_text("title").unwrap_or("Untitled")
    }

    pub fn status(&self) -> &str {
        self.entity.get_text("status").unwrap_or("active")
    }

    pub fn columns(&self) -> Vec<String> {
        let cols = self.entity.get_text_list("columns");
        if cols.is_empty() {
            vec!["Backlog".into(), "In Progress".into(), "Review".into(), "Done".into()]
        } else {
            cols
        }
    }

    pub fn owner(&self) -> Option<&str> {
        self.entity.get_text("owner")
    }

    /// Repo IDs associated with this project.
    pub fn repo_refs(&self) -> Vec<DatumRef> {
        self.entity
            .fields
            .get("repos")
            .and_then(|d| d.as_list())
            .map(|list| list.iter().filter_map(|d| d.try_as_ref()).collect())
            .unwrap_or_default()
    }
}

/// A task view — typed accessors over task entity fields.
#[derive(Debug, Clone)]
pub struct TaskView<'a> {
    pub entity: &'a Entity,
}

impl<'a> TaskView<'a> {
    pub fn from_entity(entity: &'a Entity) -> Option<Self> {
        if entity.kind == EntityKind::Task {
            Some(Self { entity })
        } else {
            None
        }
    }

    pub fn title(&self) -> &str {
        self.entity.get_text("title").unwrap_or("Untitled")
    }

    pub fn project_ref(&self) -> Option<DatumRef> {
        self.entity.get_ref("project")
    }

    pub fn column(&self) -> &str {
        self.entity.get_text("column").unwrap_or("Backlog")
    }

    pub fn priority(&self) -> i64 {
        self.entity.get_int("priority").unwrap_or(2)
    }

    pub fn status(&self) -> &str {
        self.entity.get_text("status").unwrap_or("open")
    }

    pub fn assignee(&self) -> Option<&str> {
        self.entity.get_text("assignee")
    }

    pub fn document_refs(&self) -> Vec<DatumRef> {
        self.entity
            .fields
            .get("document_refs")
            .and_then(|d| d.as_list())
            .map(|list| list.iter().filter_map(|d| d.try_as_ref()).collect())
            .unwrap_or_default()
    }

    pub fn tags(&self) -> Vec<String> {
        self.entity.get_text_list("tags")
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datum_from_toml_value() {
        let v = toml::Value::String("hello".into());
        assert_eq!(Datum::from(v), Datum::Text("hello".into()));

        let v = toml::Value::Integer(42);
        assert_eq!(Datum::from(v), Datum::Int(42));

        let v = toml::Value::Boolean(true);
        assert_eq!(Datum::from(v), Datum::Bool(true));

        let v = toml::Value::Array(vec![
            toml::Value::String("a".into()),
            toml::Value::String("b".into()),
        ]);
        assert_eq!(
            Datum::from(v),
            Datum::List(vec![Datum::Text("a".into()), Datum::Text("b".into())])
        );
    }

    #[test]
    fn datum_roundtrip_through_json() {
        let original = Datum::Map(BTreeMap::from([
            ("name".into(), Datum::Text("test".into())),
            ("count".into(), Datum::Int(7)),
            ("tags".into(), Datum::List(vec![
                Datum::Text("a".into()),
                Datum::Text("b".into()),
            ])),
        ]));
        let json = serde_json::Value::from(original.clone());
        let back = Datum::from(json);
        assert_eq!(original, back);
    }

    #[test]
    fn entity_from_frontmatter() {
        let toml_str = r#"
            id = "550e8400-e29b-41d4-a716-446655440000"
            kind = "project"

            [data]
            title = "Styrene Mesh"
            status = "active"
            columns = ["Backlog", "In Progress", "Review", "Done"]
            priority = 3
        "#;
        let val: toml::Value = toml::from_str(toml_str).unwrap();
        let entity = Entity::from_frontmatter(&val).unwrap();

        assert_eq!(entity.kind, EntityKind::Project);
        assert_eq!(entity.get_text("title"), Some("Styrene Mesh"));
        assert_eq!(entity.get_text("status"), Some("active"));
        assert_eq!(entity.get_int("priority"), Some(3));
        assert_eq!(
            entity.get_text_list("columns"),
            vec!["Backlog", "In Progress", "Review", "Done"]
        );
    }

    #[test]
    fn entity_roundtrip_through_toml() {
        let entity = Entity::new(EntityKind::Task)
            .with_field("title", Datum::Text("Fix bug".into()))
            .with_field("priority", Datum::Int(2))
            .with_field("tags", Datum::List(vec![
                Datum::Text("bug".into()),
                Datum::Text("urgent".into()),
            ]));

        let toml_val = entity.to_frontmatter_table();
        let table = toml_val.as_table().unwrap();

        assert_eq!(table.get("kind").unwrap().as_str().unwrap(), "task");
        assert!(table.get("id").unwrap().as_str().unwrap().len() > 0);

        let data = table.get("data").unwrap().as_table().unwrap();
        assert_eq!(data.get("title").unwrap().as_str().unwrap(), "Fix bug");
        assert_eq!(data.get("priority").unwrap().as_integer().unwrap(), 2);
    }

    #[test]
    fn entity_builder_pattern() {
        let project = Entity::new(EntityKind::Project)
            .with_field("title", Datum::Text("Alpha".into()))
            .with_field("owner", Datum::Text("cwilson".into()))
            .with_field("columns", Datum::List(vec![
                Datum::Text("Backlog".into()),
                Datum::Text("Done".into()),
            ]));

        assert_eq!(project.kind, EntityKind::Project);
        assert_eq!(project.get_text("title"), Some("Alpha"));
        assert_eq!(project.get_text("owner"), Some("cwilson"));
        assert_eq!(project.get_text_list("columns"), vec!["Backlog", "Done"]);
    }

    #[test]
    fn datum_try_as_ref() {
        let uuid = Uuid::new_v4();
        let text = Datum::Text(uuid.to_string());
        let r = text.try_as_ref().unwrap();
        assert_eq!(r.id, uuid);
        assert_eq!(r.kind, None);

        let not_uuid = Datum::Text("hello".into());
        assert!(not_uuid.try_as_ref().is_none());

        let int = Datum::Int(42);
        assert!(int.try_as_ref().is_none());
    }

    #[test]
    fn entity_kind_parsing() {
        assert_eq!(EntityKind::from_str("project"), EntityKind::Project);
        assert_eq!(EntityKind::from_str("task"), EntityKind::Task);
        assert_eq!(EntityKind::from_str("document"), EntityKind::Document);
        assert_eq!(EntityKind::from_str("repo"), EntityKind::Repo);
        assert_eq!(EntityKind::from_str("link"), EntityKind::Link);
        assert_eq!(EntityKind::from_str("contact"), EntityKind::Custom("contact".into()));
        assert!(EntityKind::Project.is_known());
        assert!(EntityKind::Repo.is_known());
        assert!(!EntityKind::Custom("sprint".into()).is_known());
    }

    #[test]
    fn repo_view_from_entity() {
        let repo = Entity::new(EntityKind::Repo)
            .with_field("name", Datum::Text("codex".into()))
            .with_field("url", Datum::Text("https://github.com/black-meridian/codex".into()))
            .with_field("provider", Datum::Text("github".into()))
            .with_field("org", Datum::Text("black-meridian".into()))
            .with_field("default_branch", Datum::Text("main".into()))
            .with_field("local_path", Datum::Text("/workspace/codex".into()));

        let view = RepoView::from_entity(&repo).unwrap();
        assert_eq!(view.name(), "codex");
        assert_eq!(view.url(), Some("https://github.com/black-meridian/codex"));
        assert_eq!(view.provider(), Some("github"));
        assert_eq!(view.org(), Some("black-meridian"));
        assert_eq!(view.default_branch(), "main");
        assert_eq!(view.local_path(), Some("/workspace/codex"));

        // Can't create RepoView from a non-repo entity
        let project = Entity::new(EntityKind::Project);
        assert!(RepoView::from_entity(&project).is_none());
    }

    #[test]
    fn link_view_from_entity() {
        let link = Entity::new(EntityKind::Link)
            .with_field("title", Datum::Text("Grafana Dashboard".into()))
            .with_field("url", Datum::Text("https://grafana.internal/d/latency".into()))
            .with_field("link_type", Datum::Text("dashboard".into()))
            .with_field("tags", Datum::List(vec![
                Datum::Text("monitoring".into()),
                Datum::Text("oncall".into()),
            ]));

        let view = LinkView::from_entity(&link).unwrap();
        assert_eq!(view.title(), "Grafana Dashboard");
        assert_eq!(view.url(), Some("https://grafana.internal/d/latency"));
        assert_eq!(view.link_type(), Some("dashboard"));
        assert_eq!(view.tags(), vec!["monitoring", "oncall"]);
    }

    #[test]
    fn project_view_with_repo_refs() {
        let repo_id = Uuid::new_v4();
        let project = Entity::new(EntityKind::Project)
            .with_field("title", Datum::Text("Styrene Mesh".into()))
            .with_field("status", Datum::Text("active".into()))
            .with_field("columns", Datum::List(vec![
                Datum::Text("Backlog".into()),
                Datum::Text("Done".into()),
            ]))
            .with_field("repos", Datum::List(vec![
                Datum::Text(repo_id.to_string()),
            ]));

        let view = ProjectView::from_entity(&project).unwrap();
        assert_eq!(view.title(), "Styrene Mesh");
        assert_eq!(view.columns(), vec!["Backlog", "Done"]);
        assert_eq!(view.repo_refs().len(), 1);
        assert_eq!(view.repo_refs()[0].id, repo_id);
    }

    #[test]
    fn repo_from_frontmatter() {
        let toml_str = r#"
            id = "550e8400-e29b-41d4-a716-446655440000"
            kind = "repo"

            [data]
            name = "prefon-vie"
            url = "https://github.com/black-meridian/prefon-vie"
            provider = "github"
            org = "black-meridian"
            default_branch = "main"
        "#;
        let val: toml::Value = toml::from_str(toml_str).unwrap();
        let entity = Entity::from_frontmatter(&val).unwrap();

        assert_eq!(entity.kind, EntityKind::Repo);
        let view = RepoView::from_entity(&entity).unwrap();
        assert_eq!(view.name(), "prefon-vie");
        assert_eq!(view.provider(), Some("github"));
        assert_eq!(view.org(), Some("black-meridian"));
    }
}
