use crate::bootstrap::AppContext;
use codex_core::models::{PublicationRule, PublicationVisibility};
use dioxus::prelude::*;

#[component]
pub fn PublicationRulesEditor() -> Element {
    let ctx = use_context::<AppContext>();
    let config = &ctx.vault.config.publication;

    rsx! {
        div { class: "publication-rules-editor",
            h3 { class: "settings-subheading", "Publication policy" }
            p { class: "settings-hint muted", "Selective publish is policy-driven: private by default, then promoted by matching path/tag rules or explicit per-document visibility." }

            div { class: "settings-row",
                span { class: "settings-label", "Default visibility" }
                div { class: "settings-control",
                    span { class: "muted", "{visibility_label(&config.default_visibility)}" }
                }
            }

            div { class: "settings-row",
                span { class: "settings-label", "Rules" }
                div { class: "settings-control publication-rule-list",
                    if config.rules.is_empty() {
                        p { class: "muted", "No publication rules configured." }
                    } else {
                        for rule in &config.rules {
                            PublicationRuleCard { rule: rule.clone() }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn PublicationRuleCard(rule: PublicationRule) -> Element {
    rsx! {
        div { class: "publication-rule-card",
            div { class: "publication-rule-title", "→ {visibility_label(&rule.visibility)}" }
            if let Some(tag) = rule.match_tag.as_ref() {
                div { class: "publication-rule-field muted", "tag = {tag}" }
            }
            if let Some(prefix) = rule.match_path_prefix.as_ref() {
                div { class: "publication-rule-field muted", "path prefix = {prefix}" }
            }
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
