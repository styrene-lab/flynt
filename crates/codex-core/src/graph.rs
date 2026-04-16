use crate::{
    models::{MetadataValue},
    store::{TaskFilter, VaultStore},
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
    MemoryFact,
    Communication,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphEdgeKind {
    Wikilink,
    TaskMembership,
    SemanticSupport,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub kind: GraphNodeKind,
    pub title: String,
    pub group: String,
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
}

pub fn build_graph_payload(store: &dyn VaultStore) -> Result<GraphPayload> {
    let docs = store.list_documents()?;
    let mut nodes = Vec::with_capacity(docs.len());
    let mut edges = Vec::new();
    let mut groups = HashMap::<String, ()>::new();

    for meta in docs {
        let id = meta.id.0.to_string();
        let group = top_level_group(&meta.path);
        groups.insert(group.clone(), ());
        let kind = if matches!(
            meta.metadata.get("kind").map(|field| &field.value),
            Some(MetadataValue::String(value)) if value == "agent_communication"
        ) {
            GraphNodeKind::Communication
        } else {
            GraphNodeKind::Document
        };
        nodes.push(GraphNode {
            id: id.clone(),
            kind,
            title: meta.title.clone(),
            group,
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
        });
    }

    let tasks = store.list_tasks(&TaskFilter::default())?;
    for task in tasks {
        let task_id = format!("task:{}", task.id.0);
        groups.insert(task.column.clone(), ());
        nodes.push(GraphNode {
            id: task_id.clone(),
            kind: GraphNodeKind::Task,
            title: task.title.clone(),
            group: task.column.clone(),
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

    Ok(GraphPayload { nodes, edges, groups })
}

fn top_level_group(path: &Path) -> String {
    path.components()
        .next()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .unwrap_or_else(|| "root".into())
}

#[cfg(test)]
mod tests {
    use super::{build_graph_payload, GraphEdgeKind, GraphNodeKind};
    use crate::{
        models::{Board, BoardId, Document, DocumentId, DocumentMeta, Frontmatter, MetadataField, MetadataProtection, MetadataValue, SearchResult, Task, TaskId, WikiLink},
        store::{DocumentMetadataFilter, TaskFilter, VaultStore},
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

    impl VaultStore for StubStore {
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
        fn get_task(&self, _id: &TaskId) -> Result<Option<Task>> { Ok(None) }
        fn list_tasks(&self, _filter: &TaskFilter) -> Result<Vec<Task>> { Ok(self.tasks.clone()) }
        fn save_task(&self, _task: &Task) -> Result<()> { Ok(()) }
        fn delete_task(&self, _id: &TaskId) -> Result<()> { Ok(()) }
        fn get_board(&self, _id: &BoardId) -> Result<Option<Board>> { Ok(None) }
        fn list_boards(&self) -> Result<Vec<Board>> { Ok(self.boards.clone()) }
        fn save_board(&self, _board: &Board) -> Result<()> { Ok(()) }
    }

    #[test]
    fn builds_document_nodes_and_wikilink_edges() {
        let now = Utc::now();
        let a = DocumentId::new();
        let b = DocumentId::new();
        let board_id = BoardId::new();
        let task_id = TaskId::new();
        let docs = vec![
            DocumentMeta { id: a.clone(), path: PathBuf::from("design/alpha.md"), title: "alpha".into(), tags: vec![], metadata: Default::default(), updated_at: now },
            DocumentMeta { id: b.clone(), path: PathBuf::from("design/beta.md"), title: "beta".into(), tags: vec![], metadata: Default::default(), updated_at: now },
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
                updated_at: now,
            },
        ];
        let full_docs = HashMap::from([
            (a.0.to_string(), Document { id: a.clone(), path: PathBuf::from("design/alpha.md"), title: "alpha".into(), content: String::new(), frontmatter: Frontmatter::default(), outgoing_links: vec![WikiLink { target: "beta".into(), display: None, anchor: None }], created_at: now, updated_at: now }),
            (b.0.to_string(), Document { id: b.clone(), path: PathBuf::from("design/beta.md"), title: "beta".into(), content: String::new(), frontmatter: Frontmatter::default(), outgoing_links: vec![], created_at: now, updated_at: now }),
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
                due_date: None,
                position: 0,
                created_at: now,
                updated_at: now,
            }],
        };

        let graph = build_graph_payload(&store).unwrap();
        assert!(graph.nodes.iter().any(|node| node.kind == GraphNodeKind::Document));
        assert!(graph.nodes.iter().any(|node| node.kind == GraphNodeKind::Communication));
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
