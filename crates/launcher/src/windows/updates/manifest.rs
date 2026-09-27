use super::{crypto, UpdateError, MAX_MANIFEST_BYTES, RELEASE_KEY};
use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl FromStr for Version {
    type Err = UpdateError;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut parts = text.split('.');
        let mut number = || {
            let part = parts.next().ok_or(UpdateError::MalformedManifest)?;
            if part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
            {
                return Err(UpdateError::MalformedManifest);
            }
            part.parse::<u32>()
                .map_err(|_| UpdateError::MalformedManifest)
        };
        let version = Self {
            major: number()?,
            minor: number()?,
            patch: number()?,
        };
        if parts.next().is_some() {
            return Err(UpdateError::MalformedManifest);
        }
        Ok(version)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Clone, Debug)]
pub struct Manifest {
    pub version: Version,
    pub path: String,
    pub digest: [u8; 32],
    pub text: String,
}

impl Manifest {
    pub fn verify(bytes: &[u8]) -> Result<Self, UpdateError> {
        Self::verify_with_key(bytes, RELEASE_KEY)
    }

    pub(super) fn verify_with_key(bytes: &[u8], key: &[u8; 64]) -> Result<Self, UpdateError> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(UpdateError::MalformedManifest);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| UpdateError::MalformedManifest)?;
        // The signed payload is exactly three UTF-8 lines with LF endings, including the last LF.
        let (payload, encoded_signature) = text
            .split_once("signature=")
            .ok_or(UpdateError::MalformedManifest)?;
        let lines: Vec<_> = payload.split_terminator('\n').collect();
        if lines.len() != 3 || !payload.ends_with('\n') || text.contains('\r') {
            return Err(UpdateError::MalformedManifest);
        }
        let version: Version = field(lines[0], "version=")?.parse()?;
        let path = field(lines[1], "path=")?;
        if path != format!("/pleiades-org/Core/releases/download/v{version}/core-v2.exe") {
            return Err(UpdateError::MalformedManifest);
        }
        let digest = digest(field(lines[2], "sha256=")?)?;
        let signature = crypto::signature(
            encoded_signature
                .strip_suffix('\n')
                .unwrap_or(encoded_signature),
        )?;
        crypto::verify(key, payload.as_bytes(), &signature)?;
        Ok(Self {
            version,
            path: path.into(),
            digest,
            text: text.into(),
        })
    }
}

fn field<'a>(line: &'a str, prefix: &str) -> Result<&'a str, UpdateError> {
    line.strip_prefix(prefix)
        .ok_or(UpdateError::MalformedManifest)
}

fn digest(text: &str) -> Result<[u8; 32], UpdateError> {
    if text.len() != 64
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(UpdateError::MalformedManifest);
    }
    let mut result = [0; 32];
    for (index, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
            .map_err(|_| UpdateError::MalformedManifest)?;
    }
    Ok(result)
}
