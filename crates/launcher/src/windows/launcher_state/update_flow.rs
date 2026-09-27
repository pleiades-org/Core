use super::LauncherState;
use crate::windows::updates::{UpdateMode, UpdateState, RELEASE_PAGE};
use windows::Win32::{
    Foundation::{LPARAM, WPARAM},
    UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE},
};

impl LauncherState {
    pub fn update_hint(&self) -> String {
        if self.settings.saved.preferences.updates == UpdateMode::Off {
            return "Updates are off · change Settings > Behaviour to enable".into();
        }
        if self.update_service.working() {
            return "Checking GitHub for updates…".into();
        }
        match self.update_service.state() {
            UpdateState::Staged(version) => {
                format!("Core {version} is ready · Enter to restart and install")
            }
            UpdateState::Available(version) => {
                format!("Core {version} is available · Enter to open release page")
            }
            UpdateState::Failed(error) => format!("{error} · retry after one hour"),
            UpdateState::UpToDate => format!(
                "Core {} · Enter to check for updates",
                env!("CARGO_PKG_VERSION")
            ),
        }
    }

    pub fn update_notice(&self) -> Option<String> {
        if self.settings.saved.preferences.updates == UpdateMode::Off {
            return None;
        }
        match self.update_service.state() {
            UpdateState::Staged(version) => Some(format!(
                "Core {version} ready · installs on exit · @update to restart"
            )),
            UpdateState::Available(version) => Some(format!(
                "Core {version} available · @update for the release page"
            )),
            _ => None,
        }
    }

    pub fn accept_update(&mut self) {
        if self.settings.saved.preferences.updates == UpdateMode::Off {
            self.refresh_footer();
            return;
        }
        match self.update_service.state() {
            UpdateState::Staged(_) => {
                self.restart_for_update = true;
                if let Err(error) =
                    unsafe { PostMessageW(Some(self.window), WM_CLOSE, WPARAM(0), LPARAM(0)) }
                {
                    self.restart_for_update = false;
                    if let Some(view) = &self.view {
                        view.set_footer(&format!("Could not restart: {error}"));
                    }
                }
            }
            UpdateState::Available(_) => {
                self.pending_action = Some(super::NativeAction::OpenUrl(RELEASE_PAGE.into()))
            }
            _ => {
                self.update_service.refresh(self.window);
                self.refresh_footer();
            }
        }
    }

    pub fn finish_update_exit(&mut self) -> bool {
        let mode = if self.auto_save.error.is_some() {
            UpdateMode::Off
        } else {
            self.auto_save
                .latest(&self.settings.saved)
                .preferences
                .updates
        };
        self.update_service.set_mode(mode);
        let result = if self.restart_for_update {
            self.update_service.restart()
        } else {
            self.update_service.apply_staged()
        };
        if let Err(error) = result {
            eprintln!("Could not install Core update: {error}");
            if self.restart_for_update {
                self.restart_for_update = false;
                self.close_after_save = false;
                if let Some(view) = &self.view {
                    view.set_footer(&format!("Update not installed: {error}"));
                }
                return false;
            }
        }
        true
    }
}
