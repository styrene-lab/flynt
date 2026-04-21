//! Daily notes — date-indexed periodic notes.

use chrono::{Local, NaiveDate};
use std::path::PathBuf;

/// Generate the relative path for a daily note: `Daily/YYYY-MM-DD.md`
pub fn daily_note_path(date: NaiveDate) -> PathBuf {
    PathBuf::from(format!("Daily/{}.md", date.format("%Y-%m-%d")))
}

/// Generate the content for a new daily note using a template.
pub fn daily_note_content(date: NaiveDate, template: Option<&str>) -> String {
    let title = date.format("%A, %B %-d, %Y").to_string(); // "Friday, April 19, 2026"
    let date_str = date.format("%Y-%m-%d").to_string();

    if let Some(tmpl) = template {
        expand_template(tmpl, &title, &date_str)
    } else {
        format!(
            "+++\ntitle = \"{title}\"\ntags = [\"daily\"]\ndate = \"{date_str}\"\n+++\n\n# {title}\n\n## Tasks\n\n- [ ] \n\n## Notes\n\n"
        )
    }
}

/// Today's date.
pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// Expand template variables.
pub fn expand_template(template: &str, title: &str, date: &str) -> String {
    template
        .replace("{{title}}", title)
        .replace("{{date}}", date)
        .replace("{{time}}", &Local::now().format("%H:%M").to_string())
        .replace("{{year}}", &Local::now().format("%Y").to_string())
        .replace("{{month}}", &Local::now().format("%m").to_string())
        .replace("{{day}}", &Local::now().format("%d").to_string())
        .replace("{{weekday}}", &Local::now().format("%A").to_string())
}
