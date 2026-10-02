//! Settings page lifecycle and auto-save: edits preview immediately, typing is debounced,
//! and one background writer persists the latest valid draft.
use super::LauncherState;
use crate::windows::settings::{page::SettingsAction, SettingsDocument};
use windows::Win32::{Foundation::HWND, UI::Input::KeyboardAndMouse::SetFocus};

impl LauncherState {
    pub fn open_settings(&mut self, window: HWND) {
        let Some(view) = self.view.clone() else {
            return;
        };
        if view.settings_open() {
            self.set_visible(window, true);
            return;
        }
        self.auto_save.error = None;
        self.auto_save.done_when_saved = false;
        let music_apps = self.music_app_choices();
        if let Err(error) =
            view.open_settings(self.auto_save.latest(&self.settings.saved), &music_apps)
        {
            view.set_footer(&format!("Could not open settings: {error}"));
            return;
        }
        self.pending_accept = None;
        self.cancel_background_work();
        self.set_visible(window, true);
        view.spotify_status(&crate::windows::spotify::connection_status(
            self.settings.folder(),
            &self.auto_save.latest(&self.settings.saved).music.spotify,
        ));
        if self.settings.is_saving() {
            view.settings_status("Saving changes…");
        }
        if let Some(warning) = &self.settings.warning {
            view.settings_status(warning);
        }
    }

    pub fn dismiss_settings(&mut self, window: HWND) {
        self.flush_settings(window);
        self.leave_settings();
    }

    fn leave_settings(&mut self) {
        let Some(view) = self.view.clone() else {
            return;
        };
        self.resume_shortcuts(self.window);
        self.auto_save.done_when_saved = false;
        if let Err(error) =
            view.close_settings(self.auto_save.latest(&self.settings.saved).preferences)
        {
            view.set_footer(&format!("Could not restore appearance: {error}"));
        }
        unsafe {
            let _ = SetFocus(Some(view.input));
        }
        self.refresh_media_bar(crate::windows::view::BarPresence::Free);
        self.queue_search();
    }

    pub fn settings_command(&mut self, window: HWND, identifier: usize, notification: u32) {
        let Some(view) = self.view.clone() else {
            return;
        };
        match view.settings_command(identifier, notification) {
            SettingsAction::Edit => {
                self.auto_save.done_when_saved = false;
                self.change_settings(window, true);
            }
            SettingsAction::EditQuicklink(row) => {
                self.auto_save.done_when_saved = false;
                self.edit_quicklink(window, row);
            }
            SettingsAction::Change | SettingsAction::Retry => {
                self.auto_save.done_when_saved = false;
                self.change_settings(window, false);
            }
            SettingsAction::Done => {
                self.auto_save.done_when_saved = true;
                self.flush_settings(window);
                self.finish_settings_if_ready();
            }
            SettingsAction::ClearHistory => match self.clear_command_history() {
                Ok(()) => view.settings_status("Command history cleared."),
                Err(error) => view.settings_status(&error),
            },
            SettingsAction::ConnectSpotify => {
                self.change_settings(window, false);
                if self.auto_save.error.is_none() {
                    self.connect_spotify();
                }
            }
            SettingsAction::DisconnectSpotify => self.disconnect_spotify(),
            SettingsAction::SpotifySetup => {
                self.pending_action = Some(crate::windows::execute_action::NativeAction::OpenUrl(
                    "https://developer.spotify.com/dashboard".into(),
                ))
            }

            SettingsAction::None => {}
        }
    }

    fn change_settings(&mut self, window: HWND, typing: bool) {
        self.auto_save.cancel_timer(window);
        self.auto_save.queued = None;
        let Some(view) = self.view.clone() else {
            return;
        };
        let mut draft = match view.settings_draft() {
            Ok(draft) => draft,
            Err(error) => {
                self.settings_error(error);
                return;
            }
        };
        // Preserve catalog identity when only appearance/behaviour changed.
        if draft.quicklinks == self.settings.saved.quicklinks {
            draft.quicklinks = self.settings.saved.quicklinks.clone();
        }
        // Only what changed is redone; the quicklink table repaints its own rows.
        if let Err(error) = view.apply_preferences(draft.preferences) {
            self.settings_error(format!("Could not apply settings: {error}"));
            return;
        }
        self.auto_save.error = None;
        view.settings_save_error(false);
        self.auto_save.queued = Some(draft);
        self.spotify_settings_changed();
        view.settings_status("Saving changes…");
        if typing && self.auto_save.defer(window) {
            return;
        }
        self.save_queued_settings(window);
    }

    /// Quicklink typing reports the edited row's problem at once. Checking every row against
    /// the saved list waits for the typing pause, whose timer flushes the whole draft.
    fn edit_quicklink(&mut self, window: HWND, row: Result<(), String>) {
        self.auto_save.cancel_timer(window);
        self.auto_save.queued = None;
        if let Err(error) = row {
            self.settings_error(error);
            return;
        }
        self.auto_save.error = None;
        if let Some(view) = &self.view {
            view.settings_save_error(false);
            view.settings_status("Saving changes…");
        }
        if !self.auto_save.defer(window) {
            self.change_settings(window, false);
        }
    }

    /// Flush a valid edit on Done, dismissal, or exit instead of losing the typing debounce.
    pub fn flush_settings(&mut self, window: HWND) {
        if self.view.as_ref().is_some_and(|view| view.settings_open()) {
            self.change_settings(window, false);
        } else {
            self.save_queued_settings(window);
        }
    }

    pub fn save_queued_settings(&mut self, window: HWND) {
        self.auto_save.cancel_timer(window);
        if self.settings.is_saving() {
            return;
        }
        let Some(draft) = self.auto_save.queued.take() else {
            return;
        };
        if draft == self.settings.saved && self.settings.warning.is_none() {
            self.finish_settings_if_ready();
            return;
        }
        if let Err(error) = self.save_preferences(window, draft.clone()) {
            self.settings_error(error);
        } else {
            self.auto_save.saving = Some(draft);
        }
    }

    fn settings_error(&mut self, error: String) {
        self.auto_save.done_when_saved = false;
        self.auto_save.error = Some(error.clone());
        if let Some(view) = &self.view {
            view.settings_status(&format!("Not saved · {error}"));
            view.settings_save_error(true);
            if !view.settings_open() {
                view.set_footer(&format!("Settings not saved · {error}"));
            }
        }
        eprintln!("Core settings: {error}");
    }

    fn finish_settings_if_ready(&mut self) {
        if self.settings.is_saving()
            || self.auto_save.queued.is_some()
            || self.auto_save.waiting_for_typing()
            || self.auto_save.error.is_some()
        {
            return;
        }
        if let Some(view) = &self.view {
            view.settings_status("All changes saved on this PC.");
        }
        if self.auto_save.done_when_saved {
            self.auto_save.done_when_saved = false;
            self.leave_settings();
        }
    }

    pub fn receive_settings(&mut self, window: HWND) {
        let Some(outcome) = self.settings.finish_save() else {
            return;
        };
        self.auto_save.saving = None;
        if let Err(error) = outcome {
            self.auto_save.cancel_timer(window);
            self.auto_save.queued = None;
            self.spotify_settings_changed();
            if let Some(view) = &self.view {
                if !view.settings_open() {
                    if let Err(restore) = view.apply_preferences(self.settings.saved.preferences) {
                        eprintln!("Could not restore saved appearance: {restore}");
                    }
                }
            }
            let saved_music = self.settings.saved.music.clone();
            if let Err(restore) = self.configure_media_hotkeys(window, &saved_music) {
                eprintln!("Could not restore media shortcuts: {restore}");
            }
            let restored =
                self.configure_shortcut(window, self.settings.saved.preferences.shortcut);
            self.settings_error(match restored {
                Ok(()) => error,
                Err(restore) => {
                    format!("{error} Previous shortcut could not be restored: {restore}")
                }
            });
            return;
        }
        if !self.auto_save.waiting_for_typing() {
            self.save_queued_settings(window);
        }
        if let Some(view) = &self.view {
            if !view.settings_open() && !self.settings.is_saving() {
                if let Err(error) = view.apply_preferences(self.settings.saved.preferences) {
                    view.set_footer(&format!("Could not apply saved settings: {error}"));
                }
            }
        }
        self.finish_settings_if_ready();
        self.music_settings_changed();
        self.update_service
            .set_mode(self.settings.saved.preferences.updates);
        if self.visible {
            self.update_service.refresh(window);
        }
        if self.visible {
            self.queue_search();
        }
    }

    fn save_preferences(&mut self, window: HWND, draft: SettingsDocument) -> Result<(), String> {
        self.configure_shortcut(window, draft.preferences.shortcut)?;
        let result = self
            .configure_media_hotkeys(window, &draft.music)
            .and_then(|()| self.settings.start_save(window, draft));
        if let Err(error) = result {
            let saved_music = self.settings.saved.music.clone();
            if let Err(restore) = self.configure_media_hotkeys(window, &saved_music) {
                eprintln!("Could not restore media shortcuts: {restore}");
            }
            if let Err(restore) =
                self.configure_shortcut(window, self.settings.saved.preferences.shortcut)
            {
                return Err(format!(
                    "{error} Previous shortcut could not be restored: {restore}"
                ));
            }
            return Err(error);
        }
        Ok(())
    }
}
