//! The recently used apps shown when nothing is typed: apps launched from Core first, most recent
//! first, then the rest filled from Windows' own record of what the person starts elsewhere.
use super::{
    packaged_applications::DesktopEntry,
    user_assist::{self, KnownFolders, UsageWatch},
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

/// Windows' record as Core's identifiers, kept between shows. The registry is read again only
/// after Windows has changed it, or after Core's apps (and so their aliases) have changed.
#[derive(Default)]
pub struct WindowsRecent {
    /// Created by the first read, so probes never watch the registry.
    watch: Option<UsageWatch>,
    folders: KnownFolders,
    /// `None` until read, and after `invalidate`.
    identifiers: Option<Vec<Arc<str>>>,
}

impl WindowsRecent {
    /// Core's apps changed: the next `refresh` reads the record again.
    pub fn invalidate(&mut self) {
        self.identifiers = None;
    }

    /// Reads the record if Windows may have changed it since the last read.
    pub fn refresh(&mut self, aliases: &ApplicationAliases) {
        let changed = self.watch.get_or_insert_with(UsageWatch::new).changed();
        if changed || self.identifiers.is_none() {
            self.identifiers = Some(windows_recent(aliases, &mut self.folders));
        }
    }

    pub fn identifiers(&self) -> &[Arc<str>] {
        self.identifiers.as_deref().unwrap_or_default()
    }
}

/// Apps Windows has seen started, most recent first, as Core's identifiers.
fn windows_recent(aliases: &ApplicationAliases, folders: &mut KnownFolders) -> Vec<Arc<str>> {
    first_distinct(
        user_assist::recent_usage()
            .iter()
            .filter_map(|usage| aliases.resolve(&usage.name, folders)),
    )
}

/// The first `CANDIDATE_LIMIT` distinct identifiers; later ones are never resolved. `merge` never
/// reaches them whatever Core's own list holds: these alone already fill every place before them.
fn first_distinct(identifiers: impl Iterator<Item = Arc<str>>) -> Vec<Arc<str>> {
    let mut seen = HashSet::new();
    identifiers
        .filter(|identifier| seen.insert(identifier.clone()))
        .take(CANDIDATE_LIMIT)
        .collect()
}

/// Core's launches first, then Windows' record, without repeats.
pub fn merge(core: &[Arc<str>], windows: &[Arc<str>]) -> Arc<[Arc<str>]> {
    let mut seen = HashSet::new();
    core.iter()
        .chain(windows)
        .filter(|&identifier| seen.insert(identifier.clone()))
        .take(CANDIDATE_LIMIT)
        .cloned()
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
            &[Arc::from("a"), Arc::from("c"), Arc::from("b")],
        );
        let merged: Vec<&str> = merged.iter().map(|identifier| &**identifier).collect();
        assert_eq!(merged, ["b", "a", "c"]);
    }

    fn identifiers(numbers: impl IntoIterator<Item = usize>) -> Vec<Arc<str>> {
        numbers
            .into_iter()
            .map(|number| Arc::from(format!("app:{number}")))
            .collect()
    }

    #[test]
    fn resolving_stops_after_the_first_distinct_candidates() {
        // Windows often names one app twice (its shortcut and its app ID).
        let windows = identifiers((0..200).map(|index| index / 2));
        let mut resolved = 0;
        let first = first_distinct(windows.iter().cloned().inspect(|_| resolved += 1));
        assert_eq!(first, identifiers(0..CANDIDATE_LIMIT));
        assert_eq!(resolved, CANDIDATE_LIMIT * 2 - 1);
    }

    #[test]
    fn the_first_distinct_candidates_merge_like_the_whole_record() {
        // Repeats, then a long tail beyond the limit.
        let windows = identifiers((0..300).map(|index| (index * 7) % 90 + index / 150 * 100));
        let first = first_distinct(windows.iter().cloned());
        for core in [
            identifiers([]),
            identifiers([5]),
            identifiers(0..10),
            identifiers((0..30).map(|index| index * 3)),
            identifiers(1_000..1_047),
            identifiers(1_000..1_048),
            identifiers((0..50).rev()),
            identifiers((0..25).chain(1_000..1_025)),
        ] {
            assert_eq!(merge(&core, &windows), merge(&core, &first), "{core:?}");
        }
    }
}
