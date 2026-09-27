//! What a keystroke sends to a program in the pseudo console, as xterm and Windows Terminal
//! send it. Characters are sent as UTF-8; keys without a character become escape sequences.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub control: bool,
}

impl Modifiers {
    /// xterm's modifier parameter: 1 plus 1 for Shift, 2 for Alt and 4 for Ctrl. `None` for
    /// an unmodified key.
    fn parameter(self) -> Option<u8> {
        let value = 1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.control);
        (value > 1).then_some(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Right,
    Left,
    Home,
    End,
    Insert,
    Delete,
    PageUp,
    PageDown,
    /// F1 to F12.
    Function(u8),
    /// Shift+Tab.
    BackTab,
}

/// The sequence for a key that has no character. `application_cursor` is the program's
/// cursor-key mode, used by full-screen programs.
pub fn key_sequence(key: Key, modifiers: Modifiers, application_cursor: bool) -> Vec<u8> {
    let modifier = modifiers.parameter();
    // Keys ending in a letter: `ESC [ A`, `ESC O A` in application mode, `ESC [ 1 ; 5 A`
    // with modifiers.
    let letter = |final_byte: char, application: bool| match modifier {
        Some(value) => format!("\x1b[1;{value}{final_byte}"),
        None if application => format!("\x1bO{final_byte}"),
        None => format!("\x1b[{final_byte}"),
    };
    // Keys ending in `~`: `ESC [ 3 ~`, `ESC [ 3 ; 5 ~` with modifiers.
    let tilde = |number: u8| match modifier {
        Some(value) => format!("\x1b[{number};{value}~"),
        None => format!("\x1b[{number}~"),
    };
    let text = match key {
        Key::Up => letter('A', application_cursor),
        Key::Down => letter('B', application_cursor),
        Key::Right => letter('C', application_cursor),
        Key::Left => letter('D', application_cursor),
        Key::Home => letter('H', application_cursor),
        Key::End => letter('F', application_cursor),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::Function(number @ 1..=4) => letter((b'P' + number - 1) as char, true),
        Key::Function(number) => tilde(match number {
            5 => 15,
            6 => 17,
            7 => 18,
            8 => 19,
            9 => 20,
            10 => 21,
            11 => 23,
            _ => 24,
        }),
        Key::BackTab => "\x1b[Z".into(),
    };
    text.into_bytes()
}

/// A typed character. Backspace is DEL (0x7F) in terminals and Ctrl+Backspace is BS, the
/// reverse of what Windows reports. Alt sends ESC first.
pub fn character_input(character: char, alt: bool) -> Vec<u8> {
    let character = match character {
        '\u{8}' => '\u{7f}',
        '\u{7f}' => '\u{8}',
        other => other,
    };
    let mut bytes = Vec::with_capacity(5);
    if alt {
        bytes.push(0x1B);
    }
    let mut buffer = [0; 4];
    bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
    bytes
}

/// Pasted text: line breaks become Enter (`\r`), and a program in bracketed-paste mode gets the
/// markers that tell it the text was pasted rather than typed, so it will not run each line.
pub fn paste_input(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    let mut bytes = Vec::with_capacity(text.len() + 12);
    if bracketed {
        bytes.extend_from_slice(b"\x1b[200~");
        // A pasted end marker would end the paste early.
        bytes.extend_from_slice(text.replace("\x1b[201~", "").as_bytes());
        bytes.extend_from_slice(b"\x1b[201~");
    } else {
        bytes.extend_from_slice(text.as_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: Modifiers = Modifiers {
        shift: false,
        alt: false,
        control: false,
    };

    fn sent(key: Key, modifiers: Modifiers, application: bool) -> String {
        String::from_utf8(key_sequence(key, modifiers, application)).unwrap()
    }

    #[test]
    fn arrows_follow_the_cursor_key_mode_and_modifiers() {
        assert_eq!(sent(Key::Up, PLAIN, false), "\x1b[A");
        assert_eq!(sent(Key::Up, PLAIN, true), "\x1bOA");
        assert_eq!(sent(Key::Home, PLAIN, false), "\x1b[H");
        assert_eq!(sent(Key::End, PLAIN, true), "\x1bOF");
        let control = Modifiers {
            control: true,
            ..PLAIN
        };
        assert_eq!(sent(Key::Left, control, true), "\x1b[1;5D");
        let shift_alt = Modifiers {
            shift: true,
            alt: true,
            control: false,
        };
        assert_eq!(sent(Key::Right, shift_alt, false), "\x1b[1;4C");
    }

    #[test]
    fn editing_and_function_keys_use_xterm_numbers() {
        assert_eq!(sent(Key::Delete, PLAIN, false), "\x1b[3~");
        assert_eq!(sent(Key::PageDown, PLAIN, false), "\x1b[6~");
        assert_eq!(sent(Key::Function(1), PLAIN, false), "\x1bOP");
        assert_eq!(sent(Key::Function(5), PLAIN, false), "\x1b[15~");
        assert_eq!(sent(Key::Function(12), PLAIN, false), "\x1b[24~");
        let control = Modifiers {
            control: true,
            ..PLAIN
        };
        assert_eq!(sent(Key::Delete, control, false), "\x1b[3;5~");
        assert_eq!(sent(Key::Function(2), control, false), "\x1b[1;5Q");
        assert_eq!(sent(Key::BackTab, PLAIN, false), "\x1b[Z");
    }

    #[test]
    fn typed_characters_are_utf8_with_terminal_backspace() {
        assert_eq!(character_input('a', false), b"a");
        assert_eq!(character_input('é', false), "é".as_bytes());
        assert_eq!(character_input('\u{8}', false), [0x7F]);
        assert_eq!(character_input('\u{7f}', false), [0x08]);
        assert_eq!(character_input('\r', false), b"\r");
        assert_eq!(character_input('x', true), b"\x1bx");
    }

    #[test]
    fn pasted_lines_become_enter_and_brackets_are_added_when_asked() {
        assert_eq!(paste_input("a\r\nb\nc", false), b"a\rb\rc");
        assert_eq!(
            paste_input("ls\n", true),
            b"\x1b[200~ls\r\x1b[201~".to_vec()
        );
        assert_eq!(
            paste_input("x\x1b[201~y", true),
            b"\x1b[200~xy\x1b[201~".to_vec()
        );
    }
}
