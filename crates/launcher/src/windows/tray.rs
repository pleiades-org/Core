use super::{
    app_icon::{IconResource, OwnedIcon},
    settings::Shortcut,
};
use std::sync::OnceLock;
use windows::{
    core::w,
    Win32::{
        Foundation::*,
        UI::{Shell::*, WindowsAndMessaging::*},
    },
};

pub const TRAY_EVENT: u32 = WM_APP + 3;
pub enum TrayAction {
    Open,
    Settings,
    Exit,
    None,
}

/// Explorer broadcasts this after it (re)creates the taskbar, including after a crash or restart.
pub fn taskbar_created_message() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) })
}

/// The notification-area icon. Construction never fails: when Explorer is not ready yet
/// (for example at sign-in), `show` fails and is retried on `taskbar_created_message`.
pub struct Tray {
    data: NOTIFYICONDATAW,
    // Keeps the HICON in `data` alive; `None` falls back to the shared system icon.
    icon: Option<OwnedIcon>,
    added: bool,
}

impl Tray {
    pub fn new(window: HWND, shortcut: Shortcut) -> Self {
        let mut tray = Self {
            data: NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: window,
                uID: 1,
                uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
                uCallbackMessage: TRAY_EVENT,
                ..Default::default()
            },
            icon: None,
            added: false,
        };
        tray.load_icon();
        tray.write_tooltip(shortcut);
        tray
    }

    /// Adds the icon, or re-adds it after Explorer restarted and forgot it.
    pub fn show(&mut self) -> windows::core::Result<()> {
        self.added = false;
        if !unsafe { Shell_NotifyIconW(NIM_ADD, &self.data) }.as_bool() {
            return Err(windows::core::Error::from_win32());
        }
        self.added = true;
        Ok(())
    }

    pub fn set_shortcut(&mut self, shortcut: Shortcut) -> windows::core::Result<()> {
        self.write_tooltip(shortcut);
        self.modify()
    }

    /// Swaps between the white and black glyph when the taskbar theme changes.
    pub fn refresh_theme(&mut self) -> windows::core::Result<()> {
        self.load_icon();
        self.modify()
    }

    fn modify(&self) -> windows::core::Result<()> {
        if self.added && !unsafe { Shell_NotifyIconW(NIM_MODIFY, &self.data) }.as_bool() {
            return Err(windows::core::Error::from_win32());
        }
        Ok(())
    }

    fn load_icon(&mut self) {
        match OwnedIcon::small(IconResource::for_tray()) {
            Ok(icon) => {
                self.data.hIcon = icon.0;
                // Replace only after `data` points at the new icon.
                self.icon = Some(icon);
            }
            Err(error) => {
                eprintln!("Could not load Core's tray icon: {error}");
                if self.icon.is_none() {
                    self.data.hIcon =
                        unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default();
                }
            }
        }
    }

    fn write_tooltip(&mut self, shortcut: Shortcut) {
        let label = match shortcut {
            Shortcut::WindowsKey => "Core v2 · Windows key".to_owned(),
            chord => format!("Core v2 · {chord}"),
        };
        let capacity = self.data.szTip.len() - 1;
        let units: Vec<u16> = label.encode_utf16().take(capacity).collect();
        self.data.szTip = [0; 128];
        self.data.szTip[..units.len()].copy_from_slice(&units);
    }

    /// `activate` brings Core forward so the menu closes when clicking elsewhere; background
    /// tests skip it so they never take the foreground.
    pub fn menu(window: HWND, activate: bool) -> windows::core::Result<TrayAction> {
        unsafe {
            let menu = CreatePopupMenu()?;
            let result = (|| {
                AppendMenuW(menu, MF_STRING, 1, w!("Open Core"))?;
                AppendMenuW(menu, MF_STRING, 3, w!("Settings"))?;
                AppendMenuW(menu, MF_SEPARATOR, 0, None)?;
                AppendMenuW(menu, MF_STRING, 2, w!("Exit Core"))?;
                let mut cursor = POINT::default();
                GetCursorPos(&mut cursor)?;
                if activate {
                    let _ = SetForegroundWindow(window);
                }
                let chosen = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_RIGHTBUTTON,
                    cursor.x,
                    cursor.y,
                    None,
                    window,
                    None,
                )
                .0;
                let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
                Ok(match chosen {
                    1 => TrayAction::Open,
                    2 => TrayAction::Exit,
                    3 => TrayAction::Settings,
                    _ => TrayAction::None,
                })
            })();
            DestroyMenu(menu)?;
            result
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        if self.added && !unsafe { Shell_NotifyIconW(NIM_DELETE, &self.data) }.as_bool() {
            eprintln!("Could not remove Core's tray icon");
        }
    }
}
