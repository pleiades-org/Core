use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::HINSTANCE,
        System::{
            LibraryLoader::GetModuleHandleW,
            Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD},
        },
        UI::{
            Controls::{LoadIconMetric, LIM_SMALL},
            WindowsAndMessaging::{DestroyIcon, LoadIconW, HICON},
        },
    },
};

/// Icon group resource IDs embedded by `build.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum IconResource {
    Application = 1,
    /// White glyph for dark taskbars.
    TrayWhite = 2,
    /// Black glyph for light taskbars.
    TrayBlack = 3,
}

impl IconResource {
    fn name(self) -> PCWSTR {
        // MAKEINTRESOURCEW: small integer IDs travel in the pointer value.
        PCWSTR(self as u16 as usize as *const u16)
    }

    /// The tray glyph that contrasts with the current taskbar theme.
    pub fn for_tray() -> Self {
        if taskbar_uses_light_theme() {
            Self::TrayBlack
        } else {
            Self::TrayWhite
        }
    }
}

/// Shared class icon. Windows owns resources loaded this way; never destroy it.
pub fn class_icon(instance: HINSTANCE) -> windows::core::Result<HICON> {
    unsafe { LoadIconW(Some(instance), IconResource::Application.name()) }
}

/// A DPI-correct small icon owned by Core.
pub struct OwnedIcon(pub HICON);

impl OwnedIcon {
    pub fn small(resource: IconResource) -> windows::core::Result<Self> {
        let instance: HINSTANCE = unsafe { GetModuleHandleW(None)? }.into();
        unsafe { LoadIconMetric(Some(instance), resource.name(), LIM_SMALL) }.map(Self)
    }
}

impl Drop for OwnedIcon {
    fn drop(&mut self) {
        if let Err(error) = unsafe { DestroyIcon(self.0) } {
            eprintln!("Could not release Core icon: {error}");
        }
    }
}

/// Windows stores the taskbar/Start theme separately from the app theme.
fn taskbar_uses_light_theme() -> bool {
    let mut value = 0_u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    };
    // Missing values mean the default dark taskbar.
    status.is_ok() && value != 0
}
