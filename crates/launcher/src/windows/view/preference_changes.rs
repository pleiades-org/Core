//! What a change of preferences must update, so a preview step redoes only that work.
use crate::windows::{settings::Preferences, theme::Palette};

/// Work that applying preferences needs; anything not flagged is left as it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct PreferenceChanges {
    /// New brushes and palette, then every control repaints in the new colors.
    pub palette: bool,
    /// Dark and light swapped, so native scroll bars and dropdowns need the other theme.
    pub theme: bool,
    /// The chosen display changed; its screen area is read again.
    pub screen: bool,
    /// The window moves or resizes: position, edge spacing or display.
    pub layout: bool,
    /// Corner rounding changed; a layout also re-clips, so this matters only without one.
    pub corners: bool,
}

impl PreferenceChanges {
    /// Everything, for the first appearance and for leaving Settings.
    pub const ALL: Self = Self {
        palette: true,
        theme: true,
        screen: true,
        layout: true,
        corners: true,
    };

    pub fn between(old: Preferences, new: Preferences) -> Self {
        let palette = old.background != new.background;
        let screen = old.display != new.display;
        Self {
            palette,
            theme: palette
                && Palette::for_background(old.background).is_dark()
                    != Palette::for_background(new.background).is_dark(),
            screen,
            layout: screen || old.position != new.position || old.edge_spacing != new.edge_spacing,
            corners: old.corner_radius != new.corner_radius,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::settings::{
        BackgroundColor, CornerRadius, DisplayChoice, EdgeSpacing, ScreenPosition, Shortcut,
    };
    use core_engine::search::ShellKind;

    fn color(text: &str) -> BackgroundColor {
        BackgroundColor::parse(text).unwrap()
    }

    #[test]
    fn unchanged_or_behaviour_only_preferences_need_no_work() {
        let old = Preferences::default();
        assert_eq!(
            PreferenceChanges::between(old, old),
            PreferenceChanges::default()
        );
        let behaviour = Preferences {
            shortcut: Shortcut::parse("Ctrl+Shift+F11").unwrap(),
            startup: !old.startup,
            shell: ShellKind::ALL[ShellKind::ALL.len() - 1],
            ..old
        };
        assert_eq!(
            PreferenceChanges::between(old, behaviour),
            PreferenceChanges::default()
        );
    }

    #[test]
    fn corner_rounding_alone_only_reclips() {
        let old = Preferences::default();
        let rounder = Preferences {
            corner_radius: CornerRadius::new(u32::from(CornerRadius::MAX)),
            ..old
        };
        assert_ne!(old.corner_radius, rounder.corner_radius);
        assert_eq!(
            PreferenceChanges::between(old, rounder),
            PreferenceChanges {
                corners: true,
                ..PreferenceChanges::default()
            }
        );
    }

    #[test]
    fn colors_repaint_and_swap_theme_only_across_dark_and_light() {
        let black = Preferences::default();
        let navy = Preferences {
            background: color("#182030"),
            ..black
        };
        let light = Preferences {
            background: color("#F5F5F5"),
            ..black
        };
        assert_eq!(
            PreferenceChanges::between(black, navy),
            PreferenceChanges {
                palette: true,
                ..PreferenceChanges::default()
            }
        );
        for (old, new) in [(navy, light), (light, black)] {
            assert_eq!(
                PreferenceChanges::between(old, new),
                PreferenceChanges {
                    palette: true,
                    theme: true,
                    ..PreferenceChanges::default()
                }
            );
        }
    }

    #[test]
    fn placement_changes_lay_out_and_only_a_display_change_rereads_the_screen() {
        let old = Preferences::default();
        for new in [
            Preferences {
                position: ScreenPosition::BottomRight,
                ..old
            },
            Preferences {
                edge_spacing: EdgeSpacing::new(64),
                ..old
            },
        ] {
            assert_eq!(
                PreferenceChanges::between(old, new),
                PreferenceChanges {
                    layout: true,
                    ..PreferenceChanges::default()
                }
            );
        }
        let display = Preferences {
            display: DisplayChoice::parse(r"\\.\DISPLAY2").unwrap(),
            ..old
        };
        assert_ne!(old.display, display.display);
        assert_eq!(
            PreferenceChanges::between(old, display),
            PreferenceChanges {
                screen: true,
                layout: true,
                ..PreferenceChanges::default()
            }
        );
    }

    #[test]
    fn several_changes_combine() {
        let old = Preferences::default();
        let new = Preferences {
            background: color("#FFFFFF"),
            position: ScreenPosition::Top,
            corner_radius: CornerRadius::new(0),
            ..old
        };
        assert_eq!(
            PreferenceChanges::between(old, new),
            PreferenceChanges {
                palette: true,
                theme: true,
                screen: false,
                layout: true,
                corners: true,
            }
        );
    }
}
