use super::{Action, ResultKind, SearchBatch, SearchResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerAction {
    ShutDown,
    Restart,
    Sleep,
}

impl PowerAction {
    pub const ALL: [Self; 3] = [Self::ShutDown, Self::Restart, Self::Sleep];
    pub fn label(self) -> &'static str {
        match self {
            Self::ShutDown => "Power off",
            Self::Restart => "Restart",
            Self::Sleep => "Sleep",
        }
    }
    pub fn command(self) -> &'static str {
        match self {
            Self::ShutDown => "shutdown",
            Self::Restart => "restart",
            Self::Sleep => "sleep",
        }
    }
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| {
            text.eq_ignore_ascii_case(action.command()) || text.eq_ignore_ascii_case(action.label())
        })
    }
}

pub(super) fn power_results(payload: &str) -> SearchBatch {
    if let Some(command) = payload.strip_prefix("confirm ") {
        if let Some(action) = PowerAction::parse(command) {
            return SearchBatch {
                results: vec![
                    SearchResult {
                        kind: ResultKind::Power,
                        id: "power-cancel".into(),
                        title: "Cancel".into(),
                        description: "Return to search".into(),
                        action: Action::FillQuery("".into()),
                    },
                    SearchResult {
                        kind: ResultKind::Power,
                        id: "power-confirm".into(),
                        title: format!("Confirm: {}", action.label()).into(),
                        description: "Save your work before continuing".into(),
                        action: Action::Power(action),
                    },
                ],
                message: "↓ then Enter to confirm · Cancel is selected by default",
            };
        }
    }
    let results = PowerAction::ALL
        .into_iter()
        .filter(|action| payload.is_empty() || PowerAction::parse(payload) == Some(*action))
        .map(|action| SearchResult {
            kind: ResultKind::Power,
            id: format!("power-{}", action.command()).into(),
            title: action.label().into(),
            description: "Show confirmation".into(),
            action: Action::FillQuery(format!("@power confirm {}", action.command()).into()),
        })
        .collect();
    SearchBatch {
        results,
        message: "Choose Power off, Restart or Sleep · confirmation required",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn power_actions_require_a_separate_selection_and_default_to_cancel() {
        for action in PowerAction::ALL {
            let initial = power_results(action.command());
            assert!(matches!(initial.results[0].action, Action::FillQuery(_)));
            let confirmation = power_results(&format!("confirm {}", action.command()));
            assert_eq!(confirmation.results[0].action, Action::FillQuery("".into()));
            assert_eq!(confirmation.results[1].action, Action::Power(action));
        }
    }
}
