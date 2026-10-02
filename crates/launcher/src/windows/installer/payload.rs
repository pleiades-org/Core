use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

pub(super) const SETUP_MARKER: &[u8] = include_bytes!("setup-marker.txt");

/// A PE overlay identifies setup even when Windows renames a duplicate download.
pub(super) fn is_setup(executable: &Path) -> Result<bool, String> {
    let mut file = File::open(executable)
        .map_err(|error| format!("Could not inspect Core's executable: {error}"))?;
    let size = file.metadata().map_err(|error| error.to_string())?.len();
    if size < SETUP_MARKER.len() as u64 {
        return Ok(false);
    }
    file.seek(SeekFrom::End(-(SETUP_MARKER.len() as i64)))
        .map_err(|error| error.to_string())?;
    let mut tail = vec![0; SETUP_MARKER.len()];
    file.read_exact(&mut tail)
        .map_err(|error| error.to_string())?;
    Ok(tail == SETUP_MARKER)
}

pub(super) fn application_bytes(mut bytes: Vec<u8>) -> Vec<u8> {
    if bytes.ends_with(SETUP_MARKER) {
        bytes.truncate(bytes.len() - SETUP_MARKER.len());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_marker_is_removed_only_from_the_end_of_the_application_payload() {
        let mut package = b"application".to_vec();
        package.extend_from_slice(SETUP_MARKER);
        assert_eq!(application_bytes(package), b"application");
        let mut ordinary = SETUP_MARKER.to_vec();
        ordinary.extend_from_slice(b"application");
        assert_eq!(application_bytes(ordinary.clone()), ordinary);
    }
}
