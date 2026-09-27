use crate::windows::theme::scale;
use windows::Win32::Foundation::RECT;

pub const PAGE_WIDTH: i32 = 840;
pub const PAGE_HEIGHT: i32 = 556;
/// Top of the Done / Retry row shared by every section.
pub const FOOTER_TOP: i32 = 488;

// Vertical rhythm of the content column, in 96-DPI pixels.
pub const HEADING_TOP: i32 = 26;
pub const DESCRIPTION_TOP: i32 = 64;
pub const FIRST_LABEL_TOP: i32 = 104;
/// Buttons in the first row; the text box inside them sits 6 px lower.
pub const FIRST_ROW_TOP: i32 = 140;
pub const FIRST_INPUT_TOP: i32 = 146;
pub const POSITION_LABEL_TOP: i32 = 206;
pub const POSITION_GRID_TOP: i32 = 240;
pub const POSITION_COLUMN_PITCH: i32 = 150;
pub const POSITION_ROW_PITCH: i32 = 48;
pub const POSITION_COLUMNS: usize = 4;
pub const SLIDER_LABEL_TOP: i32 = 346;
pub const SLIDER_TOP: i32 = 374;
pub const SLIDER_VALUE_TOP: i32 = 406;
pub const STATUS_TOP: i32 = 436;
pub const SHORTCUT_HELP_TOP: i32 = 190;
pub const DISPLAY_LABEL_TOP: i32 = 232;
pub const DISPLAY_TOP: i32 = 264;
pub const STARTUP_TOP: i32 = 312;
/// Command shell and history controls in Behaviour, below their heading.
pub const COMMANDS_LABEL_TOP: i32 = 360;
pub const COMMANDS_TOP: i32 = 386;
pub const SIDEBAR_FIRST_TOP: i32 = 100;
pub const SIDEBAR_PITCH: i32 = 50;
pub const SIDEBAR_INSET: i32 = 16;

// Control sizes.
pub const LABEL_WIDTH: i32 = 350;
pub const LABEL_HEIGHT: i32 = 22;
pub const INPUT_HEIGHT: i32 = 28;
pub const BUTTON_HEIGHT: i32 = 40;
pub const SIDEBAR_BUTTON_HEIGHT: i32 = 42;
pub const INPUT_INSET: i32 = 12;
pub const SWATCH_LEFT: i32 = 170;
pub const SWATCH_WIDTH: i32 = 124;
pub const SWATCH_PITCH: i32 = 134;
pub const POSITION_WIDTH: i32 = 142;
pub const POSITION_HEIGHT: i32 = 42;
pub const SIDEBAR_WIDTH: i32 = 176;
pub const CONTENT_LEFT: i32 = 208;
pub const CONTENT_RIGHT: i32 = 812;

/// Keep the whole settings panel accessible when Windows scaling exceeds the work area.
pub fn fitting_dpi(preferred: u32, work_area: RECT) -> u32 {
    let available_width = (work_area.right - work_area.left).max(1) as u32;
    let available_height = (work_area.bottom - work_area.top).max(1) as u32;
    preferred
        .min(available_width.saturating_mul(96) / PAGE_WIDTH as u32)
        .min(available_height.saturating_mul(96) / PAGE_HEIGHT as u32)
        .max(1)
}

pub fn area(left: i32, top: i32, width: i32, height: i32, dpi: u32) -> RECT {
    RECT {
        left: scale(left, dpi),
        top: scale(top, dpi),
        right: scale(left + width, dpi),
        bottom: scale(top + height, dpi),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_fit_the_available_work_area_at_large_display_scales() {
        for (width, height, preferred) in [
            (2560, 1440, 384),
            (1280, 680, 192),
            (1920, 1040, 144),
            (640, 440, 96),
        ] {
            let dpi = fitting_dpi(
                preferred,
                RECT {
                    left: -width,
                    top: 0,
                    right: 0,
                    bottom: height,
                },
            );
            assert!(dpi <= preferred);
            assert!(scale(PAGE_WIDTH, dpi) <= width);
            assert!(scale(PAGE_HEIGHT, dpi) <= height);
        }
    }
}
