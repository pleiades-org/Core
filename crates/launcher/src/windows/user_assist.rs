//! Windows' own record of the programs a person starts from Start, the taskbar and Explorer
//! (UserAssist, the data behind Start's "Most used"). Core only reads it, locally, to fill the
//! recently used apps shown when nothing is typed.
use std::{collections::HashMap, path::PathBuf};
use windows::{
    core::{GUID, PWSTR},
    Win32::{
        Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, WAIT_OBJECT_0},
        System::{
            Com::CoTaskMemFree,
            Registry::{
                RegCloseKey, RegEnumValueW, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY,
                HKEY_CURRENT_USER, KEY_NOTIFY, KEY_READ, REG_NOTIFY_CHANGE_LAST_SET,
            },
            Threading::{CreateEventW, WaitForSingleObject},
        },
        UI::Shell::{SHGetKnownFolderPath, KF_FLAG_DEFAULT},
    },
};

/// Programs and app IDs, then shortcuts (`.lnk` files).
const KEYS: [&str; 2] = [
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist\{CEBFF5CD-ACE2-4F4F-9178-9926F41749EA}\Count",
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist\{F4E57C4B-2036-45F0-A9AB-443BCFE33D9F}\Count",
];
/// Each record holds a run count at byte 4 and the last run time (a FILETIME) at byte 60.
const LAST_USED_OFFSET: usize = 60;
const MAX_VALUES: u32 = 10_000;
const MAX_NAME_CHARACTERS: usize = 16_384;

/// One program Windows has seen started, and when it last was.
#[derive(Debug, PartialEq, Eq)]
pub struct Usage {
    /// An app ID (`Microsoft.WindowsCalculator_8wekyb3d8bbwe!App`, `MSEdge`) or a path, which
    /// may start with a known folder: `{A77F5D77-2E2B-44C3-A6A2-ABA601054A51}\Notepad.lnk`.
    pub name: String,
    /// FILETIME: 100-nanosecond intervals since 1601.
    pub last_used: u64,
}

/// Every recorded program, most recently used first. Empty if Windows keeps no record.
pub fn recent_usage() -> Vec<Usage> {
    let mut usage: Vec<Usage> = KEYS.iter().flat_map(|key| read_key(key)).collect();
    usage.sort_by_key(|entry| std::cmp::Reverse(entry.last_used));
    usage
}

fn read_key(path: &str) -> Vec<Usage> {
    let path: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let mut key = HKEY::default();
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            windows::core::PCWSTR(path.as_ptr()),
            None,
            KEY_READ,
            &mut key,
        )
    };
    if opened != ERROR_SUCCESS {
        return Vec::new();
    }
    let mut usage = Vec::new();
    let mut name = vec![0_u16; MAX_NAME_CHARACTERS];
    let mut data = [0_u8; 128];
    for index in 0..MAX_VALUES {
        let mut name_length = name.len() as u32;
        let mut data_length = data.len() as u32;
        let status = unsafe {
            RegEnumValueW(
                key,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut name_length,
                None,
                None,
                Some(data.as_mut_ptr()),
                Some(&mut data_length),
            )
        };
        if status != ERROR_SUCCESS {
            // ERROR_NO_MORE_ITEMS ends the list; a record larger than expected is skipped.
            if status == windows::Win32::Foundation::ERROR_MORE_DATA {
                continue;
            }
            break;
        }
        if let Some(last_used) = last_used(&data[..data_length as usize]) {
            usage.push(Usage {
                name: rot13(&String::from_utf16_lossy(&name[..name_length as usize])),
                last_used,
            });
        }
    }
    unsafe {
        let _ = RegCloseKey(key);
    }
    usage
}

/// Tells whether Windows may have changed its record since the last check, so Core reads it
/// again only then. Windows signals each key's event at the next change; nothing polls.
pub struct UsageWatch {
    /// `None` for a key that cannot be watched, such as one Windows has not created yet.
    keys: Vec<(&'static str, Option<WatchedKey>)>,
}

impl UsageWatch {
    /// Create it before the first read, so no change after that read is missed.
    pub fn new() -> Self {
        Self::watching(&KEYS)
    }

    fn watching(paths: &[&'static str]) -> Self {
        Self {
            keys: paths
                .iter()
                .map(|path| (*path, WatchedKey::open(path)))
                .collect(),
        }
    }

    /// True if either key changed since the last call, or cannot be watched. A signalled key is
    /// re-armed before this returns, so a change made while the caller reads is reported next.
    pub fn changed(&mut self) -> bool {
        let mut changed = false;
        for (path, key) in &mut self.keys {
            match key {
                Some(watched) if !watched.signalled() => {}
                Some(watched) => {
                    changed = true;
                    if !watched.arm() {
                        *key = None;
                    }
                }
                // Read every time until the key can be watched.
                None => {
                    changed = true;
                    *key = WatchedKey::open(path);
                }
            }
        }
        changed
    }
}

/// One registry key and the auto-reset event Windows signals when a value in it changes.
struct WatchedKey {
    key: HKEY,
    event: HANDLE,
}

impl WatchedKey {
    fn open(path: &str) -> Option<Self> {
        let path: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        let mut key = HKEY::default();
        let opened = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                windows::core::PCWSTR(path.as_ptr()),
                None,
                KEY_NOTIFY,
                &mut key,
            )
        };
        if opened != ERROR_SUCCESS {
            return None;
        }
        let event = match unsafe { CreateEventW(None, false, false, None) } {
            Ok(event) => event,
            Err(_) => {
                unsafe {
                    let _ = RegCloseKey(key);
                }
                return None;
            }
        };
        // From here, dropping closes both handles.
        let watched = Self { key, event };
        watched.arm().then_some(watched)
    }

    /// Asks Windows to signal the event at the next change; each request fires once.
    fn arm(&self) -> bool {
        let status = unsafe {
            RegNotifyChangeKeyValue(
                self.key,
                false,
                REG_NOTIFY_CHANGE_LAST_SET,
                Some(self.event),
                true,
            )
        };
        status == ERROR_SUCCESS
    }

    /// Takes a pending signal without waiting.
    fn signalled(&self) -> bool {
        let status = unsafe { WaitForSingleObject(self.event, 0) };
        status == WAIT_OBJECT_0
    }
}

impl Drop for WatchedKey {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.key);
            let _ = CloseHandle(self.event);
        }
    }
}

/// The last run time, if the record has one.
fn last_used(record: &[u8]) -> Option<u64> {
    let bytes = record.get(LAST_USED_OFFSET..LAST_USED_OFFSET + 8)?;
    let last_used = u64::from_le_bytes(bytes.try_into().ok()?);
    (last_used > 0).then_some(last_used)
}

/// UserAssist stores names with each ASCII letter rotated by 13.
fn rot13(text: &str) -> String {
    text.chars()
        .map(|character| match character {
            'a'..='z' => (((character as u8 - b'a') + 13) % 26 + b'a') as char,
            'A'..='Z' => (((character as u8 - b'A') + 13) % 26 + b'A') as char,
            other => other,
        })
        .collect()
}

/// Expands a leading known-folder ID, as in `{GUID}\rest`, remembering each folder.
#[derive(Default)]
pub struct KnownFolders {
    folders: HashMap<String, Option<PathBuf>>,
}

impl KnownFolders {
    pub fn expand(&mut self, name: &str) -> Option<PathBuf> {
        let rest = name.strip_prefix('{')?;
        let (identifier, relative) = rest.split_once("}\\")?;
        let folder = self
            .folders
            .entry(identifier.to_ascii_uppercase())
            .or_insert_with(|| known_folder(identifier))
            .as_ref()?;
        Some(folder.join(relative))
    }
}

fn known_folder(identifier: &str) -> Option<PathBuf> {
    let identifier = GUID::try_from(identifier).ok()?;
    let path = unsafe { SHGetKnownFolderPath(&identifier, KF_FLAG_DEFAULT, None) }.ok()?;
    let text = unsafe { path.to_string() };
    unsafe { CoTaskMemFree(Some(path.0.cast())) };
    text.ok().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_rotated_back() {
        assert_eq!(
            rot13("Zvpebfbsg.JvaqbjfPnyphyngbe_8jrxlo3q8oojr!Ncc"),
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"
        );
        assert_eq!(rot13(&rot13("{A77F}\\Notepad.lnk")), "{A77F}\\Notepad.lnk");
    }

    #[test]
    fn records_without_a_run_time_are_ignored() {
        let mut record = [0_u8; 72];
        assert_eq!(last_used(&record), None);
        record[60..68].copy_from_slice(&133_000_000_000_000_000_u64.to_le_bytes());
        assert_eq!(last_used(&record), Some(133_000_000_000_000_000));
        assert_eq!(last_used(&record[..16]), None);
    }

    #[test]
    fn known_folders_expand_and_other_names_do_not() {
        let mut folders = KnownFolders::default();
        // FOLDERID_Windows.
        let expanded = folders
            .expand(r"{F38BF404-1D43-42F2-9305-67DE0B28FC23}\notepad.exe")
            .expect("Windows folder");
        assert!(
            expanded.ends_with("notepad.exe") && expanded.is_file(),
            "{expanded:?}"
        );
        assert_eq!(
            folders.expand("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"),
            None
        );
        assert_eq!(
            folders.expand(r"{00000000-0000-0000-0000-000000000000}\x.lnk"),
            None
        );
    }

    /// A key path no test creates, unique to this run.
    fn test_path(name: &str) -> &'static str {
        Box::leak(
            format!(
                r"Software\Pleiades\Core\Tests\{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )
            .into_boxed_str(),
        )
    }

    /// Signals arrive from the kernel; allow them a moment.
    fn eventually(mut condition: impl FnMut() -> bool) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            if condition() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn a_key_that_cannot_be_watched_always_counts_as_changed() {
        let mut watch = UsageWatch::watching(&[test_path("UsageWatchMissing")]);
        assert!(watch.changed());
        assert!(watch.changed());
    }

    #[test]
    fn watching_and_dropping_releases_every_handle() {
        use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
        let handles = || {
            let mut count = 0;
            unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) }.unwrap();
            count
        };
        drop(UsageWatch::new());
        let baseline = handles();
        for _ in 0..500 {
            let mut watch = UsageWatch::new();
            watch.changed();
        }
        // Each watch opens a key and an event per UserAssist key; a leak would add ~2,000.
        // Other tests running in parallel open and close a few handles of their own.
        assert!(handles() < baseline + 100, "{} → {}", baseline, handles());
    }

    /// A writable test key, deleted on drop.
    struct TestKey {
        path: Vec<u16>,
        key: HKEY,
    }

    impl TestKey {
        fn create(path: &str) -> Self {
            use windows::Win32::System::Registry::{
                RegCreateKeyExW, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE,
            };
            let path: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
            let mut key = HKEY::default();
            let status = unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    windows::core::PCWSTR(path.as_ptr()),
                    None,
                    windows::core::PCWSTR::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_SET_VALUE,
                    None,
                    &mut key,
                    None,
                )
            };
            assert_eq!(status, ERROR_SUCCESS);
            Self { path, key }
        }

        fn set(&self, value: u32) {
            use windows::Win32::System::Registry::{RegSetValueExW, REG_DWORD};
            let status = unsafe {
                RegSetValueExW(
                    self.key,
                    windows::core::w!("Count"),
                    None,
                    REG_DWORD,
                    Some(&value.to_le_bytes()),
                )
            };
            assert_eq!(status, ERROR_SUCCESS);
        }
    }

    impl Drop for TestKey {
        fn drop(&mut self) {
            use windows::Win32::System::Registry::RegDeleteKeyW;
            unsafe {
                let _ = RegCloseKey(self.key);
                let status =
                    RegDeleteKeyW(HKEY_CURRENT_USER, windows::core::PCWSTR(self.path.as_ptr()));
                if status != ERROR_SUCCESS {
                    eprintln!("Could not clean up the usage watch test key: {status:?}");
                }
            }
        }
    }

    #[test]
    #[ignore = "requires HKCU write access; run explicitly with --include-ignored"]
    fn a_change_is_reported_once_and_watching_continues() {
        let path = test_path("UsageWatch");
        // Not created yet: read every time, and watched once it exists.
        let mut watch = UsageWatch::watching(&[path]);
        assert!(watch.changed());
        let key = TestKey::create(path);
        assert!(watch.changed(), "the first check after creation opens it");
        assert!(!watch.changed(), "nothing changed since");
        key.set(1);
        assert!(eventually(|| watch.changed()));
        assert!(!watch.changed(), "one change is reported once");
        key.set(2);
        assert!(eventually(|| watch.changed()), "re-armed after a change");
    }

    #[test]
    fn this_account_has_readable_usage() {
        // Every Windows account that has used Start has records; the order must be newest first.
        let usage = recent_usage();
        assert!(usage
            .windows(2)
            .all(|pair| pair[0].last_used >= pair[1].last_used));
    }
}
