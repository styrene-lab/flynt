//! Save-side callback surface so higher layers (flynt-app, flynt-agent)
//! can react to task edits without flynt-store depending on the
//! network stack.
//!
//! The push pipeline needs a "task X was just saved" signal to feed
//! its debouncer. Wiring that signal through a trait keeps flynt-store
//! free of `flynt-forge`, `reqwest`, `tokio` runtime assumptions —
//! the data layer stays dep-light, the network layer subscribes.
//!
//! Set via `Project::install_save_hook` once at startup. The hook is
//! held in a `OnceLock` so reads from the save path are lock-free and
//! the install can happen any time before the first save.
//!
//! ## Why a trait and not a channel
//!
//! A channel would couple flynt-store to whichever channel impl the
//! caller chooses (broadcast vs mpsc vs crossbeam). A trait lets the
//! caller pick — the typical impl wraps a `flynt_forge::PushDebouncer`
//! and calls `note_edit`, but a test impl might just record what
//! fired into a Vec. Same surface, both work.

use uuid::Uuid;

/// Save-side notifications. Implementors register via
/// [`crate::project::Project::install_save_hook`].
pub trait SaveHook: Send + Sync {
    /// A task file (`kind = "task"`) was just saved to disk + indexed.
    /// `task_id` is the task's UUID, matching the document id.
    fn on_task_saved(&self, task_id: Uuid);
}
