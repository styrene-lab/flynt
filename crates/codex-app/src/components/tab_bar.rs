use dioxus::prelude::*;
use crate::state::TabState;

#[component]
pub fn TabBar() -> Element {
    let mut tab_state = use_context::<Signal<TabState>>();
    let tabs = tab_state.read().tabs.clone();

    if tabs.is_empty() { return rsx! { div { class: "tab-bar tab-bar-empty" } }; }

    rsx! {
        div { class: "tab-bar",
            for (idx, (_, title)) in tabs.iter().enumerate() {
                {
                    let i = idx;
                    let is_active = tab_state.read().active == i;
                    let title = title.clone();
                    rsx! {
                        div {
                            class: if is_active { "tab active" } else { "tab" },
                            onclick: move |_| tab_state.write().active = i,
                            span { class: "tab-title", "{title}" }
                            button {
                                class: "tab-close",
                                title: "Close tab",
                                onclick: move |e| {
                                    e.stop_propagation();
                                    tab_state.write().close(i);
                                },
                                "×"
                            }
                        }
                    }
                }
            }
        }
    }
}
