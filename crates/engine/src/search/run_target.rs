//! Windows Run-dialog targets: `@run notepad`, and without a prefix anything that cannot be an
//! application name: `shell:startup`, `ms-settings:display`, `%appdata%`, `C:\Users`, `\\nas`.
use super::{Action, ResultKind, SearchBatch, SearchResult};

const URI_PREFIXES: &[&str] = &["shell:", "ms-settings:"];
const MESSAGE: &str = "Enter opens like Windows Run · Esc to hide";

/// Recognised by syntax alone; search never checks whether the target exists.
pub(super) fn is_run_target(text: &str) -> bool {
    let text = text.trim();
    let bytes = text.as_bytes();
    let uri = URI_PREFIXES.iter().any(|prefix| {
        text.get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
            && text.len() > prefix.len()
    });
    let variable = text.starts_with('%') && text[1..].contains('%') && text.len() > 2;
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    let network = text.starts_with("\\\\") && text.len() > 2;
    uri || variable || drive || network
}

/// `explicit` (typed after `@run`) adds an administrator row for programs and scripts.
pub(super) fn run_results(target: &str, explicit: bool) -> SearchBatch {
    let target = target.trim();
    if target.is_empty() {
        return SearchBatch {
            results: Vec::new(),
            message: "Type a program, folder, document or URI after @run",
        };
    }
    let row = |elevated: bool, title: String, id: &str| SearchResult {
        kind: ResultKind::Terminal,
        id: id.into(),
        title: title.into(),
        description: if elevated {
            format!("{target} · after the Windows administrator prompt").into()
        } else {
            "Open as if typed in Windows Run (Win+R)".into()
        },
        action: Action::OpenRunTarget {
            target: target.into(),
            elevated,
        },
    };
    let mut results = vec![row(false, format!("Open {target}"), "run-open")];
    if explicit {
        results.push(row(true, "Run as administrator".into(), "run-admin"));
    }
    SearchBatch {
        results,
        message: MESSAGE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_unambiguous_targets_are_recognised_without_a_prefix() {
        for target in [
            "shell:startup",
            "MS-SETTINGS:display",
            "%appdata%",
            "%USERPROFILE%\\Downloads",
            "C:\\Users",
            "d:/games",
            "\\\\nas\\share",
        ] {
            assert!(is_run_target(target), "{target}");
        }
        for text in [
            "notepad",
            "shell:",
            "%",
            "%x",
            "C:",
            "xbox",
            "25% of 80",
            "c:drive",
            "\\\\",
        ] {
            assert!(!is_run_target(text), "{text}");
        }
    }

    #[test]
    fn explicit_runs_offer_administrator_rights() {
        let batch = run_results(" notepad ", true);
        assert_eq!(
            batch.results[0].action,
            Action::OpenRunTarget {
                target: "notepad".into(),
                elevated: false
            }
        );
        assert_eq!(
            batch.results[1].action,
            Action::OpenRunTarget {
                target: "notepad".into(),
                elevated: true
            }
        );
        assert_eq!(run_results("shell:startup", false).results.len(), 1);
        assert!(run_results("", true).results.is_empty());
    }
}
