use dioxus::prelude::*;
use flynt_core::models::{PublicationRule, PublicationVisibility};

#[component]
pub fn PublicationRulesEditor(
    default_visibility: Signal<PublicationVisibility>,
    rules: Signal<Vec<PublicationRule>>,
) -> Element {
    rsx! {
        div { class: "publication-rules-editor",
            h3 { class: "settings-subheading", "Publication policy" }
            p { class: "settings-hint muted", "Selective publish is policy-driven: private by default, then promoted by matching path/tag rules or explicit per-document visibility." }

            div { class: "settings-row",
                span { class: "settings-label", "Default visibility" }
                div { class: "settings-control",
                    div { class: "radio-group",
                        for visibility in [PublicationVisibility::Private, PublicationVisibility::Unlisted, PublicationVisibility::Public] {
                            PublicationVisibilityRadio {
                                visibility,
                                active: *default_visibility.read() == visibility,
                                on_select: move |_| *default_visibility.write() = visibility,
                            }
                        }
                    }
                }
            }

            div { class: "settings-row",
                span { class: "settings-label", "Rules" }
                div { class: "settings-control publication-rule-list",
                    if rules.read().is_empty() {
                        p { class: "muted", "No publication rules configured." }
                    } else {
                        for (index, rule) in rules.read().iter().cloned().enumerate() {
                            PublicationRuleCard {
                                index,
                                rule,
                                on_change: move |(index, updated)| {
                                    if let Some(slot) = rules.write().get_mut(index) {
                                        *slot = updated;
                                    }
                                },
                                on_remove: move |index| {
                                    rules.write().remove(index);
                                },
                            }
                        }
                    }
                    button {
                        class: "btn btn-ghost",
                        onclick: move |_| rules.write().push(PublicationRule::default()),
                        "Add rule"
                    }
                }
            }
        }
    }
}

#[component]
fn PublicationRuleCard(
    index: usize,
    rule: PublicationRule,
    on_change: EventHandler<(usize, PublicationRule)>,
    on_remove: EventHandler<usize>,
) -> Element {
    let tag_value = rule.match_tag.clone().unwrap_or_default();
    let path_prefix_value = rule.match_path_prefix.clone().unwrap_or_default();
    rsx! {
        div { class: "publication-rule-card",
            div { class: "publication-rule-title", "Rule {index + 1}" }
            input {
                class: "input settings-input",
                r#type: "text",
                value: "{tag_value}",
                placeholder: "tag (optional)",
                oninput: {
                    let rule = rule.clone();
                    move |e| {
                        let mut updated = rule.clone();
                        updated.match_tag = string_from_input(&e.value());
                        on_change.call((index, updated));
                    }
                },
            }
            input {
                class: "input settings-input",
                r#type: "text",
                value: "{path_prefix_value}",
                placeholder: "path prefix (optional)",
                oninput: {
                    let rule = rule.clone();
                    move |e| {
                        let mut updated = rule.clone();
                        updated.match_path_prefix = string_from_input(&e.value());
                        on_change.call((index, updated));
                    }
                },
            }
            div { class: "radio-group",
                for visibility in [PublicationVisibility::Private, PublicationVisibility::Unlisted, PublicationVisibility::Public] {
                    PublicationVisibilityRadio {
                        visibility,
                        active: rule.visibility == visibility,
                        on_select: {
                            let rule = rule.clone();
                            move |_| {
                                let mut updated = rule.clone();
                                updated.visibility = visibility;
                                on_change.call((index, updated));
                            }
                        },
                    }
                }
            }
            button {
                class: "btn btn-ghost",
                onclick: move |_| on_remove.call(index),
                "Remove rule"
            }
        }
    }
}

#[component]
fn PublicationVisibilityRadio(
    visibility: PublicationVisibility,
    active: bool,
    on_select: EventHandler<()>,
) -> Element {
    rsx! {
        button {
            class: if active { "radio-btn active" } else { "radio-btn" },
            onclick: move |_| on_select.call(()),
            div { class: if active { "radio-dot active" } else { "radio-dot" } }
            "{visibility_label(&visibility)}"
        }
    }
}

fn visibility_label(visibility: &PublicationVisibility) -> &'static str {
    match visibility {
        PublicationVisibility::Private => "private",
        PublicationVisibility::Unlisted => "unlisted",
        PublicationVisibility::Public => "public",
    }
}

fn string_from_input(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
