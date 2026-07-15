use crate::secret_store;
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

#[derive(Clone, Deserialize, Serialize)]
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
    /// Optional Bungie API key for Destiny 2 features (@d2). Get one at https://www.bungie.net/en/Application
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bungie_api_key: Option<String>,
}

impl std::fmt::Debug for LauncherSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bungie_api_key = self
            .bungie_api_key
            .as_ref()
            .map(|_| "<redacted>")
            .unwrap_or("<none>");
        f.debug_struct("LauncherSettings")
            .field("local_timezone", &self.local_timezone)
            .field("home_currency", &self.home_currency)
            .field("backdrop_blur_enabled", &self.backdrop_blur_enabled)
            .field("hotkey_enabled", &self.hotkey_enabled)
            .field("hotkey", &self.hotkey)
            .field("launch_at_startup", &self.launch_at_startup)
            .field("preferred_terminal_profile", &self.preferred_terminal_profile)
            .field("index_start_menu", &self.index_start_menu)
            .field("index_user_files", &self.index_user_files)
            .field("show_web_search_result", &self.show_web_search_result)
            .field("clipboard_history_enabled", &self.clipboard_history_enabled)
            .field("aliases", &self.aliases)
            .field("custom_commands", &self.custom_commands)
            .field("hotkeys", &self.hotkeys)
            .field("quick_note_anchor", &self.quick_note_anchor)
            .field("quick_note_offset_x", &self.quick_note_offset_x)
            .field("quick_note_offset_y", &self.quick_note_offset_y)
            .field("quick_note_width", &self.quick_note_width)
            .field("quick_note_height", &self.quick_note_height)
            .field("display_position", &self.display_position)
            .field("bungie_api_key", &bungie_api_key)
            .finish()
    }
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
            bungie_api_key: None,
        }
    }
}

impl LauncherSettings {
    pub fn load_or_create() -> Self {
        let settings_path = settings_file_path();

        match fs::read_to_string(&settings_path) {
            Ok(settings_text) => {
                match toml::from_str::<LauncherSettings>(&settings_text) {
                    Ok(mut settings) => {
                        if settings.migrate_legacy_defaults() {
                            let _ = settings.save_to_path(&settings_path);
                        }
                        settings.merge_bungie_api_key_from_secret_store();
                        // Debug aid for dev (cargo run) — shows in console
                        let key_present = settings
                            .bungie_api_key
                            .as_ref()
                            .is_some_and(|k| !k.trim().is_empty());
                        eprintln!(
                            "Parsed config successfully. bungie_api_key present and non-empty: {}",
                            key_present
                        );
                        settings
                    }
                    Err(e) => {
                        eprintln!("WARNING: Failed to parse {} as TOML: {}", settings_path.display(), e);
                        eprintln!("Falling back to default settings (bungie_api_key will be missing). Fix the TOML syntax.");
                        Self::default()
                    }
                }
            }
            Err(_) => {
                let mut settings = Self::default();
                settings.merge_bungie_api_key_from_secret_store();
                let _ = settings.save_to_path(&settings_path);
                eprintln!(
                    "No existing config at {}, created default (no bungie_api_key).",
                    settings_path.display()
                );
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

        let mut persisted_settings = self.clone();
        persisted_settings.bungie_api_key = None;
        let settings_text =
            toml::to_string_pretty(&persisted_settings).unwrap_or_else(|_| String::new());
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
                !entry.keyword.trim().eq_ignore_ascii_case(previous_keyword.trim())
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
        self.aliases.retain(|entry| {
            !entry.keyword.trim().eq_ignore_ascii_case(keyword.trim())
        });
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
        self.custom_commands.retain(|entry| {
            !entry.name.trim().eq_ignore_ascii_case(name.trim())
        });
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

        let normalized_hotkey = CommandHotkeySetting {
            hotkey: hotkey_text.to_string(),
            query: query.to_string(),
            description: hotkey.description.trim().to_string(),
        };

        if let Some(previous_hotkey) = replace_hotkey {
            self.hotkeys.retain(|entry| {
                !entry.hotkey.trim().eq_ignore_ascii_case(previous_hotkey.trim())
            });
        }

        self.hotkeys
            .retain(|entry| !entry.hotkey.trim().eq_ignore_ascii_case(hotkey_text));
        self.hotkeys.push(normalized_hotkey);
        self.hotkeys.sort_by(|left, right| {
            left.hotkey
                .to_lowercase()
                .cmp(&right.hotkey.to_lowercase())
        });
        true
    }

    pub fn remove_query_hotkey(&mut self, hotkey: &str) -> bool {
        let before = self.hotkeys.len();
        self.hotkeys.retain(|entry| {
            !entry.hotkey.trim().eq_ignore_ascii_case(hotkey.trim())
        });
        self.hotkeys.len() != before
    }

    pub fn set_bungie_api_key(&mut self, api_key: Option<String>) {
        self.bungie_api_key = api_key
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty());

        match self.bungie_api_key.as_deref() {
            Some(key) => {
                let _ = secret_store::store_bungie_api_key(key);
            }
            None => {
                let _ = secret_store::delete_bungie_api_key();
            }
        }
    }

    fn merge_bungie_api_key_from_secret_store(&mut self) {
        if let Some(api_key) = secret_store::load_bungie_api_key() {
            self.bungie_api_key = Some(api_key);
            return;
        }

        if let Some(api_key) = self
            .bungie_api_key
            .as_ref()
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty())
        {
            let _ = secret_store::store_bungie_api_key(&api_key);
            self.bungie_api_key = Some(api_key);
        }
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
