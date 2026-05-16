use flynt_core::models::{FlyntOperatorSettings, ImportedUiTheme};
use std::collections::{BTreeMap, BTreeSet};

const ALPHARIUS_CSS: &str = include_str!("../assets/themes/alpharius.css");
const TWEAKCN_PRESETS: &str = include_str!("../assets/vendor/tweakcn-presets.json");
const BUILTIN_THEME_ORDER: &[&str] = &[
    "alpharius",
    "light",
    "modern-minimal",
    "catppuccin",
    "graphite",
    "cyberpunk",
    "perpetuity",
    "vercel",
    "supabase",
    "claude",
    "twitter",
    "bubblegum",
];

#[derive(Clone, Debug, PartialEq)]
pub struct UiTheme {
    pub id: String,
    pub name: String,
    pub description: String,
    pub vars: BTreeMap<String, String>,
    pub builtin: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThemeLibrary {
    pub themes: Vec<UiTheme>,
}

impl ThemeLibrary {
    pub fn from_operator(settings: &FlyntOperatorSettings) -> Self {
        let mut themes = bundled_themes();
        let mut seen: BTreeSet<String> = themes.iter().map(|theme| theme.id.clone()).collect();

        for imported in settings.ui_theme.imported_themes.iter().cloned() {
            if imported.id.trim().is_empty()
                || imported.vars.is_empty()
                || seen.contains(&imported.id)
            {
                continue;
            }
            seen.insert(imported.id.clone());
            themes.push(UiTheme {
                id: imported.id,
                name: imported.name,
                description: imported.description,
                vars: complete_vars(imported.vars),
                builtin: false,
            });
        }

        Self { themes }
    }

    pub fn active_vars(&self, active_id: &str) -> String {
        self.theme(active_id)
            .or_else(|| self.theme("alpharius"))
            .map(inline_vars)
            .unwrap_or_else(String::new)
    }

    pub fn theme(&self, id: &str) -> Option<&UiTheme> {
        self.themes.iter().find(|theme| theme.id == id)
    }

    pub fn upsert_imported(&mut self, mut theme: UiTheme) -> String {
        if self
            .theme(&theme.id)
            .is_some_and(|existing| existing.builtin)
        {
            theme.id = self.unique_imported_id(&theme.id);
        }
        let id = theme.id.clone();
        if let Some(existing) = self
            .themes
            .iter_mut()
            .find(|existing| existing.id == theme.id)
        {
            *existing = theme;
        } else {
            self.themes.push(theme);
        }
        id
    }

    pub fn imported_for_settings(&self) -> Vec<ImportedUiTheme> {
        self.themes
            .iter()
            .filter(|theme| !theme.builtin)
            .map(|theme| ImportedUiTheme {
                id: theme.id.clone(),
                name: theme.name.clone(),
                description: theme.description.clone(),
                vars: theme.vars.clone(),
            })
            .collect()
    }

    fn unique_imported_id(&self, base: &str) -> String {
        let base = format!("custom-{base}");
        if self.theme(&base).is_none() {
            return base;
        }

        for suffix in 2.. {
            let candidate = format!("{base}-{suffix}");
            if self.theme(&candidate).is_none() {
                return candidate;
            }
        }
        unreachable!()
    }
}

pub fn import_tweakcn_theme(content: &str) -> anyhow::Result<UiTheme> {
    let value: serde_json::Value = serde_json::from_str(content)?;
    if let Some(items) = value.get("items").and_then(|items| items.as_array()) {
        for item in items {
            let id_hint = item
                .get("name")
                .and_then(|name| name.as_str())
                .unwrap_or("imported");
            if let Some(theme) = parse_theme_value(id_hint, item, false) {
                return Ok(theme);
            }
        }
    }

    if let Some(theme) = parse_theme_value("imported", &value, false) {
        return Ok(theme);
    }

    if let Some(obj) = value.as_object() {
        for (id, candidate) in obj {
            if let Some(theme) = parse_theme_value(id, candidate, false) {
                return Ok(theme);
            }
        }
    }

    anyhow::bail!("No tweak.cn theme vars found");
}

pub async fn import_tweakcn_theme_from_locator(locator: &str) -> anyhow::Result<UiTheme> {
    let candidates = theme_url_candidates(locator)?;
    let mut failures = Vec::new();

    for url in candidates {
        match fetch_tweakcn_theme_url(&url).await {
            Ok(theme) => return Ok(theme),
            Err(err) => failures.push(err),
        }
    }

    anyhow::bail!(
        "No importable tweak.cn theme JSON found. Paste an exported JSON file, a built-in theme slug, or a public /r/themes/{{id}}.json URL. {}",
        failures.join("; ")
    )
}

async fn fetch_tweakcn_theme_url(url: &str) -> Result<UiTheme, String> {
    let response = reqwest::get(url)
        .await
        .map_err(|err| format!("{url}: {err}"))?;
    let response = response
        .error_for_status()
        .map_err(|err| format!("{url}: {}", http_status_message(url, &err)))?;
    let content = response
        .text()
        .await
        .map_err(|err| format!("{url}: {err}"))?;
    import_tweakcn_theme(&content).map_err(|err| format!("{url}: {err}"))
}

fn theme_url_candidates(locator: &str) -> anyhow::Result<Vec<String>> {
    let mut candidates = Vec::new();
    let locator = locator.trim();
    if is_user_locator(locator) {
        anyhow::bail!(
            "tweak.cn does not expose public profile/user theme import. Paste a specific theme URL, theme ID, registry slug, or exported JSON instead."
        );
    }

    if locator.starts_with("http://") || locator.starts_with("https://") {
        if let Some(theme_id) = tweakcn_theme_id_from_url(locator) {
            candidates.push(format!("https://tweakcn.com/r/themes/{theme_id}.json"));
        }
        candidates.push(locator.to_string());
    } else if !locator.is_empty() {
        let slug = locator
            .rsplit('/')
            .next()
            .unwrap_or(locator)
            .trim_end_matches(".json");
        if !slug.trim().is_empty() {
            candidates.push(format!(
                "https://tweakcn.com/r/themes/{}.json",
                sanitize_id(slug)
            ));
        }
    }

    candidates.dedup();
    if candidates.is_empty() {
        anyhow::bail!("Enter a tweak.cn JSON URL, public theme URL, built-in slug, or theme ID");
    }
    Ok(candidates)
}

fn http_status_message(url: &str, err: &reqwest::Error) -> String {
    let Some(status) = err.status() else {
        return err.to_string();
    };
    if status.is_server_error() && url.contains("/r/themes/") {
        return format!(
            "upstream registry returned {status}. tweak.cn currently does this for some community/private theme IDs; use the exported JSON or a built-in registry slug."
        );
    }
    format!("upstream returned {status}")
}

fn is_user_locator(value: &str) -> bool {
    value.trim().starts_with('@')
        || value.contains("tweakcn.com/@")
        || value.contains("tweakcn.com/u/")
        || value.contains("tweakcn.com/users/")
        || value.contains("tweakcn.com/user/")
}

fn tweakcn_theme_id_from_url(url: &str) -> Option<String> {
    let marker = "/themes/";
    let (_, after) = url.split_once(marker)?;
    let id = after
        .split(['?', '#', '/'])
        .next()
        .unwrap_or_default()
        .trim_end_matches(".json");
    (!id.is_empty()).then(|| id.to_string())
}

fn bundled_themes() -> Vec<UiTheme> {
    let mut themes = vec![UiTheme {
        id: "alpharius".into(),
        name: "Alpharius".into(),
        description: "Flynt default dark operator theme.".into(),
        vars: normalize_vars(base_vars()),
        builtin: true,
    }];

    let parsed = match serde_json::from_str::<serde_json::Value>(TWEAKCN_PRESETS) {
        Ok(parsed) => parsed,
        Err(_) => return themes,
    };
    if let Some(obj) = parsed.as_object() {
        for (id, value) in obj {
            if let Some(theme) = parse_theme_value(id, value, true) {
                themes.push(theme);
            }
        }
    }
    themes.sort_by_key(|theme| {
        BUILTIN_THEME_ORDER
            .iter()
            .position(|id| *id == theme.id)
            .unwrap_or(BUILTIN_THEME_ORDER.len())
    });

    themes
}

fn parse_theme_value(id_hint: &str, value: &serde_json::Value, builtin: bool) -> Option<UiTheme> {
    let vars = vars_object(value)?;
    let name = value
        .get("title")
        .and_then(|name| name.as_str())
        .or_else(|| value.get("name").and_then(|name| name.as_str()))
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(id_hint);
    let description = value
        .get("description")
        .and_then(|description| description.as_str())
        .unwrap_or("");

    let id_source = value
        .get("id")
        .and_then(|id| id.as_str())
        .unwrap_or(if id_hint == "imported" { name } else { id_hint });
    let id = sanitize_id(id_source);
    if id.is_empty() {
        return None;
    }

    Some(UiTheme {
        id,
        name: name.trim().to_string(),
        description: description.trim().to_string(),
        vars: complete_vars(vars),
        builtin,
    })
}

fn vars_object(value: &serde_json::Value) -> Option<BTreeMap<String, String>> {
    let container = value
        .get("vars")
        .or_else(|| value.get("cssVars"))
        .or_else(|| value.get("css_vars"))
        .or_else(|| value.get("styles"))
        .or_else(|| value.get("theme").and_then(|theme| theme.get("cssVars")))
        .or_else(|| value.get("theme").and_then(|theme| theme.get("vars")))
        .unwrap_or(value)
        .as_object()?;

    let mut selected = BTreeMap::new();
    if let Some(theme_vars) = container.get("theme").and_then(|mode| mode.as_object()) {
        collect_vars(theme_vars, &mut selected);
    }

    let mode_vars = container
        .get("dark")
        .or_else(|| container.get("light"))
        .and_then(|mode| mode.as_object())
        .unwrap_or(container);
    collect_vars(mode_vars, &mut selected);

    (!selected.is_empty()).then_some(selected)
}

fn collect_vars(
    vars: &serde_json::Map<String, serde_json::Value>,
    out: &mut BTreeMap<String, String>,
) {
    for (key, value) in vars {
        let Some(value) = value.as_str() else {
            continue;
        };
        let name = normalize_var_name(key);
        if name.is_empty() || !is_safe_css_value(value) {
            continue;
        }
        let value = normalize_css_value(&name, value);
        out.insert(name, value);
    }
}

fn parse_css_vars(css: &str) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    for line in css.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("--") {
            continue;
        }
        if let Some((key, value)) = trimmed.trim_end_matches(';').split_once(':') {
            let key = normalize_var_name(key);
            let value = value.trim();
            if !key.is_empty() && is_safe_css_value(value) {
                let value = normalize_css_value(&key, value);
                vars.insert(key, value);
            }
        }
    }
    vars
}

fn base_vars() -> BTreeMap<String, String> {
    parse_css_vars(ALPHARIUS_CSS)
}

fn with_base_vars(vars: BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut merged = base_vars();
    merged.extend(vars);
    merged
}

fn complete_vars(vars: BTreeMap<String, String>) -> BTreeMap<String, String> {
    normalize_vars(with_base_vars(normalize_vars(vars)))
}

fn normalize_vars(mut vars: BTreeMap<String, String>) -> BTreeMap<String, String> {
    // Compatibility with current shadcn/tweak.cn sidebar token names.
    alias_from_any(
        &mut vars,
        "--sidebar-bg",
        &["--sidebar", "--sidebar-background"],
    );
    alias_from_any(&mut vars, "--sidebar-fg", &["--sidebar-foreground"]);
    alias_from_any(&mut vars, "--sidebar-border", &["--sidebar-border"]);
    alias_from_any(&mut vars, "--sidebar-ring", &["--sidebar-ring", "--ring"]);
    alias_from_any(&mut vars, "--sidebar-heading", &["--sidebar-foreground"]);
    alias_from_any(
        &mut vars,
        "--sidebar-item-hover",
        &["--sidebar-accent", "--accent", "--muted"],
    );
    alias_from_any(
        &mut vars,
        "--sidebar-item-active",
        &["--sidebar-accent", "--sidebar-primary", "--accent"],
    );
    alias_from_any(
        &mut vars,
        "--sidebar-item-active-fg",
        &[
            "--sidebar-accent-foreground",
            "--sidebar-primary-foreground",
            "--accent-foreground",
        ],
    );

    // Compatibility with shadcn radius expansions.
    alias_from_any(&mut vars, "--radius-sm", &["--radius"]);
    alias_from_any(&mut vars, "--radius-md", &["--radius"]);
    alias_from_any(&mut vars, "--radius-lg", &["--radius"]);
    alias_from_any(&mut vars, "--radius-xl", &["--radius"]);

    // Future-facing Flynt surface vocabulary. Most current CSS still uses the
    // legacy names below, but these give new UI work a richer stable contract.
    alias_from_any(&mut vars, "--app-bg", &["--background"]);
    alias_from_any(&mut vars, "--workspace-bg", &["--background"]);
    alias_from_any(&mut vars, "--document-bg", &["--background"]);
    alias_from_any(&mut vars, "--document-fg", &["--foreground"]);
    alias_from_any(
        &mut vars,
        "--chrome-bg",
        &["--secondary", "--muted", "--card"],
    );
    alias_from_any(
        &mut vars,
        "--chrome-fg",
        &["--secondary-foreground", "--foreground"],
    );
    alias_from_any(&mut vars, "--chrome-border", &["--border"]);
    alias_from_any(&mut vars, "--panel-bg", &["--card", "--popover"]);
    alias_from_any(
        &mut vars,
        "--panel-fg",
        &["--card-foreground", "--popover-foreground"],
    );
    alias_from_any(&mut vars, "--panel-border", &["--border"]);
    alias_from_any(&mut vars, "--panel-muted", &["--muted"]);
    alias_from_any(&mut vars, "--elevated-bg", &["--popover", "--card"]);
    alias_from_any(
        &mut vars,
        "--elevated-fg",
        &["--popover-foreground", "--card-foreground"],
    );
    alias_from_any(&mut vars, "--elevated-border", &["--border"]);
    alias_from_any(&mut vars, "--overlay-bg", &["--popover", "--card"]);
    alias_from_any(
        &mut vars,
        "--overlay-fg",
        &["--popover-foreground", "--card-foreground"],
    );
    alias_from_any(&mut vars, "--overlay-border", &["--border"]);
    alias_from_any(&mut vars, "--control-bg", &["--background"]);
    alias_from_any(&mut vars, "--control-fg", &["--foreground"]);
    alias_from_any(
        &mut vars,
        "--control-muted-fg",
        &["--muted-foreground", "--foreground"],
    );
    alias_from_any(&mut vars, "--control-border", &["--input", "--border"]);
    alias_from_any(&mut vars, "--control-hover", &["--accent", "--muted"]);
    alias_from_any(
        &mut vars,
        "--control-hover-fg",
        &["--accent-foreground", "--foreground"],
    );
    alias_from_any(&mut vars, "--control-active", &["--accent", "--primary"]);
    alias_from_any(
        &mut vars,
        "--control-active-fg",
        &["--accent-foreground", "--primary-foreground"],
    );
    alias_from_any(&mut vars, "--focus", &["--ring", "--primary"]);
    alias_from_any(&mut vars, "--selection", &["--accent", "--primary"]);
    alias_from_any(
        &mut vars,
        "--selection-foreground",
        &["--accent-foreground", "--primary-foreground"],
    );
    alias_from_any(&mut vars, "--link", &["--primary"]);
    alias_from_any(
        &mut vars,
        "--link-hover",
        &["--primary-bright", "--primary"],
    );
    alias_from_any(&mut vars, "--divider", &["--border"]);
    alias_from_any(
        &mut vars,
        "--scrollbar-thumb",
        &["--muted-foreground", "--border"],
    );
    alias_from_any(
        &mut vars,
        "--scrollbar-thumb-hover",
        &["--foreground", "--muted-foreground"],
    );

    alias_from_any(
        &mut vars,
        "--surface",
        &["--secondary", "--muted", "--card"],
    );
    alias(&mut vars, "--surface-foreground", "--card-foreground");
    alias_from_any(&mut vars, "--surface-0", &["--background"]);
    alias_from_any(&mut vars, "--surface-1", &["--surface", "--card"]);
    alias_from_any(&mut vars, "--surface-active", &["--selection", "--accent"]);
    alias_from_any(&mut vars, "--primary-muted", &["--primary"]);
    alias_from_any(&mut vars, "--primary-bright", &["--primary"]);
    alias(&mut vars, "--dim", "--muted-foreground");
    alias(&mut vars, "--border-dim", "--border");
    alias(&mut vars, "--success", "--primary");
    alias(&mut vars, "--success-foreground", "--primary-foreground");
    alias(&mut vars, "--warning", "--destructive");
    alias(
        &mut vars,
        "--warning-foreground",
        "--destructive-foreground",
    );
    alias(&mut vars, "--error", "--destructive");
    alias(&mut vars, "--error-foreground", "--destructive-foreground");
    alias(&mut vars, "--info", "--primary");
    alias(&mut vars, "--info-foreground", "--primary-foreground");
    alias_from_any(
        &mut vars,
        "--sidebar-bg",
        &["--secondary", "--muted", "--card"],
    );
    alias(&mut vars, "--sidebar-fg", "--card-foreground");
    alias(&mut vars, "--sidebar-border", "--border");
    alias(&mut vars, "--sidebar-item-hover", "--muted");
    alias(&mut vars, "--sidebar-item-active", "--accent");
    alias(&mut vars, "--sidebar-item-active-fg", "--accent-foreground");
    alias(&mut vars, "--sidebar-heading", "--muted-foreground");
    alias_from_any(
        &mut vars,
        "--toolbar-bg",
        &["--secondary", "--muted", "--card"],
    );
    alias(&mut vars, "--toolbar-border", "--border");
    alias(&mut vars, "--prose-body", "--foreground");
    alias(&mut vars, "--prose-heading", "--foreground");
    alias_from_any(&mut vars, "--prose-link", &["--link", "--primary"]);
    alias_from_any(
        &mut vars,
        "--prose-link-hover",
        &["--link-hover", "--primary"],
    );
    alias(&mut vars, "--prose-code", "--primary");
    alias(&mut vars, "--prose-code-bg", "--muted");
    alias(&mut vars, "--prose-pre-bg", "--card");
    alias(&mut vars, "--prose-pre-border", "--border");
    alias(&mut vars, "--prose-blockquote", "--muted-foreground");
    alias(&mut vars, "--prose-blockquote-bar", "--primary");
    alias(&mut vars, "--prose-hr", "--border");
    alias(&mut vars, "--prose-table-border", "--border");
    alias(&mut vars, "--prose-table-head-bg", "--muted");
    alias(&mut vars, "--prose-th", "--foreground");
    alias(&mut vars, "--prose-td", "--foreground");
    alias(&mut vars, "--prose-task-check", "--primary");
    alias(&mut vars, "--prose-footnote", "--muted-foreground");
    alias(&mut vars, "--kanban-bg", "--background");
    alias(&mut vars, "--kanban-column-bg", "--card");
    alias(&mut vars, "--kanban-column-border", "--border");
    alias(&mut vars, "--kanban-card-bg", "--surface");
    alias(&mut vars, "--kanban-card-border", "--border");
    alias(&mut vars, "--kanban-card-hover", "--muted");
    alias(&mut vars, "--graph-bg", "--background");
    alias_from_any(&mut vars, "--graph-node", &["--chart-1", "--primary"]);
    alias_from_any(
        &mut vars,
        "--graph-node-active",
        &["--chart-2", "--primary"],
    );
    alias_from_any(
        &mut vars,
        "--graph-node-muted",
        &["--chart-3", "--muted-foreground"],
    );
    alias_from_any(&mut vars, "--graph-edge", &["--chart-4", "--border"]);
    alias_from_any(
        &mut vars,
        "--graph-edge-active",
        &["--chart-5", "--primary"],
    );
    alias(&mut vars, "--graph-label", "--muted-foreground");
    alias(&mut vars, "--graph-label-active", "--foreground");
    alias_from_any(&mut vars, "--chart-primary", &["--chart-1", "--primary"]);
    alias_from_any(
        &mut vars,
        "--chart-secondary",
        &["--chart-2", "--secondary"],
    );
    alias_from_any(&mut vars, "--chart-tertiary", &["--chart-3", "--accent"]);
    alias_from_any(&mut vars, "--chart-quaternary", &["--chart-4", "--muted"]);
    alias_from_any(
        &mut vars,
        "--chart-quinary",
        &["--chart-5", "--destructive"],
    );
    alias_from_any(&mut vars, "--priority-low", &["--chart-3", "--muted"]);
    alias_from_any(&mut vars, "--priority-low-fg", &["--muted-foreground"]);
    alias_from_any(
        &mut vars,
        "--priority-medium",
        &["--chart-2", "--secondary"],
    );
    alias_from_any(
        &mut vars,
        "--priority-medium-fg",
        &["--secondary-foreground"],
    );
    alias_from_any(&mut vars, "--priority-high", &["--chart-4", "--warning"]);
    alias_from_any(&mut vars, "--priority-high-fg", &["--warning-foreground"]);
    alias_from_any(
        &mut vars,
        "--priority-critical",
        &["--chart-5", "--destructive"],
    );
    alias_from_any(
        &mut vars,
        "--priority-critical-fg",
        &["--destructive-foreground"],
    );
    alias(&mut vars, "--bg", "--background");
    alias(&mut vars, "--fg", "--foreground");
    alias(&mut vars, "--text", "--foreground");
    alias(&mut vars, "--text-muted", "--muted-foreground");
    alias(&mut vars, "--text-error", "--error");
    alias(&mut vars, "--bg-canvas", "--background");
    alias_from_any(
        &mut vars,
        "--bg-elevated",
        &["--secondary", "--muted", "--card"],
    );
    alias(&mut vars, "--bg-cell", "--card");
    vars
}

fn alias(vars: &mut BTreeMap<String, String>, target: &str, source: &str) {
    if vars.contains_key(target) {
        return;
    }
    if let Some(value) = vars.get(source).cloned() {
        vars.insert(target.to_string(), value);
    }
}

fn alias_from_any(vars: &mut BTreeMap<String, String>, target: &str, sources: &[&str]) {
    if vars.contains_key(target) {
        return;
    }
    for source in sources {
        if let Some(value) = vars.get(*source).cloned() {
            vars.insert(target.to_string(), value);
            return;
        }
    }
}

fn inline_vars(theme: &UiTheme) -> String {
    theme
        .vars
        .iter()
        .map(|(key, value)| format!("{key}: {value};"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_var_name(name: &str) -> String {
    let trimmed = name.trim();
    let without_prefix = trimmed.strip_prefix("--").unwrap_or(trimmed);
    if without_prefix.is_empty()
        || !without_prefix
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        String::new()
    } else {
        format!("--{without_prefix}")
    }
}

fn sanitize_id(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn is_safe_css_value(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && !value.contains(';')
        && !value.contains('{')
        && !value.contains('}')
        && !value.to_ascii_lowercase().contains("javascript:")
        && !value.to_ascii_lowercase().contains("expression(")
        && !value.to_ascii_lowercase().contains("url(")
        && !value.to_ascii_lowercase().contains("@import")
}

fn normalize_css_value(key: &str, value: &str) -> String {
    let value = value.trim();
    if key.contains("radius")
        || key.contains("font")
        || key.contains("space")
        || key.contains("duration")
        || key.contains("width")
        || key.contains("height")
        || key.contains("shadow")
        || value.starts_with('#')
        || value.contains('(')
        || value.starts_with("var(")
    {
        return value.to_string();
    }

    if value.contains('%')
        && value
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '%' | ' ' | '/' | '-'))
    {
        return format!("hsl({value})");
    }

    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_themes_include_alpharius_and_presets() {
        let themes = bundled_themes();
        assert!(themes.iter().any(|theme| theme.id == "alpharius"));
        assert!(themes.iter().any(|theme| theme.id == "light"));
        assert!(themes.iter().any(|theme| theme.id == "modern-minimal"));
        assert!(themes.iter().any(|theme| theme.id == "catppuccin"));
    }

    #[test]
    fn bundled_themes_follow_picker_order() {
        let ids = bundled_themes()
            .into_iter()
            .take(6)
            .map(|theme| theme.id)
            .collect::<Vec<_>>();

        assert_eq!(
            ids,
            vec![
                "alpharius".to_string(),
                "light".to_string(),
                "modern-minimal".to_string(),
                "catppuccin".to_string(),
                "graphite".to_string(),
                "cyberpunk".to_string()
            ]
        );
    }

    #[test]
    fn imported_tweakcn_theme_normalizes_aliases() {
        let theme = import_tweakcn_theme(
            r##"{
              "name": "Custom",
              "vars": {
                "--background": "#111111",
                "--foreground": "#eeeeee",
                "--card": "#222222",
                "--card-foreground": "#eeeeee",
                "--primary": "#ff00aa",
                "--primary-foreground": "#111111",
                "--border": "#333333",
                "--muted": "#202020",
                "--muted-foreground": "#999999",
                "--destructive": "#ff3333",
                "--destructive-foreground": "#ffffff",
                "--radius": "8px"
              }
            }"##,
        )
        .unwrap();

        assert_eq!(theme.id, "custom");
        assert_eq!(theme.vars.get("--surface").unwrap(), "#202020");
        assert_eq!(theme.vars.get("--sidebar-bg").unwrap(), "#202020");
        assert!(theme.vars.contains_key("--space-3"));
        assert!(theme.vars.contains_key("--toolbar-height"));
    }

    #[test]
    fn unsafe_css_values_are_dropped() {
        let theme = import_tweakcn_theme(
            r##"{
              "name": "Custom",
              "vars": {
                "--background": "#111111",
                "--foreground": "red; body { display: none }",
                "--card": "url(https://example.com/card.png)"
              }
            }"##,
        )
        .unwrap();

        assert!(theme.vars.contains_key("--background"));
        assert_eq!(theme.vars.get("--foreground").unwrap(), "#c4d8e4");
        assert_eq!(theme.vars.get("--card").unwrap(), "#0e1622");
    }

    #[test]
    fn imported_tweakcn_theme_accepts_nested_css_vars() {
        let theme = import_tweakcn_theme(
            r##"{
              "name": "Nested",
              "cssVars": {
                "light": {
                  "background": "#ffffff",
                  "foreground": "#111111"
                },
                "dark": {
                  "background": "#050505",
                  "foreground": "#eeeeee",
                  "card": "#111111"
                }
              }
            }"##,
        )
        .unwrap();

        assert_eq!(theme.id, "nested");
        assert_eq!(theme.vars.get("--background").unwrap(), "#050505");
        assert_eq!(theme.vars.get("--surface").unwrap(), "#111111");
        assert!(theme.vars.contains_key("--font-sans"));
    }

    #[test]
    fn imported_tweakcn_registry_item_merges_theme_and_mode_vars() {
        let theme = import_tweakcn_theme(
            r##"{
              "name": "cyberpunk",
              "title": "Cyberpunk",
              "description": "A theme based on the Cyberpunk color palette.",
              "cssVars": {
                "theme": {
                  "font-sans": "Orbitron, sans-serif",
                  "radius": "0.125rem"
                },
                "light": {
                  "background": "#ffffff",
                  "foreground": "#111111"
                },
                "dark": {
                  "background": "#050505",
                  "foreground": "#39ff14",
                  "card": "#101010"
                }
              }
            }"##,
        )
        .unwrap();

        assert_eq!(theme.id, "cyberpunk");
        assert_eq!(theme.name, "Cyberpunk");
        assert_eq!(
            theme.vars.get("--font-sans").unwrap(),
            "Orbitron, sans-serif"
        );
        assert_eq!(theme.vars.get("--radius").unwrap(), "0.125rem");
        assert_eq!(theme.vars.get("--background").unwrap(), "#050505");
        assert_eq!(theme.vars.get("--foreground").unwrap(), "#39ff14");
        assert_eq!(theme.vars.get("--surface").unwrap(), "#101010");
    }

    #[test]
    fn imported_tweakcn_theme_wraps_hsl_channels() {
        let theme = import_tweakcn_theme(
            r##"{
              "name": "HSL",
              "vars": {
                "background": "222.2 84% 4.9% / 0.95",
                "foreground": "210 40% 98%",
                "radius": "0.5rem"
              }
            }"##,
        )
        .unwrap();

        assert_eq!(
            theme.vars.get("--background").unwrap(),
            "hsl(222.2 84% 4.9% / 0.95)"
        );
        assert_eq!(theme.vars.get("--radius").unwrap(), "0.5rem");
    }

    #[test]
    fn imported_theme_ids_do_not_shadow_builtins() {
        let settings = FlyntOperatorSettings::default();
        let mut library = ThemeLibrary::from_operator(&settings);
        let theme = import_tweakcn_theme(
            r##"{
              "id": "light",
              "name": "Light",
              "vars": {
                "background": "#111111",
                "foreground": "#eeeeee"
              }
            }"##,
        )
        .unwrap();

        let id = library.upsert_imported(theme);

        assert_eq!(id, "custom-light");
        assert!(library.theme("light").unwrap().builtin);
        assert!(!library.theme("custom-light").unwrap().builtin);
    }

    #[test]
    fn theme_locator_builds_public_tweakcn_registry_candidates() {
        let direct = theme_url_candidates("https://tweakcn.com/themes/cyberpunk").unwrap();
        assert_eq!(direct[0], "https://tweakcn.com/r/themes/cyberpunk.json");
        assert_eq!(direct[1], "https://tweakcn.com/themes/cyberpunk");

        let slug = theme_url_candidates("catppuccin").unwrap();
        assert_eq!(slug[0], "https://tweakcn.com/r/themes/catppuccin.json");
        assert_eq!(slug.len(), 1);
    }

    #[test]
    fn theme_locator_does_not_treat_user_handles_as_theme_slugs() {
        let err = theme_url_candidates("@cwilson613").unwrap_err();
        assert!(err.to_string().contains("profile/user theme import"));

        let by_id = theme_url_candidates("cmll14cgf000204ky5ms2fdgj").unwrap();
        assert_eq!(
            by_id[0],
            "https://tweakcn.com/r/themes/cmll14cgf000204ky5ms2fdgj.json"
        );
    }

    #[test]
    fn bundled_light_theme_separates_chrome_from_document_canvas() {
        let library = ThemeLibrary::from_operator(&FlyntOperatorSettings::default());
        let theme = library.theme("light").unwrap();

        assert_eq!(theme.vars.get("--background").unwrap(), "#ffffff");
        assert_eq!(theme.vars.get("--card").unwrap(), "#ffffff");
        assert_eq!(theme.vars.get("--surface").unwrap(), "#f3f4f6");
        assert_eq!(theme.vars.get("--sidebar-bg").unwrap(), "#f3f4f6");
        assert_eq!(theme.vars.get("--toolbar-bg").unwrap(), "#f3f4f6");
    }

    #[test]
    fn imported_theme_maps_broad_tweakcn_surface_contract() {
        let theme = import_tweakcn_theme(
            r##"{
              "name": "Broad",
              "vars": {
                "background": "#ffffff",
                "foreground": "#111111",
                "card": "#fefefe",
                "card-foreground": "#111111",
                "popover": "#fafafa",
                "popover-foreground": "#111111",
                "secondary": "#f2f4f7",
                "secondary-foreground": "#111827",
                "muted": "#eef1f5",
                "muted-foreground": "#667085",
                "accent": "#e6f0ff",
                "accent-foreground": "#12315f",
                "border": "#d0d5dd",
                "input": "#cbd5e1",
                "ring": "#2563eb",
                "primary": "#1d4ed8",
                "primary-foreground": "#ffffff",
                "destructive": "#dc2626",
                "destructive-foreground": "#ffffff",
                "sidebar": "#f8fafc",
                "sidebar-foreground": "#0f172a",
                "sidebar-accent": "#e2e8f0",
                "sidebar-accent-foreground": "#0f172a",
                "sidebar-primary": "#1d4ed8",
                "sidebar-primary-foreground": "#ffffff",
                "sidebar-border": "#cbd5e1",
                "sidebar-ring": "#60a5fa",
                "chart-1": "#2563eb",
                "chart-2": "#16a34a",
                "chart-3": "#f59e0b",
                "chart-4": "#8b5cf6",
                "chart-5": "#ef4444",
                "radius": "0.625rem"
              }
            }"##,
        )
        .unwrap();

        assert_eq!(theme.vars.get("--sidebar-bg").unwrap(), "#f8fafc");
        assert_eq!(theme.vars.get("--sidebar-fg").unwrap(), "#0f172a");
        assert_eq!(theme.vars.get("--sidebar-item-active").unwrap(), "#e2e8f0");
        assert_eq!(theme.vars.get("--sidebar-ring").unwrap(), "#60a5fa");
        assert_eq!(theme.vars.get("--panel-bg").unwrap(), "#fefefe");
        assert_eq!(theme.vars.get("--overlay-bg").unwrap(), "#fafafa");
        assert_eq!(theme.vars.get("--control-border").unwrap(), "#cbd5e1");
        assert_eq!(theme.vars.get("--focus").unwrap(), "#2563eb");
        assert_eq!(theme.vars.get("--graph-node").unwrap(), "#2563eb");
        assert_eq!(theme.vars.get("--graph-node-active").unwrap(), "#16a34a");
        assert_eq!(theme.vars.get("--graph-edge").unwrap(), "#8b5cf6");
        assert_eq!(theme.vars.get("--priority-critical").unwrap(), "#ef4444");
        assert_eq!(theme.vars.get("--radius-lg").unwrap(), "0.625rem");
    }

    #[test]
    fn invalid_variable_names_are_dropped() {
        let theme = import_tweakcn_theme(
            r##"{
              "name": "Bad Key",
              "vars": {
                "background; color:red": "#111111",
                "foreground": "#eeeeee"
              }
            }"##,
        )
        .unwrap();

        assert!(!theme.vars.contains_key("--background; color:red"));
        assert_eq!(theme.vars.get("--foreground").unwrap(), "#eeeeee");
    }
}
