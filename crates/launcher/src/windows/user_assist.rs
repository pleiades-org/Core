//! Windows' own record of the programs a person starts from Start, the taskbar and Explorer
//! (UserAssist, the data behind Start's "Most used"). Core only reads it, locally, to fill the
//! recently used apps shown when nothing is typed.
use std::{collections::HashMap, path::PathBuf};
use windows::{
    core::{GUID, PWSTR},
    Win32::{
        Foundation::ERROR_SUCCESS,
        System::{
            Com::CoTaskMemFree,
            Registry::{
                RegCloseKey, RegEnumValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
            },
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

    #[test]
    fn this_account_has_readable_usage() {
        // Every Windows account that has used Start has records; the order must be newest first.
        let usage = recent_usage();
        assert!(usage
            .windows(2)
            .all(|pair| pair[0].last_used >= pair[1].last_used));
    }
}
