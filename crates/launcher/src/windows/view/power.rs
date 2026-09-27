//! The inline power menu opened from the bottom-right button.
use super::*;

impl View {
    pub fn power_menu_open(&self) -> bool {
        self.power_menu.get().is_some_and(PowerMenu::is_open)
    }

    pub fn move_power_selection(&self, backwards: bool) {
        if let Some(menu) = self.power_menu.get() {
            menu.move_focus(backwards);
        }
    }

    pub fn toggle_power_menu(&self) -> windows::core::Result<()> {
        if self.power_menu.get().is_none() {
            let instance =
                unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None)? };
            let _ = self
                .power_menu
                .set(PowerMenu::create(self.parent, instance.into())?);
        }
        let menu = self.power_menu.get().expect("power menu initialized");
        let open = !menu.is_open();
        menu.show(open);
        self.invalidate_layout();
        self.layout()?;
        if !open {
            unsafe {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(self.input));
            }
        }
        Ok(())
    }

    pub fn close_power_menu(&self) -> bool {
        if !self.power_menu_open() {
            return false;
        }
        if let Some(menu) = self.power_menu.get() {
            menu.show(false);
        }
        self.invalidate_layout();
        if let Err(error) = self.layout() {
            self.set_footer(&format!("Could not close power menu: {error}"));
        }
        true
    }
}
