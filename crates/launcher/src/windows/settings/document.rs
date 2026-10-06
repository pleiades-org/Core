use super::{
    music::{MusicFields, MusicSettings},
    Preferences,
};
use core_engine::{
    aliases::{Alias, MAX_ALIASES},
    quicklinks::{Quicklink, MAX_QUICKLINKS},
};
use std::{collections::HashSet, sync::Arc};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SettingsDocument {
    pub preferences: Preferences,
    pub quicklinks: Arc<[Quicklink]>,
    pub aliases: Arc<[Alias]>,
    pub music: MusicSettings,
}

impl SettingsDocument {
    pub fn encode(&self) -> String {
        let mut text = self.preferences.encode();
        text.push_str(&self.music.encode());
        // Older builds ignore lines they do not know, so aliases need no new file version.
        for alias in self.aliases.iter() {
            text.push_str(&format!("alias={}\t{}\n", alias.name, alias.expansion));
        }
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
        let mut aliases = Vec::new();
        let mut alias_names = HashSet::new();
        let mut music = MusicSettings::default();
        let mut music_fields = MusicFields::default();
        let version_two = text.lines().any(|line| line.trim() == "version=2");
        for line in text.lines() {
            if line.starts_with("music_") && music.decode_line(line, &mut music_fields)? {
                continue;
            }
            if let Some(entry) = line.strip_prefix("alias=") {
                let (name, expansion) = entry
                    .split_once('\t')
                    .ok_or("An alias needs Alias and Stands for columns.")?;
                let alias = Alias::new(name, expansion)?;
                if aliases.len() >= MAX_ALIASES || !alias_names.insert(alias.key()) {
                    return Err("Aliases must be unique, with at most 200 entries.".into());
                }
                aliases.push(alias);
            } else if let Some(entry) = line.strip_prefix("quicklink=") {
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
        let preferences = Preferences::decode(&preferences)?;
        // A conflicting media shortcut is dropped, never the whole file.
        music.reconcile(preferences.shortcut);
        Ok(Self {
            preferences,
            quicklinks: quicklinks.into(),
            aliases: aliases.into(),
            music,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn files_from_before_media_shortcuts_load_even_when_they_open_core_with_alt_p() {
        let legacy = "version=1\nbackground=#000000\nposition=Center\nshortcut=Alt+P\n";
        let document = SettingsDocument::decode(legacy).unwrap();
        assert_eq!(document.preferences.shortcut.to_string(), "Alt+P");
        assert_eq!(document.music.shortcuts[0].shortcut, None);
        assert!(document.music.shortcuts[1].shortcut.is_some());
    }

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
            aliases: Default::default(),
            music: Default::default(),
        };
        assert_eq!(
            SettingsDocument::decode(&document.encode()),
            Ok(document.clone())
        );
        let with_music = SettingsDocument {
            music: super::super::music::MusicSettings {
                bar: false,
                ..Default::default()
            },
            ..document
        };
        assert_eq!(
            SettingsDocument::decode(&with_music.encode()),
            Ok(with_music)
        );
        for invalid in [
            "version=2\nbackground=#000000\nposition=Center\nquicklink=bad",
            "version=999\nbackground=#000000\nposition=Center",
        ] {
            assert!(SettingsDocument::decode(invalid).is_err());
        }
    }

    #[test]
    fn aliases_round_trip_beside_everything_else_and_bad_ones_are_rejected() {
        let document = SettingsDocument {
            aliases: vec![
                Alias::new("d", "Discord").unwrap(),
                Alias::new("@s", "@song").unwrap(),
                Alias::new("ip", "/ipconfig /all").unwrap(),
            ]
            .into(),
            quicklinks: vec![Quicklink::new("Docs", "https://example.com").unwrap()].into(),
            ..Default::default()
        };
        let text = document.encode();
        assert!(text.contains("alias=@s\t@song\n"));
        assert_eq!(SettingsDocument::decode(&text), Ok(document));
        // No aliases, no lines: files of people who never use them stay as they were.
        assert!(!SettingsDocument::default().encode().contains("alias="));
        let base = "version=1\nbackground=#000000\nposition=Center\n";
        for lines in [
            "alias=d",
            "alias=\tDiscord",
            "alias=two words\tDiscord",
            "alias=/d\tDiscord",
            "alias=d\tDiscord\nalias=D\tDocker",
        ] {
            assert!(
                SettingsDocument::decode(&format!("{base}{lines}")).is_err(),
                "{lines}"
            );
        }
    }
}
