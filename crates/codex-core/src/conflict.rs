//! Git merge conflict detection and resolution helpers.

/// Check if file content contains git merge conflict markers.
pub fn has_conflict_markers(content: &str) -> bool {
    content.contains("<<<<<<<") && content.contains("=======") && content.contains(">>>>>>>")
}

/// A parsed conflict region.
#[derive(Debug, Clone)]
pub struct ConflictRegion {
    /// "Ours" (current branch) content.
    pub ours: String,
    /// "Theirs" (incoming) content.
    pub theirs: String,
    /// The conflict marker labels (e.g., "HEAD", "main").
    pub ours_label: String,
    pub theirs_label: String,
}

/// Parse all conflict regions from file content.
pub fn parse_conflicts(content: &str) -> Vec<ConflictRegion> {
    let mut regions = Vec::new();
    let mut remaining = content;

    while let Some(start) = remaining.find("<<<<<<<") {
        let after_marker = &remaining[start..];
        let ours_label = after_marker
            .lines()
            .next()
            .unwrap_or("")
            .trim_start_matches('<')
            .trim()
            .to_string();

        let Some(separator) = after_marker.find("=======") else { break };
        let Some(end_marker) = after_marker.find(">>>>>>>") else { break };

        let ours_start = after_marker.find('\n').map(|i| i + 1).unwrap_or(0);
        let ours = after_marker[ours_start..separator].to_string();
        let theirs_start = separator + "=======\n".len();
        let theirs_end = end_marker;
        let theirs = after_marker[theirs_start..theirs_end].to_string();

        let theirs_label = after_marker[end_marker..]
            .lines()
            .next()
            .unwrap_or("")
            .trim_start_matches('>')
            .trim()
            .to_string();

        regions.push(ConflictRegion {
            ours: ours.trim_end().to_string(),
            theirs: theirs.trim_end().to_string(),
            ours_label,
            theirs_label,
        });

        let end_line = after_marker[end_marker..].find('\n').unwrap_or(after_marker.len() - end_marker);
        remaining = &after_marker[end_marker + end_line..];
    }

    regions
}

/// Resolve all conflicts in content by picking "ours" for every region.
pub fn resolve_ours(content: &str) -> String {
    resolve_with(content, |region| region.ours.clone())
}

/// Resolve all conflicts in content by picking "theirs" for every region.
pub fn resolve_theirs(content: &str) -> String {
    resolve_with(content, |region| region.theirs.clone())
}

/// Resolve conflicts using a custom picker function.
fn resolve_with(content: &str, picker: impl Fn(&ConflictRegion) -> String) -> String {
    let mut result = String::with_capacity(content.len());
    let mut remaining = content;

    while let Some(start) = remaining.find("<<<<<<<") {
        result.push_str(&remaining[..start]);

        let after_marker = &remaining[start..];
        let Some(separator) = after_marker.find("=======") else {
            result.push_str(after_marker);
            return result;
        };
        let Some(end_marker) = after_marker.find(">>>>>>>") else {
            result.push_str(after_marker);
            return result;
        };

        let ours_label = after_marker.lines().next().unwrap_or("").trim_start_matches('<').trim().to_string();
        let ours_start = after_marker.find('\n').map(|i| i + 1).unwrap_or(0);
        let ours = after_marker[ours_start..separator].trim_end().to_string();
        let theirs_start = separator + "=======\n".len();
        let theirs = after_marker[theirs_start..end_marker].trim_end().to_string();
        let theirs_label = after_marker[end_marker..].lines().next().unwrap_or("").trim_start_matches('>').trim().to_string();

        let region = ConflictRegion { ours, theirs, ours_label, theirs_label };
        result.push_str(&picker(&region));
        result.push('\n');

        let end_line = after_marker[end_marker..].find('\n').map(|i| i + 1).unwrap_or(after_marker.len() - end_marker);
        remaining = &after_marker[end_marker + end_line..];
    }

    result.push_str(remaining);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFLICTED: &str = "# Title\n\nSome text before.\n\n<<<<<<< HEAD\nMy version of this paragraph.\n=======\nTheir version of this paragraph.\n>>>>>>> main\n\nText after.\n";

    #[test]
    fn detects_conflicts() {
        assert!(has_conflict_markers(CONFLICTED));
        assert!(!has_conflict_markers("# Normal note\n\nNo conflicts here.\n"));
    }

    #[test]
    fn parses_conflict_regions() {
        let regions = parse_conflicts(CONFLICTED);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].ours, "My version of this paragraph.");
        assert_eq!(regions[0].theirs, "Their version of this paragraph.");
    }

    #[test]
    fn resolves_ours() {
        let resolved = resolve_ours(CONFLICTED);
        assert!(!has_conflict_markers(&resolved));
        assert!(resolved.contains("My version"));
        assert!(!resolved.contains("Their version"));
        assert!(resolved.contains("Text after"));
    }

    #[test]
    fn resolves_theirs() {
        let resolved = resolve_theirs(CONFLICTED);
        assert!(!has_conflict_markers(&resolved));
        assert!(resolved.contains("Their version"));
        assert!(!resolved.contains("My version"));
    }
}
