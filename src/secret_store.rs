//! Windows Credential Manager storage for launcher secrets.

const BUNGIE_API_KEY_TARGET: &str = "CoreLauncher/BungieApiKey";
const GITHUB_TOKEN_TARGET: &str = "CoreLauncher/GitHubToken";

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::{BUNGIE_API_KEY_TARGET, GITHUB_TOKEN_TARGET};
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND};
    use windows::Win32::Security::Credentials::{
        CredDeleteW, CredReadW, CredWriteW, CRED_FLAGS, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    fn wide_string(text: &str) -> Vec<u16> {
        OsStr::new(text).encode_wide().chain(Some(0)).collect()
    }

    fn credential_blob(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(|character| character.to_le_bytes())
            .collect()
    }

    fn blob_to_string(blob: &[u8]) -> Option<String> {
        if blob.len() < 2 || blob.len() % 2 != 0 {
            return None;
        }

        let wide_chars: Vec<u16> = blob
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let end = wide_chars.iter().position(|&ch| ch == 0).unwrap_or(wide_chars.len());
        String::from_utf16(&wide_chars[..end]).ok()
    }

    fn store_secret(target_name: &str, secret: &str) -> bool {
        let target = wide_string(target_name);
        let blob = credential_blob(secret);
        let credential = CREDENTIALW {
            Flags: CRED_FLAGS(0),
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(target.as_ptr() as *mut u16),
            Comment: PWSTR::null(),
            LastWritten: Default::default(),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_ptr() as *mut u8,
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: std::ptr::null_mut(),
            TargetAlias: PWSTR::null(),
            UserName: PWSTR::null(),
        };

        unsafe { CredWriteW(&credential, 0).is_ok() }
    }

    fn load_secret(target_name: &str) -> Option<String> {
        let target = wide_string(target_name);
        let mut credential_ptr = std::ptr::null_mut();

        let read_result = unsafe {
            CredReadW(
                PCWSTR(target.as_ptr()),
                CRED_TYPE_GENERIC,
                Some(0),
                &mut credential_ptr,
            )
        };

        if read_result.is_err() {
            let error = unsafe { GetLastError() };
            if error == ERROR_NOT_FOUND {
                return None;
            }
            return None;
        }

        let secret = unsafe {
            let credential = &*credential_ptr;
            let blob = std::slice::from_raw_parts(
                credential.CredentialBlob,
                credential.CredentialBlobSize as usize,
            );
            blob_to_string(blob)
        };

        unsafe {
            windows::Win32::Security::Credentials::CredFree(credential_ptr as *mut _);
        }

        secret.filter(|value| !value.trim().is_empty())
    }

    fn delete_secret(target_name: &str) -> bool {
        let target = wide_string(target_name);
        match unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, Some(0)) } {
            Ok(()) => true,
            Err(_) => {
                let error = unsafe { GetLastError() };
                error == ERROR_NOT_FOUND
            }
        }
    }

    pub fn store_bungie_api_key(api_key: &str) -> bool {
        store_secret(BUNGIE_API_KEY_TARGET, api_key)
    }

    pub fn load_bungie_api_key() -> Option<String> {
        load_secret(BUNGIE_API_KEY_TARGET)
    }

    pub fn delete_bungie_api_key() -> bool {
        delete_secret(BUNGIE_API_KEY_TARGET)
    }

    pub fn store_github_token(token: &str) -> bool {
        store_secret(GITHUB_TOKEN_TARGET, token)
    }

    pub fn load_github_token() -> Option<String> {
        load_secret(GITHUB_TOKEN_TARGET)
    }

    pub fn delete_github_token() -> bool {
        delete_secret(GITHUB_TOKEN_TARGET)
    }
}

#[cfg(target_os = "windows")]
pub use windows_impl::{
    delete_bungie_api_key, delete_github_token, load_bungie_api_key, load_github_token,
    store_bungie_api_key, store_github_token,
};

#[cfg(not(target_os = "windows"))]
pub fn store_bungie_api_key(_api_key: &str) -> bool {
    false
}

#[cfg(not(target_os = "windows"))]
pub fn load_bungie_api_key() -> Option<String> {
    None
}

#[cfg(not(target_os = "windows"))]
pub fn delete_bungie_api_key() -> bool {
    true
}

#[cfg(not(target_os = "windows"))]
pub fn store_github_token(_token: &str) -> bool {
    false
}

#[cfg(not(target_os = "windows"))]
pub fn load_github_token() -> Option<String> {
    None
}

#[cfg(not(target_os = "windows"))]
pub fn delete_github_token() -> bool {
    true
}