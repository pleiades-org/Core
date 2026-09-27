use std::cell::Cell;
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::{
        Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK},
        WindowsAndMessaging::{
            PostMessageW, EVENT_SYSTEM_FOREGROUND, WINEVENT_OUTOFCONTEXT, WM_APP,
        },
    },
};

pub const FOREGROUND_CHANGED: u32 = WM_APP + 6;

thread_local! {
    // OUTOFCONTEXT callbacks run on the registering UI thread. Core has one launcher there.
    static RECIPIENT: Cell<Option<(HWND, HWINEVENTHOOK)>> = const { Cell::new(None) };
}

/// Observes completed foreground changes only while the launcher is visible.
pub struct ForegroundObserver(HWINEVENTHOOK);

impl ForegroundObserver {
    pub fn new(window: HWND) -> windows::core::Result<Self> {
        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(foreground_changed),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if hook.0.is_null() {
            return Err(windows::core::Error::from_win32());
        }
        RECIPIENT.set(Some((window, hook)));
        Ok(Self(hook))
    }

    pub fn matches(&self, identifier: usize) -> bool {
        self.0 .0 as usize == identifier
    }
}

impl Drop for ForegroundObserver {
    fn drop(&mut self) {
        RECIPIENT.set(None);
        if !unsafe { UnhookWinEvent(self.0) }.as_bool() {
            eprintln!("Could not stop Core's foreground observer");
        }
    }
}

unsafe extern "system" fn foreground_changed(
    hook: HWINEVENTHOOK,
    _event: u32,
    event_window: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    let Some((window, current_hook)) = RECIPIENT.get() else {
        return;
    };
    if current_hook != hook {
        return;
    }
    // Do not borrow launcher state from a possibly reentrant accessibility callback.
    if let Err(error) = PostMessageW(
        Some(window),
        FOREGROUND_CHANGED,
        WPARAM(hook.0 as usize),
        LPARAM(event_window.0 as isize),
    ) {
        eprintln!("Could not deliver Core's foreground change: {error}");
    }
}
