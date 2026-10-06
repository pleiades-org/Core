//! Ctrl+Backspace in Core's text boxes. Windows' own edit control types a control character
//! for it, so Core deletes the word before the caret itself, as editors and browsers do.
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::{
        Controls::{EM_GETSEL, EM_REPLACESEL, EM_SETSEL},
        WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW, SendMessageW},
    },
};

/// What one press removes together: a run of characters of one kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CharacterKind {
    Space,
    /// Letters, digits and `_`.
    Word,
    /// Punctuation and symbols, such as the `@` of a command or the `\` in a path.
    Mark,
}

fn kind(character: char) -> CharacterKind {
    if character.is_whitespace() {
        CharacterKind::Space
    } else if character.is_alphanumeric() || character == '_' {
        CharacterKind::Word
    } else {
        CharacterKind::Mark
    }
}

/// Where the word before `caret` starts. Both are in UTF-16 units, as edit controls count.
/// Spaces before the caret go with the word before them; a run of punctuation is a word of its
/// own, so `@song` and `C:\Games` are deleted a part at a time.
fn word_start(text: &[u16], caret: usize) -> usize {
    let before = &text[..caret.min(text.len())];
    // Half a surrogate pair is no character: it counts as a mark of one unit.
    let characters: Vec<(CharacterKind, usize)> = char::decode_utf16(before.iter().copied())
        .map(|decoded| match decoded {
            Ok(character) => (kind(character), character.len_utf16()),
            Err(_) => (CharacterKind::Mark, 1),
        })
        .collect();
    let mut remaining = characters.iter().rev().peekable();
    let mut start = before.len();
    while let Some((_, units)) = remaining.next_if(|(found, _)| *found == CharacterKind::Space) {
        start -= units;
    }
    let Some(&&(word, _)) = remaining.peek() else {
        return start;
    };
    while let Some((_, units)) = remaining.next_if(|(found, _)| *found == word) {
        start -= units;
    }
    start
}

fn text_units(edit: HWND) -> Vec<u16> {
    unsafe {
        let length = GetWindowTextLengthW(edit).max(0) as usize;
        let mut buffer = vec![0; length + 1];
        let copied = GetWindowTextW(edit, &mut buffer).max(0) as usize;
        buffer.truncate(copied);
        buffer
    }
}

/// Deletes what is selected in `edit`, or with nothing selected the word before the caret, as
/// one step that Ctrl+Z brings back. The box tells its window of the change as for typing.
pub fn delete_previous_word(edit: HWND) {
    let (mut start, mut end) = (0_u32, 0_u32);
    unsafe {
        SendMessageW(
            edit,
            EM_GETSEL,
            Some(WPARAM(&mut start as *mut u32 as usize)),
            Some(LPARAM(&mut end as *mut u32 as isize)),
        );
    }
    if start == end {
        let caret = end as usize;
        let word = word_start(&text_units(edit), caret);
        if word == caret {
            return;
        }
        unsafe {
            SendMessageW(
                edit,
                EM_SETSEL,
                Some(WPARAM(word)),
                Some(LPARAM(caret as isize)),
            );
        }
    }
    // Replacing the selection with nothing deletes it; the flag keeps it for Ctrl+Z.
    let nothing = [0_u16];
    unsafe {
        SendMessageW(
            edit,
            EM_REPLACESEL,
            Some(WPARAM(1)),
            Some(LPARAM(nothing.as_ptr() as isize)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::wide;
    use std::cell::Cell;
    use windows::{
        core::{w, PCWSTR},
        Win32::{
            System::LibraryLoader::GetModuleHandleW,
            UI::{
                Controls::{EM_CANUNDO, EM_UNDO},
                WindowsAndMessaging::*,
            },
        },
    };

    fn start(text: &str, caret: usize) -> usize {
        let units: Vec<u16> = text.encode_utf16().collect();
        word_start(&units, caret)
    }

    /// What is left of `text` after one press with the caret at its end.
    fn left(text: &str) -> String {
        let units: Vec<u16> = text.encode_utf16().collect();
        String::from_utf16(&units[..word_start(&units, units.len())]).unwrap()
    }

    #[test]
    fn a_press_removes_the_word_before_the_caret_with_the_spaces_after_it() {
        assert_eq!(left("open the pod bay"), "open the pod ");
        assert_eq!(left("open the pod   "), "open the ");
        assert_eq!(left("word"), "");
        assert_eq!(left("snake_case2 next"), "snake_case2 ");
        assert_eq!(left("snake_case2"), "");
        // Only spaces, or nothing at all, before the caret.
        assert_eq!(left("   "), "");
        assert_eq!(left(""), "");
        // The caret inside a word: only what is before it goes.
        assert_eq!(start("hello world", 8), 6);
        assert_eq!(start("hello world", 6), 0);
        assert_eq!(start("hello world", 0), 0);
        // A caret beyond the text, which a control never reports, is the text's end.
        assert_eq!(start("hello world", 99), 6);
    }

    #[test]
    fn punctuation_is_a_word_of_its_own() {
        // A command goes a part at a time: what was searched, the name, then the `@`.
        assert_eq!(left("@song basorexia"), "@song ");
        assert_eq!(left("@song "), "@");
        assert_eq!(left("@"), "");
        assert_eq!(left(r"C:\Games\Rocket League"), r"C:\Games\Rocket ");
        assert_eq!(left(r"C:\Games\"), r"C:\Games");
        assert_eq!(left(r"C:\Games"), r"C:\");
        assert_eq!(left("https://example.com"), "https://example.");
        assert_eq!(left("https://"), "https");
        assert_eq!(left("2 + 3 * (4"), "2 + 3 * (");
        assert_eq!(left("2 + 3 * ("), "2 + 3 * ");
        assert_eq!(left("2 + 3 * "), "2 + 3 ");
    }

    #[test]
    fn positions_count_utf_16_units_and_never_split_a_character() {
        assert_eq!(left("naïve café"), "naïve ");
        assert_eq!(left("東京 大阪"), "東京 ");
        // Each of these takes two units, and symbols are marks.
        assert_eq!(left("play 😀😀"), "play ");
        assert_eq!(start("play 😀😀", 9), 5);
        assert_eq!(left("😀 word"), "😀 ");
        // Mathematical letters outside the basic plane are letters all the same.
        assert_eq!(left("x 𝐀𝐁"), "x ");
        // Half a pair, which only a broken paste leaves behind, goes as one unit.
        let broken = [u16::from(b'a'), u16::from(b' '), 0xD83D];
        assert_eq!(word_start(&broken, broken.len()), 2);
    }

    thread_local! {
        /// How often the test window was told that its edit box changed.
        static CHANGES: Cell<u32> = const { Cell::new(0) };
    }

    unsafe extern "system" fn count_changes(
        window: HWND,
        message: u32,
        word: WPARAM,
        long: LPARAM,
    ) -> windows::Win32::Foundation::LRESULT {
        if message == WM_COMMAND && (word.0 >> 16) as u32 == EN_CHANGE {
            CHANGES.with(|changes| changes.set(changes.get() + 1));
        }
        DefWindowProcW(window, message, word, long)
    }

    /// A real edit box in a window that is never shown.
    #[test]
    fn an_edit_box_loses_the_word_tells_its_window_and_can_undo() {
        let _serial = crate::windows::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let instance = unsafe { GetModuleHandleW(None) }.unwrap().into();
        let class = WNDCLASSW {
            lpfnWndProc: Some(count_changes),
            hInstance: instance,
            lpszClassName: w!("Core.Test.WordDeletion"),
            ..Default::default()
        };
        assert_ne!(unsafe { RegisterClassW(&class) }, 0);
        let parent = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class.lpszClassName,
                w!(""),
                WS_POPUP,
                0,
                0,
                400,
                40,
                None,
                None,
                Some(instance),
                None,
            )
        }
        .unwrap();
        let edit = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("EDIT"),
                w!(""),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                0,
                0,
                400,
                40,
                Some(parent),
                None,
                Some(instance),
                None,
            )
        }
        .unwrap();
        let text = || String::from_utf16(&text_units(edit)).unwrap();
        let select = |from: usize, to: isize| unsafe {
            SendMessageW(edit, EM_SETSEL, Some(WPARAM(from)), Some(LPARAM(to)));
        };
        let set = |content: &str| {
            let content = wide(content);
            unsafe { SetWindowTextW(edit, PCWSTR(content.as_ptr())) }.unwrap();
            let end = content.len() - 1;
            select(end, end as isize);
            CHANGES.with(|changes| changes.set(0));
        };
        let changes = || CHANGES.with(|changes| changes.replace(0));

        set("@song 😀 basorexia  ");
        for expected in ["@song 😀 ", "@song ", "@", ""] {
            delete_previous_word(edit);
            assert_eq!(text(), expected);
            assert_eq!(changes(), 1, "{expected:?}");
        }
        // Nothing before the caret: nothing changes and nobody is told.
        delete_previous_word(edit);
        assert_eq!((text().as_str(), changes()), ("", 0));
        set("open the pod bay");
        select(0, 0);
        delete_previous_word(edit);
        assert_eq!((text().as_str(), changes()), ("open the pod bay", 0));
        // The caret inside a word keeps what follows it.
        select(7, 7);
        delete_previous_word(edit);
        assert_eq!(text(), "open e pod bay");
        // A selection goes instead, whatever it holds, as with Backspace alone.
        set("open the pod bay");
        select(2, 11);
        delete_previous_word(edit);
        assert_eq!(text(), "opd bay");
        // One press is one step for Ctrl+Z.
        set("open the pod bay");
        delete_previous_word(edit);
        assert_eq!(text(), "open the pod ");
        assert_ne!(unsafe { SendMessageW(edit, EM_CANUNDO, None, None) }.0, 0);
        unsafe { SendMessageW(edit, EM_UNDO, None, None) };
        assert_eq!(text(), "open the pod bay");

        unsafe { DestroyWindow(parent) }.unwrap();
        unsafe { UnregisterClassW(class.lpszClassName, Some(instance)) }.unwrap();
    }
}
