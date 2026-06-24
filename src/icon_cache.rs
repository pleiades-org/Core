use crate::paths::icons_dir;
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

const ICON_SIZE_IN_PIXELS: i32 = 32;

pub fn cached_icon_path_for(target_path: &Path) -> Option<PathBuf> {
    let icon_directory = icons_dir();
    fs::create_dir_all(&icon_directory).ok()?;

    let icon_path = icon_directory.join(format!("{:016x}.png", stable_path_hash(target_path)));
    if icon_path.exists() {
        return Some(icon_path);
    }

    extract_icon_to_png(target_path, &icon_path).ok()?;
    icon_path.exists().then_some(icon_path)
}

fn stable_path_hash(target_path: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    target_path
        .to_string_lossy()
        .to_lowercase()
        .hash(&mut hasher);
    hasher.finish()
}

#[cfg(target_os = "windows")]
fn extract_icon_to_png(target_path: &Path, icon_path: &Path) -> Result<(), String> {
    use image::RgbaImage;
    use std::{ffi::OsStr, iter, mem, os::windows::ffi::OsStrExt, ptr};
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::HANDLE,
            Graphics::Gdi::{
                CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject,
                BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
            },
            Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES,
            UI::{
                Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON},
                WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_NORMAL},
            },
        },
    };

    let wide_path = OsStr::new(target_path)
        .encode_wide()
        .chain(iter::once(0))
        .collect::<Vec<_>>();

    let mut shell_file_info = SHFILEINFOW::default();
    let shell_result = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide_path.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut shell_file_info),
            mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };

    if shell_result == 0 || shell_file_info.hIcon.is_invalid() {
        return Err(format!(
            "Windows Shell did not return an icon for {target_path:?}"
        ));
    }

    let mut bitmap_bits = ptr::null_mut();
    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: ICON_SIZE_IN_PIXELS,
            biHeight: -ICON_SIZE_IN_PIXELS,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: (ICON_SIZE_IN_PIXELS * ICON_SIZE_IN_PIXELS * 4) as u32,
            ..Default::default()
        },
        ..Default::default()
    };

    let memory_device_context = unsafe { CreateCompatibleDC(None) };
    if memory_device_context.is_invalid() {
        unsafe {
            let _ = DestroyIcon(shell_file_info.hIcon);
        }
        return Err("Could not create a memory device context for icon extraction.".to_string());
    }

    let bitmap = unsafe {
        CreateDIBSection(
            Some(memory_device_context),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bitmap_bits,
            None::<HANDLE>,
            0,
        )
    }
    .map_err(|error| error.to_string())?;

    if bitmap_bits.is_null() {
        unsafe {
            let _ = DeleteDC(memory_device_context);
            let _ = DestroyIcon(shell_file_info.hIcon);
        }
        return Err("Windows returned an empty icon bitmap.".to_string());
    }

    let old_bitmap = unsafe { SelectObject(memory_device_context, HGDIOBJ::from(bitmap)) };
    let draw_result = unsafe {
        DrawIconEx(
            memory_device_context,
            0,
            0,
            shell_file_info.hIcon,
            ICON_SIZE_IN_PIXELS,
            ICON_SIZE_IN_PIXELS,
            0,
            None,
            DI_NORMAL,
        )
    };

    let byte_count = (ICON_SIZE_IN_PIXELS * ICON_SIZE_IN_PIXELS * 4) as usize;
    let bgra_bytes = unsafe { std::slice::from_raw_parts(bitmap_bits.cast::<u8>(), byte_count) };
    let rgba_bytes = bgra_bytes
        .chunks_exact(4)
        .flat_map(|pixel| [pixel[2], pixel[1], pixel[0], pixel[3]])
        .collect::<Vec<_>>();

    unsafe {
        if !old_bitmap.is_invalid() {
            let _ = SelectObject(memory_device_context, old_bitmap);
        }
        let _ = DeleteObject(HGDIOBJ::from(bitmap));
        let _ = DeleteDC(memory_device_context);
        let _ = DestroyIcon(shell_file_info.hIcon);
    }

    draw_result.map_err(|error| error.to_string())?;

    let icon_image = RgbaImage::from_raw(
        ICON_SIZE_IN_PIXELS as u32,
        ICON_SIZE_IN_PIXELS as u32,
        rgba_bytes,
    )
    .ok_or_else(|| "Could not build RGBA app icon image.".to_string())?;

    icon_image
        .save(icon_path)
        .map_err(|error| format!("Could not save app icon: {error}"))
}

#[cfg(not(target_os = "windows"))]
fn extract_icon_to_png(_target_path: &Path, _icon_path: &Path) -> Result<(), String> {
    Err("App icon extraction is only implemented on Windows.".to_string())
}
