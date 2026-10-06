use super::LauncherState;
use crate::windows::updates::{UpdateMode, UpdateState, Version, RELEASE_NOTES_PAGE, RELEASE_PAGE};
use core_engine::search::{wants_info, AppInfo, LatestRelease};
use std::sync::Arc;
use windows::Win32::{
    Foundation::{LPARAM, WPARAM},
    UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE},
};

/// What Enter on `@update` does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UpdateStep {
    /// A verified download is waiting: restart into it.
    Restart,
    /// The release can only be announced here: open its page.
    OpenReleasePage,
    /// Ask GitHub now, and download a newer release that is waiting to be fetched.
    Check,
}

/// `installs`: a newer release can be downloaded and installed from here, which it cannot
/// under Notify or in a read-only folder. A release Core only knows of, as from the manifest
/// kept between daily checks, is then fetched rather than shown on its page.
fn update_step(state: &UpdateState, installs: bool) -> UpdateStep {
    match state {
        UpdateState::Staged(_) => UpdateStep::Restart,
        UpdateState::Available(_) if !installs => UpdateStep::OpenReleasePage,
        UpdateState::Available(_) | UpdateState::Failed(_) | UpdateState::UpToDate => {
            UpdateStep::Check
        }
    }
}

/// What Enter does about a newer release that is not downloaded, as the footer words it.
fn available_step(installs: bool) -> &'static str {
    if installs {
        "Enter to download"
    } else {
        "Enter to open release page"
    }
}

impl LauncherState {
    /// A newer release that is not downloaded, for `@info`.
    fn newer_release(&self, version: Arc<str>) -> LatestRelease {
        if self.update_service.installs_updates() {
            LatestRelease::Downloadable(version)
        } else {
            LatestRelease::Available(version)
        }
    }

    /// Core's version and what the update check last saw, for `@info`.
    pub(super) fn app_info(&self) -> Arc<AppInfo> {
        let current = env!("CARGO_PKG_VERSION");
        let text = |version: Version| Arc::<str>::from(version.to_string());
        let latest = if self.settings.saved.preferences.updates == UpdateMode::Off {
            LatestRelease::Off
        } else if self.update_service.working() {
            LatestRelease::Checking
        } else {
            match self.update_service.state() {
                UpdateState::Staged(version) => LatestRelease::Ready(text(version)),
                UpdateState::Available(version) => self.newer_release(text(version)),
                UpdateState::Failed(error) => LatestRelease::Failed(error.to_string().into()),
                UpdateState::UpToDate => match self.update_service.latest() {
                    Some(latest)
                        if current
                            .parse()
                            .is_ok_and(|current: Version| latest > current) =>
                    {
                        self.newer_release(text(latest))
                    }
                    Some(latest) => LatestRelease::UpToDate(text(latest)),
                    None => LatestRelease::Unknown,
                },
            }
        };
        Arc::new(AppInfo {
            version: current.into(),
            latest,
            notes_url: format!("{RELEASE_NOTES_PAGE}{current}").into(),
        })
    }

    /// `@info` asks for the latest release; the daily limit and error backoff still apply.
    pub(super) fn info_requested(&self, query: &str) {
        if wants_info(query) {
            self.update_service.refresh(self.window);
        }
    }

    /// An update check ended: the footer and an open `@info` show the new status.
    pub fn update_status_changed(&mut self) {
        self.refresh_footer();
        if self.visible && self.view.is_some() && wants_info(&self.acted_query()) {
            self.queue_search();
        }
    }

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
            UpdateState::Available(version) => format!(
                "Core {version} is available · {}",
                available_step(self.update_service.installs_updates())
            ),
            UpdateState::Failed(error) => {
                format!("{error} · Enter to retry (once a minute)")
            }
            UpdateState::UpToDate => format!(
                "Core {} · Enter to check GitHub now (once a minute)",
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
            UpdateState::Available(version) if self.update_service.installs_updates() => {
                Some(format!("Core {version} available · @update to download"))
            }
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
        let installs = self.update_service.installs_updates();
        match update_step(&self.update_service.state(), installs) {
            UpdateStep::Restart => {
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
            UpdateStep::OpenReleasePage => {
                self.pending_action = Some(super::NativeAction::OpenUrl(RELEASE_PAGE.into()))
            }
            // Pressing Enter is asking now: GitHub is checked at once, not at the daily check.
            UpdateStep::Check => {
                self.update_service.check_now(self.window);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::updates::UpdateError;

    #[test]
    fn enter_downloads_a_known_release_where_core_installs_updates_itself() {
        let version: Version = "9.8.7".parse().unwrap();
        // The manifest kept between daily checks names a newer release: under Automatic,
        // Enter fetches it instead of sending the person to the release page.
        assert_eq!(
            update_step(&UpdateState::Available(version), true),
            UpdateStep::Check
        );
        assert_eq!(available_step(true), "Enter to download");
        // Under Notify, or in a folder Core cannot write, the page is the way to it.
        assert_eq!(
            update_step(&UpdateState::Available(version), false),
            UpdateStep::OpenReleasePage
        );
        assert_eq!(available_step(false), "Enter to open release page");
        for installs in [true, false] {
            assert_eq!(
                update_step(&UpdateState::Staged(version), installs),
                UpdateStep::Restart
            );
            assert_eq!(
                update_step(&UpdateState::UpToDate, installs),
                UpdateStep::Check
            );
            assert_eq!(
                update_step(&UpdateState::Failed(UpdateError::HashMismatch), installs),
                UpdateStep::Check
            );
        }
    }
}
