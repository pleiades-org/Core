//! A terminal emulator for commands run in a pseudo console: it turns the console's output into
//! a screen of styled cells that Core draws, and turns keystrokes into what programs expect.
mod cell;
mod keys;
mod parser;
mod screen;

pub use cell::{character_width, Cell, Color, Style};
pub use keys::{character_input, key_sequence, paste_input, Key, Modifiers};
pub use screen::Modes;

use parser::Parser;
use screen::Screen;

pub struct Terminal {
    screen: Screen,
    parser: Parser,
}

impl Terminal {
    pub fn new(columns: usize, rows: usize) -> Self {
        Self {
            screen: Screen::new(columns, rows),
            parser: Parser::default(),
        }
    }

    /// Output from the console. Characters split across calls are joined.
    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.parser.advance(&mut self.screen, byte);
        }
    }

    /// Answers to status requests the program made, to write to its input.
    pub fn take_responses(&mut self) -> Vec<u8> {
        self.screen.take_responses()
    }

    pub fn modes(&self) -> Modes {
        self.screen.modes()
    }

    pub fn alternate(&self) -> bool {
        self.screen.alternate()
    }

    /// Shows the primary screen again. A full-screen program that was stopped never switched
    /// back itself.
    pub fn leave_alternate(&mut self) {
        self.screen.leave_alternate();
    }

    /// Lines dropped from the start of the scrollback so far.
    pub fn dropped(&self) -> usize {
        self.screen.dropped()
    }

    /// Lines to show; `include_cursor` counts the cursor's line even while it is empty, which
    /// keeps the line a running program is typing on in view.
    pub fn line_count(&self, include_cursor: bool) -> usize {
        self.screen.line_count(include_cursor)
    }

    pub fn line(&self, index: usize) -> &[Cell] {
        self.screen.line(index)
    }

    pub fn cursor(&self) -> Option<(usize, usize)> {
        self.screen.cursor()
    }

    /// Everything shown, as plain text.
    pub fn text(&self) -> String {
        self.screen.text()
    }
}

#[cfg(test)]
mod tests {
    use super::screen::{line_text, SCROLLBACK_LIMIT};
    use super::*;

    fn terminal(columns: usize, rows: usize, output: &str) -> Terminal {
        let mut terminal = Terminal::new(columns, rows);
        terminal.feed(output.as_bytes());
        terminal
    }

    #[test]
    fn text_and_line_breaks_fill_the_screen() {
        let terminal = terminal(20, 5, "one\r\ntwo\r\n");
        assert_eq!(terminal.text(), "one\r\ntwo");
        assert_eq!(terminal.line_count(false), 2);
        assert_eq!(
            terminal.line_count(true),
            3,
            "the cursor waits on the next line"
        );
        assert_eq!(terminal.cursor(), Some((2, 0)));
    }

    #[test]
    fn a_line_that_fills_the_width_wraps_only_when_more_text_follows() {
        let exact = terminal(4, 5, "abcd\r\nef");
        assert_eq!(exact.text(), "abcd\r\nef");
        let long = terminal(4, 5, "abcdef");
        assert_eq!(long.text(), "abcd\r\nef");
    }

    #[test]
    fn lines_scrolled_off_the_top_become_scrollback() {
        let terminal = terminal(10, 3, "1\r\n2\r\n3\r\n4\r\n5");
        assert_eq!(terminal.text(), "1\r\n2\r\n3\r\n4\r\n5");
        assert_eq!(terminal.line_count(false), 5);
        assert_eq!(terminal.cursor(), Some((4, 1)));
    }

    #[test]
    fn scrollback_keeps_the_newest_lines() {
        let mut terminal = Terminal::new(10, 2);
        for number in 0..SCROLLBACK_LIMIT + 10 {
            terminal.feed(format!("{number}\r\n").as_bytes());
        }
        assert_eq!(terminal.dropped(), 9);
        assert_eq!(line_text(terminal.line(0)), "9");
    }

    #[test]
    fn cursor_movement_and_erasing_rewrite_the_screen() {
        let terminal = terminal(10, 4, "hello\r\nworld\x1b[1;1Hj\x1b[2;3H\x1b[K\x1b[3;2Hx");
        assert_eq!(terminal.text(), "jello\r\nwo\r\n x");
        let cleared = self::terminal(10, 4, "a\r\nb\x1b[2J\x1b[Hc");
        assert_eq!(cleared.text(), "c");
    }

    #[test]
    fn inserting_and_deleting_shift_characters_and_lines() {
        let characters = terminal(10, 3, "abcdef\x1b[1;3H\x1b[2P\x1b[1@");
        assert_eq!(characters.text(), "ab ef");
        let lines = terminal(10, 3, "1\r\n2\r\n3\x1b[2;1H\x1b[M");
        assert_eq!(lines.text(), "1\r\n3");
        let inserted = terminal(10, 3, "1\r\n2\r\n3\x1b[2;1H\x1b[L");
        assert_eq!(inserted.text(), "1\r\n\r\n2");
    }

    #[test]
    fn a_scroll_region_scrolls_without_touching_the_rest() {
        let terminal = terminal(
            10,
            4,
            "head\x1b[2;3r\x1b[2;1Ha\r\nb\r\nc\x1b[r\x1b[4;1Hfoot",
        );
        assert_eq!(terminal.text(), "head\r\nb\r\nc\r\nfoot");
        assert_eq!(
            terminal.line_count(false),
            4,
            "region scrolling adds no scrollback"
        );
    }

    #[test]
    fn colours_and_attributes_are_kept_per_cell() {
        let terminal = terminal(10, 2, "\x1b[1;31mred\x1b[0m plain");
        let line = terminal.line(0);
        assert_eq!(line[0].style.foreground, Color::Indexed(1));
        assert!(line[0].style.bold);
        assert_eq!(line[4].style, Style::default());
        // Erasing with a background colour keeps it, as PowerShell's error lines need.
        let erased = self::terminal(4, 2, "\x1b[44m\x1b[K");
        assert_eq!(erased.line(0)[3].style.background, Color::Indexed(4));
        assert_eq!(erased.line_count(false), 1);
    }

    #[test]
    fn the_alternate_screen_hides_and_restores_the_primary_one() {
        let mut terminal = terminal(10, 3, "prompt\r\n");
        terminal.feed(b"\x1b[?1049h\x1b[Heditor");
        assert!(terminal.alternate());
        assert_eq!(terminal.line_count(false), 3);
        assert_eq!(terminal.text(), "editor");
        terminal.feed(b"\x1b[?1049l");
        assert!(!terminal.alternate());
        assert_eq!(terminal.text(), "prompt");
        assert_eq!(terminal.cursor(), Some((1, 0)));
        // A program stopped on the alternate screen is left the same way.
        terminal.feed(b"\x1b[?1049hstopped");
        terminal.leave_alternate();
        assert!(!terminal.alternate());
        assert_eq!(terminal.text(), "prompt");
    }

    #[test]
    fn status_requests_are_answered() {
        let mut terminal = terminal(10, 3, "ab\x1b[6n\x1b[c\x1b[5n");
        assert_eq!(
            terminal.take_responses(),
            b"\x1b[1;3R\x1b[?1;2c\x1b[0n".to_vec()
        );
        assert!(terminal.take_responses().is_empty());
    }

    #[test]
    fn modes_change_cursor_keys_paste_and_cursor_visibility() {
        let mut terminal = terminal(10, 3, "\x1b[?1h\x1b[?2004h\x1b[?25l");
        let modes = terminal.modes();
        assert!(modes.application_cursor && modes.bracketed_paste && !modes.cursor_visible);
        assert_eq!(terminal.cursor(), None);
        terminal.feed(b"\x1b[?1l\x1b[?2004l\x1b[?25h");
        assert_eq!(terminal.modes(), Modes::default());
    }

    #[test]
    fn titles_and_unknown_sequences_never_show_as_text() {
        let terminal = terminal(
            20,
            3,
            "\x1b]0;C:\\Windows\\cmd.exe\x07a\x1b]2;t\x1b\\b\x1bP1$r\x1b\\c\x1b[?9001h\x1b[>4;1md\x1b(Be",
        );
        assert_eq!(terminal.text(), "abcde");
        assert_eq!(terminal.line(0)[3].style, Style::default());
    }

    #[test]
    fn utf8_split_across_reads_and_wide_characters_take_two_cells() {
        let mut terminal = Terminal::new(10, 2);
        let bytes = "café 漢字".as_bytes();
        terminal.feed(&bytes[..4]);
        terminal.feed(&bytes[4..]);
        assert_eq!(terminal.text(), "café 漢字");
        assert_eq!(terminal.cursor(), Some((0, 9)));
        assert_eq!(line_text(&terminal.line(0)[..7]), "café 漢");
        // Invalid bytes show as a replacement character rather than disappearing.
        let mut broken = Terminal::new(10, 2);
        broken.feed(b"a\xffb\xc3");
        broken.feed(b"c");
        assert_eq!(broken.text(), "a\u{fffd}b\u{fffd}c");
    }

    #[test]
    fn tabs_backspace_and_reverse_index_move_the_cursor() {
        let terminal = terminal(20, 3, "a\tb\x08c");
        assert_eq!(terminal.text(), "a       c");
        let reversed = self::terminal(20, 3, "x\r\ny\x1bM\x1bMz");
        // At the top, a reverse index scrolls the screen down.
        assert_eq!(reversed.text(), " z\r\nx\r\ny");
    }

    /// Throughput of streaming output into a full scrollback. Run with
    /// `cargo test --release -p core-launcher-v2 a_million_lines -- --ignored --nocapture`.
    #[test]
    #[ignore = "timing, not a check"]
    fn a_million_lines_stream_through_the_screen() {
        const LINES: usize = 1_000_000;
        let mut output = Vec::new();
        for number in 0..LINES {
            output.extend_from_slice(format!("line {number} of the output\r\n").as_bytes());
        }
        let mut terminal = Terminal::new(120, 30);
        let started = std::time::Instant::now();
        // Chunks the size the output reader passes on.
        for chunk in output.chunks(16 * 1024) {
            terminal.feed(chunk);
        }
        let elapsed = started.elapsed();
        println!(
            "{LINES} lines, {} MiB, in {elapsed:?} ({:.0} lines/s)",
            output.len() / (1024 * 1024),
            LINES as f64 / elapsed.as_secs_f64()
        );
        assert_eq!(terminal.dropped(), LINES - 29 - SCROLLBACK_LIMIT);
    }
}
