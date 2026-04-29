//! Native menu bar for the Codex desktop app.
//!
//! Uses `muda` for macOS/Windows/Linux native menu integration.
//! Menu events are dispatched to the Dioxus app via `use_menu_event_handler`.

use muda::{
    accelerator::{Accelerator, Code, Modifiers},
    Menu, MenuItem, PredefinedMenuItem, Submenu,
};

// ── Menu item IDs ────────────────────────────────────────────────────────────

pub const NEW_NOTE: &str = "codex-new-note";
pub const OPEN_VAULT: &str = "codex-open-vault";
pub const SAVE: &str = "codex-save";
pub const CLOSE_TAB: &str = "codex-close-tab";

pub const VIEW_NOTES: &str = "codex-view-notes";
pub const VIEW_BOARD: &str = "codex-view-board";
pub const VIEW_GRAPH: &str = "codex-view-graph";
pub const VIEW_SETTINGS: &str = "codex-view-settings";
pub const TOGGLE_AGENT: &str = "codex-toggle-agent";
pub const TOGGLE_SIDEBAR: &str = "codex-toggle-sidebar";

pub const SYNC_NOW: &str = "codex-sync-now";
pub const NEW_BOARD: &str = "codex-new-board";
pub const DAILY_NOTE: &str = "codex-daily-note";

pub const NEW_DRAWING: &str = "codex-new-drawing";

pub const RENAME_NOTE: &str = "codex-rename-note";
pub const DELETE_NOTE: &str = "codex-delete-note";

/// Build the application menu bar.
pub fn build_menu_bar() -> Menu {
    let menu = Menu::new();

    // ── Codex (app menu on macOS) ────────────────────────────────────────
    let app_menu = Submenu::new("Codyx", true);
    app_menu
        .append_items(&[
            &PredefinedMenuItem::about(None, None),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(
                VIEW_SETTINGS,
                "Settings…",
                true,
                Some(Accelerator::new(Some(Modifiers::META), Code::Comma)),
            ),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(None),
        ])
        .unwrap();

    // ── File ─────────────────────────────────────────────────────────────
    let file_menu = Submenu::new("File", true);
    file_menu
        .append_items(&[
            &MenuItem::with_id(
                NEW_NOTE,
                "New Note",
                true,
                Some(Accelerator::new(Some(Modifiers::META), Code::KeyN)),
            ),
            &MenuItem::with_id(
                NEW_BOARD,
                "New Board",
                true,
                Some(Accelerator::new(
                    Some(Modifiers::META | Modifiers::SHIFT),
                    Code::KeyN,
                )),
            ),
            &MenuItem::with_id(
                DAILY_NOTE,
                "Today's Note",
                true,
                Some(Accelerator::new(Some(Modifiers::META), Code::KeyD)),
            ),
            &MenuItem::with_id(
                NEW_DRAWING,
                "New Drawing",
                true,
                Some(Accelerator::new(
                    Some(Modifiers::META | Modifiers::SHIFT),
                    Code::KeyD,
                )),
            ),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(
                OPEN_VAULT,
                "Open Vault…",
                true,
                Some(Accelerator::new(Some(Modifiers::META), Code::KeyO)),
            ),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(
                SAVE,
                "Save",
                true,
                Some(Accelerator::new(Some(Modifiers::META), Code::KeyS)),
            ),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(
                RENAME_NOTE,
                "Rename…",
                true,
                Some(Accelerator::new(Some(Modifiers::META | Modifiers::SHIFT), Code::KeyR)),
            ),
            &MenuItem::with_id(DELETE_NOTE, "Move to Trash", true, None),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(
                CLOSE_TAB,
                "Close Tab",
                true,
                Some(Accelerator::new(Some(Modifiers::META), Code::KeyW)),
            ),
        ])
        .unwrap();

    // ── Edit ─────────────────────────────────────────────────────────────
    let edit_menu = Submenu::new("Edit", true);
    edit_menu
        .append_items(&[
            &PredefinedMenuItem::undo(None),
            &PredefinedMenuItem::redo(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::cut(None),
            &PredefinedMenuItem::copy(None),
            &PredefinedMenuItem::paste(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::select_all(None),
        ])
        .unwrap();

    // ── View ─────────────────────────────────────────────────────────────
    let view_menu = Submenu::new("View", true);
    view_menu
        .append_items(&[
            &MenuItem::with_id(
                VIEW_NOTES,
                "Notes",
                true,
                Some(Accelerator::new(Some(Modifiers::META), Code::Digit1)),
            ),
            &MenuItem::with_id(
                VIEW_BOARD,
                "Board",
                true,
                Some(Accelerator::new(Some(Modifiers::META), Code::Digit2)),
            ),
            &MenuItem::with_id(
                VIEW_GRAPH,
                "Graph",
                true,
                Some(Accelerator::new(Some(Modifiers::META), Code::Digit3)),
            ),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(
                TOGGLE_AGENT,
                "Toggle Agent Panel",
                true,
                Some(Accelerator::new(
                    Some(Modifiers::META | Modifiers::SHIFT),
                    Code::KeyA,
                )),
            ),
            &MenuItem::with_id(
                TOGGLE_SIDEBAR,
                "Toggle Sidebar",
                true,
                Some(Accelerator::new(Some(Modifiers::META), Code::Backslash)),
            ),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(
                SYNC_NOW,
                "Sync Now",
                true,
                Some(Accelerator::new(
                    Some(Modifiers::META | Modifiers::SHIFT),
                    Code::KeyS,
                )),
            ),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::fullscreen(None),
        ])
        .unwrap();

    // ── Window ───────────────────────────────────────────────────────────
    let window_menu = Submenu::new("Window", true);
    window_menu
        .append_items(&[
            &PredefinedMenuItem::minimize(None),
            &PredefinedMenuItem::maximize(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::close_window(None),
        ])
        .unwrap();

    // ── Assemble ─────────────────────────────────────────────────────────
    menu.append_items(&[&app_menu, &file_menu, &edit_menu, &view_menu, &window_menu])
        .unwrap();

    #[cfg(target_os = "macos")]
    {
        window_menu.set_as_windows_menu_for_nsapp();
    }

    menu
}
