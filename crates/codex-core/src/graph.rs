use crate::{models::*, store::{DocumentMetadataFilter, TaskFilter, VaultStore}};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::{Path, PathBuf}};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphNodeKind {
    Document,
    Task,
    Board,
    MemoryFact,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct GraphPayload {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

pub fn build_graph_payload(store: &dyn VaultStore) -> Result<GraphPayload> {
    let docs = store.list_documents()?;
    let mut nodes = Vec::with_capacity(docs.len());
    let mut edges = Vec::new();

    for meta in docs {
        let id = meta.id.0.to_string();
        nodes.push(GraphNode {
            id: id.clone(),
            kind: GraphNodeKind::Document,
            title: meta.title.clone(),
            group: top_level_group(&meta.path),
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

    Ok(GraphPayload { nodes, edges })
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
    use super::*;
    use chrono::Utc;

    struct StubStore {
        docs: Vec<DocumentMeta>,
        full_docs: HashMap<String, Document>,
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
        fn list_tasks(&self, _filter: &TaskFilter) -> Result<Vec<Task>> { Ok(vec![]) }
        fn save_task(&self, _task: &Task) -> Result<()> { Ok(()) }
        fn delete_task(&self, _id: &TaskId) -> Result<()> { Ok(()) }
        fn get_board(&self, _id: &BoardId) -> Result<Option<Board>> { Ok(None) }
        fn list_boards(&self) -> Result<Vec<Board>> { Ok(vec![]) }
        fn save_board(&self, _board: &Board) -> Result<()> { Ok(()) }
    }

    #[test]
    fn builds_document_nodes_and_wikilink_edges() {
        let now = Utc::now();
        let a = DocumentId::new();
        let b = DocumentId::new();
        let docs = vec![
            DocumentMeta { id: a.clone(), path: PathBuf::from("design/alpha.md"), title: "alpha".into(), tags: vec![], metadata: Default::default(), updated_at: now },
            DocumentMeta { id: b.clone(), path: PathBuf::from("design/beta.md"), title: "beta".into(), tags: vec![], metadata: Default::default(), updated_at: now },
        ];
        let full_docs = HashMap::from([
            (a.0.to_string(), Document { id: a.clone(), path: PathBuf::from("design/alpha.md"), title: "alpha".into(), content: String::new(), frontmatter: Frontmatter::default(), outgoing_links: vec![WikiLink { target: "beta".into(), display: None, anchor: None }], created_at: now, updated_at: now }),
            (b.0.to_string(), Document { id: b.clone(), path: PathBuf::from("design/beta.md"), title: "beta".into(), content: String::new(), frontmatter: Frontmatter::default(), outgoing_links: vec![], created_at: now, updated_at: now }),
        ]);
        let store = StubStore { docs, full_docs };

        let graph = build_graph_payload(&store).unwrap();
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.nodes[0].kind, GraphNodeKind::Document);
        assert_eq!(graph.nodes[0].group, "design");
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].kind, GraphEdgeKind::Wikilink);
    }
}
