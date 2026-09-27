//! The screen a pseudo console draws on: a grid of cells with a cursor, a scroll region, an
//! alternate screen for full-screen programs, and scrollback for lines that scroll off the top.
use super::cell::{apply_sgr, character_width, Cell, Style, WIDE_TAIL};
use std::collections::VecDeque;

/// Lines kept after they scroll off the top of the screen.
pub(super) const SCROLLBACK_LIMIT: usize = 5_000;
const TAB_WIDTH: usize = 8;

type Line = Vec<Cell>;

#[derive(Clone, Copy, Default)]
struct Cursor {
    row: usize,
    column: usize,
    style: Style,
    /// The last column was just written; the next character wraps first. Delaying the wrap
    /// keeps a line that exactly fills the width from gaining a blank line after it.
    wrap_pending: bool,
}

/// Modes a program can switch that change how keys are sent or how the screen behaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Modes {
    /// Arrow keys send `ESC O A` rather than `ESC [ A` (DECCKM).
    pub application_cursor: bool,
    /// Pasted text is wrapped in `ESC [200~` and `ESC [201~`.
    pub bracketed_paste: bool,
    pub cursor_visible: bool,
    autowrap: bool,
}

impl Default for Modes {
    fn default() -> Self {
        Self {
            application_cursor: false,
            bracketed_paste: false,
            cursor_visible: true,
            autowrap: true,
        }
    }
}

pub struct Screen {
    columns: usize,
    rows: usize,
    lines: Vec<Line>,
    /// The screen not being shown: the primary one while the alternate screen is active.
    hidden: Vec<Line>,
    alternate: bool,
    scrollback: VecDeque<Line>,
    /// Lines dropped from the start of the scrollback so far, so a view can tell its line
    /// numbers moved.
    dropped: usize,
    cursor: Cursor,
    /// `ESC 7` / `CSI s`.
    saved: Cursor,
    /// The primary screen's cursor while the alternate screen is shown (mode 1049).
    primary_cursor: Cursor,
    /// The scroll region's first and last rows, inclusive.
    top: usize,
    bottom: usize,
    modes: Modes,
    /// Answers to status requests, for the program's input.
    responses: Vec<u8>,
}

impl Screen {
    pub fn new(columns: usize, rows: usize) -> Self {
        let (columns, rows) = (columns.max(1), rows.max(1));
        Self {
            columns,
            rows,
            lines: blank_lines(columns, rows, Style::default()),
            hidden: blank_lines(columns, rows, Style::default()),
            alternate: false,
            scrollback: VecDeque::new(),
            dropped: 0,
            cursor: Cursor::default(),
            saved: Cursor::default(),
            primary_cursor: Cursor::default(),
            top: 0,
            bottom: rows - 1,
            modes: Modes::default(),
            responses: Vec::new(),
        }
    }

    pub fn modes(&self) -> Modes {
        self.modes
    }

    /// A full-screen program (an editor, a pager) is using the alternate screen.
    pub fn alternate(&self) -> bool {
        self.alternate
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn take_responses(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.responses)
    }

    /// Lines to show: the scrollback, then the screen down to its last written row (and the
    /// cursor's row when `include_cursor`). The alternate screen is shown whole, without
    /// scrollback, as a terminal shows it.
    pub fn line_count(&self, include_cursor: bool) -> usize {
        if self.alternate {
            return self.rows;
        }
        self.scrollback.len() + self.used_rows(include_cursor)
    }

    /// A line counted by [`Self::line_count`]. Scrollback lines omit trailing blanks.
    pub fn line(&self, index: usize) -> &[Cell] {
        if self.alternate {
            return &self.lines[index];
        }
        match index.checked_sub(self.scrollback.len()) {
            None => &self.scrollback[index],
            Some(row) => &self.lines[row],
        }
    }

    /// The cursor's line (as [`Self::line`] counts) and column, when it is visible.
    pub fn cursor(&self) -> Option<(usize, usize)> {
        if !self.modes.cursor_visible {
            return None;
        }
        let offset = if self.alternate {
            0
        } else {
            self.scrollback.len()
        };
        Some((offset + self.cursor.row, self.cursor.column))
    }

    /// Plain text of the shown lines, without trailing spaces or trailing blank lines.
    pub fn text(&self) -> String {
        let count = self.line_count(false);
        let lines: Vec<String> = (0..count)
            .map(|index| line_text(self.line(index)))
            .collect();
        lines.join("\r\n").trim_end().to_owned()
    }

    fn used_rows(&self, include_cursor: bool) -> usize {
        let written = self
            .lines
            .iter()
            .rposition(|line| line.iter().any(|cell| !cell.is_blank()))
            .map_or(0, |row| row + 1);
        if include_cursor {
            written.max(self.cursor.row + 1)
        } else {
            written
        }
    }

    fn blank(&self) -> Cell {
        Cell::blank(self.cursor.style)
    }

    pub(super) fn select_graphic_rendition(&mut self, parameters: &[u16]) {
        apply_sgr(&mut self.cursor.style, parameters);
    }

    pub(super) fn print(&mut self, character: char) {
        let width = character_width(character);
        if width == 0 || width > self.columns {
            return;
        }
        if self.cursor.wrap_pending {
            self.wrap();
        }
        if self.cursor.column + width > self.columns {
            if self.modes.autowrap {
                self.wrap();
            } else {
                self.cursor.column = self.columns - width;
            }
        }
        let (row, column) = (self.cursor.row, self.cursor.column);
        self.split_wide(row, column);
        if width == 2 {
            self.split_wide(row, column + 1);
        }
        let style = self.cursor.style;
        self.lines[row][column] = Cell { character, style };
        if width == 2 {
            self.lines[row][column + 1] = Cell {
                character: WIDE_TAIL,
                style,
            };
        }
        if column + width >= self.columns {
            self.cursor.column = self.columns - 1;
            self.cursor.wrap_pending = self.modes.autowrap;
        } else {
            self.cursor.column = column + width;
        }
    }

    fn wrap(&mut self) {
        self.cursor.wrap_pending = false;
        self.cursor.column = 0;
        self.index();
    }

    /// Overwriting half of a double-width character erases the other half.
    fn split_wide(&mut self, row: usize, column: usize) {
        let blank = Cell::default();
        let line = &mut self.lines[row];
        if line[column].is_wide_tail() && column > 0 {
            line[column - 1] = blank;
            line[column] = blank;
        }
        if line.get(column + 1).is_some_and(Cell::is_wide_tail) {
            line[column + 1] = blank;
        }
    }

    /// C0 control characters.
    pub(super) fn control(&mut self, byte: u8) {
        match byte {
            0x08 => {
                self.cursor.column = self.cursor.column.saturating_sub(1);
                self.cursor.wrap_pending = false;
            }
            b'\t' => {
                let next = (self.cursor.column / TAB_WIDTH + 1) * TAB_WIDTH;
                self.cursor.column = next.min(self.columns - 1);
                self.cursor.wrap_pending = false;
            }
            b'\n' | 0x0B | 0x0C => {
                self.cursor.wrap_pending = false;
                self.index();
            }
            b'\r' => {
                self.cursor.column = 0;
                self.cursor.wrap_pending = false;
            }
            // BEL and the rest have nothing to show.
            _ => {}
        }
    }

    /// Down a line, scrolling the region at its bottom.
    pub(super) fn index(&mut self) {
        if self.cursor.row == self.bottom {
            self.scroll_up(1);
        } else if self.cursor.row + 1 < self.rows {
            self.cursor.row += 1;
        }
    }

    /// Up a line, scrolling the region down at its top.
    pub(super) fn reverse_index(&mut self) {
        self.cursor.wrap_pending = false;
        if self.cursor.row == self.top {
            self.scroll_down(1);
        } else {
            self.cursor.row = self.cursor.row.saturating_sub(1);
        }
    }

    pub(super) fn next_line(&mut self) {
        self.control(b'\r');
        self.index();
    }

    /// Lines leaving the top of a full-screen region on the primary screen go to the scrollback.
    pub(super) fn scroll_up(&mut self, count: usize) {
        let count = count.min(self.bottom + 1 - self.top);
        for _ in 0..count {
            let line = self.lines.remove(self.top);
            self.lines
                .insert(self.bottom, vec![self.blank(); self.columns]);
            if self.top == 0 && !self.alternate {
                self.push_scrollback(line);
            }
        }
    }

    pub(super) fn scroll_down(&mut self, count: usize) {
        let count = count.min(self.bottom + 1 - self.top);
        for _ in 0..count {
            self.lines.remove(self.bottom);
            self.lines
                .insert(self.top, vec![self.blank(); self.columns]);
        }
    }

    fn push_scrollback(&mut self, mut line: Line) {
        let used = line
            .iter()
            .rposition(|cell| !cell.is_blank())
            .map_or(0, |column| column + 1);
        line.truncate(used);
        line.shrink_to_fit();
        self.scrollback.push_back(line);
        if self.scrollback.len() > SCROLLBACK_LIMIT {
            self.scrollback.pop_front();
            self.dropped += 1;
        }
    }

    /// Absolute position, zero-based and clamped to the screen.
    pub(super) fn move_to(&mut self, row: usize, column: usize) {
        self.cursor.row = row.min(self.rows - 1);
        self.cursor.column = column.min(self.columns - 1);
        self.cursor.wrap_pending = false;
    }

    pub(super) fn set_column(&mut self, column: usize) {
        self.move_to(self.cursor.row, column);
    }

    pub(super) fn set_row(&mut self, row: usize) {
        self.move_to(row, self.cursor.column);
    }

    /// Relative movement. Up and down stop at the scroll region's edges when starting inside it.
    pub(super) fn move_by(&mut self, rows: isize, columns: isize) {
        let row = self.cursor.row;
        let (upper, lower) = if (self.top..=self.bottom).contains(&row) {
            (self.top, self.bottom)
        } else {
            (0, self.rows - 1)
        };
        let row = row.saturating_add_signed(rows).clamp(upper, lower);
        let column = self.cursor.column.saturating_add_signed(columns);
        self.move_to(row, column);
    }

    /// `CSI J`: 0 from the cursor to the end, 1 from the start to the cursor, 2 the whole
    /// screen, 3 the scrollback.
    pub(super) fn erase_display(&mut self, mode: u16) {
        let blank = self.blank();
        let row = self.cursor.row;
        match mode {
            0 => {
                self.erase_line(0);
                for line in &mut self.lines[row + 1..] {
                    line.fill(blank);
                }
            }
            1 => {
                self.erase_line(1);
                for line in &mut self.lines[..row] {
                    line.fill(blank);
                }
            }
            2 => {
                for line in &mut self.lines {
                    line.fill(blank);
                }
            }
            3 => {
                self.dropped += self.scrollback.len();
                self.scrollback.clear();
            }
            _ => {}
        }
    }

    /// `CSI K`: 0 from the cursor to the end of the line, 1 from its start, 2 the whole line.
    pub(super) fn erase_line(&mut self, mode: u16) {
        let blank = self.blank();
        let column = self.cursor.column;
        let line = &mut self.lines[self.cursor.row];
        match mode {
            0 => line[column..].fill(blank),
            1 => line[..=column].fill(blank),
            2 => line.fill(blank),
            _ => {}
        }
    }

    pub(super) fn erase_characters(&mut self, count: usize) {
        let blank = self.blank();
        let column = self.cursor.column;
        let end = (column + count).min(self.columns);
        self.lines[self.cursor.row][column..end].fill(blank);
    }

    pub(super) fn insert_characters(&mut self, count: usize) {
        let blank = self.blank();
        let (column, columns) = (self.cursor.column, self.columns);
        let line = &mut self.lines[self.cursor.row];
        let count = count.min(columns - column);
        line.splice(column..column, std::iter::repeat_n(blank, count));
        line.truncate(columns);
    }

    pub(super) fn delete_characters(&mut self, count: usize) {
        let blank = self.blank();
        let (column, columns) = (self.cursor.column, self.columns);
        let line = &mut self.lines[self.cursor.row];
        let count = count.min(columns - column);
        line.drain(column..column + count);
        line.resize(columns, blank);
    }

    /// `CSI L`: blank lines pushed in at the cursor, within the scroll region.
    pub(super) fn insert_lines(&mut self, count: usize) {
        let row = self.cursor.row;
        if !(self.top..=self.bottom).contains(&row) {
            return;
        }
        for _ in 0..count.min(self.bottom + 1 - row) {
            self.lines.remove(self.bottom);
            self.lines.insert(row, vec![self.blank(); self.columns]);
        }
        self.cursor.column = 0;
        self.cursor.wrap_pending = false;
    }

    /// `CSI M`: lines removed at the cursor, pulling the rest of the region up.
    pub(super) fn delete_lines(&mut self, count: usize) {
        let row = self.cursor.row;
        if !(self.top..=self.bottom).contains(&row) {
            return;
        }
        for _ in 0..count.min(self.bottom + 1 - row) {
            self.lines.remove(row);
            self.lines
                .insert(self.bottom, vec![self.blank(); self.columns]);
        }
        self.cursor.column = 0;
        self.cursor.wrap_pending = false;
    }

    /// `CSI r` with one-based rows; missing values mean the screen's edges.
    pub(super) fn set_scroll_region(&mut self, top: Option<u16>, bottom: Option<u16>) {
        let top = top.filter(|&row| row > 0).map_or(0, |row| row as usize - 1);
        let bottom = bottom
            .filter(|&row| row > 0)
            .map_or(self.rows - 1, |row| (row as usize - 1).min(self.rows - 1));
        if top < bottom {
            self.top = top;
            self.bottom = bottom;
            self.move_to(0, 0);
        }
    }

    pub(super) fn save_cursor(&mut self) {
        self.saved = self.cursor;
    }

    pub(super) fn restore_cursor(&mut self) {
        self.cursor = self.saved;
        self.move_to(self.cursor.row, self.cursor.column);
    }

    /// `CSI ? n h` / `CSI ? n l`. Standard (non-`?`) modes are not used by consoles.
    pub(super) fn set_private_mode(&mut self, mode: u16, enabled: bool) {
        match mode {
            1 => self.modes.application_cursor = enabled,
            7 => self.modes.autowrap = enabled,
            25 => self.modes.cursor_visible = enabled,
            47 | 1047 => self.switch_screen(enabled, false),
            1049 => self.switch_screen(enabled, true),
            2004 => self.modes.bracketed_paste = enabled,
            // Mouse reporting, focus events and win32-input-mode are not supported; programs
            // fall back to plain keys.
            _ => {}
        }
    }

    /// Back to the primary screen, as when a full-screen program exits normally.
    pub fn leave_alternate(&mut self) {
        self.switch_screen(false, true);
    }

    fn switch_screen(&mut self, alternate: bool, save_cursor: bool) {
        if alternate == self.alternate {
            return;
        }
        if alternate && save_cursor {
            self.primary_cursor = self.cursor;
        }
        std::mem::swap(&mut self.lines, &mut self.hidden);
        self.alternate = alternate;
        if alternate {
            let blank = Cell::default();
            for line in &mut self.lines {
                line.fill(blank);
            }
        } else if save_cursor {
            self.cursor = self.primary_cursor;
        }
        self.top = 0;
        self.bottom = self.rows - 1;
        self.move_to(self.cursor.row, self.cursor.column);
    }

    /// `CSI n`: 5 asks whether the terminal is working, 6 where the cursor is.
    pub(super) fn report_status(&mut self, request: u16) {
        match request {
            5 => self.responses.extend_from_slice(b"\x1b[0n"),
            6 => self.responses.extend(
                format!("\x1b[{};{}R", self.cursor.row + 1, self.cursor.column + 1).bytes(),
            ),
            _ => {}
        }
    }

    /// `CSI c` and `CSI > c`: identifies as a VT100-class terminal.
    pub(super) fn report_attributes(&mut self, secondary: bool) {
        self.responses.extend_from_slice(if secondary {
            b"\x1b[>0;10;1c"
        } else {
            b"\x1b[?1;2c"
        });
    }

    /// `ESC c`: everything back to its starting state except the scrollback.
    pub(super) fn reset(&mut self) {
        let scrollback = std::mem::take(&mut self.scrollback);
        let dropped = self.dropped;
        *self = Self::new(self.columns, self.rows);
        self.scrollback = scrollback;
        self.dropped = dropped;
    }
}

fn blank_lines(columns: usize, rows: usize, style: Style) -> Vec<Line> {
    vec![vec![Cell::blank(style); columns]; rows]
}

/// A line's characters without double-width tails or trailing spaces.
pub(super) fn line_text(line: &[Cell]) -> String {
    let text: String = line
        .iter()
        .filter(|cell| !cell.is_wide_tail())
        .map(|cell| cell.character)
        .collect();
    text.trim_end().to_owned()
}
