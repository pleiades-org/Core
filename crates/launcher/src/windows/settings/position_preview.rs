use super::ScreenPosition;
use crate::windows::{
    painting,
    theme::{scale, Fonts, Palette},
    window_placement,
};
use windows::Win32::UI::Controls::DRAWITEMSTRUCT;

/// A small screen and anchored launcher box make each position visible at a glance.
pub fn draw(
    item: &DRAWITEMSTRUCT,
    position: ScreenPosition,
    selected: bool,
    dpi: u32,
    fonts: Fonts,
    palette: Palette,
) {
    let mut screen = painting::rectangle(
        item.rcItem.left + scale(9, dpi),
        item.rcItem.top + scale(8, dpi),
        item.rcItem.left + scale(49, dpi),
        item.rcItem.top + scale(34, dpi),
    );
    painting::rounded(item.hDC, &screen, scale(3, dpi), palette.secondary);
    let inset = scale(2, dpi);
    screen.left += inset;
    screen.top += inset;
    screen.right -= inset;
    screen.bottom -= inset;
    painting::fill(item.hDC, &screen, palette.background);
    let launcher = window_placement::bounds(screen, scale(16, dpi), scale(10, dpi), position);
    let color = if selected {
        palette.accent
    } else {
        palette.secondary
    };
    painting::fill(item.hDC, &launcher, color);
    let mut label = item.rcItem;
    label.left += scale(58, dpi);
    label.right -= scale(6, dpi);
    painting::text(item.hDC, position.name(), label, fonts.detail, color);
}
