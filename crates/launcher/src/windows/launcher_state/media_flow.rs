//! Media in the launcher: reading Windows' sessions, sending controls, the now-playing bar and
//! media shortcuts. Nothing starts until media is first used, or Core is shown with the bar on;
//! while Core is hidden the worker sleeps with no subscriptions.
use super::LauncherState;
use crate::windows::{
    application_icon::ApplicationIcon,
    execute_action::NativeAction,
    media::{
        MediaAction, MediaHotkeys, MediaOutcome, MediaReading, MediaRequest, MediaService,
        TitleWatch,
    },
    settings::{shortcut_recorder, MediaShortcutAction, MusicApp, MusicSettings, Shortcut},
    view::{BarPresence, MediaBarClick, MediaBarContent, MEDIA_NEXT_ID, MEDIA_PREVIOUS_ID},
};
use core_engine::media::{
    app_key, classify_app, known_app, AppClass, MediaCommand, MediaSession, MediaState,
    PlaybackState,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::HWND,
    UI::Input::KeyboardAndMouse::{GetFocus, VIRTUAL_KEY},
};

/// After Core sends a command, that player stays the target this long, while it settles.
const STICKY_TARGET: Duration = Duration::from_secs(5);
/// Players remembered for Settings → Music in one session.
const SEEN_LIMIT: usize = 32;

#[derive(Default)]
pub struct MediaFlow {
    service: Option<MediaService>,
    sessions: Arc<[MediaSession]>,
    /// Sessions were read at least once.
    loaded: bool,
    unavailable: bool,
    art: HashMap<Arc<str>, Arc<ApplicationIcon>>,
    paused_by_core: Option<Arc<str>>,
    /// A reading since Core's pause showed that player paused.
    pause_confirmed: bool,
    last_command: Option<(Arc<str>, Instant)>,
    /// Players seen this session, offered in Settings → Music.
    seen: Vec<MusicApp>,
    hotkeys: Option<MediaHotkeys>,
    hotkey_bindings: Vec<(usize, Shortcut)>,
    /// Title changes of players read from their window, watched while Core is visible.
    title_watch: TitleWatch,
    /// A full reading arrived since Core was shown, so the bar no longer rests on what was
    /// read before Core was hidden.
    read_since_show: bool,
}

impl MediaFlow {
    /// At exit: stop the worker and release the shortcuts.
    pub fn stop(&mut self) {
        self.title_watch.clear();
        self.service.take();
        self.hotkeys.take();
    }

    fn recent_command(&self, app_id: &str) -> bool {
        self.last_command
            .as_ref()
            .is_some_and(|(app, at)| &**app == app_id && at.elapsed() < STICKY_TARGET)
    }
}

impl LauncherState {
    fn music(&self) -> &MusicSettings {
        &self.settings.saved.music
    }

    /// What search and the bar decide from; None until the sessions have been read once.
    pub(super) fn media_state(&self) -> Option<Arc<MediaState>> {
        if !self.media.loaded {
            return None;
        }
        let sticky = self
            .media
            .last_command
            .as_ref()
            .filter(|(_, at)| at.elapsed() < STICKY_TARGET)
            .map(|(app, _)| app.clone());
        Some(Arc::new(MediaState {
            sessions: self.media.sessions.clone(),
            policy: self.music().policy(),
            sticky,
            paused_by_core: self.media.paused_by_core.clone(),
            unavailable: self.media.unavailable,
        }))
    }

    /// Starts the worker the first time; a new worker reads at once while Core is visible.
    fn media_service(&mut self) -> Option<&MediaService> {
        if self.media.service.is_none() && self.options.media {
            match MediaService::start(self.window) {
                Ok(service) => {
                    if self.visible {
                        service.refresh(true, self.media_art_size());
                    }
                    self.media.service = Some(service);
                }
                Err(error) => eprintln!("Could not start media controls: {error}"),
            }
        }
        self.media.service.as_ref()
    }

    /// The worker that is already running, for the bar's volume slider: the bar it belongs to
    /// only shows what that worker read.
    pub(super) fn media_worker(&self) -> Result<&MediaService, String> {
        self.media
            .service
            .as_ref()
            .ok_or_else(|| "Media controls are unavailable".to_owned())
    }

    fn media_art_size(&self) -> u32 {
        match &self.view {
            Some(view) if self.music().bar => view.media_art_size(),
            _ => 0,
        }
    }

    pub(super) fn request_media_reading(&mut self) {
        let (watch, art) = (self.visible, self.media_art_size());
        if let Some(service) = self.media_service() {
            service.refresh(watch, art);
        }
    }

    /// Core is being shown: the bar appears at once from what was read last, then the players
    /// are read again and watched while Core stays visible.
    pub(super) fn media_shown(&mut self) {
        self.typed_since_show = false;
        self.media.read_since_show = false;
        self.refresh_media_bar(BarPresence::Free);
    }

    /// A key typed into the search box; text set any other way also counts, through its edit.
    pub fn note_typing(&mut self) {
        self.typed_since_show = true;
    }

    pub(super) fn media_after_show(&mut self) {
        if self.music().bar || self.media.service.is_some() {
            self.request_media_reading();
        }
        if let Some(view) = &self.view {
            view.set_media_progress_active(true);
        }
    }

    pub(super) fn media_hidden(&mut self, window: HWND) {
        if let Some(service) = &self.media.service {
            service.stop_watching();
        }
        self.media.title_watch.clear();
        if let Some(view) = &self.view {
            view.set_media_progress_active(false);
        }
        // A shortcut field cannot be focused while Core is hidden.
        self.resume_shortcuts(window);
    }

    /// A media query reads the sessions the first time it is typed.
    pub(super) fn media_for_query(&mut self, query: &str) {
        if self.media.service.is_none() && core_engine::search::wants_media(query) {
            self.request_media_reading();
        }
    }

    pub fn receive_media(&mut self) {
        let Some(service) = &self.media.service else {
            return;
        };
        let reading = service.take_reading();
        let outcomes = service.take_outcomes();
        let volume = service.take_volume();
        let mut results_changed = false;
        // The first full reading after showing corrects a bar drawn from the previous one,
        // removing it if the player closed meanwhile; until something is typed.
        let mut presence = if self.typed_since_show {
            BarPresence::Keep
        } else {
            BarPresence::AppearOnly
        };
        if let Some(reading) = reading {
            results_changed = !reading.timeline_only || !self.media.loaded;
            if !reading.timeline_only && !self.media.read_since_show {
                self.media.read_since_show = true;
                if !self.typed_since_show {
                    presence = BarPresence::Free;
                }
            }
            self.apply_media_reading(reading);
            self.offer_music_apps();
        }
        for outcome in outcomes {
            results_changed = true;
            self.apply_media_outcome(outcome);
        }
        self.refresh_media_bar(presence);
        // After the bar, which may have changed players: the slider is the shown player's.
        if let Some(volume) = volume {
            self.receive_mixer_volume(volume);
        }
        let shows_media = self
            .view
            .as_ref()
            .is_some_and(|view| core_engine::search::wants_media(&view.query()));
        if results_changed && self.visible && shows_media {
            self.queue_search();
        }
    }

    fn apply_media_reading(&mut self, reading: MediaReading) {
        if reading.timeline_only {
            self.media.sessions = self
                .media
                .sessions
                .iter()
                .map(|session| {
                    let mut session = session.clone();
                    if let Some(update) = reading
                        .sessions
                        .iter()
                        .find(|update| update.app_id == session.app_id)
                    {
                        session.timeline = update.timeline;
                    }
                    session
                })
                .collect();
            return;
        }
        let sessions: Vec<MediaSession> = reading
            .sessions
            .into_iter()
            .map(|mut session| {
                // Store apps are named as the Start menu names them.
                if let Some(application) = self.catalog.find(&format!("package:{}", session.app_id))
                {
                    session.app_name = application.name.clone();
                    session.class = classify_app(&session.app_id, &session.app_name);
                }
                session
            })
            .collect();
        for session in &sessions {
            let key = app_key(&session.app_id);
            if self.media.seen.len() < SEEN_LIMIT
                && !self.media.seen.iter().any(|seen| seen.key == key)
            {
                if let Ok(app) = MusicApp::new(&key, &session.app_name) {
                    self.media.seen.push(app);
                }
            }
        }
        self.media.sessions = sessions.into();
        self.media.loaded = true;
        self.media.unavailable = reading.unavailable;
        self.media.art = reading.art.into_iter().collect();
        if self.visible {
            self.media
                .title_watch
                .follow(self.window, &reading.window_processes);
        }
        self.check_paused_marker();
    }

    /// A player read from its window changed its title: a new track, a pause or a resume.
    pub fn media_title_changed(&self) {
        if let Some(service) = self.media.service.as_ref().filter(|_| self.visible) {
            service.player_changed();
        }
    }

    fn apply_media_outcome(&mut self, outcome: MediaOutcome) {
        let name = outcome
            .app_name
            .clone()
            .unwrap_or_else(|| Arc::from("the media app"));
        let message = match &outcome.result {
            Err(error) => error.clone(),
            Ok(()) => {
                let Some(app) = outcome.app_id.clone() else {
                    // Sent as a media key: Windows chose the player.
                    return;
                };
                self.media.last_command = Some((app.clone(), Instant::now()));
                let was_playing = outcome.before == Some(PlaybackState::Playing);
                let (paused, played) = match outcome.action {
                    MediaAction::Control(MediaCommand::Pause) => (true, false),
                    MediaAction::Control(MediaCommand::Play) => (false, true),
                    MediaAction::Control(MediaCommand::TogglePlayPause) => {
                        (was_playing, !was_playing)
                    }
                    _ => (false, false),
                };
                if paused {
                    self.media.paused_by_core = Some(app.clone());
                    self.media.pause_confirmed = false;
                    self.set_media_state(&app, PlaybackState::Paused);
                } else if played {
                    if self.media.paused_by_core.as_deref() == Some(&*app) {
                        self.media.paused_by_core = None;
                    }
                    self.set_media_state(&app, PlaybackState::Playing);
                }
                match outcome.action {
                    MediaAction::Control(MediaCommand::Next) => format!("Next track · {name}"),
                    MediaAction::Control(MediaCommand::Previous) => {
                        format!("Previous track · {name}")
                    }
                    MediaAction::Seek(_) => format!("Moved through the track · {name}"),
                    _ if paused => format!("Paused {name}"),
                    _ => format!("Playing {name}"),
                }
            }
        };
        match &self.view {
            Some(view) if self.visible => view.set_footer(&message),
            _ => eprintln!("Media: {message}"),
        }
    }

    /// Shows a command's effect at once; the player's own announcement follows.
    fn set_media_state(&mut self, app_id: &str, state: PlaybackState) {
        self.media.sessions = self
            .media
            .sessions
            .iter()
            .map(|session| {
                let mut session = session.clone();
                if &*session.app_id == app_id {
                    session.state = state;
                }
                session
            })
            .collect();
    }

    /// After each full reading: the bar stops offering to resume a player Core paused once it
    /// closes or plays again from anywhere. Right after Core's own pause, a reading can still
    /// show it playing, so playing counts once a reading confirmed the pause, or once the
    /// command is a few seconds old.
    fn check_paused_marker(&mut self) {
        let Some(app) = self.media.paused_by_core.clone() else {
            return;
        };
        let state = self
            .media
            .sessions
            .iter()
            .find(|session| session.app_id == app)
            .map(|session| session.state);
        match state {
            None => self.media.paused_by_core = None,
            Some(PlaybackState::Paused) => self.media.pause_confirmed = true,
            Some(PlaybackState::Playing)
                if self.media.pause_confirmed || !self.media.recent_command(&app) =>
            {
                self.media.paused_by_core = None
            }
            Some(_) => {}
        }
    }

    pub(super) fn refresh_media_bar(&self, presence: BarPresence) {
        let Some(view) = &self.view else {
            return;
        };
        let enabled = self.music().bar;
        // While Core stays open, the bar keeps its player between tracks and after a pause
        // elsewhere, instead of switching to "Nothing playing" and back.
        let shown = (presence != BarPresence::Free)
            .then(|| view.media_bar_app())
            .flatten();
        let content = if enabled {
            self.media_bar_content(shown.as_deref())
        } else {
            None
        };
        // Turning the bar off in Settings removes it at once.
        view.set_media_bar(content, if enabled { presence } else { BarPresence::Free });
    }

    fn media_bar_content(&self, shown: Option<&str>) -> Option<MediaBarContent> {
        let state = self.media_state()?;
        let session = state.bar().or_else(|| {
            state
                .sessions
                .iter()
                .find(|session| Some(&*session.app_id) == shown)
        })?;
        Some(MediaBarContent {
            app_id: session.app_id.clone(),
            title: if session.title.is_empty() {
                session.app_name.clone()
            } else {
                session.title.clone()
            },
            detail: if session.artist.is_empty() {
                session.app_name.to_string()
            } else {
                format!("{} · {}", session.artist, session.app_name)
            },
            playing: session.state == PlaybackState::Playing,
            controls: session.controls,
            timeline: session.timeline,
            art: self.media.art.get(&session.app_id).cloned(),
        })
    }

    /// Sends a control. Without a target the worker chooses from a fresh reading, with the
    /// person's priorities.
    pub fn send_media(&mut self, action: MediaAction, target: Option<Arc<str>>) {
        self.send_media_request(action, target, true);
    }

    /// `key_fallback`: without Windows' media sessions, send the keyboard media key instead.
    fn send_media_request(
        &mut self,
        action: MediaAction,
        target: Option<Arc<str>>,
        key_fallback: bool,
    ) {
        if self.options.dry_run {
            if let Some(view) = &self.view {
                view.set_footer("Verified media control · no side effect");
            }
            return;
        }
        let request = MediaRequest {
            action,
            target,
            policy: self.music().policy(),
            sticky: self
                .media
                .last_command
                .as_ref()
                .filter(|(_, at)| at.elapsed() < STICKY_TARGET)
                .map(|(app, _)| app.clone()),
            key_fallback,
        };
        let accepted = self.media_service().map(|service| service.send(request));
        let problem = match accepted {
            Some(true) => return,
            Some(false) => "Media controls are busy · try again in a moment",
            None => "Media controls are unavailable",
        };
        match &self.view {
            Some(view) if self.visible => view.set_footer(problem),
            _ => eprintln!("Media: {problem}"),
        }
    }

    /// A key pressed while Core is in front; true when it was one of the media shortcuts that
    /// work in Core. Held keys repeat; only the first press acts.
    pub fn media_shortcut_key(&mut self, focus: HWND, key: VIRTUAL_KEY, repeat: bool) -> bool {
        if self.shortcuts_suspended {
            return false;
        }
        let Some(view) = &self.view else {
            return false;
        };
        if view.settings_open() || view.is_output(focus) {
            return false;
        }
        let Ok(shortcut) = Shortcut::from_keys(shortcut_recorder::held_modifiers(), key.0) else {
            return false;
        };
        let Some(command) = self.music().in_core_command(shortcut) else {
            return false;
        };
        if !repeat {
            self.send_media(MediaAction::Control(command), None);
        }
        true
    }

    /// A media shortcut that works in every app; false for other hotkeys.
    pub fn media_hotkey(&mut self, identifier: usize) -> bool {
        let Some(slot) = MediaHotkeys::slot(identifier) else {
            return false;
        };
        if let Some(action) = MediaShortcutAction::ALL.get(slot) {
            // A shortcut on the keyboard's own media key must not answer itself with that key.
            let media_key = self.music().shortcuts[slot]
                .shortcut
                .is_some_and(|shortcut| shortcut.is_media_key());
            self.send_media_request(MediaAction::Control(action.command()), None, !media_key);
        }
        true
    }

    /// Registers the shortcuts that work in every app. On failure the previous ones stay.
    pub fn configure_media_hotkeys(
        &mut self,
        window: HWND,
        music: &MusicSettings,
    ) -> Result<(), String> {
        if self.options.probe
            || (self.options.dry_run
                && !std::env::args().any(|argument| argument == "--test-shortcut"))
        {
            return Ok(());
        }
        let bindings = music.everywhere_bindings();
        if self.shortcuts_suspended {
            // Checked now; registered when the shortcut field loses focus.
            return MediaHotkeys::register(window, &bindings).map(drop);
        }
        if self.media.hotkeys.is_some() && bindings == self.media.hotkey_bindings {
            return Ok(());
        }
        self.media.hotkeys = None;
        match MediaHotkeys::register(window, &bindings) {
            Ok(hotkeys) => {
                self.media.hotkeys = Some(hotkeys);
                self.media.hotkey_bindings = bindings;
                Ok(())
            }
            Err(error) => {
                match MediaHotkeys::register(window, &self.media.hotkey_bindings) {
                    Ok(previous) => self.media.hotkeys = Some(previous),
                    Err(restore) => eprintln!("Could not restore media shortcuts: {restore}"),
                }
                Err(error)
            }
        }
    }

    /// A settings shortcut field gained or lost the keyboard focus. Gaining it counts only if
    /// the field still has it in open Settings, since the notice arrives a moment later.
    pub fn recorder_focus(&mut self, window: HWND, focused: bool, field: HWND) {
        if !focused {
            self.resume_shortcuts(window);
            return;
        }
        let recording = self.visible
            && self.view.as_ref().is_some_and(|view| view.settings_open())
            && unsafe { GetFocus() } == field;
        if recording {
            self.suspend_shortcuts();
        }
    }

    /// A shortcut field is recording: Core's shortcuts pause, so pressing one records it.
    fn suspend_shortcuts(&mut self) {
        if self.shortcuts_suspended {
            return;
        }
        self.shortcuts_suspended = true;
        self.binding.take();
        self.media.hotkeys.take();
    }

    /// The field lost focus: the newest valid shortcuts are registered again, or the saved
    /// ones if those fail.
    pub(super) fn resume_shortcuts(&mut self, window: HWND) {
        if !self.shortcuts_suspended {
            return;
        }
        self.shortcuts_suspended = false;
        let latest = self.auto_save.latest(&self.settings.saved);
        if let Err(error) = self.configure_shortcut(window, latest.preferences.shortcut) {
            eprintln!("Core shortcut could not be registered: {error}");
            let saved = self.settings.saved.preferences.shortcut;
            if let Err(error) = self.configure_shortcut(window, saved) {
                eprintln!("Saved Core shortcut could not be registered: {error}");
            }
        }
        self.media.hotkey_bindings.clear();
        if let Err(error) = self.configure_media_hotkeys(window, &latest.music) {
            eprintln!("Media shortcuts could not be registered: {error}");
            let saved = self.settings.saved.music.clone();
            if let Err(error) = self.configure_media_hotkeys(window, &saved) {
                eprintln!("Saved media shortcuts could not be registered: {error}");
            }
        }
    }

    /// Saved music settings: the bar and priorities apply at once.
    pub(super) fn music_settings_changed(&mut self) {
        if self.visible && self.music().bar && !self.media.loaded {
            self.request_media_reading();
        }
        self.refresh_media_bar(BarPresence::Free);
    }

    pub fn media_bar_button(&mut self, identifier: usize) {
        let command = match identifier {
            MEDIA_PREVIOUS_ID => MediaCommand::Previous,
            MEDIA_NEXT_ID => MediaCommand::Next,
            _ => MediaCommand::TogglePlayPause,
        };
        let target = self.view.as_ref().and_then(|view| view.media_bar_app());
        self.send_media(MediaAction::Control(command), target);
        if let Some(view) = &self.view {
            view.return_focus_to_search();
        }
    }

    /// A click on the bar's progress line seeks; anywhere else on its art or text opens the
    /// player.
    pub fn media_info_clicked(&mut self) {
        let Some(view) = self.view.clone() else {
            return;
        };
        match view.media_info_click() {
            MediaBarClick::Seek { app_id, position } => {
                self.send_media(MediaAction::Seek(position), Some(app_id));
            }
            MediaBarClick::OpenPlayer(app_id) => self.open_media_player(&app_id),
            MediaBarClick::Nothing => {}
        }
        view.return_focus_to_search();
    }

    fn open_media_player(&mut self, app_id: &str) {
        let Some(view) = self.view.clone() else {
            return;
        };
        let Some(path) = self.player_target(app_id) else {
            view.set_footer("Core could not find this player in the Start menu");
            return;
        };
        if self.options.dry_run {
            view.set_footer("Verified action for the current query · no side effect");
            return;
        }
        // Opening a running player brings its window forward; Core then hides as for any app.
        self.pending_action = Some(NativeAction::OpenApplication(path));
    }

    /// The Start menu entry of a player: its package for Store apps, otherwise the app whose
    /// name Core recognises as the same player.
    fn player_target(&self, app_id: &str) -> Option<PathBuf> {
        if let Some(application) = self.catalog.find(&format!("package:{app_id}")) {
            return self.targets.get(&application.id).cloned();
        }
        let player = known_app(app_id)?;
        self.catalog
            .entries()
            .find(|application| {
                known_app(&application.name).is_some_and(|known| known.key == player.key)
            })
            .and_then(|application| self.targets.get(&application.id).cloned())
    }

    /// An open Settings page lists apps found since it opened.
    pub(super) fn offer_music_apps(&self) {
        if let Some(view) = self.view.as_ref().filter(|view| view.settings_open()) {
            view.add_music_apps(&self.music_app_choices());
        }
    }

    /// Settings → Music lists music apps installed here and players seen this session.
    pub(super) fn music_app_choices(&self) -> Vec<MusicApp> {
        let mut apps: Vec<MusicApp> = Vec::new();
        let installed = self.catalog.entries().filter_map(|application| {
            known_app(&application.name)
                .filter(|known| matches!(known.class, AppClass::Music | AppClass::Player))
                .and_then(|known| MusicApp::new(known.key, known.name).ok())
        });
        for app in installed.chain(self.media.seen.iter().cloned()) {
            if !apps.iter().any(|listed| listed.key == app.key) {
                apps.push(app);
            }
        }
        apps
    }
}
