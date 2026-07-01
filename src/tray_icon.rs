#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrayIconEvent {
    ShowLauncher,
    OpenSettings,
    QuitApplication,
}

#[cfg(target_os = "windows")]
mod platform {
    use super::TrayIconEvent;
    use image;
    use std::{
        sync::{mpsc::Sender, Mutex, OnceLock},
        thread,
    };
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{HWND, LPARAM, LRESULT, WPARAM},
            UI::{
                Shell::{
                    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NOTIFYICONDATAW,
                },
                WindowsAndMessaging::{
                    AppendMenuW, CreateIcon, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
                    DestroyMenu, DispatchMessageW, GetMessageW, LoadIconW, RegisterClassW,
                    TrackPopupMenu, TranslateMessage, HICON, IDI_APPLICATION, MF_STRING,
                    MSG, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE,
                    WINDOW_STYLE, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_RBUTTONUP, WM_USER,
                    WNDCLASSW, WS_EX_TOOLWINDOW,
                },
            },
        },
    };

    const TRAY_ICON_ID: u32 = 1;
    const TRAY_CALLBACK_MESSAGE: u32 = WM_USER + 44;
    const TRAY_TOOLTIP: &str = "Core Launcher - Alt+Space";
    const TRAY_ICON_SIZE: i32 = 32;
    const APP_ICON_BGRA_PIXEL: [u8; 4] = [0xed, 0x3a, 0x7c, 0xff];
    const CORE_ICON_PNG: &[u8] = include_bytes!("../assets/Core.png");
    const TRAY_MENU_OPEN: usize = 1;
    const TRAY_MENU_SETTINGS: usize = 2;
    const TRAY_MENU_QUIT: usize = 3;

    static TRAY_EVENT_SENDER: OnceLock<Mutex<Option<Sender<TrayIconEvent>>>> = OnceLock::new();

    pub fn start_tray_icon_event_loop(event_sender: Sender<TrayIconEvent>) {
        let sender_slot = TRAY_EVENT_SENDER.get_or_init(|| Mutex::new(None));
        if let Ok(mut sender_guard) = sender_slot.lock() {
            *sender_guard = Some(event_sender);
        }

        let _ = thread::Builder::new()
            .name("core-launcher-tray".to_string())
            .spawn(run_tray_icon_message_loop);
    }

    fn run_tray_icon_message_loop() {
        let class_name = encode_wide_null("CoreLauncherTrayWindow");
        let window_name = encode_wide_null("Core Launcher Tray");

        let window_class = WNDCLASSW {
            lpfnWndProc: Some(tray_window_proc),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };

        let tray_window = unsafe {
            RegisterClassW(&window_class);
            CreateWindowExW(
                WINDOW_EX_STYLE(WS_EX_TOOLWINDOW.0),
                PCWSTR(class_name.as_ptr()),
                PCWSTR(window_name.as_ptr()),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )
        };

        let Ok(tray_window) = tray_window else {
            return;
        };

        if !add_tray_icon(tray_window) {
            return;
        }

        let mut message = MSG::default();
        while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    fn add_tray_icon(tray_window: HWND) -> bool {
        let tray_icon = create_core_icon_from_png()
            .or_else(create_purple_square_icon)
            .or_else(|| unsafe { LoadIconW(None, IDI_APPLICATION) }.ok())
            .unwrap_or_default();
        let mut notify_icon_data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: tray_window,
            uID: TRAY_ICON_ID,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: TRAY_CALLBACK_MESSAGE,
            hIcon: tray_icon,
            ..Default::default()
        };
        copy_wide_text(TRAY_TOOLTIP, &mut notify_icon_data.szTip);

        unsafe { Shell_NotifyIconW(NIM_ADD, &notify_icon_data) }.as_bool()
    }

    fn create_core_icon_from_png() -> Option<HICON> {
        let icon_image = image::load_from_memory(CORE_ICON_PNG).ok()?;
        let resized = icon_image.resize_exact(
            TRAY_ICON_SIZE as u32,
            TRAY_ICON_SIZE as u32,
            image::imageops::FilterType::Triangle,
        );
        let rgba = resized.into_rgba8();
        let pixel_count = (TRAY_ICON_SIZE * TRAY_ICON_SIZE) as usize;
        let and_mask = vec![0u8; pixel_count];
        let mut bgra_pixels = Vec::with_capacity(pixel_count * 4);
        for pixel in rgba.as_raw().chunks_exact(4) {
            bgra_pixels.push(pixel[2]);
            bgra_pixels.push(pixel[1]);
            bgra_pixels.push(pixel[0]);
            bgra_pixels.push(pixel[3]);
        }

        unsafe {
            CreateIcon(
                None,
                TRAY_ICON_SIZE,
                TRAY_ICON_SIZE,
                1,
                32,
                and_mask.as_ptr(),
                bgra_pixels.as_ptr(),
            )
            .ok()
        }
    }
    fn create_purple_square_icon() -> Option<HICON> {
        let pixel_count = (TRAY_ICON_SIZE * TRAY_ICON_SIZE) as usize;
        let and_mask = vec![0; pixel_count];
        let mut bgra_pixels = Vec::with_capacity(pixel_count * APP_ICON_BGRA_PIXEL.len());
        for _ in 0..pixel_count {
            bgra_pixels.extend_from_slice(&APP_ICON_BGRA_PIXEL);
        }

        unsafe {
            CreateIcon(
                None,
                TRAY_ICON_SIZE,
                TRAY_ICON_SIZE,
                1,
                (APP_ICON_BGRA_PIXEL.len() * 8) as u8,
                and_mask.as_ptr(),
                bgra_pixels.as_ptr(),
            )
            .ok()
        }
    }

    unsafe extern "system" fn tray_window_proc(
        tray_window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == TRAY_CALLBACK_MESSAGE && wparam.0 as u32 == TRAY_ICON_ID {
            let mouse_message = lparam.0 as u32;
            match mouse_message {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                    send_tray_event(TrayIconEvent::ShowLauncher);
                    return LRESULT(0);
                }
                WM_RBUTTONUP => {
                    if let Some(event) = show_tray_context_menu(tray_window) {
                        send_tray_event(event);
                    }
                    return LRESULT(0);
                }
                _ => {}
            }
        }

        unsafe { DefWindowProcW(tray_window, message, wparam, lparam) }
    }

    fn show_tray_context_menu(tray_window: HWND) -> Option<TrayIconEvent> {
        unsafe {
            let popup_menu = CreatePopupMenu().ok()?;
            let _ = AppendMenuW(
                popup_menu,
                MF_STRING,
                TRAY_MENU_OPEN,
                PCWSTR(encode_wide_null("Open Core Launcher").as_ptr()),
            );
            let _ = AppendMenuW(
                popup_menu,
                MF_STRING,
                TRAY_MENU_SETTINGS,
                PCWSTR(encode_wide_null("Settings").as_ptr()),
            );
            let _ = AppendMenuW(
                popup_menu,
                MF_STRING,
                TRAY_MENU_QUIT,
                PCWSTR(encode_wide_null("Quit").as_ptr()),
            );

            let selected_command = TrackPopupMenu(
                popup_menu,
                TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
                0,
                0,
                None,
                tray_window,
                None,
            );
            let _ = DestroyMenu(popup_menu);

            let command_id = selected_command.0 as usize;
            if command_id == 0 {
                return None;
            }

            match command_id {
                TRAY_MENU_OPEN => Some(TrayIconEvent::ShowLauncher),
                TRAY_MENU_SETTINGS => Some(TrayIconEvent::OpenSettings),
                TRAY_MENU_QUIT => Some(TrayIconEvent::QuitApplication),
                _ => None,
            }
        }
    }

    fn send_tray_event(event: TrayIconEvent) {
        let Some(sender_slot) = TRAY_EVENT_SENDER.get() else {
            return;
        };

        let Ok(sender_guard) = sender_slot.lock() else {
            return;
        };

        let Some(event_sender) = sender_guard.as_ref() else {
            return;
        };

        let _ = event_sender.send(event);
    }

    fn copy_wide_text(source_text: &str, destination_buffer: &mut [u16]) {
        let encoded_text: Vec<u16> = source_text.encode_utf16().collect();
        let copied_length = encoded_text
            .len()
            .min(destination_buffer.len().saturating_sub(1));
        destination_buffer[..copied_length].copy_from_slice(&encoded_text[..copied_length]);
    }

    fn encode_wide_null(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::TrayIconEvent;
    use std::sync::mpsc::Sender;

    pub fn start_tray_icon_event_loop(_event_sender: Sender<TrayIconEvent>) {}
}

pub use platform::start_tray_icon_event_loop;
