//! Refresh credentials are protected for the current Windows user with DPAPI.
use super::encoding::valid_token;
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::Cryptography::*,
        Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH},
    },
};

const MAX_TOKEN_FILE_BYTES: u64 = 16 * 1024;

pub fn load(path: &Path, client_id: &str) -> Result<Option<String>, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(
                "Could not read the saved Spotify connection. Disconnect and reconnect.".into(),
            )
        }
    };
    let mut encrypted = Vec::new();
    file.take(MAX_TOKEN_FILE_BYTES + 1)
        .read_to_end(&mut encrypted)
        .map_err(|_| "Could not read the Spotify connection.")?;
    if encrypted.len() as u64 > MAX_TOKEN_FILE_BYTES {
        return Err("The Spotify connection file is too large. Disconnect and reconnect.".into());
    }
    let plaintext = protect(&encrypted, false)?;
    let text = std::str::from_utf8(&plaintext)
        .map_err(|_| "The saved Spotify connection is invalid. Disconnect and reconnect.")?;
    let (saved_client, refresh) = text
        .split_once('\n')
        .ok_or("The saved Spotify connection is invalid. Disconnect and reconnect.")?;
    if saved_client != client_id {
        return Ok(None);
    }
    if !valid_token(refresh) {
        return Err("The saved Spotify token is invalid. Disconnect and reconnect.".into());
    }
    Ok(Some(refresh.to_owned()))
}

pub fn save(path: &Path, client_id: &str, refresh: &str) -> Result<(), String> {
    if !valid_token(refresh) {
        return Err("Spotify returned an invalid refresh token.".into());
    }
    let encrypted = protect(format!("{client_id}\n{refresh}").as_bytes(), true)?;
    let parent = path
        .parent()
        .ok_or("Spotify connection folder is unavailable.")?;
    fs::create_dir_all(parent).map_err(|_| "Could not create the Spotify connection folder.")?;
    let temporary = path.with_extension("pending");
    let result = write_encrypted(&temporary, path, &encrypted);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn write_encrypted(temporary: &Path, path: &Path, encrypted: &[u8]) -> Result<(), String> {
    let mut file =
        fs::File::create(temporary).map_err(|_| "Could not save the Spotify connection.")?;
    file.write_all(encrypted)
        .and_then(|()| file.sync_all())
        .map_err(|_| "Could not save the Spotify connection.")?;
    drop(file);
    unsafe {
        MoveFileExW(
            PCWSTR(crate::windows::wide(&temporary.to_string_lossy()).as_ptr()),
            PCWSTR(crate::windows::wide(&path.to_string_lossy()).as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|_| "Could not replace the saved Spotify connection.".into())
}

pub fn remove(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Could not remove the saved Spotify connection.".into()),
    }
}

fn protect(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        let result = if encrypt {
            CryptProtectData(
                &input,
                w!("Core Spotify connection"),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        result.map_err(|_| {
            "Windows could not protect or read the Spotify connection. Disconnect and reconnect."
        })?;
        let copied = if output.cbData == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec()
        };
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        Ok(copied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn connection_storage_is_encrypted_and_bound_to_its_client_id() {
        let folder = std::env::temp_dir().join(format!(
            "core-spotify-token-{}",
            super::super::encoding::random_secret().unwrap()
        ));
        let path = folder.join("spotify-token.bin");
        save(&path, "client", "refresh-token").unwrap();
        assert!(!fs::read(&path)
            .unwrap()
            .windows(13)
            .any(|window| window == b"refresh-token"));
        assert_eq!(
            load(&path, "client").unwrap().as_deref(),
            Some("refresh-token")
        );
        assert_eq!(load(&path, "other").unwrap(), None);
        remove(&path).unwrap();
        assert_eq!(load(&path, "client").unwrap(), None);
        fs::remove_dir(folder).unwrap();
    }
}
