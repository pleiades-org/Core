//! Shortcut fields record keys instead of taking typed text: click or Tab into one, then press
//! the keys. Held modifiers show as "Ctrl+Alt+…" until a key completes the shortcut.
//! Backspace or Delete clears a field that may be empty; Esc, Tab and a bare Enter keep their
//! usual meaning (close Settings, move on, Done), so they are never recorded on their own.
use super::Shortcut;
use crate::windows::view::control_text;
use crate::windows::wide;
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::*,
        UI::{
            Controls::{EM_SETCUEBANNER, EM_SETSEL},
            Input::{Ime::ImmAssociateContextEx, KeyboardAndMouse::*},
            Shell::{DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    },
};

const RECORDER_SUBCLASS: usize = 3;
/// Sent to the parent as a WM_COMMAND notification when a field records or clears a shortcut.
pub const SHORTCUT_RECORDED: u32 = 0x0B01;
/// Posted to the parent when a field gains (word 1) or loses (word 0) the keyboard focus; the
/// field is in the long parameter. Posted rather than sent: focus can move while Core's state
/// is busy, and a sent notification would then be lost.
pub const RECORDER_FOCUS: u32 = WM_APP + 17;
/// The cue shown in an empty media shortcut field.
pub const NOT_SET_CUE: &str = "Not set · press keys";

struct Recorder {
    /// A cleared field means "no shortcut" (media shortcuts); otherwise clearing is ignored.
    clearable: bool,
    /// The field's text when it gained focus, restored if it is left showing a partial shortcut.
    previous: String,
}

/// Turns an existing edit control into a recorder. Its look, position and font are unchanged.
pub fn install(edit: HWND, clearable: bool, cue: &str) -> windows::core::Result<()> {
    let recorder = Box::into_raw(Box::new(Recorder {
        clearable,
        previous: String::new(),
    }));
    let cue = wide(cue);
    unsafe {
        if let Err(error) = SetWindowSubclass(
            edit,
            Some(recorder_proc),
            RECORDER_SUBCLASS,
            recorder as usize,
        )
        .ok()
        {
            drop(Box::from_raw(recorder));
            return Err(error);
        }
        // Keys are read as keys: an input method would compose them into text instead.
        let _ = ImmAssociateContextEx(edit, Default::default(), 0);
        SendMessageW(
            edit,
            EM_SETCUEBANNER,
            Some(WPARAM(1)),
            Some(LPARAM(cue.as_ptr() as isize)),
        );
    }
    Ok(())
}

/// Whether `window` is a recorder, so the message loop hands it every key it may record.
pub fn is_recorder(window: HWND) -> bool {
    let mut data = 0_usize;
    unsafe {
        GetWindowSubclass(
            window,
            Some(recorder_proc),
            RECORDER_SUBCLASS,
            Some(&mut data as *mut usize),
        )
    }
    .as_bool()
}

/// Modifiers held without a key yet: the field shows "Ctrl+Alt+…", which is not a shortcut.
pub fn is_partial(edit: HWND) -> bool {
    control_text(edit).ends_with('…')
}

/// Whether a key pressed in a recorder belongs to Settings rather than the shortcut.
pub fn settings_key(key: VIRTUAL_KEY) -> bool {
    key == VK_ESCAPE || key == VK_TAB || (key == VK_RETURN && held_modifiers() == 0)
}

/// Ctrl, Alt, Shift and Win as Windows hotkey modifier flags.
pub fn held_modifiers() -> u32 {
    let down = |key: VIRTUAL_KEY| unsafe { GetKeyState(i32::from(key.0)) } < 0;
    let mut modifiers = 0;
    if down(VK_CONTROL) {
        modifiers |= MOD_CONTROL.0;
    }
    if down(VK_MENU) {
        modifiers |= MOD_ALT.0;
    }
    if down(VK_SHIFT) {
        modifiers |= MOD_SHIFT.0;
    }
    if down(VK_LWIN) || down(VK_RWIN) {
        modifiers |= MOD_WIN.0;
    }
    modifiers
}

fn is_modifier(key: VIRTUAL_KEY) -> bool {
    [
        VK_CONTROL,
        VK_LCONTROL,
        VK_RCONTROL,
        VK_MENU,
        VK_LMENU,
        VK_RMENU,
        VK_SHIFT,
        VK_LSHIFT,
        VK_RSHIFT,
        VK_LWIN,
        VK_RWIN,
    ]
    .contains(&key)
}

/// What a key press in a recorder does to the field's text.
#[derive(Debug, PartialEq, Eq)]
enum Press {
    /// Modifiers held so far, shown as "Ctrl+Alt+…".
    Partial(String),
    /// A finished shortcut, or the unsupported keys pressed; Settings explains the latter.
    Recorded(String),
    Cleared,
    Ignored,
}

fn press(key: VIRTUAL_KEY, modifiers: u32, clearable: bool) -> Press {
    if is_modifier(key) {
        return Press::Partial(format!("{}…", Shortcut::modifier_text(modifiers)));
    }
    if modifiers == 0 && (key == VK_BACK || key == VK_DELETE) {
        return if clearable {
            Press::Cleared
        } else {
            Press::Ignored
        };
    }
    Press::Recorded(match Shortcut::from_keys(modifiers, key.0) {
        Ok(shortcut) => shortcut.to_string(),
        Err(_) => Shortcut::keys_text(modifiers, key.0),
    })
}

unsafe fn set_text(edit: HWND, text: &str) {
    let text = wide(text);
    let _ = SetWindowTextW(edit, PCWSTR(text.as_ptr()));
    SendMessageW(edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
}

unsafe fn post_focus(edit: HWND, focused: bool) {
    if let Ok(parent) = GetParent(edit) {
        let _ = PostMessageW(
            Some(parent),
            RECORDER_FOCUS,
            WPARAM(usize::from(focused)),
            LPARAM(edit.0 as isize),
        );
    }
}

unsafe fn notify(edit: HWND) {
    if let Ok(parent) = GetParent(edit) {
        let identifier = GetDlgCtrlID(edit) as usize;
        SendMessageW(
            parent,
            WM_COMMAND,
            Some(WPARAM(identifier | ((SHORTCUT_RECORDED as usize) << 16))),
            Some(LPARAM(edit.0 as isize)),
        );
    }
}

unsafe extern "system" fn recorder_proc(
    edit: HWND,
    message: u32,
    word: WPARAM,
    long: LPARAM,
    _subclass: usize,
    data: usize,
) -> LRESULT {
    let recorder = &mut *(data as *mut Recorder);
    match message {
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let key = VIRTUAL_KEY(word.0 as u16);
            match press(key, held_modifiers(), recorder.clearable) {
                Press::Partial(text) => set_text(edit, &text),
                Press::Recorded(text) => {
                    set_text(edit, &text);
                    notify(edit);
                }
                Press::Cleared => {
                    set_text(edit, "");
                    notify(edit);
                }
                Press::Ignored => {}
            }
            return LRESULT(0);
        }
        // Releasing modifiers without a key leaves the shortcut as it was.
        WM_KEYUP | WM_SYSKEYUP => {
            if is_partial(edit) && held_modifiers() == 0 {
                set_text(edit, &recorder.previous);
            }
            return LRESULT(0);
        }
        // Typed characters, paste and the edit menu would change the text without a shortcut.
        WM_CHAR | WM_SYSCHAR | WM_DEADCHAR | WM_SYSDEADCHAR | WM_PASTE | WM_CUT | WM_CLEAR
        | WM_UNDO | WM_CONTEXTMENU => return LRESULT(0),
        WM_SETFOCUS => {
            recorder.previous = control_text(edit);
            let result = DefSubclassProc(edit, message, word, long);
            SendMessageW(edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
            post_focus(edit, true);
            return result;
        }
        WM_KILLFOCUS => {
            if is_partial(edit) {
                set_text(edit, &recorder.previous);
            }
            post_focus(edit, false);
        }
        WM_NCDESTROY => {
            if !RemoveWindowSubclass(edit, Some(recorder_proc), RECORDER_SUBCLASS).as_bool() {
                eprintln!("Could not remove the shortcut recorder");
            }
            drop(Box::from_raw(data as *mut Recorder));
            return DefSubclassProc(edit, message, word, long);
        }
        _ => {}
    }
    DefSubclassProc(edit, message, word, long)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presses_build_shortcut_text_and_only_clearable_fields_clear() {
        assert_eq!(
            press(VK_CONTROL, MOD_CONTROL.0 | MOD_ALT.0, false),
            Press::Partial("Ctrl+Alt+…".into())
        );
        assert_eq!(
            press(VK_RIGHT, MOD_ALT.0, false),
            Press::Recorded("Alt+Right".into())
        );
        assert_eq!(
            press(VK_MEDIA_PLAY_PAUSE, 0, true),
            Press::Recorded("MediaPlayPause".into())
        );
        assert_eq!(press(VK_BACK, 0, true), Press::Cleared);
        assert_eq!(press(VK_DELETE, 0, false), Press::Ignored);
        // Ctrl+Delete is a shortcut like any other.
        assert_eq!(
            press(VK_DELETE, MOD_CONTROL.0, false),
            Press::Recorded("Ctrl+Delete".into())
        );
        // A letter alone or an unsupported key is shown, and Settings explains why it fails.
        for (key, modifiers) in [
            (VIRTUAL_KEY(u16::from(b'K')), 0),
            (VK_CAPITAL, MOD_CONTROL.0),
        ] {
            let Press::Recorded(text) = press(key, modifiers, false) else {
                panic!("not recorded");
            };
            assert!(Shortcut::parse(&text).is_err(), "{text}");
        }
    }
}
