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

/// Keys besides letters, digits, number-pad digits and F1–F24, by the name settings store.
/// Punctuation is named after the US-layout character; the physical key is what is stored.
const NAMED_KEYS: &[(&str, VIRTUAL_KEY)] = &[
    ("Space", VK_SPACE),
    ("Tab", VK_TAB),
    ("Left", VK_LEFT),
    ("Right", VK_RIGHT),
    ("Up", VK_UP),
    ("Down", VK_DOWN),
    ("Home", VK_HOME),
    ("End", VK_END),
    ("PageUp", VK_PRIOR),
    ("PageDown", VK_NEXT),
    ("Insert", VK_INSERT),
    ("Delete", VK_DELETE),
    ("Comma", VK_OEM_COMMA),
    ("Period", VK_OEM_PERIOD),
    ("Minus", VK_OEM_MINUS),
    ("Equals", VK_OEM_PLUS),
    ("Semicolon", VK_OEM_1),
    ("Slash", VK_OEM_2),
    ("Backquote", VK_OEM_3),
    ("BracketLeft", VK_OEM_4),
    ("Backslash", VK_OEM_5),
    ("BracketRight", VK_OEM_6),
    ("Quote", VK_OEM_7),
    ("NumMultiply", VK_MULTIPLY),
    ("NumAdd", VK_ADD),
    ("NumSubtract", VK_SUBTRACT),
    ("NumDecimal", VK_DECIMAL),
    ("NumDivide", VK_DIVIDE),
    ("MediaPlayPause", VK_MEDIA_PLAY_PAUSE),
    ("MediaNext", VK_MEDIA_NEXT_TRACK),
    ("MediaPrevious", VK_MEDIA_PREV_TRACK),
    ("MediaStop", VK_MEDIA_STOP),
    ("VolumeMute", VK_VOLUME_MUTE),
    ("VolumeDown", VK_VOLUME_DOWN),
    ("VolumeUp", VK_VOLUME_UP),
];

pub const SHIFT_TYPES: &str = "Add Ctrl, Alt or Win: with Shift alone the keys still type.";

const UNSUPPORTED_KEY: &str =
    "Use a letter, digit, F1–F24, arrow, punctuation, number-pad or media key.";

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
                key = Some(parse_key(part).ok_or(UNSUPPORTED_KEY)?);
            }
        }
        let key =
            key.ok_or("Add a key, for example Ctrl+Alt+Space; use Win for the Windows key alone.")?;
        Self::chord(modifiers, key)
    }

    /// The shortcut for keys pressed together, as the settings recorder reads them.
    pub fn from_keys(modifiers: u32, key: u16) -> Result<Self, String> {
        if key_name(key).is_none() {
            return Err(UNSUPPORTED_KEY.into());
        }
        let shortcut = Self::chord(modifiers, key)?;
        if shortcut.types_text() {
            return Err(SHIFT_TYPES.into());
        }
        Ok(shortcut)
    }

    /// Shift with a letter, digit, punctuation or Space still types a character, so such a
    /// shortcut would take that character away from typing. Saved files may hold one from
    /// before this check; parsing still accepts it.
    pub fn types_text(self) -> bool {
        let Self::Chord { modifiers, key } = self else {
            return false;
        };
        modifiers == MOD_SHIFT.0 && is_typing_key(key)
    }

    /// The keyboard's media and volume keys.
    pub fn is_media_key(self) -> bool {
        matches!(self, Self::Chord { key, .. } if works_alone(key) && !(VK_F1.0..=VK_F24.0).contains(&key))
    }

    fn chord(modifiers: u32, key: u16) -> Result<Self, String> {
        if modifiers == 0 && !works_alone(key) {
            return Err("Add Ctrl, Alt, Shift or Win so typing does not trigger it.".into());
        }
        Ok(Self::Chord { modifiers, key })
    }

    /// The keys as pressed, for showing what was not accepted: `Ctrl+K`, `Ctrl+Key 20`.
    pub fn keys_text(modifiers: u32, key: u16) -> String {
        let key = key_name(key).unwrap_or_else(|| format!("Key {key}"));
        format!("{}{key}", Self::modifier_text(modifiers))
    }

    /// "Ctrl+Alt+", shown while modifiers are held before the key.
    pub fn modifier_text(modifiers: u32) -> String {
        let mut text = String::new();
        for (mask, label) in MODIFIER_LABELS {
            if modifiers & mask != 0 {
                text.push_str(label);
            }
        }
        text
    }
}

const MODIFIER_LABELS: [(u32, &str); 4] = [
    (MOD_CONTROL.0, "Ctrl+"),
    (MOD_ALT.0, "Alt+"),
    (MOD_SHIFT.0, "Shift+"),
    (MOD_WIN.0, "Win+"),
];

/// Function and media keys are not typed, so they need no modifier.
fn works_alone(key: u16) -> bool {
    (VK_F1.0..=VK_F24.0).contains(&key)
        || [
            VK_MEDIA_PLAY_PAUSE,
            VK_MEDIA_NEXT_TRACK,
            VK_MEDIA_PREV_TRACK,
            VK_MEDIA_STOP,
            VK_VOLUME_MUTE,
            VK_VOLUME_DOWN,
            VK_VOLUME_UP,
        ]
        .iter()
        .any(|media| media.0 == key)
}

/// Keys that type a character: letters, digits, Space, punctuation and the number pad.
fn is_typing_key(key: u16) -> bool {
    let typing = [
        VK_SPACE,
        VK_OEM_COMMA,
        VK_OEM_PERIOD,
        VK_OEM_MINUS,
        VK_OEM_PLUS,
        VK_OEM_1,
        VK_OEM_2,
        VK_OEM_3,
        VK_OEM_4,
        VK_OEM_5,
        VK_OEM_6,
        VK_OEM_7,
        VK_MULTIPLY,
        VK_ADD,
        VK_SUBTRACT,
        VK_DECIMAL,
        VK_DIVIDE,
    ];
    u8::try_from(key).is_ok_and(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        || (VK_NUMPAD0.0..=VK_NUMPAD9.0).contains(&key)
        || typing.iter().any(|typed| typed.0 == key)
}

fn parse_key(text: &str) -> Option<u16> {
    if text.len() == 1 && text.as_bytes()[0].is_ascii_alphanumeric() {
        return Some(u16::from(text.as_bytes()[0].to_ascii_uppercase()));
    }
    if let Some((_, key)) = NAMED_KEYS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(text))
    {
        return Some(key.0);
    }
    if let Some(digit) = text
        .strip_prefix("Num")
        .or_else(|| text.strip_prefix("num"))
        .and_then(|digit| digit.parse::<u16>().ok())
        .filter(|digit| *digit <= 9)
    {
        return Some(VK_NUMPAD0.0 + digit);
    }
    let function = text.strip_prefix(['f', 'F'])?.parse::<u16>().ok()?;
    (1..=24)
        .contains(&function)
        .then_some(VK_F1.0 + function - 1)
}

/// The name `parse_key` reads back; None for keys a shortcut cannot use.
fn key_name(key: u16) -> Option<String> {
    if let Ok(byte) = u8::try_from(key) {
        if byte.is_ascii_uppercase() || byte.is_ascii_digit() {
            return Some(char::from(byte).to_string());
        }
    }
    if (VK_F1.0..=VK_F24.0).contains(&key) {
        return Some(format!("F{}", key - VK_F1.0 + 1));
    }
    if (VK_NUMPAD0.0..=VK_NUMPAD9.0).contains(&key) {
        return Some(format!("Num{}", key - VK_NUMPAD0.0));
    }
    NAMED_KEYS
        .iter()
        .find(|(_, named)| named.0 == key)
        .map(|(name, _)| (*name).to_owned())
}

impl fmt::Display for Shortcut {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self::Chord { modifiers, key } = *self else {
            return formatter.write_str("Win");
        };
        formatter.write_str(&Self::modifier_text(modifiers))?;
        formatter.write_str(&key_name(key).ok_or(fmt::Error)?)
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
            "Alt+Right",
            "Ctrl+Shift+PageDown",
            "Alt+Comma",
            "Ctrl+Num7",
            "MediaPlayPause",
            "Win+BracketLeft",
        ] {
            let shortcut = Shortcut::parse(text).unwrap();
            assert_eq!(
                Shortcut::parse(&shortcut.to_string()),
                Ok(shortcut),
                "{text}"
            );
        }
        for text in [
            "",
            "A",
            "Ctrl",
            "Ctrl+Ctrl+Space",
            "Alt+K+L",
            "F25",
            "Win+",
            "Right",
            "Num10",
            "Ctrl+Escape",
        ] {
            assert!(Shortcut::parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn recorded_keys_become_the_same_shortcuts_as_typed_text() {
        assert_eq!(
            Shortcut::from_keys(MOD_ALT.0, VK_RIGHT.0),
            Shortcut::parse("Alt+Right")
        );
        assert_eq!(
            Shortcut::from_keys(0, VK_MEDIA_NEXT_TRACK.0),
            Shortcut::parse("MediaNext")
        );
        assert!(Shortcut::from_keys(0, u16::from(b'P')).is_err());
        // Shift alone still types: a capital letter, or @ on Shift+2.
        assert_eq!(
            Shortcut::from_keys(MOD_SHIFT.0, u16::from(b'2')),
            Err(SHIFT_TYPES.into())
        );
        assert!(Shortcut::from_keys(MOD_SHIFT.0, VK_F5.0).is_ok());
        assert!(Shortcut::from_keys(MOD_SHIFT.0 | MOD_ALT.0, u16::from(b'P')).is_ok());
        assert!(Shortcut::parse("MediaNext").unwrap().is_media_key());
        assert!(!Shortcut::parse("F9").unwrap().is_media_key());
        assert!(!Shortcut::parse("Alt+P").unwrap().is_media_key());
        assert!(Shortcut::from_keys(MOD_CONTROL.0, VK_ESCAPE.0).is_err());
        assert!(Shortcut::from_keys(MOD_CONTROL.0, VK_CAPITAL.0).is_err());
        assert_eq!(
            Shortcut::modifier_text(MOD_CONTROL.0 | MOD_ALT.0 | MOD_WIN.0),
            "Ctrl+Alt+Win+"
        );
    }
}
