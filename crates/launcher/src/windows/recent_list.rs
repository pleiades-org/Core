//! A newest-first list of recently used things kept beside the settings file: plain text, one
//! entry per line. Recently run commands (Settings → Behaviour can clear them) and apps
//! launched from Core.
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

const MAX_ENTRY_LENGTH: usize = 4_096;

/// Which list, and how long it may grow.
#[derive(Clone, Copy)]
pub struct RecentKind {
    file_name: &'static str,
    max_entries: usize,
}

impl RecentKind {
    pub const COMMANDS: Self = Self {
        file_name: "command-history.txt",
        max_entries: 100,
    };
    pub const APPLICATIONS: Self = Self {
        file_name: "recent-applications.txt",
        max_entries: 50,
    };
}

pub struct RecentList {
    /// `None` keeps the list in memory only (dry runs without a settings file).
    path: Option<PathBuf>,
    max_entries: usize,
    entries: Arc<[Arc<str>]>,
}

impl RecentList {
    pub fn load(folder: Option<&Path>, kind: RecentKind) -> Self {
        let path = folder.map(|folder| folder.join(kind.file_name));
        let entries = path
            .as_deref()
            .and_then(|path| match fs::read_to_string(path) {
                Ok(text) => Some(text),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    eprintln!("Could not read {}: {error}", kind.file_name);
                    None
                }
            })
            .map(|text| parse(&text, kind.max_entries))
            .unwrap_or_default();
        Self {
            path,
            max_entries: kind.max_entries,
            entries: entries.into(),
        }
    }

    pub fn entries(&self) -> Arc<[Arc<str>]> {
        self.entries.clone()
    }

    /// Moves `entry` to the front, dropping duplicates and the oldest entries.
    pub fn record(&mut self, entry: &str) -> Result<(), String> {
        let entry = entry.trim();
        if entry.is_empty() || entry.len() > MAX_ENTRY_LENGTH || entry.contains(['\r', '\n']) {
            return Ok(());
        }
        let entries: Vec<Arc<str>> = std::iter::once(Arc::from(entry))
            .chain(
                self.entries
                    .iter()
                    .filter(|existing| &***existing != entry)
                    .cloned(),
            )
            .take(self.max_entries)
            .collect();
        self.entries = entries.into();
        self.save()
    }

    pub fn clear(&mut self) -> Result<(), String> {
        self.entries = Arc::from([]);
        match &self.path {
            Some(path) => match fs::remove_file(path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    Err(format!("Could not clear {}: {error}", path.display()))
                }
                _ => Ok(()),
            },
            None => Ok(()),
        }
    }

    fn save(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let mut text = String::new();
        for entry in self.entries.iter() {
            text.push_str(entry);
            text.push('\n');
        }
        let temporary = path.with_extension("txt.tmp");
        path.parent()
            .map(fs::create_dir_all)
            .transpose()
            .and_then(|_| fs::write(&temporary, text))
            .and_then(|_| fs::rename(&temporary, path))
            .map_err(|error| format!("Could not save {}: {error}", path.display()))
    }
}

fn parse(text: &str, max_entries: usize) -> Vec<Arc<str>> {
    let mut entries: Vec<Arc<str>> = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if entries.len() == max_entries {
            break;
        }
        if !entries.iter().any(|entry| &**entry == line) {
            entries.push(line.into());
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("core-history-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn commands_persist_newest_first_without_duplicates() {
        let folder = folder("persist");
        let mut history = RecentList::load(Some(&folder), RecentKind::COMMANDS);
        for command in ["git status", "ipconfig", "git status", "  ", "two\nlines"] {
            history.record(command).unwrap();
        }
        let reloaded = RecentList::load(Some(&folder), RecentKind::COMMANDS);
        let entries: Vec<&str> = reloaded.entries.iter().map(|entry| &**entry).collect();
        assert_eq!(entries, ["git status", "ipconfig"]);
        let mut reloaded = reloaded;
        reloaded.clear().unwrap();
        assert!(RecentList::load(Some(&folder), RecentKind::COMMANDS)
            .entries
            .is_empty());
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn history_is_capped_and_memory_only_without_a_folder() {
        let mut history = RecentList::load(None, RecentKind::COMMANDS);
        for index in 0..120 {
            history.record(&format!("echo {index}")).unwrap();
        }
        assert_eq!(history.entries.len(), 100);
        assert_eq!(&*history.entries[0], "echo 119");
        let mut applications = RecentList::load(None, RecentKind::APPLICATIONS);
        for index in 0..60 {
            applications.record(&format!("app:{index}")).unwrap();
        }
        assert_eq!(applications.entries.len(), 50);
    }
}
