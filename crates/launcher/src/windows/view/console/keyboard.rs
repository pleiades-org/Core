//! Keys and the clipboard in the console. While a command runs, keys go to it as a terminal
//! sends them; copying and pasting use the shortcuts Windows Terminal uses. Once the command
//! has finished, typing moves back to the search box.
use super::*;
use crate::windows::{
    commands::terminal::{character_input, key_sequence, paste_input, Key, Modifiers},
    execute_action::{clipboard_text, copy_text},
};
use windows::Win32::UI::Input::KeyboardAndMouse::*;

impl ConsoleView {
    /// A key pressed in the console. Returns true when it was handled, so it must not also be
    /// typed as a character.
    pub fn key_down(&self, key: VIRTUAL_KEY) -> bool {
        let modifiers = held_modifiers();
        let running = self.running();
        match key {
            // Ctrl+C copies a selection; without one it reaches the program as an interrupt.
            VK_C if modifiers.control && (modifiers.shift || self.selection.get().is_some()) => {
                self.copy_selection();
                return true;
            }
            VK_INSERT if modifiers.control && !modifiers.shift => {
                self.copy_selection();
                return true;
            }
            VK_V if modifiers.control => {
                self.paste();
                return true;
            }
            VK_INSERT if modifiers.shift => {
                self.paste();
                return true;
            }
            VK_A if modifiers.control && !running => {
                self.select_all();
                return true;
            }
            // Shift+Page Up and Page Down scroll back through the output.
            VK_PRIOR | VK_NEXT if modifiers.shift && !self.takes_escape() => {
                let page = self.visible_lines() as isize;
                self.scroll_by(if key == VK_PRIOR { -page } else { page });
                return true;
            }
            _ => {}
        }
        if !running {
            // Enter in finished output must never run the command again.
            return key == VK_RETURN;
        }
        if key == VK_SPACE && modifiers.control {
            self.send(&[0]);
            return true;
        }
        let Some(key) = special_key(key, modifiers) else {
            return false;
        };
        let application_cursor = self.terminal.borrow().modes().application_cursor;
        self.send(&key_sequence(key, modifiers, application_cursor));
        true
    }

    /// A typed character (WM_CHAR), or Alt with a character (WM_SYSCHAR).
    pub(super) fn character(&self, unit: u16, alt: bool) {
        let character = match (self.high_surrogate.take(), unit) {
            (_, 0xD800..=0xDBFF) => {
                self.high_surrogate.set(Some(unit));
                return;
            }
            (Some(high), 0xDC00..=0xDFFF) => char::decode_utf16([high, unit]).next(),
            (_, unit) => char::decode_utf16([unit]).next(),
        };
        let Some(Ok(character)) = character else {
            return;
        };
        if self.running() {
            self.send(&character_input(character, alt));
            return;
        }
        // With the command finished, typing starts the next one in the search box.
        if !character.is_control() {
            let prompt = self.prompt.get();
            unsafe {
                let _ = SetFocus(Some(prompt));
                SendMessageW(
                    prompt,
                    WM_CHAR,
                    Some(WPARAM(unit as usize)),
                    Some(LPARAM(0)),
                );
            }
        }
    }

    /// Right-click: copies a selection, or pastes when there is none.
    pub(super) fn copy_or_paste(&self) {
        if self.selection.get().is_some() {
            self.copy_selection();
        } else {
            self.paste();
        }
    }

    fn copy_selection(&self) {
        let Some(text) = self.selected_text() else {
            return;
        };
        if let Err(error) = copy_text(self.window, &text) {
            eprintln!("Could not copy command output: {error}");
            return;
        }
        self.selection.set(None);
        self.invalidate();
    }

    fn paste(&self) {
        if !self.running() {
            return;
        }
        match clipboard_text(self.window) {
            Ok(text) if !text.is_empty() => {
                let bracketed = self.terminal.borrow().modes().bracketed_paste;
                self.send(&paste_input(&text, bracketed));
            }
            Ok(_) => {}
            Err(error) => eprintln!("Could not paste into the command: {error}"),
        }
    }

    /// Typing shows the newest output again and ends any selection, as in a terminal.
    fn send(&self, bytes: &[u8]) {
        self.write(bytes);
        self.selection.set(None);
        if !self.follow.get() {
            self.scroll_to(usize::MAX);
        }
        self.invalidate();
    }

    /// Writes to the running command's input; nothing happens once it has finished.
    pub(super) fn write(&self, bytes: &[u8]) {
        let Some(input) = self.input.borrow().clone() else {
            return;
        };
        if let Err(error) = input.write(bytes) {
            eprintln!("{error}");
        }
    }
}

fn held_modifiers() -> Modifiers {
    let held = |key: VIRTUAL_KEY| unsafe { GetKeyState(i32::from(key.0)) } < 0;
    Modifiers {
        shift: held(VK_SHIFT),
        alt: held(VK_MENU),
        control: held(VK_CONTROL),
    }
}

/// Keys that have no character and are sent as escape sequences.
fn special_key(key: VIRTUAL_KEY, modifiers: Modifiers) -> Option<Key> {
    Some(match key {
        VK_UP => Key::Up,
        VK_DOWN => Key::Down,
        VK_RIGHT => Key::Right,
        VK_LEFT => Key::Left,
        VK_HOME => Key::Home,
        VK_END => Key::End,
        VK_INSERT => Key::Insert,
        VK_DELETE => Key::Delete,
        VK_PRIOR => Key::PageUp,
        VK_NEXT => Key::PageDown,
        VK_TAB if modifiers.shift => Key::BackTab,
        _ if (VK_F1.0..=VK_F12.0).contains(&key.0) => Key::Function((key.0 - VK_F1.0 + 1) as u8),
        _ => return None,
    })
}
