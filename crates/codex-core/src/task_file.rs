//! Re-export task file serialization from codex-models.
//!
//! The canonical implementation lives in `codex_models::task_file`.
//! codex-core re-exports for backward compatibility.
//!
//! Note: codex-models uses simple `+++`-delimited frontmatter extraction
//! (no comrak dependency). If you need full markdown AST parsing with
//! wikilink extraction, use `codex_core::parser::parse_document_source`
//! separately on the body text.

pub use codex_models::task_file::*;
