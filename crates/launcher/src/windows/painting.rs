use super::{
    application_icon::ApplicationIcon,
    theme::{self, scale},
    wide,
};
pub use core_engine::search::ResultKind;
use core_engine::{
    media::VolumeLevel,
    search::{Action, SearchResult},
};
use std::sync::Arc;
use windows::Win32::{Foundation::*, Graphics::Gdi::*};

/// The speakers of the icon font: a row of the volume mixer that has no program's icon.
const SPEAKERS_GLYPH: &str = "\u{e7f5}";
/// The size the icon font draws a glyph at, to centre one in its area. In 96-DPI pixels.
const GLYPH_SIZE: i32 = 20;
/// Where a row's text starts, and how far before the row's end it stops: the room at the end
/// is the Enter hint's. In 96-DPI pixels.
pub const ROW_TEXT_LEFT: i32 = 56;
const ROW_HINT_WIDTH: i32 = 40;

pub struct DisplayRow {
    pub identifier: Arc<str>,
    pub icon: Option<Arc<ApplicationIcon>>,
    pub title: Arc<str>,
    pub detail: String,
    pub kind: ResultKind,
    /// A row of the volume mixer carries its level, drawn as a slider.
    pub volume: Option<VolumeLevel>,
}

impl DisplayRow {
    pub fn new(result: &SearchResult) -> Self {
        let detail = match result.kind {
            ResultKind::Application | ResultKind::Recent => application_detail(&result.description),
            ResultKind::Web => "Open in your default browser".to_owned(),
            ResultKind::Calculator
            | ResultKind::Date
            | ResultKind::Conversion
            | ResultKind::Time
            | ResultKind::Command
            | ResultKind::Quicklink
            | ResultKind::Power
            | ResultKind::System
            | ResultKind::Terminal
            | ResultKind::Media
            | ResultKind::Volume => result.description.to_string(),
        };
        Self {
            identifier: result.id.clone(),
            icon: None,
            title: result.title.clone(),
            detail,
            kind: result.kind,
            volume: match &result.action {
                Action::Mixer { level, .. } => Some(*level),
                _ => None,
            },
        }
    }
}

fn application_detail(description: &str) -> String {
    let folder = description.rsplit(['/', '\\']).next().unwrap_or("");
    if folder.is_empty() || folder == "Programs" {
        "Application".into()
    } else {
        format!("Application · {folder}")
    }
}

pub fn rectangle(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left,
        top,
        right,
        bottom,
    }
}

/// Whether two rectangles share any pixel; an empty rectangle shares none.
pub fn intersects(first: &RECT, second: &RECT) -> bool {
    first.left.max(second.left) < first.right.min(second.right)
        && first.top.max(second.top) < first.bottom.min(second.bottom)
}

pub fn fill(context: HDC, area: &RECT, color: COLORREF) {
    unsafe {
        SetDCBrushColor(context, color);
        FillRect(context, area, HBRUSH(GetStockObject(DC_BRUSH).0));
    }
}

/// Blends `color` over what is already drawn in `area`, at `opacity` of 255: a flat colour has
/// no shape of its own, so one pixel of it stretched over the area is enough.
pub fn veil(context: HDC, area: &RECT, color: COLORREF, opacity: u8) {
    unsafe {
        let source = CreateCompatibleDC(Some(context));
        let pixel = CreateCompatibleBitmap(context, 1, 1);
        if !source.0.is_null() && !pixel.0.is_null() {
            let previous = SelectObject(source, pixel.into());
            let _ = SetPixelV(source, 0, 0, color);
            let _ = AlphaBlend(
                context,
                area.left,
                area.top,
                area.right - area.left,
                area.bottom - area.top,
                source,
                0,
                0,
                1,
                1,
                BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: opacity,
                    AlphaFormat: 0,
                },
            );
            SelectObject(source, previous);
        }
        let _ = DeleteObject(pixel.into());
        let _ = DeleteDC(source);
    }
}

/// Draws `area` off screen and copies it over in one step, so a control that repaints as fast
/// as the pointer moves never shows a half-drawn frame. `draw` uses the same coordinates as on
/// screen; if no off-screen surface can be made, it draws on screen directly.
pub fn buffered(context: HDC, area: &RECT, draw: impl FnOnce(HDC)) {
    let width = area.right - area.left;
    let height = area.bottom - area.top;
    unsafe {
        let memory = CreateCompatibleDC(Some(context));
        let surface = CreateCompatibleBitmap(context, width.max(1), height.max(1));
        if memory.0.is_null() || surface.0.is_null() {
            let _ = DeleteObject(surface.into());
            let _ = DeleteDC(memory);
            draw(context);
            return;
        }
        let previous = SelectObject(memory, surface.into());
        let _ = SetViewportOrgEx(memory, -area.left, -area.top, None);
        draw(memory);
        let _ = SetViewportOrgEx(memory, 0, 0, None);
        let _ = BitBlt(
            context,
            area.left,
            area.top,
            width,
            height,
            Some(memory),
            0,
            0,
            SRCCOPY,
        );
        SelectObject(memory, previous);
        let _ = DeleteObject(surface.into());
        let _ = DeleteDC(memory);
    }
}

pub fn rounded(context: HDC, area: &RECT, radius: i32, fill: COLORREF) {
    unsafe {
        let state = SaveDC(context);
        SelectObject(context, GetStockObject(DC_BRUSH));
        SelectObject(context, GetStockObject(NULL_PEN));
        SetDCBrushColor(context, fill);
        let _ = RoundRect(
            context,
            area.left,
            area.top,
            area.right,
            area.bottom,
            radius * 2,
            radius * 2,
        );
        let _ = RestoreDC(context, state);
    }
}

pub fn text(context: HDC, label: &str, mut area: RECT, font: HFONT, color: COLORREF) {
    let mut text = wide(label);
    text.pop();
    unsafe {
        let state = SaveDC(context);
        SelectObject(context, font.into());
        SetBkMode(context, TRANSPARENT);
        SetTextColor(context, color);
        DrawTextW(
            context,
            &mut text,
            &mut area,
            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        let _ = RestoreDC(context, state);
    }
}

/// A glyph of the icon font, centred in `area`.
pub fn centred_glyph(
    context: HDC,
    area: RECT,
    glyph: &str,
    dpi: u32,
    font: HFONT,
    color: COLORREF,
) {
    let mut glyph_area = area;
    glyph_area.left += (area.right - area.left - scale(GLYPH_SIZE, dpi)) / 2;
    text(context, glyph, glyph_area, font, color);
}

pub fn line(context: HDC, start: (i32, i32), end: (i32, i32), color: COLORREF) {
    unsafe {
        let state = SaveDC(context);
        SelectObject(context, GetStockObject(DC_PEN));
        SetDCPenColor(context, color);
        let _ = MoveToEx(context, start.0, start.1, None);
        let _ = LineTo(context, end.0, end.1);
        let _ = RestoreDC(context, state);
    }
}

pub fn search_icon(context: HDC, left: i32, top: i32, dpi: u32, color: COLORREF) {
    unsafe {
        let state = SaveDC(context);
        SelectObject(context, GetStockObject(NULL_BRUSH));
        SelectObject(context, GetStockObject(DC_PEN));
        SetDCPenColor(context, color);
        let _ = Ellipse(
            context,
            left,
            top,
            left + scale(14, dpi),
            top + scale(14, dpi),
        );
        line(
            context,
            (left + scale(12, dpi), top + scale(12, dpi)),
            (left + scale(19, dpi), top + scale(19, dpi)),
            color,
        );
        let _ = RestoreDC(context, state);
    }
}

pub fn result_row(
    context: HDC,
    area: RECT,
    row: &DisplayRow,
    selected: bool,
    fonts: theme::Fonts,
    dpi: u32,
    palette: theme::Palette,
) {
    row_surface(context, &area, selected, dpi, palette);
    if is_answer(row.kind) {
        answer_row(context, area, row, fonts, dpi, palette);
        return;
    }
    let right = area.right - scale(ROW_HINT_WIDTH, dpi);
    row_identity(context, area, right, row, fonts, dpi, palette);
    if selected {
        text(
            context,
            "↵",
            rectangle(
                right + scale(10, dpi),
                area.top,
                area.right - scale(10, dpi),
                area.bottom,
            ),
            fonts.title,
            palette.accent,
        );
    }
}

/// A row's background, and the rounded highlight when it is selected.
pub fn row_surface(context: HDC, area: &RECT, selected: bool, dpi: u32, palette: theme::Palette) {
    fill(context, area, palette.background);
    let inset = scale(2, dpi);
    let surface = rectangle(area.left, area.top + inset, area.right, area.bottom - inset);
    if selected {
        rounded(context, &surface, scale(9, dpi), palette.selected);
    }
}

/// A row's icon, and beside it the title over the detail, as far as `right`.
pub fn row_identity(
    context: HDC,
    area: RECT,
    right: i32,
    row: &DisplayRow,
    fonts: theme::Fonts,
    dpi: u32,
    palette: theme::Palette,
) {
    let icon_area = rectangle(
        area.left + scale(12, dpi),
        area.top + scale(11, dpi),
        area.left + scale(42, dpi),
        area.top + scale(41, dpi),
    );
    rounded(context, &icon_area, scale(7, dpi), palette.background);
    let icon_drawn = row
        .icon
        .as_ref()
        .is_some_and(|icon| icon.draw(context, icon_area, scale(26, dpi)));
    if !icon_drawn {
        result_icon(context, icon_area, row.kind, fonts, dpi, palette.accent);
    }
    let left = area.left + scale(ROW_TEXT_LEFT, dpi);
    text(
        context,
        &row.title,
        rectangle(
            left,
            area.top + scale(7, dpi),
            right,
            area.top + scale(29, dpi),
        ),
        fonts.title,
        palette.text,
    );
    text(
        context,
        &row.detail,
        rectangle(
            left,
            area.top + scale(29, dpi),
            right,
            area.bottom - scale(5, dpi),
        ),
        fonts.detail,
        palette.secondary,
    );
}

/// One tile of the recently used grid: the icon centred at the top, the name below it on up to
/// two lines, and a rounded highlight when hovered or selected.
pub fn app_tile(
    context: HDC,
    area: RECT,
    row: &DisplayRow,
    highlighted: bool,
    fonts: theme::Fonts,
    dpi: u32,
    palette: theme::Palette,
) {
    fill(context, &area, palette.background);
    if highlighted {
        let inset = scale(3, dpi);
        let surface = rectangle(
            area.left + inset,
            area.top + inset,
            area.right - inset,
            area.bottom - inset,
        );
        rounded(context, &surface, scale(8, dpi), palette.selected);
    }
    let icon_size = scale(theme::GRID_ICON, dpi);
    let icon_top = area.top + scale(10, dpi);
    let icon_left = area.left + (area.right - area.left - icon_size) / 2;
    let icon_area = rectangle(
        icon_left,
        icon_top,
        icon_left + icon_size,
        icon_top + icon_size,
    );
    let icon_drawn = row
        .icon
        .as_ref()
        .is_some_and(|icon| icon.draw(context, icon_area, icon_size));
    if !icon_drawn {
        result_icon(context, icon_area, row.kind, fonts, dpi, palette.accent);
    }
    let label_area = rectangle(
        area.left + scale(6, dpi),
        icon_top + icon_size + scale(6, dpi),
        area.right - scale(6, dpi),
        area.bottom - scale(2, dpi),
    );
    wrapped_text(context, &row.title, label_area, fonts.label, palette.text);
}

/// Centred text that wraps at word boundaries, ending in an ellipsis when it does not fit.
fn wrapped_text(context: HDC, label: &str, mut area: RECT, font: HFONT, color: COLORREF) {
    let mut text = wide(label);
    text.pop();
    unsafe {
        let state = SaveDC(context);
        SelectObject(context, font.into());
        SetBkMode(context, TRANSPARENT);
        SetTextColor(context, color);
        DrawTextW(
            context,
            &mut text,
            &mut area,
            DT_CENTER | DT_WORDBREAK | DT_EDITCONTROL | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        let _ = RestoreDC(context, state);
    }
}

pub fn is_answer(kind: ResultKind) -> bool {
    matches!(
        kind,
        ResultKind::Calculator | ResultKind::Date | ResultKind::Conversion | ResultKind::Time
    )
}

fn answer_row(
    context: HDC,
    area: RECT,
    row: &DisplayRow,
    fonts: theme::Fonts,
    dpi: u32,
    palette: theme::Palette,
) {
    let left = area.left + scale(18, dpi);
    let right = area.right - scale(18, dpi);
    let title_area = rectangle(
        left,
        area.top + scale(12, dpi),
        right,
        area.top + scale(63, dpi),
    );
    let title = wide(&row.title);
    let mut extent = SIZE::default();
    let font = unsafe {
        let previous = SelectObject(context, fonts.answer.into());
        let measured =
            GetTextExtentPoint32W(context, &title[..title.len() - 1], &mut extent).as_bool();
        SelectObject(context, previous);
        if measured && extent.cx > right - left {
            fonts.input
        } else {
            fonts.answer
        }
    };
    text(context, &row.title, title_area, font, palette.text);
    text(
        context,
        &row.detail,
        rectangle(
            left,
            area.top + scale(66, dpi),
            right,
            area.bottom - scale(14, dpi),
        ),
        fonts.detail,
        palette.secondary,
    );
}

fn result_icon(
    context: HDC,
    area: RECT,
    kind: ResultKind,
    fonts: theme::Fonts,
    dpi: u32,
    color: COLORREF,
) {
    if kind == ResultKind::Volume {
        centred_glyph(context, area, SPEAKERS_GLYPH, dpi, fonts.icon, color);
        return;
    }
    let left = area.left + scale(9, dpi);
    let top = area.top + scale(9, dpi);
    if matches!(kind, ResultKind::Application | ResultKind::Recent) {
        for (horizontal, vertical) in [(0, 0), (7, 0), (0, 7), (7, 7)] {
            let tile = rectangle(
                left + scale(horizontal, dpi),
                top + scale(vertical, dpi),
                left + scale(horizontal + 4, dpi),
                top + scale(vertical + 4, dpi),
            );
            fill(context, &tile, color);
        }
    } else {
        let label = match kind {
            ResultKind::Calculator => "=",
            ResultKind::Time => "◷",
            ResultKind::Web | ResultKind::Quicklink => "↗",
            ResultKind::Media => "♪",
            _ => ">",
        };
        text(
            context,
            label,
            rectangle(left, area.top, area.right, area.bottom),
            fonts.title,
            color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4 x 4 off-screen surface filled with `color`, for reading back what was drawn on it.
    struct Surface {
        context: HDC,
        bitmap: HBITMAP,
        previous: HGDIOBJ,
    }

    impl Surface {
        const SIZE: i32 = 4;

        fn filled(color: COLORREF) -> Self {
            unsafe {
                let screen = GetDC(None);
                let context = CreateCompatibleDC(Some(screen));
                let bitmap = CreateCompatibleBitmap(screen, Self::SIZE, Self::SIZE);
                ReleaseDC(None, screen);
                let previous = SelectObject(context, bitmap.into());
                fill(context, &rectangle(0, 0, Self::SIZE, Self::SIZE), color);
                Self {
                    context,
                    bitmap,
                    previous,
                }
            }
        }

        fn pixel(&self, x: i32, y: i32) -> COLORREF {
            unsafe { GetPixel(self.context, x, y) }
        }
    }

    impl Drop for Surface {
        fn drop(&mut self) {
            unsafe {
                SelectObject(self.context, self.previous);
                let _ = DeleteObject(self.bitmap.into());
                let _ = DeleteDC(self.context);
            }
        }
    }

    fn gdi_objects() -> u32 {
        use windows::Win32::System::Threading::{
            GetCurrentProcess, GetGuiResources, GR_GDIOBJECTS,
        };
        unsafe { GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS) }
    }

    #[test]
    fn a_veil_dims_only_its_area_toward_its_colour_and_releases_what_it_made() {
        let _serial = crate::windows::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let white = COLORREF(0x00FF_FFFF);
        let surface = Surface::filled(white);
        let before = gdi_objects();
        // Black at 178 of 255 leaves 30% of the white.
        veil(surface.context, &rectangle(0, 0, 2, 4), COLORREF(0), 178);
        assert_eq!(gdi_objects(), before);
        let dimmed = surface.pixel(1, 3);
        for shift in [0, 8, 16] {
            let channel = (dimmed.0 >> shift) & 0xff;
            assert!((74..=80).contains(&channel), "{channel}");
        }
        assert_eq!(surface.pixel(2, 3), white);
        // The background's own colour over itself changes nothing, so corners stay as drawn.
        veil(surface.context, &rectangle(2, 0, 4, 4), white, 178);
        assert_eq!(surface.pixel(3, 0), white);
    }

    #[test]
    fn buffered_drawing_arrives_whole_at_the_same_coordinates() {
        let _serial = crate::windows::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (white, red, blue) = (
            COLORREF(0x00FF_FFFF),
            COLORREF(0x0000_00FF),
            COLORREF(0x00FF_0000),
        );
        let surface = Surface::filled(white);
        let before = gdi_objects();
        let area = rectangle(1, 1, 4, 3);
        buffered(surface.context, &area, |context| {
            fill(context, &area, red);
            fill(context, &rectangle(2, 2, 3, 3), blue);
        });
        assert_eq!(gdi_objects(), before);
        assert_eq!(surface.pixel(1, 1), red);
        assert_eq!(surface.pixel(3, 2), red);
        assert_eq!(surface.pixel(2, 2), blue);
        // Outside the area nothing changed.
        assert_eq!(surface.pixel(0, 1), white);
        assert_eq!(surface.pixel(1, 3), white);
    }

    #[test]
    fn rectangles_intersect_only_when_they_share_a_pixel() {
        let tile = rectangle(12, 102, 114, 190);
        // A hover change invalidates this tile and its neighbour.
        assert!(intersects(&tile, &rectangle(12, 102, 216, 190)));
        assert!(intersects(&tile, &rectangle(100, 150, 400, 400)));
        assert!(intersects(&tile, &tile));
        // Touching edges share no pixel: right and bottom are exclusive.
        assert!(!intersects(&tile, &rectangle(114, 102, 216, 190)));
        assert!(!intersects(&tile, &rectangle(12, 190, 114, 278)));
        assert!(!intersects(&tile, &rectangle(0, 0, 640, 83)));
        // Empty and inverted rectangles never intersect.
        assert!(!intersects(&tile, &rectangle(50, 150, 50, 160)));
        assert!(!intersects(&tile, &rectangle(60, 160, 50, 150)));
        assert!(!intersects(&RECT::default(), &RECT::default()));
    }
}
