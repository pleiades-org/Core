use super::{Action, ResultKind, SearchResult};

/// Words that surface the taskbar result when typed on their own.
const KEYWORDS: [&str; 2] = ["taskbar", "tb"];
pub(super) const MESSAGE: &str = "Enter to show the Windows taskbar · Esc to hide";

/// Exact keyword match only, so longer queries such as `tbs` keep normal application ranking.
pub(super) fn matches_keyword(text: &str) -> bool {
    KEYWORDS
        .iter()
        .any(|keyword| text.eq_ignore_ascii_case(keyword))
}

/// Replaces the Windows-key tap for auto-hidden taskbars when Core owns that key.
pub(super) fn taskbar_result() -> SearchResult {
    SearchResult {
        kind: ResultKind::System,
        id: "taskbar".into(),
        title: "Show taskbar".into(),
        description: "Reveal and focus the Windows taskbar on this display".into(),
        action: Action::RevealTaskbar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keywords_match_whole_words_case_insensitively() {
        assert!(matches_keyword("taskbar"));
        assert!(matches_keyword("TB"));
        assert!(!matches_keyword("tbs"));
        assert!(!matches_keyword("task"));
        assert!(!matches_keyword(""));
    }
}
