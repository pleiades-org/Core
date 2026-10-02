//! URL and PKCE encoding without an embedded client secret.
use windows::Win32::Security::Cryptography::{
    BCryptGenRandom, BCryptHash, BCRYPT_SHA256_ALG_HANDLE, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
};

pub fn url_encode(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 15) as usize] as char);
        }
    }
    encoded
}

pub fn base64_url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut encoded = String::new();
    for chunk in bytes.chunks(3) {
        let bits = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for shift in [18, 12, 6, 0].into_iter().take(chunk.len() + 1) {
            encoded.push(ALPHABET[((bits >> shift) & 63) as usize] as char);
        }
    }
    encoded
}

pub fn random_secret() -> Result<String, String> {
    let mut bytes = [0; 32];
    unsafe { BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }
        .ok()
        .map_err(|_| "Could not create a secure Spotify connection.".to_owned())?;
    Ok(base64_url(&bytes))
}

pub fn challenge(verifier: &str) -> Result<String, String> {
    let mut digest = [0; 32];
    unsafe {
        BCryptHash(
            BCRYPT_SHA256_ALG_HANDLE,
            None,
            verifier.as_bytes(),
            &mut digest,
        )
    }
    .ok()
    .map_err(|_| "Could not create a Spotify authorization challenge.".to_owned())?;
    Ok(base64_url(&digest))
}

pub fn valid_token(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 4096
        && text
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'"' | b'\\'))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pkce_matches_the_rfc_7636_example() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk").unwrap(),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(url_encode("Björk & +/"), "Bj%C3%B6rk%20%26%20%2B%2F");
        assert_eq!(base64_url(b"M"), "TQ");
        assert_eq!(base64_url(b"Ma"), "TWE");
        assert!(!valid_token("token\r\nInjected: header"));
    }
}
