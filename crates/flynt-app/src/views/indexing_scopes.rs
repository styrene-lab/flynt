use dioxus::prelude::*;
use flynt_core::models::IndexScope;
use std::path::PathBuf;

const KNOWN_KINDS: &[(&str, &str)] = &[
    ("", "(none)"),
    ("document", "Document"),
    ("design_node", "Design node"),
    ("project", "Project"),
    ("repo", "Repo"),
    ("link", "Link"),
];

#[component]
pub fn IndexingScopesEditor(scopes: Signal<Vec<IndexScope>>) -> Element {
    rsx! {
        div { class: "indexing-scopes-editor",
            if scopes.read().is_empty() {
                p { class: "muted", "No scopes configured — all files follow the project-wide default." }
            } else {
                for (index, scope) in scopes.read().iter().cloned().enumerate() {
                    IndexScopeCard {
                        index,
                        scope,
                        on_change: move |(index, updated)| {
                            if let Some(slot) = scopes.write().get_mut(index) {
                                *slot = updated;
                            }
                        },
                        on_remove: move |index| {
                            scopes.write().remove(index);
                        },
                    }
                }
            }
            button {
                class: "btn btn-ghost",
                onclick: move |_| scopes.write().push(IndexScope {
                    prefix: PathBuf::new(),
                    kind: None,
                    write_frontmatter: Some(true),
                }),
                "Add scope"
            }
        }
    }
}

#[component]
fn IndexScopeCard(
    index: usize,
    scope: IndexScope,
    on_change: EventHandler<(usize, IndexScope)>,
    on_remove: EventHandler<usize>,
) -> Element {
    let prefix_value = scope.prefix.display().to_string();
    let kind_value = scope.kind.clone().unwrap_or_default();
    let wf = scope.write_frontmatter.unwrap_or(true);

    rsx! {
        div { class: "indexing-scope-card",
            div { class: "indexing-scope-header",
                span { class: "indexing-scope-title", "Scope {index + 1}" }
                button {
                    class: "btn btn-ghost btn-sm",
                    onclick: move |_| on_remove.call(index),
                    "Remove"
                }
            }
            div { class: "indexing-scope-fields",
                div { class: "indexing-scope-field",
                    label { class: "indexing-scope-label", "Path prefix" }
                    input {
                        class: "input settings-input",
                        r#type: "text",
                        value: "{prefix_value}",
                        placeholder: "e.g. design/",
                        oninput: {
                            let scope = scope.clone();
                            move |e| {
                                let mut updated = scope.clone();
                                updated.prefix = PathBuf::from(e.value());
                                on_change.call((index, updated));
                            }
                        },
                    }
                }
                div { class: "indexing-scope-field",
                    label { class: "indexing-scope-label", "Entity kind" }
                    select {
                        class: "input settings-input",
                        value: "{kind_value}",
                        onchange: {
                            let scope = scope.clone();
                            move |e| {
                                let mut updated = scope.clone();
                                let val = e.value();
                                updated.kind = if val.is_empty() { None } else { Some(val) };
                                on_change.call((index, updated));
                            }
                        },
                        for (value, label) in KNOWN_KINDS {
                            option {
                                value: *value,
                                selected: kind_value == *value,
                                "{label}"
                            }
                        }
                    }
                }
                div { class: "indexing-scope-field",
                    label { class: "checkbox-label",
                        input {
                            r#type: "checkbox",
                            checked: wf,
                            onchange: {
                                let scope = scope.clone();
                                move |e| {
                                    let mut updated = scope.clone();
                                    updated.write_frontmatter = Some(e.checked());
                                    on_change.call((index, updated));
                                }
                            },
                        }
                        "Write frontmatter"
                    }
                }
            }
        }
    }
}
