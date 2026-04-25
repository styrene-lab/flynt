//! Core data types for Codex.
//!
//! This crate contains the canonical Task model, ID newtypes, and task file
//! serialization format. It has minimal dependencies (no comrak, no graph,
//! no parser) so it can be consumed by scribe, omegon, and other ecosystem
//! crates without pulling in the full codex-core.

pub mod task;
pub mod task_file;
