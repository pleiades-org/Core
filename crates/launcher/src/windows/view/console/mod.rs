//! Core's terminal: shows a command's console screen inline, below the prompt it was typed at,
//! and sends keys typed into it straight to the running program. It scrolls through the
//! command's output, and the mouse selects text to copy.
mod keyboard;
mod paint;

use super::super::{
    commands::{
        terminal::{character_width, Terminal},
        TerminalInput,
    },
    theme::Palette,
};
use std::cell::{Cell, RefCell};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        UI::{
            Controls::SetScrollInfo,
            HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi},
            Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, VIRTUAL_KEY},
            WindowsAndMessaging::*,
        },
    },
};

const CLASS_NAME: PCWSTR = w!("Pleiades.Core.Console");
/// Lines moved per notch of the mouse wheel.
const WHEEL_LINES: isize = 3;

/// A character position: a shown line (the prompt lines come first) and a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Point {
    line: usize,
    column: usize,
}

/// Text selected with the mouse, from where the drag started to where it is now.
#[derive(Clone, Copy)]
struct Selection {
    anchor: Point,
    head: Point,
}

impl Selection {
    /// The first selected position and the position just past the last.
    fn range(self) -> (Point, Point) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
    }

    fn contains(self, point: Point) -> bool {
        let (start, end) = self.range();
        start <= point && point < end
    }
}

pub struct ConsoleView {
    window: HWND,
    /// The search box. Typing goes there once the command has finished.
    prompt: Cell<HWND>,
    terminal: RefCell<Terminal>,
    /// The prompt line shown above the output (the folder and the command), wrapped to the
    /// console's width, as characters with their columns.
    header: RefCell<Vec<Vec<(usize, char)>>>,
    /// The running command's input; `None` once it has finished.
    input: RefCell<Option<TerminalInput>>,
    font: Cell<HFONT>,
    /// A bold version of `font`, owned by the view.
    bold_font: Cell<HFONT>,
    cell_width: Cell<i32>,
    line_height: Cell<i32>,
    palette: Cell<Palette>,
    /// The first shown line while scrolled back.
    top: Cell<usize>,
    /// Shows the newest output as it arrives; scrolling back turns it off.
    follow: Cell<bool>,
    selection: Cell<Option<Selection>>,
    selecting: Cell<bool>,
    /// The terminal's dropped line count when the selection was made; lines moving invalidates it.
    dropped: Cell<usize>,
    /// The first half of a character outside the Basic Multilingual Plane, typed as two keys.
    high_surrogate: Cell<Option<u16>>,
}

impl ConsoleView {
    /// Creates the hidden console window. `identifier` lets tests find it.
    pub fn create(
        parent: HWND,
        instance: HINSTANCE,
        identifier: usize,
    ) -> windows::core::Result<Box<Self>> {
        register_class(instance)?;
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS_NAME,
                w!("Command output"),
                WS_CHILD | WS_VSCROLL | WS_TABSTOP,
                0,
                0,
                1,
                1,
                Some(parent),
                Some(HMENU(identifier as *mut _)),
                Some(instance),
                None,
            )?
        };
        let console = Box::new(Self {
            window,
            prompt: Cell::new(HWND::default()),
            terminal: RefCell::new(Terminal::new(1, 1)),
            header: RefCell::new(Vec::new()),
            input: RefCell::new(None),
            font: Cell::new(HFONT::default()),
            bold_font: Cell::new(HFONT::default()),
            cell_width: Cell::new(1),
            line_height: Cell::new(1),
            palette: Cell::new(Palette::default()),
            top: Cell::new(0),
            follow: Cell::new(true),
            selection: Cell::new(None),
            selecting: Cell::new(false),
            dropped: Cell::new(0),
            high_surrogate: Cell::new(None),
        });
        // The box keeps the view at a fixed address for the window's lifetime.
        unsafe {
            SetWindowLongPtrW(window, GWLP_USERDATA, &*console as *const Self as isize);
        }
        Ok(console)
    }

    pub fn window(&self) -> HWND {
        self.window
    }

    pub fn set_prompt(&self, prompt: HWND) {
        self.prompt.set(prompt);
    }

    pub fn set_palette(&self, palette: Palette) {
        self.palette.set(palette);
        self.invalidate();
    }

    /// Uses `font` (not owned) for text, measuring the character cell from it.
    fn set_font(&self, font: HFONT) {
        self.font.set(font);
        let bold = bold_version(font);
        let previous = self.bold_font.replace(bold);
        if !previous.0.is_null() {
            unsafe {
                let _ = DeleteObject(previous.into());
            }
        }
        let (width, height) = cell_size(self.window, font);
        self.cell_width.set(width);
        self.line_height.set(height);
        self.update_scrollbar();
        self.invalidate();
    }

    pub fn line_height(&self) -> i32 {
        self.line_height.get()
    }

    /// Columns and rows for a new command's console, filling `width` × `height` physical pixels
    /// with one line kept for the prompt line. The scroll bar's width is always set aside, so
    /// the columns do not change when it appears.
    pub fn size_for(&self, width: i32, height: i32) -> (u16, u16) {
        let dpi = unsafe { GetDpiForWindow(self.window) };
        let scroll_bar = unsafe { GetSystemMetricsForDpi(SM_CXVSCROLL, dpi) };
        let columns = (width - scroll_bar) / self.cell_width.get().max(1);
        let rows = height / self.line_height.get().max(1) - 1;
        (columns.clamp(20, 500) as u16, rows.clamp(4, 200) as u16)
    }

    /// Clears the view for a new command shown under `header`.
    pub fn start(&self, header: &str, columns: u16, rows: u16) {
        *self.terminal.borrow_mut() = Terminal::new(usize::from(columns), usize::from(rows));
        *self.header.borrow_mut() = wrap(header, usize::from(columns));
        self.input.replace(None);
        self.top.set(0);
        self.follow.set(true);
        self.selection.set(None);
        self.dropped.set(0);
        self.update_scrollbar();
        self.invalidate();
    }

    /// Connects keys to a running command.
    pub fn attach(&self, input: TerminalInput) {
        self.input.replace(Some(input));
        self.invalidate();
    }

    /// The command finished: keys no longer reach it, and its output stays on show.
    pub fn finish(&self) {
        self.input.replace(None);
        self.terminal.borrow_mut().leave_alternate();
        self.update_scrollbar();
        self.invalidate();
    }

    pub fn running(&self) -> bool {
        self.input.borrow().is_some()
    }

    /// A full-screen program is running, so Esc belongs to it rather than stopping it.
    pub fn takes_escape(&self) -> bool {
        self.running() && self.terminal.borrow().alternate()
    }

    /// Output from the command. Status requests it contains are answered.
    pub fn feed(&self, output: &[u8]) {
        if output.is_empty() {
            return;
        }
        let (responses, dropped) = {
            let mut terminal = self.terminal.borrow_mut();
            terminal.feed(output);
            (terminal.take_responses(), terminal.dropped())
        };
        self.write(&responses);
        if dropped != self.dropped.replace(dropped) {
            self.selection.set(None);
        }
        self.update_scrollbar();
        self.invalidate();
    }

    /// Lines shown: the prompt lines, the output, and the line a running command's cursor is on.
    pub fn line_count(&self) -> usize {
        self.header_lines() + self.terminal.borrow().line_count(self.running())
    }

    /// The prompt line wraps like output, so it takes one line or more.
    fn header_lines(&self) -> usize {
        self.header.borrow().len()
    }

    /// The prompt line and output as plain text, for copying and accessibility.
    pub fn transcript(&self) -> String {
        let header: Vec<String> = self
            .header
            .borrow()
            .iter()
            .map(|line| line.iter().map(|&(_, character)| character).collect())
            .collect();
        let header = header.join("\r\n");
        let output = self.terminal.borrow().text();
        if output.is_empty() {
            header
        } else {
            format!("{header}\r\n{output}")
        }
    }

    /// Characters of a shown line with their columns, without double-width tails.
    fn line_characters(&self, line: usize) -> Vec<(usize, char)> {
        if let Some(header) = self.header.borrow().get(line) {
            return header.clone();
        }
        let terminal = self.terminal.borrow();
        terminal
            .line(line - self.header_lines())
            .iter()
            .enumerate()
            .filter(|(_, cell)| !cell.is_wide_tail())
            .map(|(column, cell)| (column, cell.character))
            .collect()
    }

    fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection.get()?.range();
        if start == end {
            return None;
        }
        let last = end.line.min(self.line_count().saturating_sub(1));
        let lines: Vec<String> = (start.line..=last)
            .map(|line| {
                let from = if line == start.line { start.column } else { 0 };
                let to = if line == end.line {
                    end.column
                } else {
                    usize::MAX
                };
                let text: String = self
                    .line_characters(line)
                    .into_iter()
                    .filter(|(column, _)| (from..to).contains(column))
                    .map(|(_, character)| character)
                    .collect();
                text.trim_end().to_owned()
            })
            .collect();
        Some(lines.join("\r\n"))
    }

    fn select_all(&self) {
        let last = self.line_count().saturating_sub(1);
        let end = Point {
            line: last,
            column: usize::MAX,
        };
        self.selection.set(Some(Selection {
            anchor: Point { line: 0, column: 0 },
            head: end,
        }));
        self.invalidate();
    }

    fn visible_lines(&self) -> usize {
        let mut client = RECT::default();
        unsafe {
            let _ = GetClientRect(self.window, &mut client);
        }
        ((client.bottom - client.top) / self.line_height.get().max(1)).max(1) as usize
    }

    /// The first shown line: the newest output while following it.
    fn first_line(&self) -> usize {
        let last_top = self.line_count().saturating_sub(self.visible_lines());
        if self.follow.get() {
            last_top
        } else {
            self.top.get().min(last_top)
        }
    }

    fn scroll_to(&self, top: usize) {
        let last_top = self.line_count().saturating_sub(self.visible_lines());
        let top = top.min(last_top);
        self.top.set(top);
        self.follow.set(top == last_top);
        self.update_scrollbar();
        self.invalidate();
    }

    fn scroll_by(&self, lines: isize) {
        self.scroll_to(self.first_line().saturating_add_signed(lines));
    }

    /// The scroll bar appears only when the output is taller than the view.
    fn update_scrollbar(&self) {
        let (count, visible, top) = (self.line_count(), self.visible_lines(), self.first_line());
        let information = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: count.saturating_sub(1) as i32,
            nPage: visible as u32,
            nPos: top as i32,
            nTrackPos: 0,
        };
        unsafe {
            SetScrollInfo(self.window, SB_VERT, &information, true);
        }
    }

    fn vertical_scroll(&self, request: SCROLLBAR_COMMAND) {
        let page = self.visible_lines() as isize;
        match request {
            SB_LINEUP => self.scroll_by(-1),
            SB_LINEDOWN => self.scroll_by(1),
            SB_PAGEUP => self.scroll_by(-page),
            SB_PAGEDOWN => self.scroll_by(page),
            SB_TOP => self.scroll_to(0),
            SB_BOTTOM => self.scroll_to(usize::MAX),
            SB_THUMBTRACK | SB_THUMBPOSITION => {
                let mut information = SCROLLINFO {
                    cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                    fMask: SIF_TRACKPOS,
                    ..Default::default()
                };
                if unsafe { GetScrollInfo(self.window, SB_VERT, &mut information) }.is_ok() {
                    self.scroll_to(information.nTrackPos.max(0) as usize);
                }
            }
            _ => {}
        }
    }

    /// The character position nearest a point in client coordinates. Columns round to the
    /// nearest boundary, so dragging over half a character selects it.
    fn point_at(&self, x: i32, y: i32) -> Point {
        let line_height = self.line_height.get().max(1);
        let cell_width = self.cell_width.get().max(1);
        let row = if y < 0 { -1 } else { y / line_height };
        let line = self.first_line().saturating_add_signed(row as isize);
        Point {
            line: line.min(self.line_count().saturating_sub(1)),
            column: ((x + cell_width / 2) / cell_width).max(0) as usize,
        }
    }

    fn begin_selection(&self, x: i32, y: i32) {
        unsafe {
            let _ = SetFocus(Some(self.window));
            SetCapture(self.window);
        }
        let point = self.point_at(x, y);
        self.selection.set(Some(Selection {
            anchor: point,
            head: point,
        }));
        self.selecting.set(true);
        self.invalidate();
    }

    fn extend_selection(&self, x: i32, y: i32) {
        let Some(selection) = self.selection.get().filter(|_| self.selecting.get()) else {
            return;
        };
        // Dragging past the top or bottom scrolls, so a selection can span more than a page.
        let mut client = RECT::default();
        unsafe {
            let _ = GetClientRect(self.window, &mut client);
        }
        if y < 0 {
            self.scroll_by(-1);
        } else if y >= client.bottom {
            self.scroll_by(1);
        }
        self.selection.set(Some(Selection {
            head: self.point_at(x, y.clamp(0, (client.bottom - 1).max(0))),
            ..selection
        }));
        self.invalidate();
    }

    fn end_selection(&self) {
        if !self.selecting.replace(false) {
            return;
        }
        unsafe {
            let _ = ReleaseCapture();
        }
        if self
            .selection
            .get()
            .is_some_and(|selection| selection.anchor == selection.head)
        {
            self.selection.set(None);
            self.invalidate();
        }
    }

    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.window), None, false);
        }
    }
}

impl Drop for ConsoleView {
    fn drop(&mut self) {
        unsafe {
            if IsWindow(Some(self.window)).as_bool() {
                SetWindowLongPtrW(self.window, GWLP_USERDATA, 0);
            }
            let bold = self.bold_font.get();
            if !bold.0.is_null() {
                let _ = DeleteObject(bold.into());
            }
        }
    }
}

fn register_class(instance: HINSTANCE) -> windows::core::Result<()> {
    let class = WNDCLASSW {
        lpfnWndProc: Some(console_proc),
        hInstance: instance,
        lpszClassName: CLASS_NAME,
        hCursor: unsafe { LoadCursorW(None, IDC_IBEAM)? },
        ..Default::default()
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        let error = windows::core::Error::from_win32();
        if error.code() != ERROR_CLASS_ALREADY_EXISTS.to_hresult() {
            return Err(error);
        }
    }
    Ok(())
}

/// `font` at bold weight, or a null font when it cannot be made; text then stays regular.
fn bold_version(font: HFONT) -> HFONT {
    let mut description = LOGFONTW::default();
    let size = std::mem::size_of::<LOGFONTW>() as i32;
    unsafe {
        if GetObjectW(
            font.into(),
            size,
            Some((&mut description as *mut LOGFONTW).cast()),
        ) == 0
        {
            return HFONT::default();
        }
        description.lfWeight = FW_BOLD.0 as i32;
        CreateFontIndirectW(&description)
    }
}

/// A character's width and a line's height for `font`, in physical pixels.
fn cell_size(window: HWND, font: HFONT) -> (i32, i32) {
    unsafe {
        let context = GetDC(Some(window));
        if context.is_invalid() {
            return (1, 1);
        }
        let previous = SelectObject(context, font.into());
        let mut metrics = TEXTMETRICW::default();
        let measured = GetTextMetricsW(context, &mut metrics).as_bool();
        SelectObject(context, previous);
        ReleaseDC(Some(window), context);
        if measured {
            (metrics.tmAveCharWidth.max(1), metrics.tmHeight.max(1))
        } else {
            (1, 1)
        }
    }
}

/// `text` as lines of at most `columns` columns, each character with its column.
fn wrap(text: &str, columns: usize) -> Vec<Vec<(usize, char)>> {
    let mut lines = vec![Vec::new()];
    let mut column = 0;
    for character in text.chars() {
        let width = character_width(character).max(1);
        if column + width > columns.max(width) {
            lines.push(Vec::new());
            column = 0;
        }
        lines.last_mut().expect("a line").push((column, character));
        column += width;
    }
    lines
}

/// The client coordinates in a mouse message.
fn client_point(long: LPARAM) -> (i32, i32) {
    (
        i32::from((long.0 & 0xffff) as u16 as i16),
        i32::from(((long.0 >> 16) & 0xffff) as u16 as i16),
    )
}

unsafe extern "system" fn console_proc(
    window: HWND,
    message: u32,
    word: WPARAM,
    long: LPARAM,
) -> LRESULT {
    let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *const ConsoleView;
    let Some(console) = pointer.as_ref() else {
        return DefWindowProcW(window, message, word, long);
    };
    match message {
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let context = BeginPaint(window, &mut paint);
            console.paint(context);
            let _ = EndPaint(window, &paint);
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_SIZE => console.update_scrollbar(),
        WM_SETFONT => console.set_font(HFONT(word.0 as *mut _)),
        WM_SETFOCUS | WM_KILLFOCUS => console.invalidate(),
        WM_VSCROLL => console.vertical_scroll(SCROLLBAR_COMMAND((word.0 & 0xffff) as i32)),
        WM_MOUSEWHEEL => {
            let notches = isize::from((word.0 >> 16) as u16 as i16) / WHEEL_DELTA as isize;
            console.scroll_by(-notches * WHEEL_LINES);
        }
        WM_LBUTTONDOWN => {
            let (x, y) = client_point(long);
            console.begin_selection(x, y);
        }
        WM_MOUSEMOVE => {
            let (x, y) = client_point(long);
            console.extend_selection(x, y);
        }
        WM_LBUTTONUP | WM_CAPTURECHANGED => console.end_selection(),
        // As in Windows' console: right-click copies a selection, otherwise pastes.
        WM_RBUTTONUP => console.copy_or_paste(),
        // Core's message loop passes key presses to `key_down` before translating them into
        // characters; Alt combinations and F10 arrive here instead.
        WM_KEYDOWN => {}
        WM_SYSKEYDOWN => {
            if !console.key_down(VIRTUAL_KEY(word.0 as u16)) {
                return DefWindowProcW(window, message, word, long);
            }
        }
        WM_CHAR => console.character(word.0 as u16, false),
        WM_SYSCHAR if console.running() => console.character(word.0 as u16, true),
        WM_GETTEXTLENGTH => {
            return LRESULT(console.transcript().encode_utf16().count() as isize);
        }
        WM_GETTEXT => {
            let capacity = word.0;
            if capacity == 0 || long.0 == 0 {
                return LRESULT(0);
            }
            let text: Vec<u16> = console.transcript().encode_utf16().collect();
            let copied = text.len().min(capacity - 1);
            let destination = long.0 as *mut u16;
            std::ptr::copy_nonoverlapping(text.as_ptr(), destination, copied);
            *destination.add(copied) = 0;
            return LRESULT(copied as isize);
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            return DefWindowProcW(window, message, word, long);
        }
        _ => return DefWindowProcW(window, message, word, long),
    }
    LRESULT(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_line_wraps_at_the_console_width() {
        let text = |line: &Vec<(usize, char)>| -> String {
            line.iter().map(|&(_, character)| character).collect()
        };
        let lines = wrap("~> echo hello", 5);
        assert_eq!(
            lines.iter().map(text).collect::<Vec<_>>(),
            ["~> ec", "ho he", "llo"]
        );
        assert_eq!(lines[1][0], (0, 'h'));
        // A double-width character that would overhang moves to the next line.
        let wide = wrap("abcd漢", 5);
        assert_eq!(wide[1], [(0, '漢')]);
        assert_eq!(wrap("", 5), [Vec::new()]);
    }

    #[test]
    fn selections_run_from_the_earlier_point_and_exclude_the_end() {
        let at = |line, column| Point { line, column };
        let selection = Selection {
            anchor: at(3, 4),
            head: at(1, 2),
        };
        assert_eq!(selection.range(), (at(1, 2), at(3, 4)));
        assert!(selection.contains(at(1, 2)));
        assert!(selection.contains(at(2, 90)));
        assert!(!selection.contains(at(3, 4)));
        assert!(!selection.contains(at(1, 1)));
    }
}
