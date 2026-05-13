//! HelpHint — a small `?` icon that opens a hover tooltip with an
//! explanation of an adjacent setting. The styling intentionally
//! stays minimal so it doesn't compete visually with the form field
//! it accompanies. Native `title` attributes have OS-specific styling
//! and a long delay; this gives us a consistent, instant tooltip
//! everywhere.
//!
//! Usage:
//!
//! ```rsx
//! SettingsRow { label: "Theme",
//!     HelpHint { text: "Pick a visual theme — affects sidebar, editor, and rendered preview." }
//!     ThemeGrid { /* ... */ }
//! }
//! ```
//!
//! The hint can carry rich text via the `body` slot if a one-liner
//! isn't enough.

use dioxus::prelude::*;

/// One-line hover tooltip. For longer explanations, use the slot
/// variant via the optional `body` prop.
#[component]
pub fn HelpHint(text: String) -> Element {
    rsx! {
        span { class: "help-hint", tabindex: 0,
            span { class: "help-hint-icon", "?" }
            div { class: "help-hint-tooltip", "{text}" }
        }
    }
}
