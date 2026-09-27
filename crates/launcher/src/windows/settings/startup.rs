use std::os::windows::ffi::OsStrExt;
use windows::{
    core::{w, PCWSTR},
    Win32::{Foundation::*, System::Registry::*},
};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: PCWSTR = w!("PleiadesCoreV2");

/// Rolls back this one Run value if saving the configuration subsequently fails.
pub(super) struct StartupChange {
    key: HKEY,
    previous: Option<(REG_VALUE_TYPE, Vec<u8>)>,
    committed: bool,
}

impl StartupChange {
    pub(super) fn apply(enabled: bool) -> Result<Self, String> {
        Self::apply_at(RUN_KEY, enabled)
    }

    fn apply_at(path: PCWSTR, enabled: bool) -> Result<Self, String> {
        let mut key = HKEY::default();
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path,
                None,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_QUERY_VALUE | KEY_SET_VALUE,
                None,
                &mut key,
                None,
            )
            .ok()
        }
        .map_err(|error| format!("Could not open Windows startup settings: {error}"))?;
        let mut change = Self {
            key,
            previous: None,
            committed: true,
        };
        change.previous = read_value(key)?;
        let command = if enabled {
            let executable = std::env::current_exe()
                .map_err(|error| format!("Could not locate Core: {error}"))?;
            let mut command: Vec<u16> = vec![b'"' as u16];
            command.extend(executable.as_os_str().encode_wide());
            command.extend("\" --start-hidden\0".encode_utf16());
            Some(
                command
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
        } else {
            None
        };
        write_value(
            key,
            command.as_ref().map(|bytes| (REG_SZ, bytes.as_slice())),
        )?;
        change.committed = false;
        Ok(change)
    }
    pub(super) fn commit(&mut self) {
        self.committed = true;
    }
}

fn read_value(key: HKEY) -> Result<Option<(REG_VALUE_TYPE, Vec<u8>)>, String> {
    let mut kind = REG_VALUE_TYPE(0);
    let mut length = 0;
    let status = unsafe {
        RegQueryValueExW(
            key,
            VALUE_NAME,
            None,
            Some(&mut kind),
            None,
            Some(&mut length),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    status
        .ok()
        .map_err(|error| format!("Could not read Core startup entry: {error}"))?;
    if length > 32_768 {
        return Err("Core's existing startup entry is too large to update safely.".into());
    }
    let mut bytes = vec![0; length as usize];
    unsafe {
        RegQueryValueExW(
            key,
            VALUE_NAME,
            None,
            Some(&mut kind),
            Some(bytes.as_mut_ptr()),
            Some(&mut length),
        )
        .ok()
    }
    .map_err(|error| format!("Could not read Core startup entry: {error}"))?;
    bytes.truncate(length as usize);
    Ok(Some((kind, bytes)))
}

fn write_value(key: HKEY, value: Option<(REG_VALUE_TYPE, &[u8])>) -> Result<(), String> {
    let status = unsafe {
        match value {
            Some((kind, bytes)) => RegSetValueExW(key, VALUE_NAME, None, kind, Some(bytes)),
            None => RegDeleteValueW(key, VALUE_NAME),
        }
    };
    if status == ERROR_FILE_NOT_FOUND && value.is_none() {
        return Ok(());
    }
    status
        .ok()
        .map_err(|error| format!("Could not update Windows startup: {error}"))
}

impl Drop for StartupChange {
    fn drop(&mut self) {
        if !self.committed {
            if let Err(error) = write_value(
                self.key,
                self.previous
                    .as_ref()
                    .map(|(kind, bytes)| (*kind, bytes.as_slice())),
            ) {
                eprintln!("Could not restore startup setting after save failure: {error}");
            }
        }
        let status = unsafe { RegCloseKey(self.key) };
        if status != ERROR_SUCCESS {
            eprintln!("Could not close startup registry key: {status:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires HKCU write access; run explicitly with --include-ignored"]
    fn startup_entry_is_quoted_and_failed_save_restores_exact_previous_value() {
        let path = crate::windows::wide(&format!(
            "Software\\Pleiades\\Core\\Tests\\Startup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _cleanup = TestKeyCleanup(path.clone());
        let path = PCWSTR(path.as_ptr());
        let mut enabled = StartupChange::apply_at(path, true).unwrap();
        let expected = read_value(enabled.key).unwrap().unwrap();
        let text: Vec<u16> = expected
            .1
            .chunks_exact(2)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
            .collect();
        let text = String::from_utf16(&text).unwrap();
        assert!(text.starts_with('"') && text.ends_with("\" --start-hidden\0"));
        enabled.commit();
        drop(enabled);
        {
            let disabled = StartupChange::apply_at(path, false).unwrap();
            assert_eq!(disabled.previous.as_ref(), Some(&expected));
            assert!(read_value(disabled.key).unwrap().is_none());
            // An uncommitted change models a failed settings-file replacement.
        }
        let mut inspected = StartupChange::apply_at(path, false).unwrap();
        assert_eq!(inspected.previous, Some(expected));
        inspected.commit();
        drop(inspected);
        let empty = StartupChange::apply_at(path, false).unwrap();
        assert!(empty.previous.is_none());
    }

    struct TestKeyCleanup(Vec<u16>);
    impl Drop for TestKeyCleanup {
        fn drop(&mut self) {
            let status = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(self.0.as_ptr())) };
            if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
                eprintln!("Could not clean up isolated startup test key: {status:?}");
            }
        }
    }
}
