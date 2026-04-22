//! HTML/CSS context menus for right-click actions.

use dioxus::prelude::*;

#[derive(Clone, PartialEq)]
pub struct ContextMenuItem {
    pub label: String,
    pub id: String,
    pub danger: bool,
    pub separator_before: bool,
}

impl ContextMenuItem {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self { id: id.into(), label: label.into(), danger: false, separator_before: false }
    }
    pub fn danger(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self { id: id.into(), label: label.into(), danger: true, separator_before: false }
    }
    pub fn sep(mut self) -> Self {
        self.separator_before = true;
        self
    }
}

#[component]
pub fn ContextMenu(
    x: f64,
    y: f64,
    items: Vec<ContextMenuItem>,
    on_select: EventHandler<String>,
    on_close: EventHandler<()>,
) -> Element {
    rsx! {
        div {
            class: "ctx-menu-overlay",
            onclick: move |_| on_close.call(()),
        }
        div {
            class: "ctx-menu",
            style: "left: {x}px; top: {y}px;",
            for item in items.iter() {
                {
                    let id = item.id.clone();
                    let sep = item.separator_before;
                    if sep {
                        rsx! {
                            div { class: "ctx-menu-sep" }
                            button {
                                class: if item.danger { "ctx-menu-item danger" } else { "ctx-menu-item" },
                                onclick: move |_| on_select.call(id.clone()),
                                "{item.label}"
                            }
                        }
                    } else {
                        rsx! {
                            button {
                                class: if item.danger { "ctx-menu-item danger" } else { "ctx-menu-item" },
                                onclick: move |_| on_select.call(id.clone()),
                                "{item.label}"
                            }
                        }
                    }
                }
            }
        }
    }
}
