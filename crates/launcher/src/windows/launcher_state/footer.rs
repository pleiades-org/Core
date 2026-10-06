use super::LauncherState;
use core_engine::{
    media::MediaCommand,
    search::{Action, PlayMode, RunMode, SongStatus},
};

impl LauncherState {
    /// Keep the footer tied to the selected action while preserving errors and run status.
    pub fn refresh_footer(&self) {
        let Some(view) = &self.view else { return };
        let query = self.acted_query();
        if let Some(result) = self.batch.results.get(view.selected()) {
            if let Some(message) = self.songs.playback_notice(&query, &result.id) {
                view.set_footer(message);
                return;
            }
        }
        if query.trim().eq_ignore_ascii_case("@update") {
            view.set_footer(&self.update_hint());
            return;
        }
        if core_engine::search::wants_info(&query) {
            match self
                .batch
                .results
                .get(view.selected())
                .map(|row| &row.action)
            {
                Some(Action::Update) => view.set_footer(&self.update_hint()),
                Some(Action::OpenUrl(_)) => view.set_footer("Enter to open the release notes"),
                Some(Action::CopyText(_)) => view.set_footer("Enter to copy the version"),
                _ => view.set_footer(self.batch.message),
            }
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
            match &result.action {
                // The buttons beside a playlist, album or artist change what Enter does.
                Action::PlayCollection(_) => play_hint(view.play_mode(), view.row_takes_arrows()),
                action => action_hint(action),
            }
        } else if self.batch.message.starts_with("Enter to open") {
            "No matching applications"
        } else {
            self.batch.message
        };
        view.set_footer(message);
    }
}

/// What Enter does to the selected playlist, album or artist: from its row, or from the
/// button beside it that Right moved onto. `among_rows`: a row was picked, so Right leads to
/// the buttons; while the person still types it would only move the caret.
fn play_hint(mode: PlayMode, among_rows: bool) -> &'static str {
    match (mode, among_rows) {
        (PlayMode::AsItIs, false) => SongStatus::Ready.message(),
        (PlayMode::AsItIs, true) => "Enter to play · → shuffle or repeat",
        (PlayMode::Shuffled, _) => "Enter to play shuffled · ← back",
        (PlayMode::Looped, _) => "Enter to play on repeat · ← back",
    }
}

fn action_hint(action: &Action) -> &'static str {
    match action {
        Action::PlaySong(_) => "Enter to play in Spotify",
        Action::PlayCollection(_) => play_hint(PlayMode::AsItIs, false),
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
            MediaCommand::Shuffle => "Enter to turn shuffle on or off",
            MediaCommand::Repeat => "Enter to change repeat",
        },
        Action::Mixer { level, .. } if level.muted => "← → volume · Enter to unmute",
        Action::Mixer { .. } => "← → volume · Enter to mute",
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

    #[test]
    fn a_playlist_says_what_enter_does_from_its_row_and_from_each_button() {
        let hints: Vec<&str> = [PlayMode::AsItIs, PlayMode::Shuffled, PlayMode::Looped]
            .into_iter()
            .map(|mode| play_hint(mode, true))
            .collect();
        assert_eq!(
            hints,
            [
                "Enter to play · → shuffle or repeat",
                "Enter to play shuffled · ← back",
                "Enter to play on repeat · ← back"
            ]
        );
        // While the person types, Right is the caret's: the hint does not promise the buttons.
        assert_eq!(
            play_hint(PlayMode::AsItIs, false),
            "Enter to play in Spotify · ↑ ↓ to select"
        );
    }

    #[test]
    fn a_mixer_row_says_what_enter_does_to_it() {
        use core_engine::media::VolumeLevel;
        let row = |muted: bool| Action::Mixer {
            app: "game".into(),
            level: VolumeLevel::new(40, muted),
        };
        assert_eq!(action_hint(&row(false)), "← → volume · Enter to mute");
        assert_eq!(action_hint(&row(true)), "← → volume · Enter to unmute");
    }
}
