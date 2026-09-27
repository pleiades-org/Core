mod windows_key;
mod windows_key_state;
use super::settings::Shortcut;
use windows::Win32::{Foundation::HWND, UI::Input::KeyboardAndMouse::*};
pub use windows_key::SHORTCUT_ERROR;

pub struct ActivationBinding {
    pub shortcut: Shortcut,
    pub identifier: i32,
    window: HWND,
    hook: Option<windows_key::WindowsKeyHook>,
}

impl ActivationBinding {
    pub fn install(window: HWND, shortcut: Shortcut, identifier: i32) -> Result<Self, String> {
        let hook = match shortcut {
            Shortcut::WindowsKey => Some(windows_key::WindowsKeyHook::start(window)?),
            Shortcut::Chord { modifiers, key } => {
                unsafe { RegisterHotKey(Some(window), identifier, HOT_KEY_MODIFIERS(modifiers) | MOD_NOREPEAT, u32::from(key)) }.map_err(|error| format!("{shortcut} is unavailable or already used by another app. Choose another shortcut. ({error})"))?;
                None
            }
        };
        Ok(Self {
            shortcut,
            identifier,
            window,
            hook,
        })
    }
}
impl ActivationBinding {
    pub fn reset_windows_key(&self) {
        if let Some(hook) = &self.hook {
            hook.reset();
        }
    }
}

impl Drop for ActivationBinding {
    fn drop(&mut self) {
        if self.hook.is_none() {
            if let Err(error) = unsafe { UnregisterHotKey(Some(self.window), self.identifier) } {
                eprintln!("Could not release Core shortcut: {error}");
            }
        }
    }
}
