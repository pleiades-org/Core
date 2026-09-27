//! The recently used apps shown when nothing is typed: apps launched from Core first, most recent
//! first, then the rest filled from Windows' own record of what the person starts elsewhere.
use super::{
    packaged_applications::DesktopEntry,
    user_assist::{self, KnownFolders},
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

/// More than the grid shows, so apps that have since been uninstalled cannot leave gaps.
pub const CANDIDATE_LIMIT: usize = 48;

/// The names Windows uses for Core's apps: app IDs and shortcut paths, lower-cased.
#[derive(Default)]
pub struct ApplicationAliases {
    /// App ID or shortcut path → Core's identifier.
    ids: HashMap<String, Arc<str>>,
    /// Shortcut or app name → Core's identifier, for shortcuts elsewhere such as taskbar pins.
    names: HashMap<String, Arc<str>>,
}

impl ApplicationAliases {
    pub fn add_shortcut(&mut self, path: &Path, identifier: &Arc<str>) {
        self.ids
            .insert(path.to_string_lossy().to_lowercase(), identifier.clone());
        if let Some(stem) = path.file_stem() {
            self.names
                .entry(stem.to_string_lossy().to_lowercase())
                .or_insert_with(|| identifier.clone());
        }
    }

    pub fn add_packaged(&mut self, app_id: &str, identifier: &Arc<str>) {
        self.ids.insert(app_id.to_lowercase(), identifier.clone());
    }

    /// A desktop app is listed by its Start Menu shortcut, which has the same name.
    pub fn add_desktop(&mut self, entry: &DesktopEntry) {
        if let Some(identifier) = self.names.get(&entry.name.to_lowercase()).cloned() {
            self.ids
                .entry(entry.app_id.to_lowercase())
                .or_insert(identifier);
        }
    }

    /// Core's identifier for a name from Windows' record, if Core lists that app.
    pub fn resolve(&self, name: &str, folders: &mut KnownFolders) -> Option<Arc<str>> {
        if let Some(identifier) = self.ids.get(&name.to_lowercase()) {
            return Some(identifier.clone());
        }
        let path = folders.expand(name).unwrap_or_else(|| PathBuf::from(name));
        if let Some(identifier) = self.ids.get(&path.to_string_lossy().to_lowercase()) {
            return Some(identifier.clone());
        }
        // A shortcut Core does not list, such as a taskbar pin, found by its name.
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
        {
            return None;
        }
        let stem = path.file_stem()?.to_string_lossy().to_lowercase();
        let stem = stem.strip_suffix(" - shortcut").unwrap_or(&stem);
        self.names.get(stem).cloned()
    }
}

/// Apps Windows has seen started, most recent first, as Core's identifiers.
pub fn windows_recent(aliases: &ApplicationAliases) -> Vec<Arc<str>> {
    let mut folders = KnownFolders::default();
    user_assist::recent_usage()
        .iter()
        .filter_map(|usage| aliases.resolve(&usage.name, &mut folders))
        .collect()
}

/// Core's launches first, then Windows' record, without repeats.
pub fn merge(core: &[Arc<str>], windows: Vec<Arc<str>>) -> Arc<[Arc<str>]> {
    let mut seen = HashSet::new();
    core.iter()
        .cloned()
        .chain(windows)
        .filter(|identifier| seen.insert(identifier.clone()))
        .take(CANDIDATE_LIMIT)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aliases() -> ApplicationAliases {
        let mut aliases = ApplicationAliases::default();
        aliases.add_shortcut(
            Path::new(r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Microsoft Edge.lnk"),
            &Arc::from("app:edge"),
        );
        aliases.add_shortcut(
            Path::new(r"C:\Users\Me\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Discord Inc\Discord.lnk"),
            &Arc::from("app:discord"),
        );
        aliases.add_packaged(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App",
            &Arc::from("package:calculator"),
        );
        aliases.add_desktop(&DesktopEntry {
            app_id: "MSEdge".into(),
            name: "Microsoft Edge".into(),
        });
        aliases.add_desktop(&DesktopEntry {
            app_id: "Unlisted.App".into(),
            name: "Not In Start".into(),
        });
        aliases
    }

    #[test]
    fn windows_names_resolve_to_cores_apps() {
        let aliases = aliases();
        let mut folders = KnownFolders::default();
        let mut resolve = |name: &str| aliases.resolve(name, &mut folders).map(|id| id.to_string());
        assert_eq!(
            resolve("microsoft.windowscalculator_8wekyb3d8bbwe!App").as_deref(),
            Some("package:calculator")
        );
        assert_eq!(resolve("MSEdge").as_deref(), Some("app:edge"));
        assert_eq!(
            resolve(r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Microsoft Edge.lnk")
                .as_deref(),
            Some("app:edge")
        );
        // A taskbar pin of an app Core lists by its Start Menu shortcut.
        assert_eq!(
            resolve(r"{9E3995AB-1F9C-4F13-B827-48B24B6C7174}\TaskBar\Discord - Shortcut.lnk")
                .as_deref(),
            Some("app:discord")
        );
        assert_eq!(resolve("Unlisted.App"), None);
        assert_eq!(resolve(r"C:\Tools\core-v2.exe"), None);
    }

    #[test]
    fn core_launches_come_first_without_repeats() {
        let merged = merge(
            &[Arc::from("b"), Arc::from("a")],
            vec![Arc::from("a"), Arc::from("c"), Arc::from("b")],
        );
        let merged: Vec<&str> = merged.iter().map(|identifier| &**identifier).collect();
        assert_eq!(merged, ["b", "a", "c"]);
    }
}
