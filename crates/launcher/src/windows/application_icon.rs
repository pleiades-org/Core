use std::{os::windows::ffi::OsStrExt, path::Path};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::RECT,
        Graphics::Gdi::HDC,
        Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES,
        UI::{
            Controls::{ImageList_GetIcon, HIMAGELIST, ILD_TRANSPARENT},
            Shell::{
                SHGetFileInfoW, SHParseDisplayName, SHFILEINFOW, SHGFI_LARGEICON, SHGFI_PIDL,
                SHGFI_SYSICONINDEX,
            },
            WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_NORMAL, HICON},
        },
    },
};

/// An immutable, exclusively owned HICON. Icon handles are process-wide, not thread-affine.
/// Store the address for transfer; Arc keeps it alive throughout painting and cache eviction.
pub struct ApplicationIcon(usize);

impl ApplicationIcon {
    /// Call on a COM-initialized background thread: Shell extensions can block on I/O.
    pub fn load(path: &Path) -> Option<Self> {
        let packaged = path
            .as_os_str()
            .to_string_lossy()
            .starts_with("shell:AppsFolder\\");
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut item = std::ptr::null_mut();
        if packaged
            && unsafe { SHParseDisplayName(PCWSTR(path.as_ptr()), None, &mut item, 0, None) }
                .is_err()
        {
            return None;
        }
        let mut information = SHFILEINFOW::default();
        let result = unsafe {
            SHGetFileInfoW(
                if packaged {
                    PCWSTR(item.cast())
                } else {
                    PCWSTR(path.as_ptr())
                },
                FILE_FLAGS_AND_ATTRIBUTES(0),
                Some(&mut information),
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_SYSICONINDEX
                    | SHGFI_LARGEICON
                    | if packaged {
                        SHGFI_PIDL
                    } else {
                        Default::default()
                    },
            )
        };
        if !item.is_null() {
            unsafe {
                windows::Win32::System::Com::CoTaskMemFree(Some(item.cast()));
            }
        }
        if result == 0 {
            return None;
        }
        // Borrow the Shell list read-only and copy the base icon without shortcut overlays.
        let icon = unsafe {
            ImageList_GetIcon(
                HIMAGELIST(result as isize),
                information.iIcon,
                ILD_TRANSPARENT,
            )
        };
        if icon.0.is_null() {
            None
        } else {
            Some(Self(icon.0 as usize))
        }
    }

    /// Takes ownership of an icon created by this process, such as a decoded website icon.
    pub fn from_handle(icon: HICON) -> Option<Self> {
        (!icon.0.is_null()).then_some(Self(icon.0 as usize))
    }

    pub fn draw(&self, context: HDC, area: RECT, size: i32) -> bool {
        let left = area.left + (area.right - area.left - size) / 2;
        let top = area.top + (area.bottom - area.top - size) / 2;
        unsafe {
            DrawIconEx(
                context,
                left,
                top,
                HICON(self.0 as *mut _),
                size,
                size,
                0,
                None,
                DI_NORMAL,
            )
            .is_ok()
        }
    }
}

impl Drop for ApplicationIcon {
    fn drop(&mut self) {
        if let Err(error) = unsafe { DestroyIcon(HICON(self.0 as *mut _)) } {
            eprintln!("Could not release an application icon: {error}");
        }
    }
}
