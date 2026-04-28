pub mod agent_rail;
pub mod command_palette;
pub mod context_menu;
pub mod daemon_settings;
pub mod identity_settings;
pub mod provider_settings;
pub mod sidebar;
pub mod tab_bar;
pub mod toolbar;

pub use agent_rail::AgentRail;
pub use command_palette::CommandPalette;
pub use context_menu::{ContextMenu, ContextMenuItem};
pub use sidebar::{initial_note_id_for_vault, Sidebar};
pub use tab_bar::TabBar;
pub use toolbar::Toolbar;
