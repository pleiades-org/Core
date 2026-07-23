//! Shared visual tokens for the Core launcher UI.
//!
//! Prefer these helpers over ad-hoc `rgb`/`rgba` literals so surfaces, type,
//! and category accents stay consistent across home, browse, and settings.

use crate::command::CommandCategory;
use gpui::{rgb, rgba, Rgba};

pub mod colors {
    use super::*;

    pub const SURFACE: u32 = 0x090909;
    pub const SURFACE_BLUR: u32 = 0x090909f7;
    pub const SURFACE_ELEVATED: u32 = 0x050505;
    pub const SURFACE_CARD: u32 = 0x0c0c0c;

    pub const OVERLAY_LOW: u32 = 0xffffff0a;
    pub const OVERLAY_MID: u32 = 0xffffff12;
    pub const OVERLAY_HIGH: u32 = 0xffffff1a;

    pub const BORDER_SUBTLE: u32 = 0xffffff14;
    pub const BORDER_MUTED: u32 = 0xffffff0c;
    pub const BORDER_STRONG: u32 = 0xffffff20;
    pub const BORDER_CARD: u32 = 0x171717;
    pub const BORDER_GLOW_HEX: u32 = 0xa78bfa15;

    pub fn border_glow() -> Rgba {
        rgba(BORDER_GLOW_HEX)
    }

    pub fn text_primary() -> Rgba {
        rgb(0xffffff)
    }

    pub fn text_secondary() -> Rgba {
        rgb(0xf4f4f5)
    }

    pub fn text_muted() -> Rgba {
        rgb(0xa1a1aa)
    }

    pub fn text_faint() -> Rgba {
        rgb(0x71717a)
    }

    pub fn text_disabled() -> Rgba {
        rgb(0x52525b)
    }

    pub fn accent() -> Rgba {
        rgb(0x7c3aed)
    }

    pub fn accent_soft() -> Rgba {
        rgb(0xa78bfa)
    }

    pub fn accent_hover() -> Rgba {
        rgb(0x6d28d9)
    }

    pub fn accent_border() -> Rgba {
        rgba(0xa78bfa80)
    }

    pub fn accent_glow() -> Rgba {
        rgba(0x7c3aed30)
    }

    pub fn danger() -> Rgba {
        rgb(0xfca5a5)
    }

    pub fn success() -> Rgba {
        rgb(0x22c55e)
    }

    pub fn warning() -> Rgba {
        rgb(0xfbbf24)
    }

    pub fn toggle_on_track() -> Rgba {
        accent()
    }

    pub fn toggle_off_track() -> Rgba {
        rgb(0x171717)
    }

    pub fn toggle_on_knob() -> Rgba {
        rgb(0xffffff)
    }

    pub fn toggle_off_knob() -> Rgba {
        rgb(0xd4d4d8)
    }
}

pub mod radii {
    pub const SM: f32 = 4.;
    pub const MD: f32 = 6.;
    pub const LG: f32 = 8.;
    pub const XL: f32 = 12.;
    pub const PILL: f32 = 999.;
}

pub mod spacing {
    pub const XS: f32 = 4.;
    pub const SM: f32 = 6.;
    pub const MD: f32 = 8.;
    pub const LG: f32 = 12.;
    pub const XL: f32 = 16.;
    pub const XXL: f32 = 24.;
}

pub mod type_scale {
    pub const CAPTION: f32 = 10.;
    pub const LABEL: f32 = 11.;
    pub const BODY_SM: f32 = 12.;
    pub const BODY: f32 = 13.;
    pub const BODY_LG: f32 = 14.;
    pub const TITLE: f32 = 16.;
    pub const TITLE_LG: f32 = 18.;
}

pub fn launcher_background(backdrop_blur_enabled: bool) -> Rgba {
    if backdrop_blur_enabled {
        rgba(colors::SURFACE_BLUR)
    } else {
        rgb(colors::SURFACE)
    }
}

pub fn surface_overlay_low() -> Rgba {
    rgba(colors::OVERLAY_LOW)
}

pub fn surface_overlay_mid() -> Rgba {
    rgba(colors::OVERLAY_MID)
}

pub fn surface_overlay_high() -> Rgba {
    rgba(colors::OVERLAY_HIGH)
}

pub fn surface_muted() -> Rgba {
    surface_overlay_low()
}

pub fn border_subtle() -> Rgba {
    rgba(colors::BORDER_SUBTLE)
}

pub fn border_muted() -> Rgba {
    rgba(colors::BORDER_MUTED)
}

pub fn elevated_surface_background() -> Rgba {
    rgb(colors::SURFACE_ELEVATED)
}

pub fn card_surface_background() -> Rgba {
    rgb(colors::SURFACE_CARD)
}

pub fn card_border() -> Rgba {
    rgb(colors::BORDER_CARD)
}

pub fn panel_surface_background() -> Rgba {
    surface_overlay_low()
}

pub fn result_row_background(is_selected: bool) -> Rgba {
    if is_selected {
        result_row_hover_background()
    } else {
        rgba(0x00000000)
    }
}

pub fn result_row_hover_background() -> Rgba {
    surface_overlay_mid()
}

pub fn result_row_badge_background() -> Rgba {
    rgba(0xffffff10)
}

pub fn input_field_background(focused: bool) -> Rgba {
    if focused {
        surface_overlay_mid()
    } else {
        surface_overlay_low()
    }
}

pub fn input_field_border(focused: bool) -> Rgba {
    if focused {
        colors::accent_border()
    } else {
        rgba(0x00000000)
    }
}

pub fn icon_button_background() -> Rgba {
    rgba(0x00000000)
}

pub fn icon_button_hover_background() -> Rgba {
    surface_overlay_mid()
}

pub fn settings_sidebar_background() -> Rgba {
    rgba(0x00000000)
}

pub fn settings_row_background() -> Rgba {
    rgba(0x00000000)
}

pub fn settings_row_hover_background() -> Rgba {
    surface_overlay_mid()
}

pub fn settings_row_selected_background() -> Rgba {
    surface_overlay_mid()
}

/// Single source of truth for category accent colours used by icons, badges, and labels.
#[derive(Clone, Copy)]
pub struct CategoryTheme {
    pub accent: Rgba,
    pub icon_bg: Rgba,
    pub icon_border: Rgba,
    pub badge_bg: Rgba,
    pub badge_border: Rgba,
}

pub fn category_theme(category: &CommandCategory) -> CategoryTheme {
    match category {
        CommandCategory::Application => CategoryTheme {
            accent: rgb(0x4ade80),
            icon_bg: rgba(0x22c55e14),
            icon_border: rgba(0x22c55e30),
            badge_bg: rgba(0x22c55e10),
            badge_border: rgba(0x22c55e25),
        },
        CommandCategory::Calculation => CategoryTheme {
            accent: rgb(0xfbbf24),
            icon_bg: rgba(0xf59e0b14),
            icon_border: rgba(0xf59e0b30),
            badge_bg: rgba(0xf59e0b10),
            badge_border: rgba(0xf59e0b25),
        },
        CommandCategory::File => CategoryTheme {
            accent: rgb(0xe4e4e7),
            icon_bg: rgba(0xffffff08),
            icon_border: rgba(0xffffff14),
            badge_bg: rgba(0xffffff06),
            badge_border: rgba(0xffffff0c),
        },
        CommandCategory::BuiltIn
        | CommandCategory::System
        | CommandCategory::WindowManagement
        | CommandCategory::Help => CategoryTheme {
            accent: colors::accent_soft(),
            icon_bg: rgba(0x7c3aed14),
            icon_border: rgba(0x7c3aed30),
            badge_bg: rgba(0x7c3aed10),
            badge_border: rgba(0x7c3aed25),
        },
        CommandCategory::Note | CommandCategory::Snippet => CategoryTheme {
            accent: rgb(0xf472b6),
            icon_bg: rgba(0xec489914),
            icon_border: rgba(0xec489930),
            badge_bg: rgba(0xec489910),
            badge_border: rgba(0xec489925),
        },
        CommandCategory::Web | CommandCategory::Quicklink => CategoryTheme {
            accent: rgb(0x60a5fa),
            icon_bg: rgba(0x3b82f614),
            icon_border: rgba(0x3b82f630),
            badge_bg: rgba(0x3b82f610),
            badge_border: rgba(0x3b82f625),
        },
        CommandCategory::Focus => CategoryTheme {
            accent: rgb(0xef4444),
            icon_bg: rgba(0xef444414),
            icon_border: rgba(0xef444430),
            badge_bg: rgba(0xef444410),
            badge_border: rgba(0xef444425),
        },
        CommandCategory::Clipboard => CategoryTheme {
            accent: rgb(0x14b8a6),
            icon_bg: rgba(0x14b8a614),
            icon_border: rgba(0x14b8a630),
            badge_bg: rgba(0x14b8a610),
            badge_border: rgba(0x14b8a625),
        },
        CommandCategory::Calendar => CategoryTheme {
            accent: rgb(0x818cf8),
            icon_bg: rgba(0x6366f114),
            icon_border: rgba(0x6366f130),
            badge_bg: rgba(0x6366f110),
            badge_border: rgba(0x6366f125),
        },
        CommandCategory::Emoji => CategoryTheme {
            accent: rgb(0xfb7185),
            icon_bg: rgba(0xf43f5e14),
            icon_border: rgba(0xf43f5e30),
            badge_bg: rgba(0xf43f5e10),
            badge_border: rgba(0xf43f5e25),
        },
        CommandCategory::Context => CategoryTheme {
            accent: rgb(0x94a3b8),
            icon_bg: rgba(0x94a3b814),
            icon_border: rgba(0x94a3b830),
            badge_bg: rgba(0x94a3b810),
            badge_border: rgba(0x94a3b825),
        },
        CommandCategory::DevTools => CategoryTheme {
            accent: rgb(0xfbbf24),
            icon_bg: rgba(0xf59e0b14),
            icon_border: rgba(0xf59e0b30),
            badge_bg: rgba(0xf59e0b10),
            badge_border: rgba(0xf59e0b25),
        },
        CommandCategory::Git => CategoryTheme {
            accent: rgb(0xf97316),
            icon_bg: rgba(0xf9731614),
            icon_border: rgba(0xf9731630),
            badge_bg: rgba(0xf9731610),
            badge_border: rgba(0xf9731625),
        },
        CommandCategory::Package => CategoryTheme {
            accent: rgb(0x34d399),
            icon_bg: rgba(0x10b98114),
            icon_border: rgba(0x10b98130),
            badge_bg: rgba(0x10b98110),
            badge_border: rgba(0x10b98125),
        },
        CommandCategory::Lookup => CategoryTheme {
            accent: rgb(0x60a5fa),
            icon_bg: rgba(0x3b82f614),
            icon_border: rgba(0x3b82f630),
            badge_bg: rgba(0x3b82f610),
            badge_border: rgba(0x3b82f625),
        },
        CommandCategory::Media => CategoryTheme {
            accent: rgb(0xe879f9),
            icon_bg: rgba(0xd946ef14),
            icon_border: rgba(0xd946ef30),
            badge_bg: rgba(0xd946ef10),
            badge_border: rgba(0xd946ef25),
        },
        CommandCategory::Network => CategoryTheme {
            accent: rgb(0x4ade80),
            icon_bg: rgba(0x22c55e14),
            icon_border: rgba(0x22c55e30),
            badge_bg: rgba(0x22c55e10),
            badge_border: rgba(0x22c55e25),
        },
    }
}

pub fn category_color(category: &CommandCategory) -> Rgba {
    category_theme(category).accent
}

pub fn category_label(category: &CommandCategory) -> &'static str {
    match category {
        CommandCategory::Calculation => "Calc",
        CommandCategory::Application => "App",
        CommandCategory::File => "File",
        CommandCategory::BuiltIn => "Core",
        CommandCategory::Web => "Web",
        CommandCategory::Help => "Help",
        CommandCategory::Note => "Note",
        CommandCategory::Focus => "Focus",
        CommandCategory::Clipboard => "Clip",
        CommandCategory::WindowManagement => "Window",
        CommandCategory::Snippet => "Snippet",
        CommandCategory::Quicklink => "Link",
        CommandCategory::Calendar => "Calendar",
        CommandCategory::System => "System",
        CommandCategory::Emoji => "Emoji",
        CommandCategory::Context => "Ctx",
        CommandCategory::DevTools => "Dev",
        CommandCategory::Git => "Git",
        CommandCategory::Package => "Pkg",
        CommandCategory::Lookup => "Lookup",
        CommandCategory::Media => "Media",
        CommandCategory::Network => "Net",
    }
}
