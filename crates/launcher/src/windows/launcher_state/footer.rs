use super::LauncherState;
use core_engine::{
    media::MediaCommand,
    search::{Action, RunMode},
};

impl LauncherState {
    /// Keep the footer tied to the selected action while preserving errors and run status.
    pub fn refresh_footer(&self) {
        let Some(view) = &self.view else { return };
        if view.query().trim().eq_ignore_ascii_case("@update") {
            view.set_footer(&self.update_hint());
            return;
        }
        if view.query().is_empty() {
            if let Some(notice) = self.update_notice() {
                view.set_footer(&notice);
                return;
            }
        }
        if view.output_visible() {
            if let Some(status) = self.command_status() {
                view.set_footer(&status);
                return;
            }
        }
        let message = if !self.catalog_ready && view.query().is_empty() {
            "Discovering applications…"
        } else if let Some(result) = self.batch.results.get(view.selected()) {
            action_hint(&result.action)
        } else if self.batch.message.starts_with("Enter to open") {
            "No matching applications"
        } else {
            self.batch.message
        };
        view.set_footer(message);
    }
}

fn action_hint(action: &Action) -> &'static str {
    match action {
        Action::CopyText(_) => "Enter to copy",
        Action::LaunchApplication(_) | Action::OpenQuicklink(_) => "Enter to open",
        Action::OpenUrl(_) => "Enter to search",
        Action::FillQuery(query) if query.is_empty() => "Enter to cancel",
        Action::FillQuery(query) if query.starts_with("@power confirm ") => "Enter to review",
        Action::FillQuery(_) => "Enter to select",
        Action::Power(_) => "Enter to confirm",
        Action::RevealTaskbar => "Enter to show taskbar",
        Action::Update => "Enter to check for updates",
        Action::RunCommand {
            mode: RunMode::Elevated,
            ..
        }
        | Action::OpenRunTarget { elevated: true, .. } => "Enter to run as admin",
        Action::RunCommand {
            mode: RunMode::Terminal,
            ..
        } => "Enter to open terminal",
        Action::RunCommand { .. } | Action::OpenRunTarget { .. } => "Enter to run",
        Action::Media { command, .. } => match command {
            MediaCommand::TogglePlayPause => "Enter to play or pause",
            MediaCommand::Play => "Enter to play",
            MediaCommand::Pause => "Enter to pause",
            MediaCommand::Next => "Enter for the next track",
            MediaCommand::Previous => "Enter for the previous track",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_engine::{applications::ApplicationCatalog, search::SearchEngine};

    #[test]
    fn hints_follow_the_selected_action_in_mixed_result_lists() {
        let mut engine = SearchEngine::default();
        let catalog = ApplicationCatalog::default();
        let confirmation = engine.search("@power confirm restart", &catalog);
        assert_eq!(
            action_hint(&confirmation.results[0].action),
            "Enter to cancel"
        );
        assert_eq!(
            action_hint(&confirmation.results[1].action),
            "Enter to confirm"
        );
        let command = engine.search("/echo hello", &catalog);
        let hints: Vec<_> = command
            .results
            .iter()
            .map(|result| action_hint(&result.action))
            .collect();
        assert_eq!(
            hints,
            [
                "Enter to run",
                "Enter to open terminal",
                "Enter to run as admin"
            ]
        );
    }
}
