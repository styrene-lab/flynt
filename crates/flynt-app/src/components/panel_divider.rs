//! Drag dividers for resizing panels.
//!
//! Entire drag lifecycle runs in JS via document-level pointer events.
//! Persists widths to localStorage across restarts.

use dioxus::prelude::*;

/// Right-side divider — placed to the LEFT of the agent rail.
#[component]
pub fn PanelDivider() -> Element {
    // Restore saved width on mount
    use_effect(move || {
        spawn(async {
            dioxus::prelude::document::eval(
                r#"
                (function() {
                    var w = localStorage.getItem('flynt-rail-width');
                    if (w) { var el = document.querySelector('.agent-rail'); if (el) el.style.width = w + 'px'; }
                })()
            "#,
            );
        });
    });

    // Set up drag entirely in JS — no Dioxus event → JS race
    use_effect(move || {
        spawn(async {
            dioxus::prelude::document::eval(
                r#"
                (function() {
                    var d = document.querySelector('.panel-divider:not(.sidebar-divider-handle)');
                    if (!d || d._dragBound) return;
                    d._dragBound = true;
                    d.addEventListener('pointerdown', function(e) {
                        e.preventDefault();
                        var rail = document.querySelector('.agent-rail');
                        if (!rail) return;
                        document.body.style.cursor = 'col-resize';
                        document.body.style.userSelect = 'none';
                        d.classList.add('active');
                        function onMove(ev) {
                            var w = window.innerWidth - ev.clientX;
                            rail.style.width = Math.max(280, Math.min(700, w)) + 'px';
                        }
                        function onUp() {
                            document.removeEventListener('pointermove', onMove);
                            document.removeEventListener('pointerup', onUp);
                            document.body.style.cursor = '';
                            document.body.style.userSelect = '';
                            d.classList.remove('active');
                            localStorage.setItem('flynt-rail-width', parseInt(rail.style.width));
                        }
                        document.addEventListener('pointermove', onMove);
                        document.addEventListener('pointerup', onUp);
                    });
                })()
            "#,
            );
        });
    });

    rsx! { div { class: "panel-divider" } }
}

/// Left-side divider — placed to the RIGHT of the sidebar.
#[component]
pub fn SidebarDivider() -> Element {
    use_effect(move || {
        spawn(async {
            dioxus::prelude::document::eval(
                r#"
                (function() {
                    var w = localStorage.getItem('flynt-sidebar-width');
                    if (w) { var el = document.querySelector('.sidebar'); if (el) el.style.width = w + 'px'; }
                })()
            "#,
            );
        });
    });

    use_effect(move || {
        spawn(async {
            dioxus::prelude::document::eval(
                r#"
                (function() {
                    var d = document.querySelector('.sidebar-divider-handle');
                    if (!d || d._dragBound) return;
                    d._dragBound = true;
                    d.addEventListener('pointerdown', function(e) {
                        e.preventDefault();
                        var sidebar = document.querySelector('.sidebar');
                        if (!sidebar) return;
                        document.body.style.cursor = 'col-resize';
                        document.body.style.userSelect = 'none';
                        d.classList.add('active');
                        function onMove(ev) {
                            var w = ev.clientX;
                            sidebar.style.width = Math.max(180, Math.min(450, w)) + 'px';
                        }
                        function onUp() {
                            document.removeEventListener('pointermove', onMove);
                            document.removeEventListener('pointerup', onUp);
                            document.body.style.cursor = '';
                            document.body.style.userSelect = '';
                            d.classList.remove('active');
                            localStorage.setItem('flynt-sidebar-width', parseInt(sidebar.style.width));
                        }
                        document.addEventListener('pointermove', onMove);
                        document.addEventListener('pointerup', onUp);
                    });
                })()
            "#,
            );
        });
    });

    rsx! { div { class: "panel-divider sidebar-divider-handle" } }
}
