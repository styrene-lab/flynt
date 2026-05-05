//! Re-export task file serialization from flynt-models.
//!
//! The canonical implementation lives in `flynt_models::task_file`.
//! flynt-core re-exports for backward compatibility.
//!
//! Note: flynt-models uses simple `+++`-delimited frontmatter extraction
//! (no comrak dependency). If you need full markdown AST parsing with
//! wikilink extraction, use `flynt_core::parser::parse_document_source`
//! separately on the body text.

pub use flynt_models::task_file::*;
