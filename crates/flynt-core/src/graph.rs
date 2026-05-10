use crate::{
    models::{MetadataValue},
    store::{TaskFilter, ProjectStore},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphNodeKind {
    Document,
    Task,
    Board,
    Repo,
    Link,
    MemoryFact,
    Communication,
    DesignNode,
    Scenario,
    WorkspaceLease,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphEdgeKind {
    Wikilink,
    TaskMembership,
    SemanticSupport,
    Dependency,
    Validates,
    ParentChild,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub kind: GraphNodeKind,
    pub title: String,
    pub group: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    /// Task-specific: priority (1=low, 2=medium, 3=high, 4=critical)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    /// Task-specific: status (todo, in_progress, done, archived)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub kind: GraphEdgeKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphPayload {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub groups: Vec<String>,
    pub all_tags: Vec<String>,
}

pub fn build_graph_payload(store: &dyn ProjectStore) -> Result<GraphPayload> {
    let docs = store.list_documents()?;
    let mut nodes = Vec::with_capacity(docs.len());
    let mut edges = Vec::new();
    let mut groups = HashMap::<String, ()>::new();
    let mut tag_set = HashMap::<String, ()>::new();

    for meta in docs {
        let id = meta.id.0.to_string();
        let group = top_level_group(&meta.path);
        groups.insert(group.clone(), ());
        let kind = match &meta.entity_kind {
            Some(crate::datum::EntityKind::Repo) => GraphNodeKind::Repo,
            Some(crate::datum::EntityKind::Link) => GraphNodeKind::Link,
            Some(crate::datum::EntityKind::Task) => GraphNodeKind::Task,
            Some(crate::datum::EntityKind::DesignNode) => GraphNodeKind::DesignNode,
            Some(crate::datum::EntityKind::OpenSpecScenario) => GraphNodeKind::Scenario,
            Some(crate::datum::EntityKind::WorkspaceLease) => GraphNodeKind::WorkspaceLease,
            _ if matches!(
                meta.metadata.get("kind").map(|field| &field.value),
                Some(MetadataValue::String(value)) if value == "agent_communication"
            ) => GraphNodeKind::Communication,
            _ if matches!(
                meta.metadata.get("kind").map(|field| &field.value),
                Some(MetadataValue::String(value)) if value == "memory_fact"
            ) => GraphNodeKind::MemoryFact,
            _ => GraphNodeKind::Document,
        };
        for tag in &meta.tags {
            tag_set.insert(tag.clone(), ());
        }
        // Extract status from the entity for design nodes
        let node_status = if kind == GraphNodeKind::DesignNode {
            meta.metadata
                .get("status")
                .and_then(|f| match &f.value {
                    MetadataValue::String(s) => Some(s.clone()),
                    _ => None,
                })
                .or_else(|| {
                    // Fall back to entity fields if available
                    store.get_document(&meta.id).ok().flatten()
                        .and_then(|doc| doc.entity.as_ref()
                            .and_then(|e| e.get_text("status").map(String::from)))
                })
        } else {
            None
        };

        nodes.push(GraphNode {
            id: id.clone(),
            kind: kind.clone(),
            title: meta.title.clone(),
            group,
            tags: meta.tags.clone(),
            updated_at: Some(meta.updated_at.to_rfc3339()),
            priority: None,
            status: node_status,
        });

        if let Some(doc) = store.get_document(&meta.id)? {
            for link in doc.outgoing_links {
                if let Some(target) = store.find_document_by_slug(&link.target)? {
                    edges.push(GraphEdge {
                        source: id.clone(),
                        target: target.id.0.to_string(),
                        kind: GraphEdgeKind::Wikilink,
                    });
                }
            }

            // Design node dependency + parent-child edges
            if kind == GraphNodeKind::DesignNode {
                if let Some(entity) = &doc.entity {
                    for dep_id in entity.get_text_list("dependencies") {
                        edges.push(GraphEdge {
                            source: id.clone(),
                            target: dep_id,
                            kind: GraphEdgeKind::Dependency,
                        });
                    }
                    if let Some(parent_id) = entity.get_text("parent") {
                        edges.push(GraphEdge {
                            source: parent_id.to_string(),
                            target: id.clone(),
                            kind: GraphEdgeKind::ParentChild,
                        });
                    }
                }
            }
        }
    }

    let boards = store.list_boards()?;
    for board in &boards {
        groups.insert("boards".into(), ());
        nodes.push(GraphNode {
            id: format!("board:{}", board.id.0),
            kind: GraphNodeKind::Board,
            title: board.name.clone(),
            group: "boards".into(),
            tags: vec![],
            updated_at: Some(board.created_at.to_rfc3339()),
            priority: None,
            status: None,
        });
    }

    let tasks = store.list_tasks(&TaskFilter::default())?;
    for task in tasks {
        let task_id = format!("task:{}", task.id.0);
        groups.insert(task.column.clone(), ());
        for tag in &task.tags {
            tag_set.insert(tag.clone(), ());
        }
        let priority_num = match task.priority {
            crate::models::Priority::Low => 1,
            crate::models::Priority::Medium => 2,
            crate::models::Priority::High => 3,
            crate::models::Priority::Critical => 4,
        };
        let status_str = match task.status {
            crate::models::TaskStatus::Todo => "todo",
            crate::models::TaskStatus::InProgress => "in_progress",
            crate::models::TaskStatus::Done => "done",
            crate::models::TaskStatus::Archived => "archived",
        };
        nodes.push(GraphNode {
            id: task_id.clone(),
            kind: GraphNodeKind::Task,
            title: task.title.clone(),
            group: task.column.clone(),
            tags: task.tags.clone(),
            updated_at: Some(task.updated_at.to_rfc3339()),
            priority: Some(priority_num),
            status: Some(status_str.into()),
        });
        edges.push(GraphEdge {
            source: format!("board:{}", task.board_id.0),
            target: task_id.clone(),
            kind: GraphEdgeKind::TaskMembership,
        });
        for doc_ref in task.document_refs {
            edges.push(GraphEdge {
                source: task_id.clone(),
                target: doc_ref.0.to_string(),
                kind: GraphEdgeKind::SemanticSupport,
            });
        }
    }

    let mut groups = groups.into_keys().collect::<Vec<_>>();
    groups.sort();
    let mut all_tags = tag_set.into_keys().collect::<Vec<_>>();
    all_tags.sort();

    Ok(GraphPayload { nodes, edges, groups, all_tags })
}

/// Public helper for matching node kind strings (used by MCP filter).
pub fn format_kind(kind: &GraphNodeKind) -> &'static str {
    match kind {
        GraphNodeKind::Document => "document",
        GraphNodeKind::Task => "task",
        GraphNodeKind::Board => "board",
        GraphNodeKind::Repo => "repo",
        GraphNodeKind::Link => "link",
        GraphNodeKind::MemoryFact => "memory",
        GraphNodeKind::Communication => "communication",
        GraphNodeKind::DesignNode => "design_node",
        GraphNodeKind::Scenario => "scenario",
        GraphNodeKind::WorkspaceLease => "workspace_lease",
    }
}

fn top_level_group(path: &Path) -> String {
    let components: Vec<_> = path.components().collect();
    if components.len() > 1 {
        // Has a parent folder — use the top-level folder name
        components[0].as_os_str().to_string_lossy().into_owned()
    } else {
        // Root-level file
        "root".into()
    }
}

// ── Force-directed layout + SVG rendering ────────────────────────────────────

/// Configuration for the force-directed layout.
pub struct LayoutConfig {
    pub width: f64,
    pub height: f64,
    pub iterations: usize,
    pub repel_strength: f64,
    pub link_distance: f64,
    pub link_strength: f64,
    pub center_gravity: f64,
    pub node_radius: f64,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            width: 900.0,
            height: 700.0,
            iterations: 120,
            repel_strength: 1200.0,
            link_distance: 60.0,
            link_strength: 0.03,
            center_gravity: 0.01,
            node_radius: 5.0,
        }
    }
}

/// Compute force-directed positions for graph nodes.
pub fn force_layout(payload: &GraphPayload, config: &LayoutConfig) -> Vec<(f64, f64)> {
    let n = payload.nodes.len();
    if n == 0 {
        return vec![];
    }

    let cx = config.width / 2.0;
    let cy = config.height / 2.0;
    let r = (config.width.min(config.height) / 2.5).min(250.0);

    // Circle initialization
    let mut pos: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let angle = 2.0 * std::f64::consts::PI * (i as f64) / (n as f64);
            (cx + r * angle.cos(), cy + r * angle.sin())
        })
        .collect();

    let id_to_idx: HashMap<&str, usize> = payload
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.id.as_str(), i))
        .collect();

    for iter in 0..config.iterations {
        let mut dx = vec![0.0f64; n];
        let mut dy = vec![0.0f64; n];
        let alpha = 1.0 - (iter as f64 / config.iterations as f64); // cooling

        // Repulsion
        for i in 0..n {
            for j in (i + 1)..n {
                let vx = pos[i].0 - pos[j].0;
                let vy = pos[i].1 - pos[j].1;
                let dist = (vx * vx + vy * vy).sqrt().max(1.0);
                let force = config.repel_strength / (dist * dist) * alpha;
                let fx = vx / dist * force;
                let fy = vy / dist * force;
                dx[i] += fx;
                dy[i] += fy;
                dx[j] -= fx;
                dy[j] -= fy;
            }
        }

        // Edge attraction
        for edge in &payload.edges {
            if let (Some(&si), Some(&ti)) = (
                id_to_idx.get(edge.source.as_str()),
                id_to_idx.get(edge.target.as_str()),
            ) {
                let vx = pos[ti].0 - pos[si].0;
                let vy = pos[ti].1 - pos[si].1;
                let dist = (vx * vx + vy * vy).sqrt().max(1.0);
                let force = (dist - config.link_distance) * config.link_strength * alpha;
                let fx = vx / dist * force;
                let fy = vy / dist * force;
                dx[si] += fx;
                dy[si] += fy;
                dx[ti] -= fx;
                dy[ti] -= fy;
            }
        }

        // Center gravity
        for i in 0..n {
            dx[i] += (cx - pos[i].0) * config.center_gravity;
            dy[i] += (cy - pos[i].1) * config.center_gravity;
        }

        // Apply
        let max_move = 15.0 * alpha + 1.0;
        for i in 0..n {
            pos[i].0 += dx[i].clamp(-max_move, max_move);
            pos[i].1 += dy[i].clamp(-max_move, max_move);
            pos[i].0 = pos[i].0.clamp(15.0, config.width - 15.0);
            pos[i].1 = pos[i].1.clamp(15.0, config.height - 15.0);
        }
    }

    pos
}

/// Node kind to CSS-friendly color string.
pub fn kind_color(kind: &GraphNodeKind) -> &'static str {
    match kind {
        GraphNodeKind::Document => "rgb(103,232,200)",
        GraphNodeKind::Task => "rgb(234,179,8)",
        GraphNodeKind::Board => "rgb(59,130,246)",
        GraphNodeKind::Repo => "rgb(139,92,246)",
        GraphNodeKind::Link => "rgb(113,113,122)",
        GraphNodeKind::MemoryFact => "rgb(249,115,22)",
        GraphNodeKind::Communication => "rgb(236,72,153)",
        GraphNodeKind::DesignNode => "rgb(16,185,129)",
        GraphNodeKind::Scenario => "rgb(34,197,94)",
        GraphNodeKind::WorkspaceLease => "rgb(168,85,247)",
    }
}

/// Status icon for design node lifecycle phases.
pub fn design_node_status_icon(status: &str) -> &'static str {
    match status {
        "seed" => "\u{25CC}",           // ◌  hollow circle
        "exploring" => "\u{25D0}",      // ◐  half-filled
        "resolved" => "\u{25C9}",       // ◉  mostly filled
        "decided" => "\u{25CF}",        // ●  filled
        "implementing" => "\u{2699}",   // ⚙  gear
        "implemented" => "\u{2713}",    // ✓  checkmark
        "blocked" => "\u{2715}",        // ✕  X
        "deferred" => "\u{25D1}",       // ◑  half-right
        "archived" => "\u{25CB}",       // ○  dimmed (open circle)
        _ => "\u{25CC}",               // ◌  default to seed
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Render a graph payload as an SVG string. Pure Rust, no JS.
pub fn render_graph_svg(payload: &GraphPayload, config: &LayoutConfig) -> String {
    let positions = force_layout(payload, config);
    let w = config.width;
    let h = config.height;
    let r = config.node_radius;

    let id_to_idx: HashMap<&str, usize> = payload
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.id.as_str(), i))
        .collect();

    let mut svg = format!(
        r#"<svg viewBox="0 0 {w} {h}" xmlns="http://www.w3.org/2000/svg" style="width:100%;height:100%;background:transparent">"#
    );

    // Edges
    for edge in &payload.edges {
        if let (Some(&si), Some(&ti)) = (
            id_to_idx.get(edge.source.as_str()),
            id_to_idx.get(edge.target.as_str()),
        ) {
            let (x1, y1) = positions[si];
            let (x2, y2) = positions[ti];
            let opacity = match edge.kind {
                GraphEdgeKind::Wikilink => "0.4",
                GraphEdgeKind::TaskMembership => "0.3",
                GraphEdgeKind::SemanticSupport => "0.2",
                GraphEdgeKind::Dependency => "0.5",
                GraphEdgeKind::ParentChild => "0.6",
                GraphEdgeKind::Validates => "0.5",
            };
            svg.push_str(&format!(
                r#"<line x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}" stroke="rgb(45,49,64)" stroke-width="0.8" opacity="{opacity}"/>"#
            ));
        }
    }

    // Nodes
    for (i, node) in payload.nodes.iter().enumerate() {
        let (x, y) = positions[i];
        let color = kind_color(&node.kind);
        // Design nodes: dim fill when archived, otherwise normal
        let fill_opacity = if node.kind == GraphNodeKind::DesignNode {
            match node.status.as_deref() {
                Some("archived") => "0.35",
                _ => "1",
            }
        } else {
            "1"
        };
        svg.push_str(&format!(
            r#"<circle cx="{x:.1}" cy="{y:.1}" r="{r}" fill="{color}" fill-opacity="{fill_opacity}" stroke="rgb(15,17,23)" stroke-width="1"/>"#
        ));
        // Design node status icon overlay
        if node.kind == GraphNodeKind::DesignNode {
            let status_str = node.status.as_deref().unwrap_or("seed");
            let icon = design_node_status_icon(status_str);
            svg.push_str(&format!(
                r#"<text x="{x:.1}" y="{:.1}" font-size="7" fill="rgb(15,17,23)" text-anchor="middle" font-family="system-ui,sans-serif">{icon}</text>"#,
                y + 2.5,
            ));
        }
        // Label
        let label = if node.title.len() > 20 {
            format!("{}…", &node.title[..18])
        } else {
            node.title.clone()
        };
        svg.push_str(&format!(
            r#"<text x="{:.1}" y="{:.1}" font-size="9" fill="rgb(113,113,122)" font-family="system-ui,sans-serif">{}</text>"#,
            x + r + 3.0,
            y + 3.0,
            html_escape(&label)
        ));
    }

    svg.push_str("</svg>");
    svg
}

#[cfg(test)]
mod tests {
    use super::{build_graph_payload, GraphEdgeKind, GraphNodeKind};
    use crate::{
        models::{Board, BoardId, Document, DocumentId, DocumentMeta, Frontmatter, MetadataField, MetadataProtection, MetadataValue, SearchResult, Task, TaskId, WikiLink},
        store::{DocumentMetadataFilter, TaskFilter, ProjectStore},
    };
    use anyhow::Result;
    use chrono::Utc;
    use std::{collections::HashMap, path::{Path, PathBuf}};

    struct StubStore {
        docs: Vec<DocumentMeta>,
        full_docs: HashMap<String, Document>,
        boards: Vec<Board>,
        tasks: Vec<Task>,
    }

    impl ProjectStore for StubStore {
        fn get_document(&self, id: &DocumentId) -> Result<Option<Document>> {
            Ok(self.full_docs.get(&id.0.to_string()).cloned())
        }
        fn get_document_by_path(&self, _path: &Path) -> Result<Option<Document>> { Ok(None) }
        fn find_document_by_slug(&self, slug: &str) -> Result<Option<DocumentMeta>> {
            Ok(self.docs.iter().find(|doc| doc.title.eq_ignore_ascii_case(slug) || doc.path.to_string_lossy().contains(slug)).cloned())
        }
        fn list_documents(&self) -> Result<Vec<DocumentMeta>> { Ok(self.docs.clone()) }
        fn list_documents_by_metadata(&self, _filter: &DocumentMetadataFilter) -> Result<Vec<DocumentMeta>> { Ok(vec![]) }
        fn save_document(&self, _doc: &Document) -> Result<()> { Ok(()) }
        fn delete_document(&self, _id: &DocumentId) -> Result<()> { Ok(()) }
        fn search_documents(&self, _query: &str) -> Result<Vec<SearchResult>> { Ok(vec![]) }
        fn get_backlinks(&self, _id: &DocumentId) -> Result<Vec<DocumentMeta>> { Ok(vec![]) }
        fn list_entities_by_kind(&self, kind: &crate::datum::EntityKind) -> Result<Vec<DocumentMeta>> {
            Ok(self.docs.iter().filter(|d| d.entity_kind.as_ref() == Some(kind)).cloned().collect())
        }
        fn get_task(&self, _id: &TaskId) -> Result<Option<Task>> { Ok(None) }
        fn list_tasks(&self, _filter: &TaskFilter) -> Result<Vec<Task>> { Ok(self.tasks.clone()) }
        fn save_task(&self, _task: &Task) -> Result<()> { Ok(()) }
        fn update_task(&self, _id: &TaskId, _patch: &flynt_models::TaskPatch) -> Result<bool> { Ok(true) }
        fn delete_task(&self, _id: &TaskId) -> Result<()> { Ok(()) }
        fn get_board(&self, _id: &BoardId) -> Result<Option<Board>> { Ok(None) }
        fn list_boards(&self) -> Result<Vec<Board>> { Ok(self.boards.clone()) }
        fn save_board(&self, _board: &Board) -> Result<()> { Ok(()) }
        fn delete_board(&self, _id: &BoardId) -> Result<()> { Ok(()) }
        fn get_engagement(
            &self,
            _id: &flynt_models::engagement::EngagementId,
        ) -> Result<Option<flynt_models::engagement::Engagement>> { Ok(None) }
        fn list_engagements(&self) -> Result<Vec<flynt_models::engagement::Engagement>> { Ok(vec![]) }
        fn save_engagement(&self, _e: &flynt_models::engagement::Engagement) -> Result<()> { Ok(()) }
        fn delete_engagement(
            &self,
            _id: &flynt_models::engagement::EngagementId,
        ) -> Result<bool> { Ok(true) }
        fn list_dirty_tasks(&self, _project_id: &uuid::Uuid) -> Result<Vec<Task>> { Ok(vec![]) }
        fn list_dirty_documents(&self, _project_id: &uuid::Uuid) -> Result<Vec<Document>> { Ok(vec![]) }
        fn mark_committed(&self, _task_ids: &[TaskId], _doc_ids: &[DocumentId], _at: chrono::DateTime<chrono::Utc>) -> Result<()> { Ok(()) }
        fn record_project_deletion(&self, _entity_id: &uuid::Uuid, _entity_kind: &str, _project_id: &uuid::Uuid) -> Result<()> { Ok(()) }
        fn list_pending_deletions(&self, _project_id: &uuid::Uuid) -> Result<Vec<(uuid::Uuid, String)>> { Ok(vec![]) }
        fn mark_deletions_committed(&self, _entity_ids: &[uuid::Uuid]) -> Result<()> { Ok(()) }
    }

    #[test]
    fn builds_document_nodes_and_wikilink_edges() {
        let now = Utc::now();
        let a = DocumentId::new();
        let b = DocumentId::new();
        let board_id = BoardId::new();
        let task_id = TaskId::new();
        let docs = vec![
            DocumentMeta { id: a.clone(), path: PathBuf::from("design/alpha.md"), title: "alpha".into(), tags: vec![], metadata: Default::default(), entity_kind: None, updated_at: now },
            DocumentMeta { id: b.clone(), path: PathBuf::from("design/beta.md"), title: "beta".into(), tags: vec![], metadata: Default::default(), entity_kind: None, updated_at: now },
            DocumentMeta {
                id: DocumentId::new(),
                path: PathBuf::from("references/comms/vox/standup.md"),
                title: "Standup Recall".into(),
                tags: vec![],
                metadata: std::collections::BTreeMap::from([(
                    "kind".into(),
                    MetadataField {
                        value: MetadataValue::String("agent_communication".into()),
                        protection: MetadataProtection::PlaintextIndexed,
                    },
                )]),
                entity_kind: None,
                updated_at: now,
            },
            DocumentMeta {
                id: DocumentId::new(),
                path: PathBuf::from("ai/memory/storage/canonical-vs-local.md"),
                title: "Canonical vs Local".into(),
                tags: vec![],
                metadata: std::collections::BTreeMap::from([(
                    "kind".into(),
                    MetadataField {
                        value: MetadataValue::String("memory_fact".into()),
                        protection: MetadataProtection::PlaintextIndexed,
                    },
                )]),
                entity_kind: None,
                updated_at: now,
            },
        ];
        let full_docs = HashMap::from([
            (a.0.to_string(), Document { id: a.clone(), path: PathBuf::from("design/alpha.md"), title: "alpha".into(), content: String::new(), frontmatter: Frontmatter::default(), outgoing_links: vec![WikiLink { target: "beta".into(), display: None, anchor: None }], created_at: now, updated_at: now, entity: None }),
            (b.0.to_string(), Document { id: b.clone(), path: PathBuf::from("design/beta.md"), title: "beta".into(), content: String::new(), frontmatter: Frontmatter::default(), outgoing_links: vec![], created_at: now, updated_at: now, entity: None }),
        ]);
        let store = StubStore {
            docs,
            full_docs,
            boards: vec![Board::default_sprint("Sprint")],
            tasks: vec![Task {
                id: task_id,
                board_id,
                column: "Backlog".into(),
                title: "Task one".into(),
                description: String::new(),
                priority: Default::default(),
                status: Default::default(),
                tags: vec![],
                document_refs: vec![a.clone()],
                external_refs: vec![],
                due_date: None,
                position: 0,
                created_at: now,
                updated_at: now,
                decay: Default::default(),
                last_touched_at: Some(now),
                design_node_id: None,
                openspec_change: None,
                engagement_id: None,
                execution: None,
            }],
        };

        let graph = build_graph_payload(&store).unwrap();
        assert!(graph.nodes.iter().any(|node| node.kind == GraphNodeKind::Document));
        assert!(graph.nodes.iter().any(|node| node.kind == GraphNodeKind::Communication));
        assert!(graph.nodes.iter().any(|node| node.kind == GraphNodeKind::MemoryFact));
        assert!(graph.nodes.iter().any(|node| node.kind == GraphNodeKind::Board));
        assert!(graph.nodes.iter().any(|node| node.kind == GraphNodeKind::Task));
        assert!(graph.edges.iter().any(|edge| edge.kind == GraphEdgeKind::Wikilink));
        assert!(graph.edges.iter().any(|edge| edge.kind == GraphEdgeKind::TaskMembership));
        assert!(graph.edges.iter().any(|edge| edge.kind == GraphEdgeKind::SemanticSupport));
        assert!(graph.groups.contains(&"design".into()));
        assert!(graph.groups.contains(&"boards".into()));
        assert!(graph.groups.contains(&"Backlog".into()));
    }
}
