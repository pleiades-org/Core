use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::HWND,
        Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST},
        UI::WindowsAndMessaging::{FindWindowExW, FindWindowW, SetForegroundWindow},
    },
};

const PRIMARY_TASKBAR_CLASS: PCWSTR = w!("Shell_TrayWnd");
const SECONDARY_TASKBAR_CLASS: PCWSTR = w!("Shell_SecondaryTrayWnd");

/// Activating Explorer's taskbar window slides an auto-hidden taskbar into view, matching what a
/// Windows-key tap does when Core has not replaced that key. Called while Core is still the
/// foreground window, so Windows permits the foreground change.
pub fn reveal(window: HWND) -> Result<(), String> {
    let taskbar = taskbar_on_monitor_of(window).ok_or_else(|| {
        "Could not find the Windows taskbar. Explorer may be restarting".to_owned()
    })?;
    if unsafe { SetForegroundWindow(taskbar) }.as_bool() {
        Ok(())
    } else {
        Err("Windows did not allow Core to show the taskbar".into())
    }
}

/// Prefers the taskbar on Core's display; falls back to the primary taskbar.
fn taskbar_on_monitor_of(window: HWND) -> Option<HWND> {
    let primary = unsafe { FindWindowW(PRIMARY_TASKBAR_CLASS, PCWSTR::null()) }.ok();
    let monitor = unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST) };
    let on_monitor =
        |taskbar: HWND| unsafe { MonitorFromWindow(taskbar, MONITOR_DEFAULTTONEAREST) } == monitor;
    if primary.is_some_and(on_monitor) {
        return primary;
    }
    let mut previous = None;
    while let Ok(secondary) =
        unsafe { FindWindowExW(None, previous, SECONDARY_TASKBAR_CLASS, PCWSTR::null()) }
    {
        if on_monitor(secondary) {
            return Some(secondary);
        }
        previous = Some(secondary);
    }
    primary
}
