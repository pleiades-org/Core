//! Windows CNG verification; release signatures are IEEE P1363 (32-byte r followed by s).
use super::UpdateError;
use windows::Win32::Security::Cryptography::*;

pub fn sha256(bytes: &[u8]) -> Result<[u8; 32], UpdateError> {
    let mut digest = [0; 32];
    unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, bytes, &mut digest) }
        .ok()
        .map_err(UpdateError::Crypto)?;
    Ok(digest)
}

struct PublicKey(BCRYPT_KEY_HANDLE);
impl Drop for PublicKey {
    fn drop(&mut self) {
        unsafe {
            let _ = BCryptDestroyKey(self.0);
        }
    }
}

pub fn verify(public: &[u8; 64], message: &[u8], signature: &[u8; 64]) -> Result<(), UpdateError> {
    let mut blob = Vec::with_capacity(72);
    blob.extend_from_slice(&BCRYPT_ECDSA_PUBLIC_P256_MAGIC.to_le_bytes());
    blob.extend_from_slice(&32_u32.to_le_bytes());
    blob.extend_from_slice(public);
    let mut key = BCRYPT_KEY_HANDLE::default();
    unsafe {
        BCryptImportKeyPair(
            BCRYPT_ECDSA_P256_ALG_HANDLE,
            None,
            BCRYPT_ECCPUBLIC_BLOB,
            &mut key,
            &blob,
            0,
        )
    }
    .ok()
    .map_err(UpdateError::Crypto)?;
    let key = PublicKey(key);
    let digest = sha256(message)?;
    unsafe { BCryptVerifySignature(key.0, None, &digest, signature, BCRYPT_FLAGS(0)) }
        .ok()
        .map_err(|_| UpdateError::BadSignature)
}

pub fn signature(text: &str) -> Result<[u8; 64], UpdateError> {
    // Exact length and alphabet prevent CryptoAPI's permissive whitespace/header parsing.
    if text.len() != 88
        || !text.ends_with("==")
        || !text[..86]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
    {
        return Err(UpdateError::MalformedManifest);
    }
    let encoded: Vec<u16> = text.encode_utf16().collect();
    let mut decoded = [0; 64];
    let mut length = decoded.len() as u32;
    unsafe {
        CryptStringToBinaryW(
            &encoded,
            CRYPT_STRING(CRYPT_STRING_BASE64.0 | CRYPT_STRING_STRICT.0),
            Some(decoded.as_mut_ptr()),
            &mut length,
            None,
            None,
        )
    }
    .map_err(|_| UpdateError::MalformedManifest)?;
    if length != 64 {
        return Err(UpdateError::MalformedManifest);
    }
    Ok(decoded)
}
