#[cfg(test)]
mod tests;

use super::wide;
use std::{
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::Arc,
};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::*,
        System::{DataExchange::*, Memory::*},
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    },
};

const UNICODE_TEXT_FORMAT: u32 = 13;

pub enum NativeAction {
    OpenApplication(PathBuf),
    CopyText(Arc<str>),
    OpenUrl(Arc<str>),
    OpenQuicklink(Arc<str>),
    Power(core_engine::search::PowerAction),
    RevealTaskbar,
    OpenTerminal {
        resolved: super::commands::ResolvedShell,
        command: Arc<str>,
        directory: std::path::PathBuf,
    },
    RunElevated {
        resolved: super::commands::ResolvedShell,
        command: Arc<str>,
        directory: std::path::PathBuf,
    },
    OpenRunTarget {
        target: Arc<str>,
        elevated: bool,
    },
}

impl NativeAction {
    pub fn execute(self, window: HWND) -> Result<(), String> {
        match self {
            Self::OpenApplication(path) => open_application(window, &path),
            Self::CopyText(text) => copy_text(window, &text),
            Self::OpenUrl(url) => open_url(window, &url),
            Self::OpenQuicklink(link) => {
                let target = core_engine::quicklinks::validate_target(&link)?;
                open_application(window, Path::new(&target))
            }
            Self::Power(action) => super::power::execute(action),
            Self::RevealTaskbar => super::taskbar::reveal(window),
            Self::OpenTerminal {
                resolved,
                command,
                directory,
            } => super::commands::open_terminal(&resolved, &command, &directory),
            Self::RunElevated {
                resolved,
                command,
                directory,
            } => super::commands::run_elevated(window, &resolved, &command, &directory),
            Self::OpenRunTarget { target, elevated } => {
                super::commands::open_run_target(window, &target, elevated)
            }
        }
    }
}

pub fn open_application(window: HWND, path: &Path) -> Result<(), String> {
    let target: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    open_target(window, &target, executable_directory(path))
}

fn executable_directory(path: &Path) -> Option<&Path> {
    if !path.is_absolute() || !path.extension()?.to_str()?.eq_ignore_ascii_case("exe") {
        return None;
    }
    // Direct executables often read data relative to their installation folder. Let Windows
    // retain its own launch rules for shortcuts, documents, URLs and packaged applications.
    path.parent()
}

pub fn open_url(window: HWND, url: &str) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://"))
        || url.chars().any(char::is_control)
    {
        return Err("Core can only open HTTP or HTTPS web links".into());
    }
    open_target(window, &wide(url), None)
}

fn open_target(window: HWND, target: &[u16], directory: Option<&Path>) -> Result<(), String> {
    let directory: Option<Vec<u16>> =
        directory.map(|path| path.as_os_str().encode_wide().chain(Some(0)).collect());
    let result = unsafe {
        ShellExecuteW(
            Some(window),
            w!("open"),
            PCWSTR(target.as_ptr()),
            None,
            directory
                .as_ref()
                .map_or(PCWSTR::null(), |path| PCWSTR(path.as_ptr())),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as usize <= 32 {
        return Err(format!(
            "Windows could not open the selected target (shell error {})",
            result.0 as usize
        ));
    }
    Ok(())
}

pub fn copy_text(window: HWND, text: &str) -> Result<(), String> {
    let text = wide(text);
    unsafe {
        OpenClipboard(Some(window)).map_err(|error| format!("Clipboard is busy: {error}"))?;
        let _clipboard = ClipboardGuard;
        let allocation =
            GlobalAlloc(GMEM_MOVEABLE, text.len() * 2).map_err(|error| error.to_string())?;
        let mut memory = ClipboardMemory(Some(allocation));
        let destination = GlobalLock(allocation).cast::<u16>();
        if destination.is_null() {
            return Err("Could not allocate clipboard text".into());
        }
        std::ptr::copy_nonoverlapping(text.as_ptr(), destination, text.len());
        // A zero return can mean the lock count reached zero, not an error.
        let _ = GlobalUnlock(allocation);
        EmptyClipboard().map_err(|error| error.to_string())?;
        SetClipboardData(UNICODE_TEXT_FORMAT, Some(HANDLE(allocation.0)))
            .map_err(|error| error.to_string())?;
        memory.0 = None; // Windows owns the allocation after successful transfer.
        Ok(())
    }
}

/// The clipboard's text, or an empty string when it holds none.
pub fn clipboard_text(window: HWND) -> Result<String, String> {
    unsafe {
        if IsClipboardFormatAvailable(UNICODE_TEXT_FORMAT).is_err() {
            return Ok(String::new());
        }
        OpenClipboard(Some(window)).map_err(|error| format!("Clipboard is busy: {error}"))?;
        let _clipboard = ClipboardGuard;
        let data = GetClipboardData(UNICODE_TEXT_FORMAT)
            .map_err(|error| format!("Could not read the clipboard: {error}"))?;
        let memory = HGLOBAL(data.0);
        let source = GlobalLock(memory).cast::<u16>();
        if source.is_null() {
            return Err("Could not read the clipboard".into());
        }
        // The text ends at its terminating null, within the allocation.
        let capacity = GlobalSize(memory) / 2;
        let length = (0..capacity)
            .take_while(|&index| *source.add(index) != 0)
            .count();
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(source, length));
        let _ = GlobalUnlock(memory);
        Ok(text)
    }
}

struct ClipboardGuard;
impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        if let Err(error) = unsafe { CloseClipboard() } {
            eprintln!("Could not close clipboard: {error}");
        }
    }
}
struct ClipboardMemory(Option<HGLOBAL>);
impl Drop for ClipboardMemory {
    fn drop(&mut self) {
        if let Some(memory) = self.0 {
            unsafe {
                let _ = GlobalFree(Some(memory));
            }
        }
    }
}
