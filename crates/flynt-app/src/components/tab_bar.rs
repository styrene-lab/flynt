use dioxus::prelude::*;
use flynt_core::store::VaultStore;
use crate::bootstrap::AppContext;
use crate::state::TabState;

#[component]
pub fn TabBar() -> Element {
    let tab_state = use_context::<Signal<TabState>>();
    let tabs = tab_state.read().tabs.clone();

    if tabs.is_empty() { return rsx! { div { class: "tab-bar tab-bar-empty" } }; }

    rsx! {
        div { class: "tab-bar",
            for (idx, (doc_id, title)) in tabs.iter().enumerate() {
                {
                    let i = idx;
                    let is_active = tab_state.read().active == i;
                    let title = title.clone();
                    let doc_id = doc_id.clone();
                    rsx! {
                        TabItem {
                            index: i,
                            title: title,
                            doc_id: doc_id,
                            is_active: is_active,
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TabItem(index: usize, title: String, doc_id: flynt_core::models::DocumentId, is_active: bool) -> Element {
    let ctx = use_context::<AppContext>();
    let mut tab_state = use_context::<Signal<TabState>>();
    let mut renaming = use_signal(|| false);
    let mut rename_input = use_signal(|| title.clone());

    let i = index;

    rsx! {
        div {
            class: if is_active { "tab active" } else { "tab" },
            onclick: move |_| tab_state.write().active = i,
            ondoubleclick: move |_| {
                *rename_input.write() = title.clone();
                *renaming.write() = true;
            },
            if *renaming.read() {
                input {
                    class: "tab-rename-input",
                    autofocus: true,
                    value: "{rename_input}",
                    onclick: move |e| e.stop_propagation(),
                    oninput: move |e| *rename_input.write() = e.value(),
                    onkeydown: {
                        let doc_id = doc_id.clone();
                        let title = title.clone();
                        move |e: KeyboardEvent| {
                            if e.key() == Key::Escape {
                                *renaming.write() = false;
                            }
                            if e.key() == Key::Enter {
                                let new_title = rename_input.read().trim().to_string();
                                if new_title.is_empty() || new_title == title {
                                    *renaming.write() = false;
                                    return;
                                }
                                let c = ctx.clone();
                                let did = doc_id.clone();
                                let nt = new_title.clone();
                                spawn(async move {
                                    let vault = c.vault();
                                    if let Ok(Some(doc)) = vault.store.get_document(&did) {
                                        let _ = vault.rename_document(&doc.path, &nt);
                                        let _ = vault.reindex();
                                    }
                                });
                                // Update tab title immediately
                                tab_state.write().tabs[i].1 = new_title;
                                *renaming.write() = false;
                            }
                        }
                    },
                    onmounted: move |e| { let _ = e.set_focus(true); },
                }
            } else {
                span { class: "tab-title", "{title}" }
            }
            button {
                class: "tab-close",
                title: "Close tab",
                onclick: move |e| {
                    e.stop_propagation();
                    tab_state.write().close(i);
                },
                "\u{00d7}"
            }
        }
    }
}
