use std::fmt;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    WindowsKey,
    Chord { modifiers: u32, key: u16 },
}

impl Default for Shortcut {
    fn default() -> Self {
        Self::Chord {
            modifiers: MOD_CONTROL.0 | MOD_ALT.0,
            key: VK_SPACE.0,
        }
    }
}

impl Shortcut {
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.trim().eq_ignore_ascii_case("win") || text.trim().eq_ignore_ascii_case("windows") {
            return Ok(Self::WindowsKey);
        }
        let mut modifiers = 0;
        let mut key = None;
        for part in text.split('+').map(str::trim) {
            let modifier =
                if part.eq_ignore_ascii_case("ctrl") || part.eq_ignore_ascii_case("control") {
                    Some(MOD_CONTROL.0)
                } else if part.eq_ignore_ascii_case("alt") {
                    Some(MOD_ALT.0)
                } else if part.eq_ignore_ascii_case("shift") {
                    Some(MOD_SHIFT.0)
                } else if part.eq_ignore_ascii_case("win") {
                    Some(MOD_WIN.0)
                } else {
                    None
                };
            if let Some(modifier) = modifier {
                if modifiers & modifier != 0 {
                    return Err("Shortcut contains a repeated modifier.".into());
                }
                modifiers |= modifier;
            } else {
                if key.is_some() {
                    return Err("Choose one key, plus Ctrl, Alt, Shift or Win.".into());
                }
                key = Some(
                    parse_key(part)
                        .ok_or("Use letters, digits, Space, Tab or F1–F24 for the shortcut.")?,
                );
            }
        }
        let key =
            key.ok_or("Add a key, for example Ctrl+Alt+Space; use Win for the Windows key alone.")?;
        if modifiers == 0 && !(VK_F1.0..=VK_F24.0).contains(&key) {
            return Err("Add a modifier so typing does not open Core.".into());
        }
        Ok(Self::Chord { modifiers, key })
    }
}

fn parse_key(text: &str) -> Option<u16> {
    if text.len() == 1 && text.as_bytes()[0].is_ascii_alphanumeric() {
        return Some(u16::from(text.as_bytes()[0].to_ascii_uppercase()));
    }
    if text.eq_ignore_ascii_case("space") {
        return Some(VK_SPACE.0);
    }
    if text.eq_ignore_ascii_case("tab") {
        return Some(VK_TAB.0);
    }
    let function = text.strip_prefix(['f', 'F'])?.parse::<u16>().ok()?;
    (1..=24)
        .contains(&function)
        .then_some(VK_F1.0 + function - 1)
}

impl fmt::Display for Shortcut {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self::Chord { modifiers, key } = *self else {
            return formatter.write_str("Win");
        };
        for (mask, label) in [
            (MOD_CONTROL.0, "Ctrl+"),
            (MOD_ALT.0, "Alt+"),
            (MOD_SHIFT.0, "Shift+"),
            (MOD_WIN.0, "Win+"),
        ] {
            if modifiers & mask != 0 {
                formatter.write_str(label)?;
            }
        }
        match key {
            key if key == VK_SPACE.0 => formatter.write_str("Space"),
            key if key == VK_TAB.0 => formatter.write_str("Tab"),
            key if (VK_F1.0..=VK_F24.0).contains(&key) => {
                write!(formatter, "F{}", key - VK_F1.0 + 1)
            }
            key => write!(
                formatter,
                "{}",
                char::from_u32(u32::from(key)).ok_or(fmt::Error)?
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcuts_validate_and_round_trip() {
        for text in [
            "Ctrl+Alt+Space",
            "alt + shift + k",
            "win",
            "Win+Shift+7",
            "F12",
        ] {
            let shortcut = Shortcut::parse(text).unwrap();
            assert_eq!(Shortcut::parse(&shortcut.to_string()), Ok(shortcut));
        }
        for text in ["", "A", "Ctrl", "Ctrl+Ctrl+Space", "Alt+K+L", "F25", "Win+"] {
            assert!(Shortcut::parse(text).is_err(), "{text}");
        }
    }
}
