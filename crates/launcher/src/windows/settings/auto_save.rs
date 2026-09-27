use super::SettingsDocument;
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{KillTimer, SetTimer},
};

pub const AUTO_SAVE_TIMER: usize = 41;
const TYPING_DELAY_MS: u32 = 400;

/// Coalesce edits while the single settings writer finishes; never poll while idle.
#[derive(Default)]
pub struct AutoSave {
    pub queued: Option<SettingsDocument>,
    pub saving: Option<SettingsDocument>,
    pub error: Option<String>,
    pub done_when_saved: bool,
    timer_active: bool,
}

impl AutoSave {
    pub fn latest(&self, saved: &SettingsDocument) -> SettingsDocument {
        self.queued
            .as_ref()
            .or(self.saving.as_ref())
            .unwrap_or(saved)
            .clone()
    }

    pub fn waiting_for_typing(&self) -> bool {
        self.timer_active
    }

    pub fn defer(&mut self, window: HWND) -> bool {
        self.cancel_timer(window);
        self.timer_active =
            unsafe { SetTimer(Some(window), AUTO_SAVE_TIMER, TYPING_DELAY_MS, None) } != 0;
        self.timer_active
    }

    pub fn cancel_timer(&mut self, window: HWND) {
        if self.timer_active {
            if let Err(error) = unsafe { KillTimer(Some(window), AUTO_SAVE_TIMER) } {
                eprintln!("Could not stop the settings typing timer: {error}");
            }
            self.timer_active = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::settings::{BackgroundColor, Preferences};

    fn document(color: &str) -> SettingsDocument {
        SettingsDocument {
            preferences: Preferences {
                background: BackgroundColor::parse(color).unwrap(),
                ..Preferences::default()
            },
            ..SettingsDocument::default()
        }
    }

    #[test]
    fn the_newest_unsaved_draft_is_what_settings_reopen_with() {
        let saved = document("#000000");
        let mut auto_save = AutoSave::default();
        assert_eq!(auto_save.latest(&saved), saved);
        auto_save.saving = Some(document("#111111"));
        assert_eq!(auto_save.latest(&saved), document("#111111"));
        auto_save.queued = Some(document("#222222"));
        assert_eq!(auto_save.latest(&saved), document("#222222"));
        assert!(!auto_save.waiting_for_typing());
    }
}
