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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_note_path_format() {
        let date = NaiveDate::from_ymd_opt(2026, 4, 20).unwrap();
        let path = daily_note_path(date);
        assert_eq!(path.to_string_lossy(), "Daily/2026-04-20.md");
    }

    #[test]
    fn daily_note_path_leap_year() {
        let date = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
        let path = daily_note_path(date);
        assert_eq!(path.to_string_lossy(), "Daily/2024-02-29.md");
    }

    #[test]
    fn daily_note_content_default_template() {
        let date = NaiveDate::from_ymd_opt(2026, 4, 20).unwrap();
        let content = daily_note_content(date, None);
        assert!(content.contains("Monday, April 20, 2026"));
        assert!(content.contains("tags = [\"daily\"]"));
        assert!(content.contains("2026-04-20"));
        assert!(content.contains("## Tasks"));
    }

    #[test]
    fn daily_note_content_custom_template() {
        let date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let content = daily_note_content(date, Some("# {{title}}\nDate: {{date}}"));
        assert!(content.contains("# Thursday, January 1, 2026"));
        assert!(content.contains("Date: 2026-01-01"));
    }

    #[test]
    fn expand_template_replaces_all_vars() {
        let result = expand_template("{{title}} on {{date}}", "My Title", "2026-04-20");
        assert!(result.contains("My Title"));
        assert!(result.contains("2026-04-20"));
    }

    #[test]
    fn expand_template_no_placeholders_passthrough() {
        let result = expand_template("plain text", "title", "date");
        assert_eq!(result, "plain text");
    }

    #[test]
    fn today_returns_local_date() {
        let t = today();
        let now = Local::now().date_naive();
        assert_eq!(t, now);
    }
}
