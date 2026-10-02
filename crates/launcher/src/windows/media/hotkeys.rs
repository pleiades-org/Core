//! Media shortcuts that work in every app. Windows delivers them as WM_HOTKEY, so they cost
//! nothing while idle; identifiers start at `MEDIA_HOTKEY_FIRST`, clear of the Open Core ones.
use crate::windows::settings::Shortcut;
use windows::Win32::{
    Foundation::HWND,
    UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_NOREPEAT,
    },
};

pub const MEDIA_HOTKEY_FIRST: usize = 0x100;

pub struct MediaHotkeys {
    window: HWND,
    identifiers: Vec<i32>,
}

impl MediaHotkeys {
    /// Registers every binding or none: `slot` is each shortcut's position, recovered from the
    /// hotkey's identifier when it is pressed.
    pub fn register(window: HWND, bindings: &[(usize, Shortcut)]) -> Result<Self, String> {
        let mut registered = Self {
            window,
            identifiers: Vec::new(),
        };
        for (slot, shortcut) in bindings {
            let Shortcut::Chord { modifiers, key } = *shortcut else {
                return Err("The Windows key alone cannot be a media shortcut.".into());
            };
            let identifier = (MEDIA_HOTKEY_FIRST + slot) as i32;
            unsafe {
                RegisterHotKey(
                    Some(window),
                    identifier,
                    HOT_KEY_MODIFIERS(modifiers) | MOD_NOREPEAT,
                    u32::from(key),
                )
            }
            .map_err(|error| {
                format!("{shortcut} is unavailable or already used by another app. Choose another shortcut. ({error})")
            })?;
            registered.identifiers.push(identifier);
        }
        Ok(registered)
    }

    /// The shortcut position a WM_HOTKEY identifier stands for, if it is a media shortcut.
    pub fn slot(identifier: usize) -> Option<usize> {
        identifier
            .checked_sub(MEDIA_HOTKEY_FIRST)
            .filter(|slot| *slot < 16)
    }
}

impl Drop for MediaHotkeys {
    fn drop(&mut self) {
        for identifier in &self.identifiers {
            if let Err(error) = unsafe { UnregisterHotKey(Some(self.window), *identifier) } {
                eprintln!("Could not release a media shortcut: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_media_hotkey_identifiers_map_to_slots() {
        assert_eq!(MediaHotkeys::slot(MEDIA_HOTKEY_FIRST), Some(0));
        assert_eq!(MediaHotkeys::slot(MEDIA_HOTKEY_FIRST + 2), Some(2));
        for identifier in [0, 1, 2, MEDIA_HOTKEY_FIRST - 1, MEDIA_HOTKEY_FIRST + 16] {
            assert_eq!(MediaHotkeys::slot(identifier), None);
        }
    }
}
