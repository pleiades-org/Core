//! Players read from their window announce a new track, or a pause, only by changing the
//! window's title. While Core is visible, an out-of-context hook on just those processes passes
//! title changes on, so the bar follows without polling. It is removed when Core hides.
use std::cell::Cell;
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::{
        Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK},
        WindowsAndMessaging::{
            GetAncestor, PostMessageW, CHILDID_SELF, EVENT_OBJECT_NAMECHANGE, GA_ROOT,
            OBJID_WINDOW, WINEVENT_OUTOFCONTEXT, WM_APP,
        },
    },
};

pub const MEDIA_TITLE_CHANGED: u32 = WM_APP + 18;

thread_local! {
    // OUTOFCONTEXT callbacks run on the registering UI thread. Core has one window there.
    static RECIPIENT: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
}

#[derive(Default)]
pub struct TitleWatch {
    hooks: Vec<(u32, HWINEVENTHOOK)>,
}

impl TitleWatch {
    /// Watches exactly `processes`: new players are hooked and closed ones released.
    pub fn follow(&mut self, window: HWND, processes: &[u32]) {
        RECIPIENT.set(window);
        self.hooks.retain(|(process, hook)| {
            let open = processes.contains(process);
            if !open {
                release(*hook);
            }
            open
        });
        for process in processes {
            if self.hooks.iter().any(|(watched, _)| watched == process) {
                continue;
            }
            let hook = unsafe {
                SetWinEventHook(
                    EVENT_OBJECT_NAMECHANGE,
                    EVENT_OBJECT_NAMECHANGE,
                    None,
                    Some(title_changed),
                    *process,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                )
            };
            if hook.0.is_null() {
                eprintln!(
                    "Could not watch a player's window: {}",
                    windows::core::Error::from_win32()
                );
            } else {
                self.hooks.push((*process, hook));
            }
        }
    }

    /// Core hid: stop watching.
    pub fn clear(&mut self) {
        for (_, hook) in self.hooks.drain(..) {
            release(hook);
        }
    }
}

impl Drop for TitleWatch {
    fn drop(&mut self) {
        self.clear();
    }
}

fn release(hook: HWINEVENTHOOK) {
    if !unsafe { UnhookWinEvent(hook) }.as_bool() {
        eprintln!("Could not stop watching a player's window");
    }
}

/// Only a top-level window's own title counts; the player's inner objects change names often.
unsafe extern "system" fn title_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    window: HWND,
    object: i32,
    child: i32,
    _thread: u32,
    _time: u32,
) {
    if object != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 || window.0.is_null() {
        return;
    }
    if GetAncestor(window, GA_ROOT) != window {
        return;
    }
    let recipient = RECIPIENT.get();
    if !recipient.0.is_null() {
        let _ = PostMessageW(Some(recipient), MEDIA_TITLE_CHANGED, WPARAM(0), LPARAM(0));
    }
}
