//! Other names Windows knows apps by, so typing `cmd` finds Command Prompt and `wt` finds
//! Windows Terminal: the program a Start Menu shortcut starts, and the app execution aliases
//! Store apps register in `%LOCALAPPDATA%\Microsoft\WindowsApps`.
use std::{
    collections::HashMap,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::Arc,
};
use windows::{
    core::{Interface, PCWSTR},
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        },
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile,
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ,
            },
            IO::DeviceIoControl,
        },
        UI::Shell::{IShellLinkW, ShellLink},
    },
};

/// Programs that start other programs; as an alias they would name the wrong thing. Explorer
/// also opens folder shortcuts, and File Explorer is found by its name anyway.
const GENERIC_PROGRAMS: [&str; 10] = [
    "update",
    "rundll32",
    "msiexec",
    "setup",
    "uninstall",
    "unins000",
    "launcher",
    "helper",
    "wscript",
    "explorer",
];
/// `SLGP_RAWPATH`: the stored path, without resolving the target (fast, and no disk access).
const RAW_PATH: u32 = 0x4;
const FSCTL_GET_REPARSE_POINT: u32 = 0x0009_00A8;
const IO_REPARSE_TAG_APPEXECLINK: u32 = 0x8000_001B;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const MAX_REPARSE_BYTES: usize = 16 * 1024;

/// Reads the program behind Start Menu shortcuts. One per discovery pass.
pub struct ShortcutReader {
    link: IShellLinkW,
    file: IPersistFile,
    /// The apartment this reader opened, closed when it is dropped.
    initialized: bool,
}

impl ShortcutReader {
    pub fn new() -> Option<Self> {
        let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
        let reader = (|| {
            let link: IShellLinkW =
                unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.ok()?;
            let file: IPersistFile = link.cast().ok()?;
            Some((link, file))
        })();
        match reader {
            Some((link, file)) => Some(Self {
                link,
                file,
                initialized,
            }),
            None => {
                if initialized {
                    unsafe { CoUninitialize() };
                }
                None
            }
        }
    }

    /// `cmd` for a shortcut to `%windir%\system32\cmd.exe`.
    pub fn program_alias(&self, shortcut: &Path) -> Option<Arc<str>> {
        let path: Vec<u16> = shortcut.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe { self.file.Load(PCWSTR(path.as_ptr()), STGM_READ) }.ok()?;
        let mut target = [0_u16; 1024];
        unsafe {
            self.link
                .GetPath(&mut target, std::ptr::null_mut(), RAW_PATH)
        }
        .ok()?;
        let length = target.iter().position(|&unit| unit == 0)?;
        alias_for_program(&String::from_utf16_lossy(&target[..length]))
    }
}

impl Drop for ShortcutReader {
    fn drop(&mut self) {
        if self.initialized {
            unsafe { CoUninitialize() };
        }
    }
}

/// The alias a program path gives: its file name without `.exe`, unless it is a launcher of
/// other programs.
fn alias_for_program(target: &str) -> Option<Arc<str>> {
    let target = Path::new(target.trim());
    let extension = target.extension()?.to_string_lossy().to_ascii_lowercase();
    if extension != "exe" && extension != "msc" {
        return None;
    }
    let stem = target.file_stem()?.to_string_lossy().to_lowercase();
    if stem.is_empty() || GENERIC_PROGRAMS.contains(&stem.as_str()) {
        return None;
    }
    Some(stem.into())
}

/// App execution aliases by the app ID they open, such as `wt` for
/// `Microsoft.WindowsTerminal_8wekyb3d8bbwe!App`.
pub fn execution_aliases() -> HashMap<String, Vec<Arc<str>>> {
    let mut aliases: HashMap<String, Vec<Arc<str>>> = HashMap::new();
    let Some(folder) = std::env::var_os("LOCALAPPDATA")
        .map(|base| PathBuf::from(base).join(r"Microsoft\WindowsApps"))
    else {
        return aliases;
    };
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return aliases;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(alias) = alias_for_program(&path.to_string_lossy()) else {
            continue;
        };
        if let Some(app_id) = read_reparse_point(&path).and_then(|data| app_exec_link(&data)) {
            aliases.entry(app_id).or_default().push(alias);
        }
    }
    aliases
}

fn read_reparse_point(path: &Path) -> Option<Vec<u8>> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            Some(HANDLE::default()),
        )
    }
    .ok()?;
    let mut data = vec![0_u8; MAX_REPARSE_BYTES];
    let mut returned = 0_u32;
    let read = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_GET_REPARSE_POINT,
            None,
            0,
            Some(data.as_mut_ptr().cast()),
            data.len() as u32,
            Some(&mut returned),
            None,
        )
    };
    unsafe {
        let _ = CloseHandle(handle);
    }
    read.ok()?;
    data.truncate(returned as usize);
    Some(data)
}

/// The app ID in an `APPEXECLINK` reparse point: a tag, a length, a version, then
/// NUL-separated UTF-16 strings for the package, the app ID and the target program.
fn app_exec_link(data: &[u8]) -> Option<String> {
    let tag = u32::from_le_bytes(data.get(..4)?.try_into().ok()?);
    if tag != IO_REPARSE_TAG_APPEXECLINK {
        return None;
    }
    let strings: Vec<u16> = data
        .get(12..)?
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let app_id = strings.split(|&unit| unit == 0).nth(1)?;
    let app_id = String::from_utf16(app_id).ok()?;
    app_id.contains('!').then_some(app_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_names_become_aliases_except_launchers() {
        assert_eq!(
            alias_for_program(r"%windir%\system32\cmd.exe").as_deref(),
            Some("cmd")
        );
        assert_eq!(
            alias_for_program(r"C:\Program Files\Microsoft VS Code\Code.exe").as_deref(),
            Some("code")
        );
        assert_eq!(
            alias_for_program(r"%windir%\system32\compmgmt.msc").as_deref(),
            Some("compmgmt")
        );
        assert_eq!(
            alias_for_program(r"C:\Users\Me\AppData\Local\Discord\Update.exe"),
            None
        );
        assert_eq!(alias_for_program(r"C:\Docs\readme.txt"), None);
        assert_eq!(alias_for_program(""), None);
    }

    #[test]
    fn app_exec_links_give_their_app_id() {
        let mut data = Vec::new();
        data.extend_from_slice(&IO_REPARSE_TAG_APPEXECLINK.to_le_bytes());
        data.extend_from_slice(&[0; 4]);
        data.extend_from_slice(&3_u32.to_le_bytes());
        for text in [
            "Microsoft.WindowsTerminal_8wekyb3d8bbwe",
            "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
            r"C:\Program Files\WindowsApps\Terminal\wt.exe",
        ] {
            for unit in text.encode_utf16().chain(Some(0)) {
                data.extend_from_slice(&unit.to_le_bytes());
            }
        }
        assert_eq!(
            app_exec_link(&data).as_deref(),
            Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App")
        );
        data[0] = 0;
        assert_eq!(app_exec_link(&data), None);
        assert_eq!(app_exec_link(&[1, 2]), None);
    }

    #[test]
    fn this_pc_resolves_its_own_shortcuts_and_aliases() {
        // Command Prompt's shortcut ships with every Windows installation.
        let reader = ShortcutReader::new().expect("Shell link reader");
        let shortcut = PathBuf::from(std::env::var_os("APPDATA").unwrap())
            .join(r"Microsoft\Windows\Start Menu\Programs\System Tools\Command Prompt.lnk");
        if shortcut.exists() {
            assert_eq!(reader.program_alias(&shortcut).as_deref(), Some("cmd"));
        }
        // Every alias found must name a Store app ID.
        assert!(execution_aliases()
            .keys()
            .all(|app_id| app_id.contains('!')));
    }
}
