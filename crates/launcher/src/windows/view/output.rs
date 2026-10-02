//! Command output, shown inline in place of the result rows while the query is a shell command.
//! It is Core's terminal ([`console`](super::console)): it grows with its text up to
//! `theme::OUTPUT_HEIGHT`, then scrolls, and keys typed into it reach the running command.
use super::*;
use crate::windows::commands::TerminalInput;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus, VIRTUAL_KEY};

pub const OUTPUT_ID: usize = 103;

/// Space below the last line, in 96-DPI pixels, so descenders are never clipped.
const OUTPUT_PADDING: i32 = 4;

impl View {
    pub fn output_visible(&self) -> bool {
        self.output_visible.get()
    }

    pub fn terminal(&self) -> bool {
        self.terminal.get()
    }

    pub fn is_output(&self, window: HWND) -> bool {
        window == self.output
    }

    /// Terminal mode hides the result rows; `output` also shows the command output. Takes
    /// effect at the next layout: `set_rows` or `relayout`.
    pub fn set_terminal(&self, terminal: bool, output: bool) {
        let output = terminal && output;
        self.terminal.set(terminal);
        if self.output_visible.replace(output) != output {
            unsafe {
                let _ = ShowWindow(self.output, if output { SW_SHOWNA } else { SW_HIDE });
            }
        }
    }

    /// Clears the output for a new command shown under `header`, and returns the console size
    /// the command should get: as wide as the output and as tall as it can grow.
    pub fn start_console(&self, header: &str) -> (u16, u16) {
        let dpi = self.dpi.get();
        let area = SearchLayout::new(scale(theme::WIDTH, dpi), 0, dpi, 0).output;
        let height = scale(theme::OUTPUT_HEIGHT - OUTPUT_PADDING, dpi);
        let (columns, rows) = self.console.size_for(area.right - area.left, height);
        self.console.start(header, columns, rows);
        (columns, rows)
    }

    /// Connects the console to a started command and moves the keyboard to it, so typing
    /// reaches the command.
    pub fn attach_console(&self, input: TerminalInput) {
        self.console.attach(input);
        unsafe {
            let _ = SetFocus(Some(self.output));
        }
    }

    /// The command finished: keys no longer reach it, and typing returns to the search box.
    pub fn finish_console(&self) {
        self.console.finish();
        if self.output_visible.get()
            && self.fitted_output_height() != self.layout_key.get().output_height
        {
            self.relayout();
        }
        unsafe {
            if GetFocus() == self.output {
                let _ = SetFocus(Some(self.input));
            }
        }
    }

    /// Shows new output, growing the output area when it no longer fits.
    pub fn feed_console(&self, output: &[u8]) {
        self.console.feed(output);
        // Layout only runs when the fitted height changed.
        if self.output_visible.get()
            && self.fitted_output_height() != self.layout_key.get().output_height
        {
            self.relayout();
        }
    }

    /// A key pressed in the output. Returns true when it was handled and must not be typed.
    pub fn console_key(&self, key: VIRTUAL_KEY) -> bool {
        self.console.key_down(key)
    }

    /// Keys reach a running command, including Tab.
    pub fn console_running(&self) -> bool {
        self.console.running()
    }

    /// A full-screen program in the output takes Esc; otherwise Esc stops the command.
    pub fn console_takes_escape(&self) -> bool {
        self.console.takes_escape()
    }

    /// Logical height that shows every line, capped at `theme::OUTPUT_HEIGHT`.
    pub(super) fn fitted_output_height(&self) -> i32 {
        let lines = self.console.line_count() as i32;
        let dpi = self.dpi.get() as i32;
        let pixels = lines.max(1) * self.console.line_height() + scale(OUTPUT_PADDING, dpi as u32);
        // Round up so the last line is never cut off.
        ((pixels * 96 + dpi - 1) / dpi).min(theme::OUTPUT_HEIGHT)
    }
}
