use super::{CornerRadius, DisplayChoice, EdgeSpacing, Shortcut};
use core_engine::search::ShellKind;
use std::fmt;
use windows::Win32::Foundation::COLORREF;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BackgroundColor(u32);

impl BackgroundColor {
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim().strip_prefix('#').unwrap_or(text.trim());
        if text.len() != 6 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("Enter a six-digit color, such as #000000 or #182030.".into());
        }
        u32::from_str_radix(text, 16)
            .map(Self)
            .map_err(|error| format!("Could not read background color: {error}"))
    }

    pub fn channels(self) -> [u32; 3] {
        [self.0 >> 16, (self.0 >> 8) & 255, self.0 & 255]
    }

    pub fn native(self) -> COLORREF {
        let [red, green, blue] = self.channels();
        COLORREF(red | (green << 8) | (blue << 16))
    }
}

impl fmt::Display for BackgroundColor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "#{:06X}", self.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScreenPosition {
    #[default]
    Center,
    Top,
    Bottom,
    Left,
    Right,
    BottomLeft,
    BottomRight,
}

impl ScreenPosition {
    pub const ALL: [Self; 7] = [
        Self::Center,
        Self::Top,
        Self::Bottom,
        Self::Left,
        Self::Right,
        Self::BottomLeft,
        Self::BottomRight,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Center => "Center",
            Self::Top => "Top",
            Self::Bottom => "Bottom",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::BottomLeft => "Bottom left",
            Self::BottomRight => "Bottom right",
        }
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|position| position.name().eq_ignore_ascii_case(text))
            .ok_or_else(|| "Choose one of the seven screen positions.".into())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Preferences {
    pub background: BackgroundColor,
    pub position: ScreenPosition,
    pub shortcut: Shortcut,
    pub display: DisplayChoice,
    pub startup: bool,
    pub corner_radius: CornerRadius,
    pub edge_spacing: EdgeSpacing,
    /// Shell for `/` commands; `Default` follows the terminal's default profile.
    pub shell: ShellKind,
}

impl Preferences {
    pub fn encode(self) -> String {
        let mut text = format!(
            "version=1\nbackground={}\nposition={}\nshortcut={}\ndisplay={}\nstartup={}\n",
            self.background,
            self.position.name(),
            self.shortcut,
            self.display,
            self.startup
        );
        // Default metrics are omitted so files stay readable by builds without these fields.
        if self.corner_radius != CornerRadius::default() {
            text.push_str(&format!("corner_radius={}\n", self.corner_radius));
        }
        if self.edge_spacing != EdgeSpacing::default() {
            text.push_str(&format!("edge_spacing={}\n", self.edge_spacing));
        }
        if self.shell != ShellKind::Default {
            text.push_str(&format!("shell={}\n", self.shell.id()));
        }
        text
    }

    pub fn decode(text: &str) -> Result<Self, String> {
        let mut settings = Self::default();
        let mut fields = 0_u16;
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let (key, value) = line
                .split_once('=')
                .ok_or("Settings must use key=value lines.")?;
            let field = match key.trim() {
                "version" if value.trim() == "1" => 1,
                "version" => return Err("This settings version is not supported.".into()),
                "background" => {
                    settings.background = BackgroundColor::parse(value)?;
                    2
                }
                "position" => {
                    settings.position = ScreenPosition::parse(value.trim())?;
                    4
                }
                "shortcut" => {
                    settings.shortcut = Shortcut::parse(value.trim())?;
                    8
                }
                "display" => {
                    settings.display = DisplayChoice::parse(value.trim())?;
                    16
                }
                "startup" => {
                    settings.startup = value
                        .trim()
                        .parse()
                        .map_err(|_| "Startup must be true or false.")?;
                    32
                }
                "corner_radius" => {
                    settings.corner_radius = CornerRadius::parse(value)?;
                    64
                }
                "edge_spacing" => {
                    settings.edge_spacing = EdgeSpacing::parse(value)?;
                    128
                }
                "shell" => {
                    settings.shell = ShellKind::parse(value)
                        .ok_or("Shell must be default, cmd, powershell, pwsh, wsl or gitbash.")?;
                    256
                }
                // Newer builds may add fields. Ignore them so an older build can still load
                // and save; the unknown value is dropped on the next save.
                unknown => {
                    eprintln!("Ignoring unknown settings field: {unknown}");
                    continue;
                }
            };
            if fields & field != 0 {
                return Err("Settings contain a duplicate field.".into());
            }
            fields |= field;
        }
        if fields & 7 != 7 {
            return Err("Settings are missing a required field.".into());
        }
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_and_settings_round_trip_without_confusing_rgb_and_native_bgr() {
        let color = BackgroundColor::parse(" #12abEF ").unwrap();
        assert_eq!(color.to_string(), "#12ABEF");
        assert_eq!(color.native(), COLORREF(0xEFAB12));
        for position in ScreenPosition::ALL {
            let settings = Preferences {
                background: color,
                position,
                ..Preferences::default()
            };
            assert_eq!(Preferences::decode(&settings.encode()), Ok(settings));
        }
    }

    #[test]
    fn window_metrics_round_trip_and_default_when_absent() {
        let settings = Preferences {
            corner_radius: CornerRadius::new(0),
            edge_spacing: EdgeSpacing::new(64),
            ..Preferences::default()
        };
        assert_eq!(Preferences::decode(&settings.encode()), Ok(settings));
        let defaults = Preferences::default().encode();
        assert!(!defaults.contains("corner_radius") && !defaults.contains("edge_spacing"));
        let legacy = Preferences::decode("version=1\nbackground=#000000\nposition=Center").unwrap();
        assert_eq!(legacy.corner_radius, CornerRadius::default());
        assert_eq!(legacy.edge_spacing, EdgeSpacing::default());
        for text in [
            "version=1\nbackground=#000000\nposition=Center\ncorner_radius=33",
            "version=1\nbackground=#000000\nposition=Center\nedge_spacing=-4",
            "version=1\nbackground=#000000\nposition=Center\nedge_spacing=1\nedge_spacing=2",
        ] {
            assert!(Preferences::decode(text).is_err(), "{text}");
        }
    }

    #[test]
    fn the_command_shell_round_trips_and_defaults_when_absent() {
        for shell in ShellKind::ALL {
            let settings = Preferences {
                shell,
                ..Preferences::default()
            };
            assert_eq!(Preferences::decode(&settings.encode()), Ok(settings));
        }
        assert!(!Preferences::default().encode().contains("shell="));
        assert!(
            Preferences::decode("version=1\nbackground=#000000\nposition=Top\nshell=fish").is_err()
        );
    }

    #[test]
    fn unknown_fields_from_newer_builds_are_ignored() {
        let settings =
            Preferences::decode("version=1\nbackground=#000000\nposition=Top\nfuture_option=7")
                .unwrap();
        assert_eq!(settings.position, ScreenPosition::Top);
    }

    #[test]
    fn rejects_invalid_colors_versions_and_ambiguous_settings() {
        for text in ["", "#fff", "#12345678", "GG0000", "💜123"] {
            assert!(BackgroundColor::parse(text).is_err());
        }
        for text in [
            "",
            "version=2\nbackground=#000000\nposition=Center",
            "version=1\nposition=bad",
            "version=1\nbackground=#000000\nposition=Left\nposition=Right",
        ] {
            assert!(Preferences::decode(text).is_err());
        }
    }
}
