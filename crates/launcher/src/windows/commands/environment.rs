//! A fresh environment for each command. Core usually starts at sign-in, so its own PATH misses
//! anything installed since; Explorer's Run dialog rebuilds the environment the same way.
use std::path::PathBuf;
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    Security::{TOKEN_DUPLICATE, TOKEN_IMPERSONATE, TOKEN_QUERY},
    System::{
        Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock},
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

/// A Unicode environment block (`NAME=value\0…\0\0`) with `extra` variables set, replacing any
/// of the same name, or `None` when Windows cannot build one, in which case the child inherits
/// Core's environment.
pub fn fresh_block(extra: &[(&str, String)]) -> Option<Vec<u16>> {
    let mut token = HANDLE::default();
    unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_IMPERSONATE,
            &mut token,
        )
        .ok()?;
    }
    let mut block = std::ptr::null_mut();
    let created = unsafe { CreateEnvironmentBlock(&mut block, Some(token), false) };
    unsafe {
        let _ = CloseHandle(token);
    }
    created.ok()?;
    let mut entries = Vec::new();
    unsafe {
        let mut cursor = block as *const u16;
        // Copy entries until the empty entry that ends the block.
        loop {
            let length = (0..).take_while(|&index| *cursor.add(index) != 0).count();
            if length == 0 {
                break;
            }
            let entry = std::slice::from_raw_parts(cursor, length + 1);
            if !extra.iter().any(|(name, _)| names_variable(entry, name)) {
                entries.extend_from_slice(entry);
            }
            cursor = cursor.add(length + 1);
        }
        let _ = DestroyEnvironmentBlock(block);
    }
    for (name, value) in extra {
        entries.extend(format!("{name}={value}").encode_utf16().chain(Some(0)));
    }
    entries.push(0);
    Some(entries)
}

/// Whether a `NAME=value` entry sets `name`. Names ignore case, as Windows treats them.
/// Compared as UTF-16, so the block's many entries are checked without allocating.
fn names_variable(entry: &[u16], name: &str) -> bool {
    let Some(end) = entry.iter().position(|&unit| unit == u16::from(b'=')) else {
        return false;
    };
    let upper =
        |unit: u16| u8::try_from(unit).map_or(unit, |byte| byte.to_ascii_uppercase().into());
    entry[..end]
        .iter()
        .map(|&unit| upper(unit))
        .eq(name.encode_utf16().map(upper))
}

/// Where commands start, like a new terminal: the user's profile folder.
pub fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fresh_environment_has_a_path_and_the_extra_variables() {
        let block = fresh_block(&[("CORE_TEST", "1".into())]).expect("environment block");
        let text = String::from_utf16_lossy(&block);
        let entries: Vec<&str> = text.split('\0').filter(|entry| !entry.is_empty()).collect();
        assert!(entries
            .iter()
            .any(|entry| entry.to_ascii_uppercase().starts_with("PATH=")));
        assert!(entries.contains(&"CORE_TEST=1"));
        assert!(block.ends_with(&[0, 0]));
        // An extra variable replaces one Windows already set, so it appears once.
        let block = fresh_block(&[("Path", "C:\\core-only".into())]).expect("environment block");
        let text = String::from_utf16_lossy(&block);
        let paths: Vec<&str> = text
            .split('\0')
            .filter(|entry| entry.to_ascii_uppercase().starts_with("PATH="))
            .collect();
        assert_eq!(paths, ["Path=C:\\core-only"]);
    }

    #[test]
    fn variable_names_match_ignoring_ascii_case_only() {
        let entry = |text: &str| -> Vec<u16> { text.encode_utf16().chain(Some(0)).collect() };
        assert!(names_variable(&entry("Path=C:\\Windows"), "PATH"));
        assert!(names_variable(&entry("core_test="), "CORE_TEST"));
        assert!(!names_variable(&entry("PATHEXT=.COM"), "PATH"));
        assert!(!names_variable(&entry("PAT=x"), "PATH"));
        // Windows keeps per-drive folders in entries whose name is empty.
        assert!(!names_variable(&entry("=C:=C:\\Users"), "C:"));
        assert!(!names_variable(&entry("PATH"), "PATH"));
        assert!(names_variable(&entry("Größe=1"), "GRößE"));
        assert!(!names_variable(&entry("Größe=1"), "GRÖSSE"));
    }
}
