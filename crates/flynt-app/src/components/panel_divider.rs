//! Drag divider for resizing the agent panel.
//!
//! Uses document-level pointer events via eval() so the drag continues
//! even when the cursor leaves the divider element.

use dioxus::prelude::*;

#[component]
pub fn PanelDivider() -> Element {
    rsx! {
        div {
            class: "panel-divider",
            onmousedown: move |_| {
                // Inject document-level drag handling via JS.
                // Sets a CSS variable on .agent-rail to control width.
                spawn(async move {
                    let js = r#"
                        (function() {
                            const rail = document.querySelector('.agent-rail');
                            if (!rail) return;
                            document.body.style.cursor = 'col-resize';
                            document.body.style.userSelect = 'none';
                            const divider = document.querySelector('.panel-divider');
                            if (divider) divider.classList.add('active');

                            function onMove(e) {
                                const w = window.innerWidth - e.clientX;
                                const clamped = Math.max(280, Math.min(700, w));
                                rail.style.width = clamped + 'px';
                            }
                            function onUp() {
                                document.removeEventListener('pointermove', onMove);
                                document.removeEventListener('pointerup', onUp);
                                document.body.style.cursor = '';
                                document.body.style.userSelect = '';
                                if (divider) divider.classList.remove('active');
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
