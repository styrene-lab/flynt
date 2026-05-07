//! Drag divider for resizing panels.
//!
//! Uses document-level pointer events via eval() so the drag continues
//! even when the cursor leaves the divider element.
//! Persists widths to localStorage so they survive restarts.

use dioxus::prelude::*;

/// Right-side divider — placed to the LEFT of the agent rail.
#[component]
pub fn PanelDivider() -> Element {
    // Restore saved width on mount
    use_effect(move || {
        spawn(async {
            dioxus::prelude::document::eval(r#"
                (function() {
                    var w = localStorage.getItem('flynt-rail-width');
                    if (w) { var el = document.querySelector('.agent-rail'); if (el) el.style.width = w + 'px'; }
                })()
            "#);
        });
    });

    rsx! {
        div {
            class: "panel-divider",
            onmousedown: move |_| {
                spawn(async move {
                    dioxus::prelude::document::eval(r#"
                        (function() {
                            var rail = document.querySelector('.agent-rail');
                            if (!rail) return;
                            document.body.style.cursor = 'col-resize';
                            document.body.style.userSelect = 'none';
                            var d = document.querySelector('.panel-divider');
                            if (d) d.classList.add('active');
                            function onMove(e) {
                                var w = window.innerWidth - e.clientX;
                                rail.style.width = Math.max(280, Math.min(700, w)) + 'px';
                            }
                            function onUp() {
                                document.removeEventListener('pointermove', onMove);
                                document.removeEventListener('pointerup', onUp);
                                document.body.style.cursor = '';
                                document.body.style.userSelect = '';
                                if (d) d.classList.remove('active');
                                localStorage.setItem('flynt-rail-width', parseInt(rail.style.width));
                            }
                            document.addEventListener('pointermove', onMove);
                            document.addEventListener('pointerup', onUp);
                        })()
                    "#);
                });
            },
        }
    }
}

/// Left-side divider — placed to the RIGHT of the sidebar.
#[component]
pub fn SidebarDivider() -> Element {
    use_effect(move || {
        spawn(async {
            dioxus::prelude::document::eval(r#"
                (function() {
                    var w = localStorage.getItem('flynt-sidebar-width');
                    if (w) { var el = document.querySelector('.sidebar'); if (el) el.style.width = w + 'px'; }
                })()
            "#);
        });
    });

    rsx! {
        div {
            class: "panel-divider sidebar-divider-handle",
            onmousedown: move |_| {
                spawn(async move {
                    dioxus::prelude::document::eval(r#"
                        (function() {
                            var sidebar = document.querySelector('.sidebar');
                            if (!sidebar) return;
                            document.body.style.cursor = 'col-resize';
                            document.body.style.userSelect = 'none';
                            var d = document.querySelector('.sidebar-divider-handle');
                            if (d) d.classList.add('active');
                            function onMove(e) {
                                var w = e.clientX;
                                sidebar.style.width = Math.max(180, Math.min(450, w)) + 'px';
                            }
                            function onUp() {
                                document.removeEventListener('pointermove', onMove);
                                document.removeEventListener('pointerup', onUp);
                                document.body.style.cursor = '';
                                document.body.style.userSelect = '';
                                if (d) d.classList.remove('active');
                                localStorage.setItem('flynt-sidebar-width', parseInt(sidebar.style.width));
                            }
                            document.addEventListener('pointermove', onMove);
                            document.addEventListener('pointerup', onUp);
                        })()
                    "#);
                });
            },
        }
    }
}
