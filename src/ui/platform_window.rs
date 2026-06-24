use gpui::Window;

#[cfg(target_os = "windows")]
pub fn show_platform_window(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{ShowWindow, SW_SHOW},
    };

    if let Ok(window_handle) = HasWindowHandle::window_handle(window) {
        if let RawWindowHandle::Win32(win32_window_handle) = window_handle.as_raw() {
            unsafe {
                let _ = ShowWindow(
                    HWND(win32_window_handle.hwnd.get() as *mut std::ffi::c_void),
                    SW_SHOW,
                );
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn show_platform_window(_window: &Window) {}

#[cfg(target_os = "windows")]
pub fn hide_platform_window(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{ShowWindow, SW_HIDE},
    };

    if let Ok(window_handle) = HasWindowHandle::window_handle(window) {
        if let RawWindowHandle::Win32(win32_window_handle) = window_handle.as_raw() {
            unsafe {
                let _ = ShowWindow(
                    HWND(win32_window_handle.hwnd.get() as *mut std::ffi::c_void),
                    SW_HIDE,
                );
            }
            return;
        }
    }

    window.minimize_window();
}

#[cfg(not(target_os = "windows"))]
pub fn hide_platform_window(window: &Window) {
    window.minimize_window();
}