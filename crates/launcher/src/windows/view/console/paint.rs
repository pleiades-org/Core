//! Drawing the console: each line as runs of characters that share a look, on a fixed character
//! grid, with the selection highlighted and the cursor shown while a command runs.
use super::*;
use crate::windows::commands::terminal::{Cell as ScreenCell, Color, Style};
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;

/// Windows Terminal's "Campbell" scheme for the 16 ANSI colours.
const ANSI: [(u8, u8, u8); 16] = [
    (12, 12, 12),
    (197, 15, 31),
    (19, 161, 14),
    (193, 156, 0),
    (0, 55, 218),
    (136, 23, 152),
    (58, 150, 221),
    (204, 204, 204),
    (118, 118, 118),
    (231, 72, 86),
    (22, 198, 12),
    (249, 241, 165),
    (59, 120, 255),
    (180, 0, 158),
    (97, 214, 214),
    (242, 242, 242),
];

/// How a character is drawn once colours are resolved against Core's palette.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Look {
    foreground: COLORREF,
    background: COLORREF,
    bold: bool,
    underline: bool,
}

/// Characters drawn with one call: they share a look and sit next to each other.
struct Run {
    column: usize,
    columns: usize,
    look: Look,
    text: Vec<u16>,
    /// Advance for each UTF-16 unit, which keeps every character on the grid.
    advances: Vec<i32>,
}

impl ConsoleView {
    pub(super) fn paint(&self, context: HDC) {
        let mut client = RECT::default();
        unsafe {
            let _ = GetClientRect(self.window, &mut client);
        }
        let (width, height) = (client.right.max(1), client.bottom.max(1));
        unsafe {
            // Drawn off screen first, so streaming output never flickers.
            let memory = CreateCompatibleDC(Some(context));
            let bitmap = CreateCompatibleBitmap(context, width, height);
            let previous_bitmap = SelectObject(memory, bitmap.into());
            self.paint_lines(memory, client);
            let _ = BitBlt(context, 0, 0, width, height, Some(memory), 0, 0, SRCCOPY);
            SelectObject(memory, previous_bitmap);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory);
        }
    }

    fn paint_lines(&self, context: HDC, client: RECT) {
        let palette = self.palette.get();
        unsafe {
            let brush = CreateSolidBrush(palette.background);
            FillRect(context, &client, brush);
            let _ = DeleteObject(brush.into());
            SetBkMode(context, OPAQUE);
        }
        let previous_font = unsafe { SelectObject(context, self.font.get().into()) };
        let (top, visible) = (self.first_line(), self.visible_lines());
        let count = self.line_count();
        for (row, line) in (top..count.min(top + visible)).enumerate() {
            let y = row as i32 * self.line_height.get();
            for run in self.runs(line) {
                self.draw_run(context, y, &run);
            }
        }
        self.draw_cursor(context, top);
        unsafe {
            SelectObject(context, previous_font);
        }
    }

    /// A shown line split into runs. The prompt lines come first, in the secondary colour.
    fn runs(&self, line: usize) -> Vec<Run> {
        let palette = self.palette.get();
        let selection = self.selection.get();
        let selected_look = |look: Look, column| {
            if selection.is_some_and(|selection| selection.contains(Point { line, column })) {
                Look {
                    foreground: palette.background,
                    background: palette.accent,
                    ..look
                }
            } else {
                look
            }
        };
        let mut runs: Vec<Run> = Vec::new();
        let mut add = |column: usize, character: char, columns: usize, look: Look| {
            let look = selected_look(look, column);
            let mut units = [0_u16; 2];
            let units = character.encode_utf16(&mut units);
            let advance = columns as i32 * self.cell_width.get();
            match runs.last_mut() {
                Some(run) if run.look == look && run.column + run.columns == column => {
                    run.columns += columns;
                    extend(run, units, advance);
                }
                _ => {
                    let mut run = Run {
                        column,
                        columns,
                        look,
                        text: Vec::new(),
                        advances: Vec::new(),
                    };
                    extend(&mut run, units, advance);
                    runs.push(run);
                }
            }
        };
        if line < self.header_lines() {
            let look = Look {
                foreground: palette.secondary,
                background: palette.background,
                bold: false,
                underline: false,
            };
            for (column, character) in self.line_characters(line) {
                add(column, character, character_width(character).max(1), look);
            }
            return runs;
        }
        let terminal = self.terminal.borrow();
        let cells = terminal.line(line - self.header_lines());
        for (column, cell) in cells.iter().enumerate() {
            if cell.is_wide_tail() {
                continue;
            }
            let columns = if cells.get(column + 1).is_some_and(ScreenCell::is_wide_tail) {
                2
            } else {
                1
            };
            add(
                column,
                cell.character,
                columns,
                resolve(cell.style, palette),
            );
        }
        runs
    }

    fn draw_run(&self, context: HDC, y: i32, run: &Run) {
        let cell_width = self.cell_width.get();
        let area = RECT {
            left: run.column as i32 * cell_width,
            top: y,
            right: (run.column + run.columns) as i32 * cell_width,
            bottom: y + self.line_height.get(),
        };
        let bold = self.bold_font.get();
        unsafe {
            let previous =
                (run.look.bold && !bold.0.is_null()).then(|| SelectObject(context, bold.into()));
            SetTextColor(context, run.look.foreground);
            SetBkColor(context, run.look.background);
            let _ = ExtTextOutW(
                context,
                area.left,
                y,
                ETO_OPAQUE | ETO_CLIPPED,
                Some(&area),
                PCWSTR(run.text.as_ptr()),
                run.text.len() as u32,
                Some(run.advances.as_ptr()),
            );
            if run.look.underline {
                let brush = CreateSolidBrush(run.look.foreground);
                let line = RECT {
                    top: area.bottom - 1,
                    ..area
                };
                FillRect(context, &line, brush);
                let _ = DeleteObject(brush.into());
            }
            if let Some(previous) = previous {
                SelectObject(context, previous);
            }
        }
    }

    /// A block while the console has focus, an outline otherwise; none once the command ends.
    fn draw_cursor(&self, context: HDC, top: usize) {
        if !self.running() {
            return;
        }
        let Some((line, column)) = self.terminal.borrow().cursor() else {
            return;
        };
        // The prompt lines come before the output.
        let Some(row) = (line + self.header_lines()).checked_sub(top) else {
            return;
        };
        if row >= self.visible_lines() {
            return;
        }
        let cell_width = self.cell_width.get();
        let area = RECT {
            left: column as i32 * cell_width,
            top: row as i32 * self.line_height.get(),
            right: (column as i32 + 1) * cell_width,
            bottom: (row as i32 + 1) * self.line_height.get(),
        };
        unsafe {
            if GetFocus() == self.window {
                let _ = InvertRect(context, &area);
            } else {
                let brush = CreateSolidBrush(self.palette.get().secondary);
                FrameRect(context, &area, brush);
                let _ = DeleteObject(brush.into());
            }
        }
    }
}

fn extend(run: &mut Run, units: &[u16], advance: i32) {
    run.text.extend_from_slice(units);
    run.advances.push(advance);
    // The second half of a surrogate pair does not move the pen again.
    run.advances.extend(std::iter::repeat_n(0, units.len() - 1));
}

fn resolve(style: Style, palette: Palette) -> Look {
    let mut foreground = color(style.foreground, palette.text);
    let mut background = color(style.background, palette.background);
    if style.inverse {
        std::mem::swap(&mut foreground, &mut background);
    }
    Look {
        foreground,
        background,
        bold: style.bold,
        underline: style.underline,
    }
}

/// A terminal colour as Windows draws it; `Default` is Core's own colour.
fn color(color: Color, default: COLORREF) -> COLORREF {
    let rgb = |red: u8, green: u8, blue: u8| {
        COLORREF(u32::from(red) | (u32::from(green) << 8) | (u32::from(blue) << 16))
    };
    match color {
        Color::Default => default,
        Color::Rgb(red, green, blue) => rgb(red, green, blue),
        Color::Indexed(index @ 0..=15) => {
            let (red, green, blue) = ANSI[usize::from(index)];
            rgb(red, green, blue)
        }
        // The 6 × 6 × 6 colour cube.
        Color::Indexed(index @ 16..=231) => {
            let level = |value: u8| if value == 0 { 0 } else { 55 + value * 40 };
            let index = index - 16;
            rgb(level(index / 36), level(index / 6 % 6), level(index % 6))
        }
        // The grey ramp.
        Color::Indexed(index) => {
            let grey = 8 + (index - 232) * 10;
            rgb(grey, grey, grey)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_colours_map_to_the_xterm_palette() {
        let fallback = COLORREF(0x123456);
        assert_eq!(color(Color::Default, fallback), fallback);
        assert_eq!(
            color(Color::Indexed(1), fallback),
            COLORREF(197 | 15 << 8 | 31 << 16)
        );
        assert_eq!(color(Color::Indexed(16), fallback), COLORREF(0));
        assert_eq!(color(Color::Indexed(231), fallback), COLORREF(0xFFFFFF));
        assert_eq!(color(Color::Indexed(232), fallback), COLORREF(0x080808));
        assert_eq!(color(Color::Indexed(255), fallback), COLORREF(0xEEEEEE));
        assert_eq!(color(Color::Rgb(1, 2, 3), fallback), COLORREF(0x030201));
    }

    #[test]
    fn inverse_swaps_the_resolved_colours() {
        let palette = Palette::default();
        let look = resolve(
            Style {
                inverse: true,
                ..Style::default()
            },
            palette,
        );
        assert_eq!(look.foreground, palette.background);
        assert_eq!(look.background, palette.text);
    }
}
