//! One character position on the terminal screen, its colours, and how wide a character is.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Color {
    /// The view's own text or background colour.
    #[default]
    Default,
    /// One of the 256 xterm colours; 0–15 are the classic ANSI colours.
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub foreground: Color,
    pub background: Color,
    pub bold: bool,
    pub underline: bool,
    /// Foreground and background swap places when drawn.
    pub inverse: bool,
}

/// The right half of a double-width character. The cell before it draws the character.
pub const WIDE_TAIL: char = '\0';

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub character: char,
    pub style: Style,
}

impl Cell {
    /// An erased cell. Erasing keeps the current background, as terminals do, so a coloured
    /// line stays coloured to its end.
    pub fn blank(style: Style) -> Self {
        Self {
            character: ' ',
            style: Style {
                background: style.background,
                ..Style::default()
            },
        }
    }

    /// Nothing visible: a space on the default background.
    pub fn is_blank(&self) -> bool {
        self.character == ' ' && self.style.background == Color::Default && !self.style.inverse
    }

    pub fn is_wide_tail(&self) -> bool {
        self.character == WIDE_TAIL
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self::blank(Style::default())
    }
}

/// Applies a Select Graphic Rendition sequence (`ESC [ … m`). An empty list resets the style.
pub fn apply_sgr(style: &mut Style, parameters: &[u16]) {
    if parameters.is_empty() {
        *style = Style::default();
        return;
    }
    let mut index = 0;
    while index < parameters.len() {
        match parameters[index] {
            0 => *style = Style::default(),
            1 => style.bold = true,
            4 | 21 => style.underline = true,
            7 => style.inverse = true,
            22 => style.bold = false,
            24 => style.underline = false,
            27 => style.inverse = false,
            code @ 30..=37 => style.foreground = Color::Indexed((code - 30) as u8),
            39 => style.foreground = Color::Default,
            code @ 40..=47 => style.background = Color::Indexed((code - 40) as u8),
            49 => style.background = Color::Default,
            code @ 90..=97 => style.foreground = Color::Indexed((code - 90 + 8) as u8),
            code @ 100..=107 => style.background = Color::Indexed((code - 100 + 8) as u8),
            code @ (38 | 48) => {
                let (color, used) = extended_color(&parameters[index + 1..]);
                if let Some(color) = color {
                    if code == 38 {
                        style.foreground = color;
                    } else {
                        style.background = color;
                    }
                }
                index += used;
            }
            // Dim, italic, blink and the rest have no distinct look here.
            _ => {}
        }
        index += 1;
    }
}

/// `5;n` (256 colours) or `2;r;g;b` (true colour) after 38 or 48, and how many values it used.
fn extended_color(rest: &[u16]) -> (Option<Color>, usize) {
    let byte = |value: u16| value.min(255) as u8;
    match rest {
        [5, index, ..] => (Some(Color::Indexed(byte(*index))), 2),
        [2, red, green, blue, ..] => (Some(Color::Rgb(byte(*red), byte(*green), byte(*blue))), 4),
        [5] | [2, ..] => (None, rest.len()),
        _ => (None, 0),
    }
}

/// Columns a character takes: 0 for combining marks (dropped), 2 for East Asian wide
/// characters and emoji, otherwise 1. An approximation of Unicode's width tables that matches
/// the characters consoles commonly show.
pub fn character_width(character: char) -> usize {
    let code = character as u32;
    if code < 0x300 {
        return 1;
    }
    if ZERO_WIDTH.iter().any(|range| range.contains(&code)) {
        0
    } else if WIDE.iter().any(|range| range.contains(&code)) {
        2
    } else {
        1
    }
}

const ZERO_WIDTH: &[std::ops::RangeInclusive<u32>] = &[
    0x0300..=0x036F,
    0x0483..=0x0489,
    0x0591..=0x05BD,
    0x0610..=0x061A,
    0x064B..=0x065F,
    0x1AB0..=0x1AFF,
    0x1DC0..=0x1DFF,
    0x200B..=0x200F,
    0x20D0..=0x20FF,
    0xFE00..=0xFE0F,
    0xFE20..=0xFE2F,
    0xE0100..=0xE01EF,
];

const WIDE: &[std::ops::RangeInclusive<u32>] = &[
    0x1100..=0x115F,
    0x231A..=0x231B,
    0x2329..=0x232A,
    0x23E9..=0x23EC,
    0x23F0..=0x23F0,
    0x23F3..=0x23F3,
    0x25FD..=0x25FE,
    0x2614..=0x2615,
    0x2648..=0x2653,
    0x267F..=0x267F,
    0x2693..=0x2693,
    0x26A1..=0x26A1,
    0x26AA..=0x26AB,
    0x26BD..=0x26BE,
    0x26C4..=0x26C5,
    0x26CE..=0x26CE,
    0x26D4..=0x26D4,
    0x26EA..=0x26EA,
    0x26F2..=0x26F5,
    0x26FA..=0x26FA,
    0x26FD..=0x26FD,
    0x2705..=0x2705,
    0x270A..=0x270B,
    0x2728..=0x2728,
    0x274C..=0x274C,
    0x274E..=0x274E,
    0x2753..=0x2757,
    0x2795..=0x2797,
    0x27B0..=0x27B0,
    0x27BF..=0x27BF,
    0x2B1B..=0x2B1C,
    0x2B50..=0x2B50,
    0x2B55..=0x2B55,
    0x2E80..=0x303E,
    0x3041..=0x33FF,
    0x3400..=0x4DBF,
    0x4E00..=0x9FFF,
    0xA000..=0xA4CF,
    0xA960..=0xA97F,
    0xAC00..=0xD7A3,
    0xF900..=0xFAFF,
    0xFE10..=0xFE19,
    0xFE30..=0xFE6F,
    0xFF00..=0xFF60,
    0xFFE0..=0xFFE6,
    0x16FE0..=0x16FE4,
    0x17000..=0x18AFF,
    0x1B000..=0x1B2FF,
    0x1F004..=0x1F004,
    0x1F0CF..=0x1F0CF,
    0x1F18E..=0x1F18E,
    0x1F191..=0x1F19A,
    0x1F200..=0x1F251,
    0x1F300..=0x1F64F,
    0x1F680..=0x1F6FF,
    0x1F7E0..=0x1F7EB,
    0x1F90C..=0x1F9FF,
    0x1FA70..=0x1FAFF,
    0x20000..=0x2FFFD,
    0x30000..=0x3FFFD,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sgr_sets_and_resets_attributes_and_colours() {
        let mut style = Style::default();
        apply_sgr(&mut style, &[1, 4, 31, 42]);
        assert_eq!(
            style,
            Style {
                foreground: Color::Indexed(1),
                background: Color::Indexed(2),
                bold: true,
                underline: true,
                inverse: false,
            }
        );
        apply_sgr(&mut style, &[22, 24, 39, 49, 7]);
        assert_eq!(
            style,
            Style {
                inverse: true,
                ..Style::default()
            }
        );
        apply_sgr(&mut style, &[]);
        assert_eq!(style, Style::default());
    }

    #[test]
    fn sgr_reads_bright_256_and_true_colours() {
        let mut style = Style::default();
        apply_sgr(&mut style, &[91, 104]);
        assert_eq!(style.foreground, Color::Indexed(9));
        assert_eq!(style.background, Color::Indexed(12));
        apply_sgr(&mut style, &[38, 5, 208, 48, 2, 10, 20, 300, 1]);
        assert_eq!(style.foreground, Color::Indexed(208));
        assert_eq!(style.background, Color::Rgb(10, 20, 255));
        assert!(style.bold, "the value after a colour is read normally");
        // A truncated colour is ignored rather than misreading what follows.
        apply_sgr(&mut style, &[38, 2, 1]);
        assert_eq!(style.foreground, Color::Indexed(208));
    }

    #[test]
    fn widths_follow_east_asian_and_emoji_ranges() {
        assert_eq!(character_width('a'), 1);
        assert_eq!(character_width('é'), 1);
        assert_eq!(character_width('\u{301}'), 0);
        assert_eq!(character_width('漢'), 2);
        assert_eq!(character_width('한'), 2);
        assert_eq!(character_width('🙂'), 2);
        assert_eq!(character_width('─'), 1);
    }

    #[test]
    fn erased_cells_keep_only_the_background() {
        let style = Style {
            foreground: Color::Indexed(1),
            background: Color::Indexed(4),
            bold: true,
            underline: true,
            inverse: false,
        };
        let cell = Cell::blank(style);
        assert_eq!(cell.style.background, Color::Indexed(4));
        assert_eq!(cell.style.foreground, Color::Default);
        assert!(!cell.is_blank(), "a coloured background is visible");
        assert!(Cell::default().is_blank());
    }
}
