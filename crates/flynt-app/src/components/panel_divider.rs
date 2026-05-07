//! Drag divider for resizing panels.
//!
//! Uses document-level pointer events via eval() so the drag continues
//! even when the cursor leaves the divider element.

use dioxus::prelude::*;

/// Right-side divider — placed to the LEFT of the agent rail.
/// Dragging left makes the rail wider, dragging right makes it narrower.
#[component]
pub fn PanelDivider() -> Element {
    rsx! {
        div {
            class: "panel-divider",
            onmousedown: move |_| {
                spawn(async move {
                    let js = r#"
                        (function() {
                            const rail = document.querySelector('.agent-rail');
                            if (!rail) return;
                            document.body.style.cursor = 'col-resize';
                            document.body.style.userSelect = 'none';
                            const d = document.querySelector('.panel-divider');
                            if (d) d.classList.add('active');
                            function onMove(e) {
                                const w = window.innerWidth - e.clientX;
                                rail.style.width = Math.max(280, Math.min(700, w)) + 'px';
                            }
                            function onUp() {
                                document.removeEventListener('pointermove', onMove);
                                document.removeEventListener('pointerup', onUp);
                                document.body.style.cursor = '';
                                document.body.style.userSelect = '';
                                if (d) d.classList.remove('active');
                            }
                            document.addEventListener('pointermove', onMove);
                            document.addEventListener('pointerup', onUp);
                        })()
                    "#;
                    dioxus::prelude::document::eval(js);
                });
            },
        }
    }
}

/// Left-side divider — placed to the RIGHT of the sidebar.
/// Dragging right makes the sidebar wider, dragging left makes it narrower.
#[component]
pub fn SidebarDivider() -> Element {
    rsx! {
        div {
            class: "panel-divider sidebar-divider-handle",
            onmousedown: move |_| {
                spawn(async move {
                    let js = r#"
                        (function() {
                            const sidebar = document.querySelector('.sidebar');
                            if (!sidebar) return;
                            document.body.style.cursor = 'col-resize';
                            document.body.style.userSelect = 'none';
                            const d = document.querySelector('.sidebar-divider-handle');
                            if (d) d.classList.add('active');
                            function onMove(e) {
                                const w = e.clientX;
                                sidebar.style.width = Math.max(180, Math.min(450, w)) + 'px';
                            }
                            function onUp() {
                                document.removeEventListener('pointermove', onMove);
                                document.removeEventListener('pointerup', onUp);
                                document.body.style.cursor = '';
                                document.body.style.userSelect = '';
                                if (d) d.classList.remove('active');
                            }
                            document.addEventListener('pointermove', onMove);
                            document.addEventListener('pointerup', onUp);
                        })()
                    "#;
                    dioxus::prelude::document::eval(js);
                });
            },
        }
    }
}
