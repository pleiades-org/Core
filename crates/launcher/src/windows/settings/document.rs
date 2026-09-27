use super::Preferences;
use core_engine::quicklinks::{Quicklink, MAX_QUICKLINKS};
use std::{collections::HashSet, sync::Arc};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SettingsDocument {
    pub preferences: Preferences,
    pub quicklinks: Arc<[Quicklink]>,
}

impl SettingsDocument {
    pub fn encode(&self) -> String {
        let mut text = self.preferences.encode();
        if !self.quicklinks.is_empty() {
            text = text.replacen("version=1", "version=2", 1);
            for entry in self.quicklinks.iter() {
                text.push_str(&format!("quicklink={}\t{}\n", entry.link, entry.name));
            }
        }
        text
    }

    pub fn decode(text: &str) -> Result<Self, String> {
        let mut preferences = String::new();
        let mut quicklinks = Vec::new();
        let mut names = HashSet::new();
        let version_two = text.lines().any(|line| line.trim() == "version=2");
        for line in text.lines() {
            if let Some(entry) = line.strip_prefix("quicklink=") {
                if !version_two || quicklinks.len() >= MAX_QUICKLINKS {
                    return Err("Quicklinks require version 2 and at most 1000 entries.".into());
                }
                let (link, name) = entry
                    .split_once('\t')
                    .ok_or("A quicklink needs Link and Name columns.")?;
                let quicklink = Quicklink::new(name, link)?;
                if !names.insert(quicklink.name.to_lowercase()) {
                    return Err("Quicklink names must be unique.".into());
                }
                quicklinks.push(quicklink);
            } else {
                preferences.push_str(if line.trim() == "version=2" {
                    "version=1"
                } else {
                    line
                });
                preferences.push('\n');
            }
        }
        Ok(Self {
            preferences: Preferences::decode(&preferences)?,
            quicklinks: quicklinks.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_preferences_and_unicode_links_round_trip_together() {
        let legacy = Preferences::default();
        assert_eq!(
            SettingsDocument::decode(&legacy.encode())
                .unwrap()
                .preferences,
            legacy
        );
        let document = SettingsDocument {
            preferences: legacy,
            quicklinks: vec![
                Quicklink::new("Référence", "https://example.com/?a=1&b=2").unwrap(),
                Quicklink::new("My files", "C:\\My Files").unwrap(),
            ]
            .into(),
        };
        assert_eq!(SettingsDocument::decode(&document.encode()), Ok(document));
        for invalid in [
            "version=2\nbackground=#000000\nposition=Center\nquicklink=bad",
            "version=999\nbackground=#000000\nposition=Center",
        ] {
            assert!(SettingsDocument::decode(invalid).is_err());
        }
    }
}
