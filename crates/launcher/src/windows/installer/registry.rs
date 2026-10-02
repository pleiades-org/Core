use crate::windows::wide;
use windows::{
    core::PCWSTR,
    Win32::{Foundation::*, System::Registry::*},
};

#[derive(Clone)]
pub(super) struct RegistryValue {
    kind: REG_VALUE_TYPE,
    bytes: Vec<u8>,
}

impl RegistryValue {
    pub fn text(text: &str) -> Self {
        Self {
            kind: REG_SZ,
            bytes: wide(text).into_iter().flat_map(u16::to_le_bytes).collect(),
        }
    }

    pub fn number(number: u32) -> Self {
        Self {
            kind: REG_DWORD,
            bytes: number.to_le_bytes().to_vec(),
        }
    }

    pub fn as_text(&self) -> Option<String> {
        if self.kind != REG_SZ || !self.bytes.len().is_multiple_of(2) {
            return None;
        }
        let units: Vec<u16> = self
            .bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|unit| *unit != 0)
            .collect();
        String::from_utf16(&units).ok()
    }
}

pub(super) struct RegistryKey(HKEY);

impl RegistryKey {
    pub fn create(path: &str) -> Result<Self, String> {
        let mut key = HKEY::default();
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(wide(path).as_ptr()),
                None,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_QUERY_VALUE | KEY_SET_VALUE,
                None,
                &mut key,
                None,
            )
        }
        .ok()
        .map_err(|error| format!("Could not open Core's Windows registration: {error}"))?;
        Ok(Self(key))
    }

    pub fn read(&self, name: &str) -> Result<Option<RegistryValue>, String> {
        let name = wide(name);
        let mut kind = REG_VALUE_TYPE::default();
        let mut size = 0;
        let status = unsafe {
            RegQueryValueExW(
                self.0,
                PCWSTR(name.as_ptr()),
                None,
                Some(&mut kind),
                None,
                Some(&mut size),
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        status.ok().map_err(|error| error.to_string())?;
        if size > 64 * 1024 {
            return Err(
                "Core's existing Windows registration is too large to update safely.".into(),
            );
        }
        let mut bytes = vec![0; size as usize];
        unsafe {
            RegQueryValueExW(
                self.0,
                PCWSTR(name.as_ptr()),
                None,
                Some(&mut kind),
                Some(bytes.as_mut_ptr()),
                Some(&mut size),
            )
        }
        .ok()
        .map_err(|error| error.to_string())?;
        bytes.truncate(size as usize);
        Ok(Some(RegistryValue { kind, bytes }))
    }

    pub fn write(&self, name: &str, value: Option<&RegistryValue>) -> Result<(), String> {
        let name = wide(name);
        let status = unsafe {
            match value {
                Some(value) => RegSetValueExW(
                    self.0,
                    PCWSTR(name.as_ptr()),
                    None,
                    value.kind,
                    Some(&value.bytes),
                ),
                None => RegDeleteValueW(self.0, PCWSTR(name.as_ptr())),
            }
        };
        if value.is_none() && status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        status
            .ok()
            .map_err(|error| format!("Could not update Core's Windows registration: {error}"))
    }
}

impl Drop for RegistryKey {
    fn drop(&mut self) {
        if let Err(error) = unsafe { RegCloseKey(self.0) }.ok() {
            eprintln!("Could not close Core's installation registry key: {error}");
        }
    }
}

pub(super) fn remove_key(path: &str) -> Result<(), String> {
    let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(wide(path).as_ptr())) };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    status
        .ok()
        .map_err(|error| format!("Could not remove Core's uninstall registration: {error}"))
}

pub(super) struct RegistryTransaction {
    originals: Vec<(String, String, Option<RegistryValue>)>,
    committed: bool,
}

impl RegistryTransaction {
    pub fn new() -> Self {
        Self {
            originals: Vec::new(),
            committed: false,
        }
    }

    pub fn write(
        &mut self,
        path: &str,
        name: &str,
        value: Option<&RegistryValue>,
    ) -> Result<(), String> {
        let key = RegistryKey::create(path)?;
        self.originals
            .push((path.into(), name.into(), key.read(name)?));
        key.write(name, value)
    }

    pub fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for RegistryTransaction {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for (path, name, value) in self.originals.iter().rev() {
            let result = RegistryKey::create(path).and_then(|key| key.write(name, value.as_ref()));
            if let Err(error) = result {
                eprintln!("Could not restore Core's registration after setup failed: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_text_round_trips_unicode_but_rejects_invalid_types_and_encoding() {
        assert_eq!(
            RegistryValue::text("Core – résumé").as_text().as_deref(),
            Some("Core – résumé")
        );
        assert_eq!(RegistryValue::number(2).as_text(), None);
        assert_eq!(
            RegistryValue {
                kind: REG_SZ,
                bytes: vec![1]
            }
            .as_text(),
            None
        );
        assert_eq!(
            RegistryValue {
                kind: REG_SZ,
                bytes: vec![0, 0xd8, 0, 0]
            }
            .as_text(),
            None
        );
    }
}
