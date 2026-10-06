//! `@volume` and `@mix`: Windows' volume mixer as result rows. While they show, the mixer is
//! read again every couple of seconds. Left and Right, a row's slider and its mute button
//! change a volume at once, and Enter mutes the selected row; the row shows the new level
//! before Windows confirms it.
use super::LauncherState;
use crate::windows::media::MixerChange;
use core_engine::{
    applications::ApplicationCatalog,
    media::{known_program, sort_mixer, step_volume, MixerApp, VolumeLevel},
    search::{wants_mixer, Action},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

pub const MIXER_TIMER: usize = 47;
/// Programs start and stop sounding, and volumes change elsewhere: while the mixer shows, it
/// is read again this often.
const REFRESH_MS: u32 = 2_000;
/// A level the person chose stands this long against readings that do not show it yet.
const HOLD: Duration = Duration::from_millis(1_500);

/// A level the person chose, shown at once and kept until Windows' mixer says the same.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Held {
    app: Arc<str>,
    level: VolumeLevel,
    until: Instant,
}

#[derive(Default)]
pub struct MixerFlow {
    /// The mixer as last read, with the levels the person chose since; None until read.
    apps: Option<Arc<[MixerApp]>>,
    /// The results are the mixer's, so it is read again on a timer.
    showing: bool,
    /// The rows have this showing's own order, which later readings keep.
    settled: bool,
    held: Option<Held>,
}

/// `apps` with one program at `level`.
fn with_level(apps: &[MixerApp], app: &str, level: VolumeLevel) -> Arc<[MixerApp]> {
    apps.iter()
        .cloned()
        .map(|mut listed| {
            if &*listed.id == app {
                listed.level = level;
            }
            listed
        })
        .collect()
}

/// A reading as the rows show it: a level the person chose stands in for the one read, which
/// may have been taken before Windows applied it. The hold ends once the two agree, when its
/// time is up, or when the program is gone.
fn shown_reading(mut apps: Vec<MixerApp>, held: &mut Option<Held>, now: Instant) -> Vec<MixerApp> {
    let Some(hold) = held.as_ref() else {
        return apps;
    };
    match apps.iter_mut().find(|app| app.id == hold.app) {
        Some(app) if now < hold.until && app.level != hold.level => app.level = hold.level,
        _ => *held = None,
    }
    apps
}

/// A reading in the order the rows already have. While the mixer shows, a program that
/// starts or stops sounding keeps its row, so nothing moves under the pointer; programs that
/// are new follow those listed, in the reading's own order.
fn in_shown_order(mut apps: Vec<MixerApp>, shown: &[MixerApp]) -> Vec<MixerApp> {
    apps.sort_by_key(|app| {
        shown
            .iter()
            .position(|listed| listed.id == app.id)
            .unwrap_or(usize::MAX)
    });
    apps
}

/// What the Start menu calls a program, when the entries that start it agree on one name.
/// Entries know their program by its file name without `.exe`, in lowercase.
fn start_menu_name(executable: &str, catalog: &ApplicationCatalog) -> Option<Arc<str>> {
    let file = executable.rsplit(['\\', '/']).next()?;
    let program = file
        .rsplit_once('.')
        .map_or(file, |(stem, _)| stem)
        .to_lowercase();
    let mut names = catalog
        .entries()
        .filter(|entry| entry.aliases.iter().any(|alias| **alias == *program))
        .map(|entry| &entry.name);
    let name = names.next()?;
    names.all(|other| other == name).then(|| name.clone())
}

/// Programs Core does not know by name are called what the Start menu calls them, as they
/// are in search: a file often describes itself less well, as in "A native Spotify client".
/// The list is put in order again by the names it now has.
fn named_by_start_menu(mut apps: Vec<MixerApp>, catalog: &ApplicationCatalog) -> Vec<MixerApp> {
    for app in &mut apps {
        if app.is_system() || known_program(&app.id).is_some() {
            continue;
        }
        if let Some(name) = start_menu_name(&app.id, catalog) {
            app.name = name;
        }
    }
    sort_mixer(&mut apps);
    apps
}

impl LauncherState {
    /// What search lists for `@volume`; None until the mixer has been read.
    pub(super) fn mixer_snapshot(&self) -> Option<Arc<[MixerApp]>> {
        self.mixer.apps.clone()
    }

    /// Runs with every search. A query for the mixer reads it and keeps it fresh, until
    /// another query or hiding Core ends that.
    pub(super) fn mixer_for_query(&mut self, query: &str) {
        let wanted = self.visible && wants_mixer(query);
        if wanted == self.mixer.showing {
            return;
        }
        if !wanted {
            self.stop_mixer();
            return;
        }
        self.mixer.showing = true;
        self.read_mixer();
        if unsafe { SetTimer(Some(self.window), MIXER_TIMER, REFRESH_MS, None) } == 0 {
            // The rows still show the first reading and follow the person's own changes.
            eprintln!(
                "Could not keep the volume mixer fresh: {}",
                windows::core::Error::from_win32()
            );
        }
    }

    /// The mixer no longer shows: nothing reads it until it does again.
    pub(super) fn stop_mixer(&mut self) {
        if !std::mem::take(&mut self.mixer.showing) {
            return;
        }
        self.mixer.held = None;
        self.mixer.settled = false;
        // The timer is not there when it could not be set.
        unsafe {
            let _ = KillTimer(Some(self.window), MIXER_TIMER);
        }
    }

    fn read_mixer(&mut self) {
        if let Some(service) = self.media_service() {
            service.read_mixer();
        }
    }

    /// [`MIXER_TIMER`] fired. A slider the person holds is not read under their hand, and
    /// nothing is read for rows that Settings covers.
    pub fn mixer_timer(&mut self) {
        let waits = self
            .view
            .as_ref()
            .is_some_and(|view| view.mixer_dragging() || view.settings_open());
        if self.mixer.showing && !waits {
            self.read_mixer();
        }
    }

    /// The media worker read the mixer.
    pub(super) fn receive_mixer(&mut self, apps: Vec<MixerApp>) {
        let apps = shown_reading(apps, &mut self.mixer.held, Instant::now());
        let mut apps = named_by_start_menu(apps, &self.catalog);
        // The first reading of a showing orders the rows; what was listed before it may be
        // from the last time the mixer was open.
        if let Some(shown) = self.mixer.apps.as_deref().filter(|_| self.mixer.settled) {
            apps = in_shown_order(apps, shown);
        }
        self.mixer.settled = self.mixer.showing;
        let apps: Arc<[MixerApp]> = apps.into();
        if self.mixer.apps.as_ref() == Some(&apps) {
            return;
        }
        self.mixer.apps = Some(apps);
        if self.mixer.showing {
            self.queue_search();
        }
    }

    fn mixer_level(&self, app: &str) -> Option<VolumeLevel> {
        let apps = self.mixer.apps.as_ref()?;
        apps.iter()
            .find(|listed| &*listed.id == app)
            .map(|listed| listed.level)
    }

    /// The program of the mixer row with this result identifier.
    fn mixer_app(&self, row: &str) -> Option<Arc<str>> {
        let result = self
            .batch
            .results
            .iter()
            .find(|result| &*result.id == row)?;
        match &result.action {
            Action::Mixer { app, .. } => Some(app.clone()),
            _ => None,
        }
    }

    /// The selected row's program and its level now, which may be newer than the row's.
    fn selected_mixer_row(&self) -> Option<(Arc<str>, VolumeLevel)> {
        let selected = self.view.as_ref()?.selected();
        let Action::Mixer { app, .. } = &self.batch.results.get(selected)?.action else {
            return None;
        };
        Some((app.clone(), self.mixer_level(app)?))
    }

    /// The slider the person holds or just let go of: its program and the level they chose.
    fn chosen_mixer_level(&self) -> Option<(Arc<str>, u8)> {
        let choice = self.view.as_ref()?.mixer_choice()?;
        Some((self.mixer_app(&choice.row)?, choice.percent))
    }

    /// Left or Right on the selected row: one step quieter or louder.
    pub fn mixer_step(&mut self, louder: bool) {
        let Some((app, level)) = self.selected_mixer_row() else {
            return;
        };
        let percent = step_volume(level.percent, louder);
        let stepped = level.moved_to(percent);
        // Already as quiet or as loud as it goes.
        if stepped != level {
            self.set_mixer_level(app, stepped, MixerChange::Volume(percent));
        }
    }

    /// Enter, a double click or the mute button on the selected row.
    pub fn toggle_mixer_mute(&mut self) {
        let Some((app, level)) = self.selected_mixer_row() else {
            return;
        };
        let muted = !level.muted;
        self.set_mixer_level(
            app,
            VolumeLevel::new(level.percent, muted),
            MixerChange::Muted(muted),
        );
    }

    /// The person moves a row's slider: Windows follows at once. The row draws their choice
    /// itself, so nothing is searched or read while they hold it, unless the move ends a mute.
    pub fn mixer_slider_moved(&mut self) {
        let Some((app, percent)) = self.chosen_mixer_level() else {
            return;
        };
        // Raising a muted row ends its mute, which its text says at once.
        let unmuted = self
            .mixer_level(&app)
            .map(|level| (level, level.moved_to(percent)))
            .filter(|(level, moved)| level.muted != moved.muted);
        if let Some((_, moved)) = unmuted {
            self.set_mixer_level(app, moved, MixerChange::Volume(percent));
            return;
        }
        if self.mixer_change_is_dry() {
            return;
        }
        if let Some(service) = self.media_service() {
            service.change_mixer(app, MixerChange::Volume(percent), false);
        }
    }

    /// They let go: the rows take the level they chose, and the mixer is read again.
    pub fn mixer_slider_released(&mut self) {
        let Some((app, percent)) = self.chosen_mixer_level() else {
            return;
        };
        let Some(level) = self.mixer_level(&app) else {
            return;
        };
        self.set_mixer_level(app, level.moved_to(percent), MixerChange::Volume(percent));
    }

    /// Shows `level` on a program's row at once, and has Windows' mixer follow and be read.
    fn set_mixer_level(&mut self, app: Arc<str>, level: VolumeLevel, change: MixerChange) {
        if self.mixer_change_is_dry() {
            return;
        }
        let Some(apps) = &self.mixer.apps else {
            return;
        };
        self.mixer.apps = Some(with_level(apps, &app, level));
        self.mixer.held = Some(Held {
            app: app.clone(),
            level,
            until: Instant::now() + HOLD,
        });
        if let Some(service) = self.media_service() {
            service.change_mixer(app, change, true);
        }
        self.queue_search();
    }

    /// In a dry run a volume change is only acknowledged.
    fn mixer_change_is_dry(&self) -> bool {
        if !self.options.dry_run {
            return false;
        }
        if let Some(view) = &self.view {
            view.set_footer("Verified volume change · no side effect");
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_engine::{applications::Application, media::SYSTEM_VOLUME_ID};

    fn entry(name: &str, programs: &[&str]) -> Application {
        Application {
            id: name.into(),
            name: name.into(),
            description: "Programs".into(),
            pinned: false,
            launches: 0,
            aliases: programs.iter().map(|program| Arc::from(*program)).collect(),
        }
    }

    fn program(path: &str, name: &str) -> MixerApp {
        MixerApp {
            id: path.into(),
            name: name.into(),
            level: VolumeLevel::new(50, false),
            active: false,
        }
    }

    #[test]
    fn the_start_menu_names_a_program_when_its_entries_agree() {
        let catalog = ApplicationCatalog::new(vec![
            entry("Spotifast", &["spotifast"]),
            entry("Steam", &["steam"]),
            entry("Editor", &["tool"]),
            entry("Editor (safe mode)", &["tool"]),
            entry("Notes", &[]),
        ]);
        let name = |path: &str| start_menu_name(path, &catalog);
        assert_eq!(
            name(r"c:\users\me\programs\spotifast\spotifast.exe").as_deref(),
            Some("Spotifast")
        );
        assert_eq!(name("C:/Games/Steam/STEAM.EXE").as_deref(), Some("Steam"));
        // Two entries start this program under different names: neither names it for certain.
        assert_eq!(name(r"c:\tools\tool.exe"), None);
        assert_eq!(name(r"c:\games\unlisted.exe"), None);
        assert_eq!(name(""), None);
    }

    #[test]
    fn only_programs_core_does_not_know_take_the_start_menus_name() {
        let catalog = ApplicationCatalog::new(vec![
            entry("Spotifast", &["spotifast"]),
            entry("My Browser", &["chrome"]),
            entry("Everything", &[SYSTEM_VOLUME_ID]),
        ]);
        let helium = r"c:\users\me\appdata\local\imput\helium\application\chrome.exe";
        let named = named_by_start_menu(
            vec![
                program(SYSTEM_VOLUME_ID, "System volume"),
                program(
                    r"c:\programs\spotifast\spotifast.exe",
                    "A native Spotify client",
                ),
                program(helium, "Helium"),
                program(r"c:\games\game.exe", "Game"),
            ],
            &catalog,
        );
        let names: Vec<&str> = named.iter().map(|app| &*app.name).collect();
        // In order by the names shown, the whole PC first.
        assert_eq!(names, ["System volume", "Game", "Helium", "Spotifast"]);
    }

    fn app(id: &str, percent: u8, muted: bool) -> MixerApp {
        MixerApp {
            id: id.into(),
            name: id.into(),
            level: VolumeLevel::new(percent, muted),
            active: true,
        }
    }

    fn mixer() -> Vec<MixerApp> {
        vec![
            app(SYSTEM_VOLUME_ID, 80, false),
            app("spotify", 40, false),
            app("game", 60, true),
        ]
    }

    fn held(app: &str, percent: u8, until: Instant) -> Option<Held> {
        Some(Held {
            app: app.into(),
            level: VolumeLevel::new(percent, false),
            until,
        })
    }

    #[test]
    fn rows_keep_their_places_while_the_mixer_shows_and_new_programs_follow() {
        let shown = [
            app(SYSTEM_VOLUME_ID, 80, false),
            app("spotify", 40, false),
            app("game", 60, true),
        ];
        // As read: the game now plays and sorts first, and two programs are new.
        let read = vec![
            app(SYSTEM_VOLUME_ID, 75, false),
            app("game", 60, false),
            app("browser", 100, false),
            app("chat", 100, false),
            app("spotify", 40, false),
        ];
        let ordered = in_shown_order(read.clone(), &shown);
        let ids: Vec<&str> = ordered.iter().map(|app| &*app.id).collect();
        assert_eq!(
            ids,
            [SYSTEM_VOLUME_ID, "spotify", "game", "browser", "chat"]
        );
        // The rows carry what was read, not what they showed.
        assert_eq!(ordered[0].level, VolumeLevel::new(75, false));
        assert_eq!(ordered[2].level, VolumeLevel::new(60, false));
        // With nothing listed yet the reading's own order stands.
        assert_eq!(in_shown_order(read.clone(), &[]), read);
    }

    #[test]
    fn one_programs_level_changes_and_the_rest_stay() {
        let changed = with_level(&mixer(), "spotify", VolumeLevel::new(45, true));
        assert_eq!(changed[1].level, VolumeLevel::new(45, true));
        assert_eq!(changed[0], mixer()[0]);
        assert_eq!(changed[2], mixer()[2]);
        // A program that is not listed changes nothing.
        assert_eq!(
            &*with_level(&mixer(), "closed", VolumeLevel::new(5, false)),
            &mixer()[..]
        );
    }

    #[test]
    fn a_chosen_level_stands_until_windows_agrees_or_its_time_is_up() {
        let now = Instant::now();
        let later = now + HOLD;
        // A reading taken before Windows applied the choice still shows the choice.
        let mut hold = held("spotify", 70, later);
        let shown = shown_reading(mixer(), &mut hold, now);
        assert_eq!(shown[1].level, VolumeLevel::new(70, false));
        assert_eq!(shown[0], mixer()[0]);
        assert!(hold.is_some());
        // Once Windows says the same, later readings are shown as they are.
        let mut agreed = mixer();
        agreed[1].level = VolumeLevel::new(70, false);
        assert_eq!(shown_reading(agreed.clone(), &mut hold, now), agreed);
        assert!(hold.is_none());
        assert_eq!(shown_reading(mixer(), &mut hold, now), mixer());
        // A level Windows did not take, as a device with coarse steps may not, gives way.
        let mut expired = held("spotify", 70, later);
        assert_eq!(shown_reading(mixer(), &mut expired, later), mixer());
        assert!(expired.is_none());
        // The program closed: there is nothing to hold.
        let mut gone = held("closed", 70, later);
        assert_eq!(shown_reading(mixer(), &mut gone, now), mixer());
        assert!(gone.is_none());
    }
}
