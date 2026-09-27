//! How each kind of settings control looks, so its kind is clear at a glance: switches for
//! on/off settings, filled buttons for actions. Dropdowns are native combo boxes.
use crate::windows::{
    painting,
    theme::{scale, Fonts, Palette},
};
use windows::Win32::{
    Foundation::{COLORREF, RECT},
    Graphics::Gdi::*,
};

/// Switch track size and the gap to the control's right edge, in 96-DPI pixels.
const SWITCH_WIDTH: i32 = 40;
const SWITCH_HEIGHT: i32 = 20;
const SWITCH_MARGIN: i32 = 14;
const RADIUS: i32 = 8;

/// What every control is drawn with.
#[derive(Clone, Copy)]
pub struct Look {
    pub dpi: u32,
    pub fonts: Fonts,
    pub palette: Palette,
}

/// A labelled on/off switch, like Windows Settings: the name on the left, "On" or "Off" and
/// the switch on the right. Highlighted while hovered or focused.
pub fn draw_toggle(context: HDC, area: RECT, label: &str, on: bool, highlighted: bool, look: Look) {
    let Look {
        dpi,
        fonts,
        palette,
    } = look;
    painting::fill(context, &area, palette.background);
    painting::rounded(
        context,
        &area,
        scale(RADIUS, dpi),
        if highlighted {
            palette.selected
        } else {
            palette.background
        },
    );
    if !highlighted {
        outline(context, area, scale(RADIUS, dpi), palette.selected);
    }
    let track_right = area.right - scale(SWITCH_MARGIN, dpi);
    let track_left = track_right - scale(SWITCH_WIDTH, dpi);
    let track_top = area.top + (area.bottom - area.top - scale(SWITCH_HEIGHT, dpi)) / 2;
    let track = painting::rectangle(
        track_left,
        track_top,
        track_right,
        track_top + scale(SWITCH_HEIGHT, dpi),
    );
    let track_radius = scale(SWITCH_HEIGHT, dpi) / 2;
    if on {
        painting::rounded(context, &track, track_radius, palette.accent);
    } else {
        outline(context, track, track_radius, palette.secondary);
    }
    let knob_size = scale(SWITCH_HEIGHT - 8, dpi);
    let knob_left = if on {
        track.right - scale(4, dpi) - knob_size
    } else {
        track.left + scale(4, dpi)
    };
    let knob_top = track.top + scale(4, dpi);
    painting::rounded(
        context,
        &painting::rectangle(
            knob_left,
            knob_top,
            knob_left + knob_size,
            knob_top + knob_size,
        ),
        knob_size / 2,
        if on {
            palette.background
        } else {
            palette.secondary
        },
    );
    let state_area = painting::rectangle(
        track_left - scale(40, dpi),
        area.top,
        track_left - scale(8, dpi),
        area.bottom,
    );
    painting::text(
        context,
        if on { "On" } else { "Off" },
        state_area,
        fonts.detail,
        palette.secondary,
    );
    let label_area = painting::rectangle(
        area.left + scale(16, dpi),
        area.top,
        state_area.left,
        area.bottom,
    );
    painting::text(context, label, label_area, fonts.detail, palette.text);
}

/// A button that does something when clicked: always filled, outlined in the accent colour
/// while hovered or focused. `swatch` shows the colour a preset applies.
pub fn draw_action(
    context: HDC,
    area: RECT,
    label: &str,
    swatch: Option<COLORREF>,
    highlighted: bool,
    look: Look,
) {
    let Look {
        dpi,
        fonts,
        palette,
    } = look;
    painting::fill(context, &area, palette.background);
    painting::rounded(context, &area, scale(RADIUS, dpi), palette.selected);
    if highlighted {
        outline(context, area, scale(RADIUS, dpi), palette.accent);
    }
    let mut text_left = area.left + scale(16, dpi);
    if let Some(color) = swatch {
        let size = scale(14, dpi);
        let top = area.top + (area.bottom - area.top - size) / 2;
        let swatch_area = painting::rectangle(text_left, top, text_left + size, top + size);
        painting::rounded(context, &swatch_area, size / 2, color);
        outline(context, swatch_area, size / 2, palette.secondary);
        text_left += size + scale(10, dpi);
    }
    painting::text(
        context,
        label,
        painting::rectangle(
            text_left,
            area.top,
            area.right - scale(12, dpi),
            area.bottom,
        ),
        fonts.detail,
        palette.text,
    );
}

/// A one-pixel rounded border.
fn outline(context: HDC, area: RECT, radius: i32, color: COLORREF) {
    unsafe {
        let state = SaveDC(context);
        let pen = CreatePen(PS_SOLID, 1, color);
        SelectObject(context, pen.into());
        SelectObject(context, GetStockObject(NULL_BRUSH));
        let _ = RoundRect(
            context,
            area.left,
            area.top,
            area.right - 1,
            area.bottom - 1,
            radius * 2,
            radius * 2,
        );
        let _ = RestoreDC(context, state);
        let _ = DeleteObject(pen.into());
    }
}
