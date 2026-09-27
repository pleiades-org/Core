//! Reads the pseudo console's output: UTF-8 text mixed with control characters and escape
//! sequences, following the states of the DEC/xterm parser. Sequences a console does not use
//! are consumed and ignored so they never show as text.
use super::screen::Screen;

/// More parameters than any sequence a console sends; extras are ignored.
const PARAMETER_LIMIT: usize = 32;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum State {
    #[default]
    Ground,
    Escape,
    EscapeIntermediate,
    Csi,
    /// A malformed control sequence, consumed up to its final byte.
    CsiIgnore,
    /// An operating system command (a window title), which ends with BEL or `ESC \`.
    Osc,
    OscEscape,
    /// Device control and other strings, which end with `ESC \`.
    IgnoreString,
    IgnoreStringEscape,
}

#[derive(Default)]
pub struct Parser {
    state: State,
    parameters: Vec<u16>,
    /// The parameter being read; `None` until a digit arrives.
    current: Option<u16>,
    /// `?`, `>`, `=` or `<` at the start of a control sequence.
    private: Option<u8>,
    intermediate: Option<u8>,
    /// A UTF-8 character split across reads.
    utf8: [u8; 4],
    utf8_length: usize,
    utf8_expected: usize,
}

impl Parser {
    pub fn advance(&mut self, screen: &mut Screen, byte: u8) {
        if self.utf8_expected > 0 {
            if (0x80..=0xBF).contains(&byte) {
                self.utf8[self.utf8_length] = byte;
                self.utf8_length += 1;
                if self.utf8_length == self.utf8_expected {
                    let character = std::str::from_utf8(&self.utf8[..self.utf8_length])
                        .ok()
                        .and_then(|text| text.chars().next())
                        .unwrap_or(char::REPLACEMENT_CHARACTER);
                    self.utf8_expected = 0;
                    screen.print(character);
                }
                return;
            }
            // A character cut short.
            self.utf8_expected = 0;
            screen.print(char::REPLACEMENT_CHARACTER);
        }
        match self.state {
            State::Ground => self.ground(screen, byte),
            State::Escape => self.escape(screen, byte),
            State::EscapeIntermediate => self.escape_intermediate(screen, byte),
            State::Csi => self.csi(screen, byte),
            State::CsiIgnore => {
                if self.execute_in_sequence(screen, byte) {
                    return;
                }
                if (0x40..=0x7E).contains(&byte) {
                    self.state = State::Ground;
                }
            }
            State::Osc => match byte {
                0x07 => self.state = State::Ground,
                0x1B => self.state = State::OscEscape,
                _ => {}
            },
            State::OscEscape => {
                self.state = State::Ground;
                if byte != b'\\' {
                    self.enter_escape();
                    self.escape(screen, byte);
                }
            }
            State::IgnoreString => {
                if byte == 0x1B {
                    self.state = State::IgnoreStringEscape;
                }
            }
            State::IgnoreStringEscape => {
                self.state = if byte == b'\\' {
                    State::Ground
                } else {
                    State::IgnoreString
                };
            }
        }
    }

    fn ground(&mut self, screen: &mut Screen, byte: u8) {
        match byte {
            0x1B => self.enter_escape(),
            0x00..=0x1F => screen.control(byte),
            0x20..=0x7E => screen.print(byte as char),
            0x7F => {}
            0xC2..=0xDF => self.start_utf8(byte, 2),
            0xE0..=0xEF => self.start_utf8(byte, 3),
            0xF0..=0xF4 => self.start_utf8(byte, 4),
            _ => screen.print(char::REPLACEMENT_CHARACTER),
        }
    }

    fn start_utf8(&mut self, byte: u8, length: usize) {
        self.utf8[0] = byte;
        self.utf8_length = 1;
        self.utf8_expected = length;
    }

    fn enter_escape(&mut self) {
        self.state = State::Escape;
        self.intermediate = None;
    }

    /// Control characters inside a sequence act immediately; ESC starts over, and CAN and
    /// SUB cancel. Returns whether `byte` was one of them.
    fn execute_in_sequence(&mut self, screen: &mut Screen, byte: u8) -> bool {
        match byte {
            0x1B => self.enter_escape(),
            0x18 | 0x1A => self.state = State::Ground,
            0x00..=0x1F => screen.control(byte),
            _ => return false,
        }
        true
    }

    fn escape(&mut self, screen: &mut Screen, byte: u8) {
        if self.execute_in_sequence(screen, byte) {
            return;
        }
        self.state = State::Ground;
        match byte {
            b'[' => {
                self.parameters.clear();
                self.current = None;
                self.private = None;
                self.intermediate = None;
                self.state = State::Csi;
            }
            b']' => self.state = State::Osc,
            b'P' | b'X' | b'^' | b'_' => self.state = State::IgnoreString,
            0x20..=0x2F => {
                self.intermediate = Some(byte);
                self.state = State::EscapeIntermediate;
            }
            b'7' => screen.save_cursor(),
            b'8' => screen.restore_cursor(),
            b'D' => screen.index(),
            b'E' => screen.next_line(),
            b'M' => screen.reverse_index(),
            b'c' => screen.reset(),
            // Keypad modes, tab stops and the like change nothing shown.
            _ => {}
        }
    }

    /// Character set designations (`ESC ( B`) and similar; consoles send Unicode instead.
    fn escape_intermediate(&mut self, screen: &mut Screen, byte: u8) {
        if self.execute_in_sequence(screen, byte) {
            return;
        }
        if !(0x20..=0x2F).contains(&byte) {
            self.state = State::Ground;
        }
    }

    fn csi(&mut self, screen: &mut Screen, byte: u8) {
        if self.execute_in_sequence(screen, byte) {
            return;
        }
        match byte {
            b'0'..=b'9' => {
                let digit = u16::from(byte - b'0');
                self.current = Some(
                    self.current
                        .unwrap_or(0)
                        .saturating_mul(10)
                        .saturating_add(digit),
                );
            }
            b';' | b':' => {
                self.push_parameter();
                self.current = None;
            }
            0x3C..=0x3F if self.parameters.is_empty() && self.current.is_none() => {
                self.private = Some(byte);
            }
            0x3C..=0x3F => self.state = State::CsiIgnore,
            0x20..=0x2F => self.intermediate = Some(byte),
            0x40..=0x7E => {
                if self.current.is_some() || !self.parameters.is_empty() {
                    self.push_parameter();
                }
                self.state = State::Ground;
                self.dispatch(screen, byte);
            }
            _ => {}
        }
    }

    fn push_parameter(&mut self) {
        if self.parameters.len() < PARAMETER_LIMIT {
            self.parameters.push(self.current.unwrap_or(0));
        }
    }

    /// Parameter `index`, where a missing or zero value means `default`.
    fn count(&self, index: usize, default: u16) -> usize {
        usize::from(
            self.parameters
                .get(index)
                .copied()
                .filter(|&value| value != 0)
                .unwrap_or(default),
        )
    }

    fn mode(&self) -> u16 {
        self.parameters.first().copied().unwrap_or(0)
    }

    fn dispatch(&mut self, screen: &mut Screen, final_byte: u8) {
        let count = |index| self.count(index, 1);
        let signed = |index| count(index) as isize;
        match (self.private, self.intermediate, final_byte) {
            (None, None, b'A') => screen.move_by(-signed(0), 0),
            (None, None, b'B' | b'e') => screen.move_by(signed(0), 0),
            (None, None, b'C' | b'a') => screen.move_by(0, signed(0)),
            (None, None, b'D') => screen.move_by(0, -signed(0)),
            (None, None, b'E') => {
                screen.move_by(signed(0), 0);
                screen.set_column(0);
            }
            (None, None, b'F') => {
                screen.move_by(-signed(0), 0);
                screen.set_column(0);
            }
            (None, None, b'G' | b'`') => screen.set_column(count(0) - 1),
            (None, None, b'H' | b'f') => screen.move_to(count(0) - 1, count(1) - 1),
            (None, None, b'd') => screen.set_row(count(0) - 1),
            (None, None, b'J') => screen.erase_display(self.mode()),
            (None, None, b'K') => screen.erase_line(self.mode()),
            (None, None, b'L') => screen.insert_lines(count(0)),
            (None, None, b'M') => screen.delete_lines(count(0)),
            (None, None, b'@') => screen.insert_characters(count(0)),
            (None, None, b'P') => screen.delete_characters(count(0)),
            (None, None, b'X') => screen.erase_characters(count(0)),
            (None, None, b'S') => screen.scroll_up(count(0)),
            (None, None, b'T') => screen.scroll_down(count(0)),
            (None, None, b'm') => screen.select_graphic_rendition(&self.parameters),
            (None, None, b'r') => screen.set_scroll_region(
                self.parameters.first().copied(),
                self.parameters.get(1).copied(),
            ),
            (None, None, b's') => screen.save_cursor(),
            (None, None, b'u') => screen.restore_cursor(),
            (None, None, b'n') => screen.report_status(self.mode()),
            (None, None, b'c') if self.mode() == 0 => screen.report_attributes(false),
            (Some(b'>'), None, b'c') if self.mode() == 0 => screen.report_attributes(true),
            (Some(b'?'), None, b'h' | b'l') => {
                for &mode in &self.parameters {
                    screen.set_private_mode(mode, final_byte == b'h');
                }
            }
            // Window operations, cursor shapes, keyboard modes and other sequences that change
            // nothing Core shows.
            _ => {}
        }
    }
}
