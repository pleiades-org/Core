//! Music settings: which player Core prefers, the now-playing bar and media shortcuts.
use super::Shortcut;
use core_engine::media::{MediaCommand, MediaPolicy, PriorityMode};
use std::sync::Arc;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

/// Most apps a preference or ignore list keeps.
pub const MAX_LISTED_APPS: usize = 64;
const MAX_APP_TEXT: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaShortcutAction {
    PlayPause,
    Next,
    Previous,
}

impl MediaShortcutAction {
    pub const ALL: [Self; 3] = [Self::PlayPause, Self::Next, Self::Previous];

    pub fn label(self) -> &'static str {
        match self {
            Self::PlayPause => "Play / pause",
            Self::Next => "Next track",
            Self::Previous => "Previous track",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::PlayPause => "play-pause",
            Self::Next => "next",
            Self::Previous => "previous",
        }
    }

    pub fn command(self) -> MediaCommand {
        match self {
            Self::PlayPause => MediaCommand::TogglePlayPause,
            Self::Next => MediaCommand::Next,
            Self::Previous => MediaCommand::Previous,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShortcutScope {
    /// Only while Core is open and in front.
    #[default]
    InCore,
    /// In every app, through a Windows hotkey.
    Everywhere,
}

impl ShortcutScope {
    pub const ALL: [Self; 2] = [Self::InCore, Self::Everywhere];

    pub fn label(self) -> &'static str {
        match self {
            Self::InCore => "While Core is open",
            Self::Everywhere => "In every app",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::InCore => "core",
            Self::Everywhere => "everywhere",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|scope| text.eq_ignore_ascii_case(scope.id()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MediaShortcut {
    /// None when the person cleared it.
    pub shortcut: Option<Shortcut>,
    pub scope: ShortcutScope,
}

/// An app in the preference or ignore list: the engine's app key and the name shown for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MusicApp {
    pub key: Arc<str>,
    pub name: Arc<str>,
}

impl MusicApp {
    pub fn new(key: &str, name: &str) -> Result<Self, String> {
        let (key, name) = (key.trim(), name.trim());
        if key.is_empty()
            || key.len() > MAX_APP_TEXT
            || name.len() > MAX_APP_TEXT
            || [key, name]
                .iter()
                .any(|text| text.chars().any(char::is_control))
        {
            return Err(
                "A music app entry is empty, too long or contains control characters.".into(),
            );
        }
        Ok(Self {
            key: key.to_lowercase().into(),
            name: if name.is_empty() { key } else { name }.into(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MusicSettings {
    pub spotify: super::SpotifySettings,
    pub priority: PriorityMode,
    /// The now-playing bar above the search box.
    pub bar: bool,
    /// In `MediaShortcutAction::ALL` order.
    pub shortcuts: [MediaShortcut; 3],
    /// In the person's order; the first open one wins.
    pub preferred: Arc<[MusicApp]>,
    pub ignored: Arc<[MusicApp]>,
}

impl Default for MusicSettings {
    fn default() -> Self {
        let in_core = |key: VIRTUAL_KEY| MediaShortcut {
            shortcut: Some(Shortcut::Chord {
                modifiers: MOD_ALT.0,
                key: key.0,
            }),
            scope: ShortcutScope::InCore,
        };
        Self {
            spotify: super::SpotifySettings::default(),
            priority: PriorityMode::default(),
            bar: true,
            shortcuts: [
                in_core(VIRTUAL_KEY(u16::from(b'P'))),
                in_core(VK_RIGHT),
                in_core(VK_LEFT),
            ],
            preferred: Arc::from([]),
            ignored: Arc::from([]),
        }
    }
}

impl MusicSettings {
    pub fn policy(&self) -> MediaPolicy {
        let keys = |apps: &[MusicApp]| apps.iter().map(|app| app.key.clone()).collect();
        MediaPolicy {
            mode: self.priority,
            preferred: keys(&self.preferred),
            ignored: keys(&self.ignored),
        }
    }

    /// Lines for the settings file. Defaults write nothing, so files stay as they were for
    /// people who never change these settings.
    pub fn encode(&self) -> String {
        if *self == Self::default() {
            return String::new();
        }
        let mut text = format!(
            "music_priority={}\nmusic_bar={}\n",
            self.priority.id(),
            self.bar
        );
        if self.spotify != super::SpotifySettings::default() {
            text.push_str(&format!(
                "music_spotify_enabled={}\nmusic_spotify_client_id={}\n",
                self.spotify.enabled, self.spotify.client_id
            ));
        }
        for (action, shortcut) in MediaShortcutAction::ALL.iter().zip(&self.shortcuts) {
            text.push_str(&format!(
                "music_shortcut={}\t{}\t{}\n",
                action.id(),
                shortcut
                    .shortcut
                    .map(|shortcut| shortcut.to_string())
                    .unwrap_or_default(),
                shortcut.scope.id()
            ));
        }
        for (field, apps) in [
            ("music_preferred", &self.preferred),
            ("music_ignored", &self.ignored),
        ] {
            for app in apps.iter() {
                text.push_str(&format!("{field}={}\t{}\n", app.key, app.name));
            }
        }
        text
    }

    /// Reads `music_*` lines; false for any other line.
    pub fn decode_line(&mut self, line: &str, seen: &mut MusicFields) -> Result<bool, String> {
        let Some((key, value)) = line.split_once('=') else {
            return Ok(false);
        };
        let field = match key.trim() {
            "music_spotify_enabled" => {
                self.spotify.enabled = value
                    .trim()
                    .parse()
                    .map_err(|_| "Spotify song search must be true or false.")?;
                MusicFields::SPOTIFY_ENABLED
            }
            "music_spotify_client_id" => {
                self.spotify.client_id = value.trim().to_owned();
                self.spotify.validate()?;
                MusicFields::SPOTIFY_CLIENT
            }
            "music_priority" => {
                self.priority = PriorityMode::parse(value.trim())
                    .ok_or("Music priority must be music-first or playing-first.")?;
                MusicFields::PRIORITY
            }
            "music_bar" => {
                self.bar = value
                    .trim()
                    .parse()
                    .map_err(|_| "The now-playing bar setting must be true or false.")?;
                MusicFields::BAR
            }
            "music_shortcut" => {
                let mut columns = value.split('\t');
                let (Some(action), Some(shortcut), Some(scope), None) = (
                    columns.next(),
                    columns.next(),
                    columns.next(),
                    columns.next(),
                ) else {
                    return Err("A media shortcut needs action, shortcut and scope columns.".into());
                };
                let index = MediaShortcutAction::ALL
                    .iter()
                    .position(|candidate| candidate.id() == action.trim())
                    .ok_or("Unknown media shortcut action.")?;
                self.shortcuts[index] = MediaShortcut {
                    shortcut: if shortcut.trim().is_empty() {
                        None
                    } else {
                        Some(Shortcut::parse(shortcut)?)
                    },
                    scope: ShortcutScope::parse(scope.trim())
                        .ok_or("A media shortcut scope must be core or everywhere.")?,
                };
                MusicFields::SHORTCUT << index
            }
            field @ ("music_preferred" | "music_ignored") => {
                let (key, name) = value.split_once('\t').unwrap_or((value, ""));
                let app = MusicApp::new(key, name)?;
                let list = if field == "music_preferred" {
                    &mut self.preferred
                } else {
                    &mut self.ignored
                };
                if list.len() >= MAX_LISTED_APPS || list.iter().any(|listed| listed.key == app.key)
                {
                    return Err("Music app lists hold up to 64 different apps.".into());
                }
                *list = list.iter().cloned().chain([app]).collect();
                return Ok(true);
            }
            _ => return Ok(false),
        };
        if seen.0 & field != 0 {
            return Err("Settings contain a duplicate music field.".into());
        }
        seen.0 |= field;
        Ok(true)
    }

    /// Conflicts make the draft invalid, so the previous shortcuts stay registered.
    pub fn validate(&self, open_core: Shortcut) -> Result<(), String> {
        for (index, (action, entry)) in MediaShortcutAction::ALL
            .iter()
            .zip(&self.shortcuts)
            .enumerate()
        {
            let Some(shortcut) = entry.shortcut else {
                continue;
            };
            if shortcut == Shortcut::WindowsKey {
                return Err("The Windows key alone cannot be a media shortcut.".into());
            }
            if shortcut == open_core {
                return Err(format!("{shortcut} already opens Core."));
            }
            if core_reserved(shortcut) {
                return Err(format!(
                    "Core uses {shortcut} for typing and moving through results."
                ));
            }
            if shortcut.types_text() {
                return Err(super::shortcut::SHIFT_TYPES.into());
            }
            if let Some((other, _)) = MediaShortcutAction::ALL
                .iter()
                .zip(&self.shortcuts)
                .skip(index + 1)
                .find(|(_, other)| other.shortcut == Some(shortcut))
            {
                return Err(format!(
                    "{shortcut} is set for both {} and {}.",
                    action.label().to_lowercase(),
                    other.label().to_lowercase()
                ));
            }
        }
        Ok(())
    }

    /// For a settings file being loaded: a media shortcut that conflicts is cleared instead of
    /// rejecting the whole file. A file from before media shortcuts existed gets the defaults,
    /// and its Open Core shortcut may be one of them (Alt+P); that default then gives way.
    pub fn reconcile(&mut self, open_core: Shortcut) {
        for index in 0..self.shortcuts.len() {
            let Some(shortcut) = self.shortcuts[index].shortcut else {
                continue;
            };
            let mut alone = self.clone();
            for (other, entry) in alone.shortcuts.iter_mut().enumerate() {
                if other != index {
                    entry.shortcut = None;
                }
            }
            let repeated = self.shortcuts[..index]
                .iter()
                .any(|earlier| earlier.shortcut == Some(shortcut));
            if repeated || alone.validate(open_core).is_err() {
                self.shortcuts[index].shortcut = None;
            }
        }
    }

    /// The command for keys pressed while Core is in front.
    pub fn in_core_command(&self, shortcut: Shortcut) -> Option<MediaCommand> {
        MediaShortcutAction::ALL
            .iter()
            .zip(&self.shortcuts)
            .find(|(_, entry)| {
                entry.scope == ShortcutScope::InCore && entry.shortcut == Some(shortcut)
            })
            .map(|(action, _)| action.command())
    }

    /// Shortcuts to register with Windows, by their position in `MediaShortcutAction::ALL`.
    pub fn everywhere_bindings(&self) -> Vec<(usize, Shortcut)> {
        self.shortcuts
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.scope == ShortcutScope::Everywhere)
            .filter_map(|(index, entry)| Some((index, entry.shortcut?)))
            .collect()
    }
}

/// Which single-valued music fields a file has set, to reject duplicates.
#[derive(Default)]
pub struct MusicFields(u8);

impl MusicFields {
    const SPOTIFY_ENABLED: u8 = 32;
    const SPOTIFY_CLIENT: u8 = 64;
    const PRIORITY: u8 = 1;
    const BAR: u8 = 2;
    const SHORTCUT: u8 = 4;
}

/// Keys the search box and result list already use: editing, selection and navigation.
fn core_reserved(shortcut: Shortcut) -> bool {
    let Shortcut::Chord { modifiers, key } = shortcut else {
        return false;
    };
    let editing = [
        VK_LEFT, VK_RIGHT, VK_UP, VK_DOWN, VK_HOME, VK_END, VK_DELETE, VK_INSERT, VK_TAB,
    ];
    let control_letters = [b'A', b'C', b'V', b'X', b'Y', b'Z'];
    (modifiers & !(MOD_CONTROL.0 | MOD_SHIFT.0) == 0 && editing.iter().any(|edit| edit.0 == key))
        || (modifiers == MOD_CONTROL.0
            && (control_letters
                .iter()
                .any(|letter| u16::from(*letter) == key)
                || key == VK_OEM_COMMA.0))
        || (modifiers == MOD_ALT.0 && key == VK_F4.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shortcut(text: &str) -> Option<Shortcut> {
        Some(Shortcut::parse(text).unwrap())
    }

    fn decode(text: &str) -> Result<MusicSettings, String> {
        let mut settings = MusicSettings::default();
        let mut seen = MusicFields::default();
        for line in text.lines() {
            assert!(settings.decode_line(line, &mut seen)?, "{line}");
        }
        Ok(settings)
    }

    #[test]
    fn defaults_write_nothing_and_changes_round_trip() {
        assert_eq!(MusicSettings::default().encode(), "");
        let settings = MusicSettings {
            spotify: super::super::SpotifySettings {
                enabled: true,
                client_id: "0123456789abcdef0123456789abcdef".into(),
            },
            priority: PriorityMode::PlayingFirst,
            bar: false,
            shortcuts: [
                MediaShortcut {
                    shortcut: shortcut("Ctrl+Alt+P"),
                    scope: ShortcutScope::Everywhere,
                },
                MediaShortcut {
                    shortcut: None,
                    scope: ShortcutScope::InCore,
                },
                MediaShortcut {
                    shortcut: shortcut("MediaPrevious"),
                    scope: ShortcutScope::Everywhere,
                },
            ],
            preferred: Arc::from([
                MusicApp::new("spotify", "Spotify").unwrap(),
                MusicApp::new("applemusic", "Apple Music").unwrap(),
            ]),
            ignored: Arc::from([MusicApp::new("chrome", "Google Chrome").unwrap()]),
        };
        assert_eq!(decode(&settings.encode()), Ok(settings.clone()));
        let policy = settings.policy();
        assert_eq!(&*policy.preferred[1], "applemusic");
        assert_eq!(&*policy.ignored[0], "chrome");
    }

    #[test]
    fn malformed_or_duplicate_music_lines_are_rejected() {
        for text in [
            "music_priority=loudest",
            "music_bar=maybe",
            "music_spotify_enabled=maybe",
            "music_spotify_enabled=true\nmusic_spotify_enabled=false",
            "music_spotify_client_id=not-a-client-id",
            "music_spotify_client_id=\nmusic_spotify_client_id=",
            "music_shortcut=play-pause\tAlt+P",
            "music_shortcut=rewind\tAlt+R\tcore",
            "music_shortcut=next\tAlt+Right\tsomewhere",
            "music_bar=true\nmusic_bar=false",
            "music_preferred=spotify\tSpotify\nmusic_preferred=Spotify\tSpotify",
            "music_ignored=\tNameless",
        ] {
            assert!(decode(text).is_err(), "{text}");
        }
        let mut settings = MusicSettings::default();
        assert_eq!(
            settings.decode_line("background=#000000", &mut MusicFields::default()),
            Ok(false)
        );
    }

    #[test]
    fn conflicting_or_reserved_shortcuts_are_explained() {
        let open_core = Shortcut::default();
        assert_eq!(MusicSettings::default().validate(open_core), Ok(()));
        let with = |index: usize, text: &str| {
            let mut settings = MusicSettings::default();
            settings.shortcuts[index].shortcut = shortcut(text);
            settings.validate(open_core)
        };
        assert!(with(0, "Ctrl+Alt+Space")
            .unwrap_err()
            .contains("opens Core"));
        assert!(with(0, "Alt+Right").unwrap_err().contains("both"));
        for reserved in [
            "Ctrl+A",
            "Ctrl+Left",
            "Shift+Home",
            "Ctrl+Comma",
            "Alt+F4",
            "Shift+P",
            "Shift+2",
        ] {
            assert!(with(1, reserved).is_err(), "{reserved}");
        }
        let mut windows_key = MusicSettings::default();
        windows_key.shortcuts[2].shortcut = Some(Shortcut::WindowsKey);
        assert!(windows_key.validate(open_core).is_err());
    }

    #[test]
    fn loading_clears_media_shortcuts_that_conflict_instead_of_failing() {
        let open_core = Shortcut::parse("Alt+P").unwrap();
        let mut settings = MusicSettings::default();
        assert!(settings.validate(open_core).is_err());
        settings.reconcile(open_core);
        assert_eq!(settings.shortcuts[0].shortcut, None);
        assert_eq!(settings.shortcuts[1].shortcut, shortcut("Alt+Right"));
        assert_eq!(settings.validate(open_core), Ok(()));
        let mut repeated = MusicSettings::default();
        repeated.shortcuts[2].shortcut = shortcut("Alt+Right");
        repeated.reconcile(Shortcut::default());
        assert_eq!(repeated.shortcuts[1].shortcut, shortcut("Alt+Right"));
        assert_eq!(repeated.shortcuts[2].shortcut, None);
    }

    #[test]
    fn shortcuts_split_by_where_they_work() {
        let mut settings = MusicSettings::default();
        settings.shortcuts[1].scope = ShortcutScope::Everywhere;
        assert_eq!(
            settings.in_core_command(Shortcut::parse("Alt+P").unwrap()),
            Some(MediaCommand::TogglePlayPause)
        );
        assert_eq!(
            settings.in_core_command(Shortcut::parse("Alt+Right").unwrap()),
            None
        );
        assert_eq!(
            settings.everywhere_bindings(),
            [(1, Shortcut::parse("Alt+Right").unwrap())]
        );
    }
}
