use serde::{Deserialize, Serialize};
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    sync::OnceLock,
};

const DEFAULT_HOTKEY: &str = "Alt+Space";
const LEGACY_DEFAULT_HOTKEY: &str = "Ctrl+Shift+Space";
const DEFAULT_QUICK_NOTE_ANCHOR: &str = "top-right";
const FALLBACK_TIMEZONE: &str = "Europe/London";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum DisplayPosition {
    #[default]
    Center,
    Top,
    Bottom,
    Left,
    Right,
}

impl DisplayPosition {
    pub fn as_str(&self) -> &'static str {
        match self {
            DisplayPosition::Center => "center",
            DisplayPosition::Top => "top",
            DisplayPosition::Bottom => "bottom",
            DisplayPosition::Left => "left",
            DisplayPosition::Right => "right",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct LauncherSettings {
    pub local_timezone: String,
    pub home_currency: Option<String>,
    pub backdrop_blur_enabled: bool,
    pub hotkey_enabled: bool,
    pub hotkey: String,
    pub launch_at_startup: bool,
    pub preferred_terminal_profile: Option<String>,
    pub index_start_menu: bool,
    pub index_user_files: bool,
    pub show_web_search_result: bool,
    pub clipboard_history_enabled: bool,
    pub aliases: Vec<CommandAliasSetting>,
    pub custom_commands: Vec<CustomCommandSetting>,
    pub hotkeys: Vec<CommandHotkeySetting>,
    pub quick_note_anchor: String,
    pub quick_note_offset_x: i32,
    pub quick_note_offset_y: i32,
    pub quick_note_width: f32,
    pub quick_note_height: f32,
    pub display_position: DisplayPosition,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct CommandAliasSetting {
    pub keyword: String,
    pub expands_to: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct CustomCommandSetting {
    pub name: String,
    pub description: String,
    pub command: String,
    pub aliases: Vec<String>,
    pub hotkey: Option<String>,
    pub working_directory: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct CommandHotkeySetting {
    pub hotkey: String,
    pub query: String,
    pub description: String,
}

impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
            local_timezone: iana_time_zone::get_timezone()
                .unwrap_or_else(|_| FALLBACK_TIMEZONE.to_string()),
            home_currency: None,
            backdrop_blur_enabled: false,
            hotkey_enabled: true,
            hotkey: DEFAULT_HOTKEY.to_string(),
            launch_at_startup: true,
            preferred_terminal_profile: None,
            index_start_menu: true,
            index_user_files: true,
            show_web_search_result: true,
            clipboard_history_enabled: true,
            aliases: Vec::new(),
            custom_commands: Vec::new(),
            hotkeys: Vec::new(),
            quick_note_anchor: DEFAULT_QUICK_NOTE_ANCHOR.to_string(),
            quick_note_offset_x: 24,
            quick_note_offset_y: 24,
            quick_note_width: 380.,
            quick_note_height: 300.,
            display_position: DisplayPosition::Center,
        }
    }
}

impl LauncherSettings {
    pub fn load_or_create() -> Self {
        let settings_path = settings_file_path();

        match fs::read_to_string(&settings_path) {
            Ok(settings_text) => match toml::from_str::<LauncherSettings>(&settings_text) {
                Ok(mut settings) => {
                    if settings.migrate_legacy_defaults() {
                        let _ = settings.save_to_path(&settings_path);
                    }
                    settings
                }
                Err(e) => {
                    eprintln!(
                        "WARNING: Failed to parse {} as TOML: {}",
                        settings_path.display(),
                        e
                    );
                    eprintln!("Falling back to default settings. Fix the TOML syntax.");
                    Self::default()
                }
            },
            Err(_) => {
                let settings = Self::default();
                let _ = settings.save_to_path(&settings_path);
                settings
            }
        }
    }

    pub fn save(&self) -> io::Result<()> {
        self.save_to_path(&settings_file_path())
    }

    pub fn save_to_path(&self, settings_path: &Path) -> io::Result<()> {
        if let Some(settings_directory) = settings_path.parent() {
            fs::create_dir_all(settings_directory)?;
        }

        let settings_text = toml::to_string_pretty(self).unwrap_or_else(|_| String::new());
        fs::write(settings_path, settings_text)
    }

    fn migrate_legacy_defaults(&mut self) -> bool {
        if self.hotkey == LEGACY_DEFAULT_HOTKEY {
            self.hotkey = DEFAULT_HOTKEY.to_string();
            return true;
        }

        false
    }

    pub fn upsert_alias(
        &mut self,
        alias: CommandAliasSetting,
        replace_keyword: Option<&str>,
    ) -> bool {
        let keyword = alias.keyword.trim();
        let expands_to = alias.expands_to.trim();
        if keyword.is_empty() || expands_to.is_empty() {
            return false;
        }

        let normalized_alias = CommandAliasSetting {
            keyword: keyword.to_string(),
            expands_to: expands_to.to_string(),
        };

        if let Some(previous_keyword) = replace_keyword {
            self.aliases.retain(|entry| {
                !entry
                    .keyword
                    .trim()
                    .eq_ignore_ascii_case(previous_keyword.trim())
            });
        }

        self.aliases
            .retain(|entry| !entry.keyword.trim().eq_ignore_ascii_case(keyword));
        self.aliases.push(normalized_alias);
        self.aliases
            .sort_by(|left, right| left.keyword.to_lowercase().cmp(&right.keyword.to_lowercase()));
        true
    }

    pub fn remove_alias(&mut self, keyword: &str) -> bool {
        let before = self.aliases.len();
        self.aliases
            .retain(|entry| !entry.keyword.trim().eq_ignore_ascii_case(keyword.trim()));
        self.aliases.len() != before
    }

    pub fn upsert_custom_command(
        &mut self,
        custom_command: CustomCommandSetting,
        replace_name: Option<&str>,
    ) -> bool {
        let name = custom_command.name.trim();
        let command = custom_command.command.trim();
        if name.is_empty() || command.is_empty() {
            return false;
        }

        let normalized_command = CustomCommandSetting {
            name: name.to_string(),
            description: custom_command.description.trim().to_string(),
            command: command.to_string(),
            aliases: custom_command
                .aliases
                .into_iter()
                .map(|alias| alias.trim().to_string())
                .filter(|alias| !alias.is_empty())
                .collect(),
            hotkey: custom_command
                .hotkey
                .as_ref()
                .map(|hotkey| hotkey.trim().to_string())
                .filter(|hotkey| !hotkey.is_empty()),
            working_directory: custom_command.working_directory,
        };

        if let Some(previous_name) = replace_name {
            self.custom_commands.retain(|entry| {
                !entry.name.trim().eq_ignore_ascii_case(previous_name.trim())
            });
        }

        self.custom_commands
            .retain(|entry| !entry.name.trim().eq_ignore_ascii_case(name));
        self.custom_commands.push(normalized_command);
        self.custom_commands.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
        });
        true
    }

    pub fn remove_custom_command(&mut self, name: &str) -> bool {
        let before = self.custom_commands.len();
        self.custom_commands
            .retain(|entry| !entry.name.trim().eq_ignore_ascii_case(name.trim()));
        self.custom_commands.len() != before
    }

    pub fn upsert_query_hotkey(
        &mut self,
        hotkey: CommandHotkeySetting,
        replace_hotkey: Option<&str>,
    ) -> bool {
        let hotkey_text = hotkey.hotkey.trim();
        let query = hotkey.query.trim();
        if hotkey_text.is_empty() || query.is_empty() {
            return false;
        }

        let normalized = CommandHotkeySetting {
            hotkey: hotkey_text.to_string(),
            query: query.to_string(),
            description: hotkey.description.trim().to_string(),
        };

        if let Some(previous) = replace_hotkey {
            self.hotkeys
                .retain(|entry| !entry.hotkey.trim().eq_ignore_ascii_case(previous.trim()));
        }

        self.hotkeys
            .retain(|entry| !entry.hotkey.trim().eq_ignore_ascii_case(hotkey_text));
        self.hotkeys.push(normalized);
        self.hotkeys.sort_by(|left, right| {
            left.hotkey
                .to_lowercase()
                .cmp(&right.hotkey.to_lowercase())
        });
        true
    }

    pub fn remove_query_hotkey(&mut self, hotkey: &str) -> bool {
        let before = self.hotkeys.len();
        self.hotkeys
            .retain(|entry| !entry.hotkey.trim().eq_ignore_ascii_case(hotkey.trim()));
        self.hotkeys.len() != before
    }
}

pub fn settings_file_path() -> PathBuf {
    // Development convenience for `cargo run`:
    // If a config.toml exists in the current working directory (next to your Cargo.toml),
    // prefer it. This makes it easy to have a dev config without touching the
    // installed %APPDATA% one. In normal installed use this file usually doesn't exist,
    // so it falls back to the real user config in %APPDATA%\Core Launcher\config.toml.
    if let Ok(cwd) = env::current_dir() {
        let local = cwd.join("config.toml");
        if local.exists() {
            return local;
        }
    }

    crate::paths::config_file()
}

static CONFIG_DIRECTORY: OnceLock<PathBuf> = OnceLock::new();

pub fn config_directory() -> PathBuf {
    CONFIG_DIRECTORY
        .get_or_init(|| {
            env::var_os("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
                .join("Core Launcher")
        })
        .clone()
}
