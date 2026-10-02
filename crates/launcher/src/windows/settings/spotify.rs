//! Optional Spotify configuration. The Client ID is public; account tokens are stored separately.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpotifySettings {
    pub enabled: bool,
    pub client_id: String,
}

impl SpotifySettings {
    pub fn validate(&self) -> Result<(), String> {
        if !self.client_id.is_empty()
            && (self.client_id.len() != 32
                || !self.client_id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err("Spotify Client ID must contain 32 hexadecimal characters. No client secret is needed.".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spotify_is_optional_and_client_ids_are_validated() {
        assert!(!SpotifySettings::default().enabled);
        assert!(SpotifySettings::default().validate().is_ok());
        for client_id in ["abc", "../../token", "gggggggggggggggggggggggggggggggg"] {
            assert!(SpotifySettings {
                enabled: true,
                client_id: client_id.into()
            }
            .validate()
            .is_err());
        }
        assert!(SpotifySettings {
            enabled: true,
            client_id: "0123456789abcdef0123456789abcdef".into()
        }
        .validate()
        .is_ok());
    }
}
