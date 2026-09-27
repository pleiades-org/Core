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
fn names_variable(entry: &[u16], name: &str) -> bool {
    let text = String::from_utf16_lossy(entry);
    text.split_once('=')
        .is_some_and(|(entry_name, _)| entry_name.eq_ignore_ascii_case(name))
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
}
