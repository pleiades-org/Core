//! Command mode: typing `/` at the start of the search box switches it to a command prompt. The
//! `/` itself is hidden, so the box holds only the command, but queries still start with `/`.
//! Backspace in the empty box switches back to search.
use super::*;

pub(super) const SEARCH_CUE: &str = "Search apps, commands, or calculate…";
const COMMAND_CUE: &str = "Type a command · Backspace returns to search";

impl View {
    /// Replaces the query. A leading `/` enters command mode and stays out of the box; any other
    /// text leaves it. The caret goes to the end.
    pub fn set_query(&self, query: &str) -> windows::core::Result<()> {
        let text = match query.strip_prefix('/') {
            Some(command) => {
                self.set_command_mode(true);
                command
            }
            None => {
                self.set_command_mode(false);
                query
            }
        };
        self.set_input(text, text.encode_utf16().count())
    }

    /// Typed or pasted text that starts with `/` enters command mode, dropping the `/`.
    /// In command mode a `/` is part of the command, as in `/usr/bin/ls`.
    pub fn absorb_command_prefix(&self) -> windows::core::Result<bool> {
        if self.command_mode.get() {
            return Ok(false);
        }
        let text = control_text(self.input);
        let Some(command) = text.strip_prefix('/') else {
            return Ok(false);
        };
        // Keep the caret where it was, one character left for the removed `/`.
        let mut caret = 0_u32;
        unsafe {
            SendMessageW(
                self.input,
                EM_GETSEL,
                None,
                Some(LPARAM(&mut caret as *mut u32 as isize)),
            );
        }
        self.set_command_mode(true);
        self.set_input(command, (caret as usize).saturating_sub(1))?;
        Ok(true)
    }

    /// Backspace in an empty command prompt returns to search. Returns whether it did.
    pub fn leave_empty_command_mode(&self) -> bool {
        if !self.command_mode.get() || unsafe { GetWindowTextLengthW(self.input) } > 0 {
            return false;
        }
        self.set_command_mode(false);
        true
    }

    pub fn is_input(&self, window: HWND) -> bool {
        window == self.input
    }

    fn set_command_mode(&self, enabled: bool) {
        if self.command_mode.replace(enabled) == enabled {
            return;
        }
        let cue = wide(if enabled { COMMAND_CUE } else { SEARCH_CUE });
        unsafe {
            SendMessageW(
                self.input,
                EM_SETCUEBANNER,
                Some(WPARAM(1)),
                Some(LPARAM(cue.as_ptr() as isize)),
            );
        }
    }

    fn set_input(&self, text: &str, caret: usize) -> windows::core::Result<()> {
        let text = wide(text);
        unsafe {
            SetWindowTextW(self.input, PCWSTR(text.as_ptr()))?;
            SendMessageW(
                self.input,
                EM_SETSEL,
                Some(WPARAM(caret)),
                Some(LPARAM(caret as isize)),
            );
        }
        // Core's own state may be busy when the box announces this change, so it is told here.
        self.query_changed();
        Ok(())
    }
}
