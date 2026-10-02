use crate::windows::wide;
use std::{fs, path::Path};
use windows::{
    core::{Interface, PCWSTR},
    Win32::{
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED, STGM_READ,
        },
        UI::Shell::{IShellLinkW, ShellLink},
    },
};

const RAW_TARGET_PATH: u32 = 0x4;
const MAX_TARGET_UNITS: usize = 32768;

struct Apartment;

impl Apartment {
    fn initialize() -> Result<Self, String> {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .map_err(|error| format!("Windows could not initialize shortcut creation: {error}"))?;
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

pub(super) fn create_bytes(executable: &Path, directory: &Path) -> Result<Vec<u8>, String> {
    let _apartment = Apartment::initialize()?;
    let temporary = directory.join(format!("core-shortcut-{}.lnk", std::process::id()));
    if temporary.exists() {
        return Err(
            "A previous shortcut creation has not finished; close setup and try again.".into(),
        );
    }
    let result = (|| {
        let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
            .map_err(|error| error.to_string())?;
        let executable = wide(&executable.to_string_lossy());
        let working_directory = wide(&directory.to_string_lossy());
        (|| -> windows::core::Result<()> {
            unsafe {
                link.SetPath(PCWSTR(executable.as_ptr()))?;
                link.SetWorkingDirectory(PCWSTR(working_directory.as_ptr()))?;
                link.SetDescription(windows::core::w!("Core — application launcher"))?;
                link.SetIconLocation(PCWSTR(executable.as_ptr()), 0)?;
                let file: IPersistFile = link.cast()?;
                file.Save(PCWSTR(wide(&temporary.to_string_lossy()).as_ptr()), true)
            }
        })()
        .map_err(|error: windows::core::Error| error.to_string())?;
        fs::read(&temporary)
            .map_err(|error| format!("Could not read the generated shortcut: {error}"))
    })();
    if temporary.exists() {
        super::files::remove_file(&temporary)?;
    }
    result
}

pub(super) fn belongs_to(shortcut: &Path, executable: &Path) -> Result<bool, String> {
    if !shortcut.exists() {
        return Ok(false);
    }
    let _apartment = Apartment::initialize()?;
    let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
        .map_err(|error| error.to_string())?;
    let file: IPersistFile = link.cast().map_err(|error| error.to_string())?;
    let mut target = [0_u16; MAX_TARGET_UNITS];
    unsafe {
        file.Load(
            PCWSTR(wide(&shortcut.to_string_lossy()).as_ptr()),
            STGM_READ,
        )
        .map_err(|error| error.to_string())?;
        link.GetPath(&mut target, std::ptr::null_mut(), RAW_TARGET_PATH)
    }
    .map_err(|error: windows::core::Error| error.to_string())?;
    let length = target
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(target.len());
    Ok(String::from_utf16_lossy(&target[..length])
        .eq_ignore_ascii_case(&executable.to_string_lossy()))
}
