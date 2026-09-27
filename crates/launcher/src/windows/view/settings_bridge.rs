//! Switching between search and the settings page, and forwarding settings calls.
use super::*;

impl View {
    pub fn settings_open(&self) -> bool {
        self.settings_open.get()
    }

    pub fn open_settings(
        &self,
        saved: crate::windows::settings::SettingsDocument,
    ) -> windows::core::Result<()> {
        self.close_power_menu();
        if self.settings_page.get().is_none() {
            let instance =
                unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None)? };
            let page = SettingsPage::create(self.parent, instance.into())?;
            page.apply_theme(self.palette.get());
            let _ = self.settings_page.set(page);
        }
        let page = self.settings_page.get().expect("created settings page");
        page.reset(saved);
        self.settings_open.set(true);
        self.set_clock_active(false);
        self.show_search_controls(false);
        page.show(true);
        // Showing Core then lays out and repaints once, on the display it opens on.
        Ok(())
    }

    pub fn close_settings(&self, saved: Preferences) -> windows::core::Result<()> {
        if let Some(page) = self.settings_page.get() {
            page.show(false);
        }
        self.settings_open.set(false);
        self.show_search_controls(true);
        self.set_clock_active(unsafe { IsWindowVisible(self.parent) }.as_bool());
        // The search controls return, so everything is applied and repainted.
        self.apply(saved, PreferenceChanges::ALL)
    }

    fn show_search_controls(&self, visible: bool) {
        unsafe {
            let _ = ShowWindow(
                self.output,
                if visible && self.output_visible.get() {
                    SW_SHOWNA
                } else {
                    SW_HIDE
                },
            );
        }
        for control in [
            self.input,
            self.results,
            self.footer,
            self.clock,
            self.power_button,
            self.settings_button,
        ] {
            unsafe {
                let _ = ShowWindow(control, if visible { SW_SHOWNA } else { SW_HIDE });
            }
        }
    }

    pub fn focus_target(&self) -> HWND {
        if self.settings_open.get() {
            self.settings_page
                .get()
                .expect("open settings page")
                .focus_target()
        } else if self.output_visible.get() && self.console.running() {
            // A running command keeps the keyboard, so reopening Core resumes typing into it.
            self.output
        } else {
            self.input
        }
    }

    pub fn settings_command(&self, identifier: usize, notification: u32) -> SettingsAction {
        if !self.settings_open.get() {
            return SettingsAction::None;
        }
        self.settings_page
            .get()
            .expect("open settings page")
            .command(identifier, notification)
    }

    pub fn scroll_quicklinks(&self, command: u16, wheel: Option<i16>) {
        if self.settings_open() {
            if let Some(page) = self.settings_page.get() {
                page.scroll_quicklinks(command, wheel);
            }
            unsafe {
                let _ = InvalidateRect(Some(self.parent), None, false);
            }
        }
    }

    pub fn advance_quicklink_tab(&self, identifier: usize, backwards: bool) -> bool {
        self.settings_open()
            && self
                .settings_page
                .get()
                .is_some_and(|page| page.advance_quicklink_tab(identifier, backwards))
    }

    pub fn settings_draft(&self) -> Result<crate::windows::settings::SettingsDocument, String> {
        self.settings_page
            .get()
            .ok_or("Settings are not open.")?
            .draft()
    }

    pub fn settings_status(&self, message: &str) {
        if let Some(page) = self.settings_page.get() {
            page.status(message);
        }
    }

    pub fn settings_save_error(&self, failed: bool) {
        if let Some(page) = self.settings_page.get() {
            page.set_save_error(failed && self.settings_open());
        }
    }
}
