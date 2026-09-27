use super::windows_key_state::WindowsKeyState;
use std::{
    cell::RefCell,
    sync::mpsc,
    thread::{self, JoinHandle},
};
use windows::Win32::{
    Foundation::*,
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

const REPLAY_TAG: usize = 0x434F5245;
pub const SHORTCUT_ERROR: u32 = WM_APP + 11;
/// Posted to the hook thread, which owns the key state.
const RESET_STATE: u32 = WM_APP + 12;
thread_local! { static CONTEXT: RefCell<Option<HookContext>> = const { RefCell::new(None) }; }
struct HookContext {
    window: HWND,
    state: WindowsKeyState,
}

pub(super) struct WindowsKeyHook {
    thread: Option<JoinHandle<()>>,
    identifier: u32,
}

impl WindowsKeyHook {
    pub(super) fn start(window: HWND) -> Result<Self, String> {
        let address = window.0 as usize;
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("core-windows-key".into())
            .spawn(move || {
                let result = unsafe {
                    GetModuleHandleW(None).and_then(|module| {
                        SetWindowsHookExW(
                            WH_KEYBOARD_LL,
                            Some(keyboard_hook),
                            Some(module.into()),
                            0,
                        )
                    })
                };
                let hook = match result {
                    Ok(hook) => hook,
                    Err(error) => {
                        let _ = sender.send(Err(error.to_string()));
                        return;
                    }
                };
                CONTEXT.with(|context| {
                    *context.borrow_mut() = Some(HookContext {
                        window: HWND(address as *mut _),
                        state: WindowsKeyState::default(),
                    })
                });
                let mut message = MSG::default();
                unsafe {
                    let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
                }
                if sender.send(Ok(unsafe { GetCurrentThreadId() })).is_ok() {
                    loop {
                        let status = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
                        if status <= 0 {
                            break;
                        }
                        if message.message == RESET_STATE {
                            CONTEXT.with(|context| {
                                if let Some(context) = context.borrow_mut().as_mut() {
                                    context.state = WindowsKeyState::default();
                                }
                            });
                            continue;
                        }
                        unsafe {
                            let _ = TranslateMessage(&message);
                            DispatchMessageW(&message);
                        }
                    }
                }
                unsafe {
                    if let Err(error) = UnhookWindowsHookEx(hook) {
                        eprintln!("Could not remove Windows-key hook: {error}");
                    }
                }
                CONTEXT.with(|context| context.borrow_mut().take());
            })
            .map_err(|error| format!("Could not start Windows-key handling: {error}"))?;
        match receiver.recv() {
            Ok(Ok(identifier)) => Ok(Self {
                thread: Some(thread),
                identifier,
            }),
            outcome => {
                let _ = thread.join();
                Err(format!(
                    "Could not install Windows-key handling: {outcome:?}"
                ))
            }
        }
    }
}

impl WindowsKeyHook {
    /// Key releases on the secure desktop (lock screen, UAC) never reach the hook. Forget any
    /// Windows key still recorded as held so the next tap opens Core instead of Start.
    pub(super) fn reset(&self) {
        if let Err(error) =
            unsafe { PostThreadMessageW(self.identifier, RESET_STATE, WPARAM(0), LPARAM(0)) }
        {
            eprintln!("Could not reset Windows-key state: {error}");
        }
    }
}

impl Drop for WindowsKeyHook {
    fn drop(&mut self) {
        if let Err(error) =
            unsafe { PostThreadMessageW(self.identifier, WM_QUIT, WPARAM(0), LPARAM(0)) }
        {
            eprintln!("Could not stop Windows-key thread: {error}");
        }
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                eprintln!("Windows-key thread stopped unexpectedly");
            }
        }
    }
}

unsafe extern "system" fn keyboard_hook(code: i32, word: WPARAM, long: LPARAM) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(None, code, word, long) };
    }
    let input = unsafe { &*(long.0 as *const KBDLLHOOKSTRUCT) };
    if input.dwExtraInfo == REPLAY_TAG {
        return unsafe { CallNextHookEx(None, code, word, long) };
    }
    let down = matches!(word.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
    let block = CONTEXT.with(|context| {
        let Ok(mut context) = context.try_borrow_mut() else {
            return false;
        };
        let Some(context) = context.as_mut() else {
            return false;
        };
        let modified = down
            && matches!(input.vkCode as u16, 0x5B | 0x5C)
            && [VK_CONTROL, VK_MENU, VK_SHIFT]
                .into_iter()
                .any(|key| unsafe { GetAsyncKeyState(key.0 as i32) } < 0);
        let decision = context.state.event(input.vkCode as u16, down, modified);
        if decision.activate {
            unsafe {
                let _ = PostMessageW(Some(context.window), WM_HOTKEY, WPARAM(0), LPARAM(0));
            }
        }
        if decision.replay_windows != 0 && !replay_shortcut(decision.replay_windows, input) {
            unsafe {
                let _ = PostMessageW(Some(context.window), SHORTCUT_ERROR, WPARAM(0), LPARAM(0));
            }
            return false;
        }
        decision.block
    });
    if block {
        LRESULT(1)
    } else {
        unsafe { CallNextHookEx(None, code, word, long) }
    }
}

fn replay_shortcut(pending: u8, original: &KBDLLHOOKSTRUCT) -> bool {
    let mut inputs = [INPUT::default(); 3];
    let mut count = 0;
    for (mask, key) in [(1, VK_LWIN), (2, VK_RWIN)] {
        if pending & mask != 0 {
            inputs[count] = keyboard_input(key, 0, KEYEVENTF_EXTENDEDKEY);
            count += 1;
        }
    }
    let flags = if original.flags.0 & LLKHF_EXTENDED.0 != 0 {
        KEYEVENTF_EXTENDEDKEY
    } else {
        KEYBD_EVENT_FLAGS(0)
    };
    inputs[count] = keyboard_input(
        VIRTUAL_KEY(original.vkCode as u16),
        original.scanCode as u16,
        flags,
    );
    count += 1;
    unsafe { SendInput(&inputs[..count], std::mem::size_of::<INPUT>() as i32) == count as u32 }
}

fn keyboard_input(key: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: scan,
                dwFlags: flags,
                dwExtraInfo: REPLAY_TAG,
                ..Default::default()
            },
        },
    }
}
