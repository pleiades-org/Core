use windows::core::w;
use windows::Win32::{Foundation::COLORREF, Graphics::Gdi::*};

pub const WIDTH: i32 = 640;
pub const RESULTS_TOP: i32 = 102;
pub const ROW_HEIGHT: i32 = 52;
pub const ANSWER_HEIGHT: i32 = 108;
pub const FOOTER_HEIGHT: i32 = 48;
/// Command output grows with its text up to this height, then scrolls. Below it, a gap
/// before the footer. Both in 96-DPI pixels.
pub const OUTPUT_HEIGHT: i32 = 360;
pub const OUTPUT_GAP: i32 = 8;
/// Recently used apps, shown like Start's pinned grid: tiles with a large icon over a name that
/// may wrap to two lines. In 96-DPI pixels.
pub const GRID_COLUMNS: usize = 6;
pub const GRID_TILE_HEIGHT: i32 = 88;
pub const GRID_ICON: i32 = 32;
/// Space below the last row of tiles.
pub const GRID_GAP: i32 = 6;
pub const BACKGROUND: COLORREF = rgb(0, 0, 0);
pub const SELECTED: COLORREF = rgb(18, 18, 18);
pub const TEXT: COLORREF = rgb(239, 241, 245);
pub const SECONDARY: COLORREF = rgb(163, 171, 187);
pub const ACCENT: COLORREF = rgb(191, 204, 234);

const fn rgb(red: u32, green: u32, blue: u32) -> COLORREF {
    COLORREF(red | (green << 8) | (blue << 16))
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub background: COLORREF,
    pub selected: COLORREF,
    pub text: COLORREF,
    pub secondary: COLORREF,
    pub accent: COLORREF,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            background: BACKGROUND,
            selected: SELECTED,
            text: TEXT,
            secondary: SECONDARY,
            accent: ACCENT,
        }
    }
}

impl Palette {
    /// Dark backgrounds get dark native scrollbars.
    pub fn is_dark(&self) -> bool {
        luminance(self.background) <= 0.179
    }

    pub fn for_background(color: super::settings::BackgroundColor) -> Self {
        let background = color.native();
        if background == BACKGROUND {
            return Self::default();
        }
        let text = if luminance(background) > 0.179 {
            rgb(0, 0, 0)
        } else {
            rgb(255, 255, 255)
        };
        let mut selected = blend(background, text, 10);
        if contrast(text, selected) < 4.5 {
            selected = blend(background, COLORREF(text.0 ^ 0xFFFFFF), 15);
        }
        let secondary = blend(background, text, 75);
        Self {
            background,
            selected,
            text,
            secondary: if contrast(secondary, background) >= 4.5 {
                secondary
            } else {
                text
            },
            accent: text,
        }
    }
}

fn blend(background: COLORREF, foreground: COLORREF, percent: u32) -> COLORREF {
    let channel = |shift: u32| {
        (((background.0 >> shift) & 255) * (100 - percent)
            + ((foreground.0 >> shift) & 255) * percent)
            / 100
    };
    rgb(channel(0), channel(8), channel(16))
}

fn luminance(color: COLORREF) -> f64 {
    let channel = |shift: u32| {
        let component = ((color.0 >> shift) & 255) as f64 / 255.;
        if component <= 0.04045 {
            component / 12.92
        } else {
            ((component + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(0) + 0.7152 * channel(8) + 0.0722 * channel(16)
}

fn contrast(first: COLORREF, second: COLORREF) -> f64 {
    let first = luminance(first);
    let second = luminance(second);
    (first.max(second) + 0.05) / (first.min(second) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_backgrounds_keep_text_readable_on_rows_and_background() {
        for color in [
            "000000", "FFFFFF", "777777", "123456", "FF0000", "00FF00", "0000FF",
        ] {
            let palette = Palette::for_background(
                super::super::settings::BackgroundColor::parse(color).unwrap(),
            );
            assert!(contrast(palette.text, palette.background) >= 4.5, "{color}");
            assert!(
                contrast(palette.text, palette.selected) >= 4.5,
                "selected {color}"
            );
        }
    }
}

pub fn scale(logical_pixels: i32, dpi: u32) -> i32 {
    (logical_pixels * dpi.max(1) as i32 + 48) / 96
}

#[derive(Clone, Copy, Default)]
pub struct Fonts {
    pub input: HFONT,
    pub title: HFONT,
    pub detail: HFONT,
    pub answer: HFONT,
    pub icon: HFONT,
    /// Command output; Consolas ships with every supported Windows version.
    pub mono: HFONT,
    /// App names in the recently used grid.
    pub label: HFONT,
}

impl Fonts {
    pub fn create(dpi: u32) -> windows::core::Result<Self> {
        let mut fonts = Self::default();
        let result = (|| {
            fonts.input = create_font(22, 400, dpi)?;
            fonts.title = create_font(16, 500, dpi)?;
            fonts.detail = create_font(12, 400, dpi)?;
            fonts.answer = create_font(32, 500, dpi)?;
            fonts.icon = create_font_face(20, 400, dpi, w!("Segoe MDL2 Assets"))?;
            fonts.mono = create_font_face(13, 400, dpi, w!("Consolas"))?;
            fonts.label = create_font(13, 400, dpi)?;
            Ok(fonts)
        })();
        if result.is_err() {
            fonts.delete();
        }
        result
    }

    pub fn delete(self) {
        for font in [
            self.input,
            self.title,
            self.detail,
            self.answer,
            self.icon,
            self.mono,
            self.label,
        ] {
            if !font.0.is_null() {
                unsafe {
                    let _ = DeleteObject(font.into());
                }
            }
        }
    }
}

fn create_font(height: i32, weight: i32, dpi: u32) -> windows::core::Result<HFONT> {
    create_font_face(height, weight, dpi, w!("Segoe UI"))
}

fn create_font_face(
    height: i32,
    weight: i32,
    dpi: u32,
    face: windows::core::PCWSTR,
) -> windows::core::Result<HFONT> {
    let font = unsafe {
        CreateFontW(
            -scale(height, dpi),
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            face,
        )
    };
    if font.0.is_null() {
        return Err(windows::core::Error::from_win32());
    }
    Ok(font)
}
