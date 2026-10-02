mod command_flow;
pub use command_flow::{run_mode, COMMAND_OUTPUT_TIMER};
mod footer;
mod media_flow;
mod settings_flow;
mod update_flow;

use super::{
    discover_applications::Discovery,
    exchange_rates::{ExchangeRateService, RateSource},
    execute_action::NativeAction,
    favicon::WebsiteOrigin,
    foreground_observer::ForegroundObserver,
    icon_worker::{IconRequest, IconSource, IconWorker, MissingIcons},
    motion::{MotionPreference, VisibilityTransition},
    recent_applications::{self, ApplicationAliases, WindowsRecent},
    recent_list::{RecentKind, RecentList},
    search_worker::{SearchContext, SearchWorker},
    settings::{AutoSave, SettingsStore},
    tray::Tray,
    view::View,
};
use core_engine::{
    applications::{Application, ApplicationCatalog},
    conversions::ExchangeRates,
    search::{Action, RunMode, SearchBatch, SearchEngine},
};
use std::{collections::HashMap, path::PathBuf, rc::Rc, sync::Arc, time::Instant};
use windows::Win32::UI::Controls::EM_SETSEL;
use windows::Win32::{
    Foundation::*,
    UI::{
        Input::KeyboardAndMouse::{SendInput, SetFocus, INPUT, INPUT_MOUSE},
        WindowsAndMessaging::*,
    },
};

#[derive(Clone, Copy)]
pub struct Options {
    pub hidden: bool,
    pub probe: bool,
    pub dry_run: bool,
    pub stay_open_for_test: bool,
    /// `--dry-run --test-background`: Core never takes the foreground, so automated tests do
    /// not pull the person at the keyboard out of their work. Implies `stay_open_for_test`,
    /// because clicks elsewhere must not dismiss a window that was never activated.
    pub background_for_test: bool,
    /// `--dry-run --test-commands`: `/command` output runs are allowed in a dry run. Terminal
    /// and administrator runs stay suppressed.
    pub test_commands: bool,
    /// Website icons and exchange rates may be downloaded. Off for probes and dry runs
    /// unless a test opts in with `--test-network`.
    pub network: bool,
    /// The person's media players are read. Off for probes and dry runs unless a test opts in
    /// with `--test-media`, so checks do not depend on what happens to be playing.
    pub media: bool,
    pub motion: MotionPreference,
}
impl Options {
    pub fn read() -> Self {
        let arguments: Vec<_> = std::env::args().collect();
        let probe = arguments
            .iter()
            .any(|argument| argument == "--probe-hidden");
        let dry_run = arguments.iter().any(|argument| argument == "--dry-run");
        let background_for_test = dry_run
            && arguments
                .iter()
                .any(|argument| argument == "--test-background");
        Self {
            probe,
            hidden: probe
                || arguments
                    .iter()
                    .any(|argument| argument == "--start-hidden"),
            dry_run: arguments.iter().any(|argument| argument == "--dry-run"),
            network: !probe
                && (!arguments.iter().any(|argument| argument == "--dry-run")
                    || arguments.iter().any(|argument| {
                        argument == "--test-network" || argument == "--test-website-icons"
                    })),
            media: !probe
                && (!dry_run || arguments.iter().any(|argument| argument == "--test-media")),
            stay_open_for_test: background_for_test
                || (dry_run
                    && arguments
                        .iter()
                        .any(|argument| argument == "--test-stay-open")),
            background_for_test,
            test_commands: dry_run
                && arguments
                    .iter()
                    .any(|argument| argument == "--test-commands"),
            motion: if arguments
                .iter()
                .any(|argument| argument == "--reduced-motion")
            {
                MotionPreference::Reduced
            } else if arguments
                .iter()
                .any(|argument| argument == "--system-motion")
            {
                MotionPreference::System
            } else {
                MotionPreference::Enabled
            },
        }
    }
}

pub struct LauncherState {
    pub update_service: super::updates::UpdateService,
    pub restart_for_update: bool,
    pub view: Option<Rc<View>>,
    pub transition: VisibilityTransition,
    pub worker: Option<SearchWorker>,
    pub icon_worker: Option<IconWorker>,
    pub foreground_observer: Option<ForegroundObserver>,
    pub tray: Option<Tray>,
    pub visible: bool,
    pub binding: Option<super::activation::ActivationBinding>,
    pub options: Options,
    pub pending_action: Option<NativeAction>,
    pub settings: SettingsStore,
    pub close_after_save: bool,
    auto_save: AutoSave,
    catalog: Arc<ApplicationCatalog>,
    targets: HashMap<Arc<str>, PathBuf>,
    batch: SearchBatch,
    generation: u64,
    ready_generation: Option<u64>,
    pending_accept: Option<u64>,
    /// The query the latest search was for, so Enter never acts on results for older text.
    searched_query: String,
    catalog_ready: bool,
    selected_identifier: Option<Arc<str>>,
    /// Icons that failed recently; not asked for again until the worker would retry them.
    missing_icons: MissingIcons,
    exchange_rates: Option<Arc<ExchangeRates>>,
    rate_service: ExchangeRateService,
    /// Core's own window, for background notifications.
    window: HWND,
    command: Option<command_flow::CommandSession>,
    history: RecentList,
    /// Apps launched from Core, most recent first.
    launched_applications: RecentList,
    aliases: ApplicationAliases,
    /// Shown when nothing is typed: Core's launches, then Windows' record.
    recent_applications: Arc<[Arc<str>]>,
    /// Windows' record of the apps a person starts, read again only after Windows changes it.
    windows_recent: WindowsRecent,
    recall: Option<command_flow::Recall>,
    /// Where commands start; follows `cd` from one command to the next.
    working_directory: PathBuf,
    /// How Enter runs a shell command; kept until an accept that had to wait completes.
    accept_mode: RunMode,
    pub media: media_flow::MediaFlow,
    /// Something was typed since Core was shown, so the now-playing bar keeps its place.
    typed_since_show: bool,
    /// A settings shortcut field is recording, so Core's shortcuts are released meanwhile.
    shortcuts_suspended: bool,
}

impl LauncherState {
    pub fn new(options: Options) -> Self {
        let settings = SettingsStore::load(options.dry_run || options.probe);
        // History lives beside the settings file; dry runs without one keep it in memory.
        let settings_folder = settings.folder();
        if let Some(warning) = &settings.warning {
            eprintln!("Core settings: {warning}");
        }
        let applications = if options.probe {
            fixture_applications()
        } else {
            Vec::new()
        };
        let catalog = Arc::new(ApplicationCatalog::new(applications));
        let batch = SearchEngine::default().search("", &catalog);
        let rate_source = match exchange_rates_file() {
            Some(path) => RateSource::File(path),
            None if options.network => RateSource::online(),
            None => RateSource::Disabled,
        };
        Self {
            update_service: super::updates::UpdateService::new(
                options.network && !options.dry_run && !options.probe,
                settings.saved.preferences.updates,
            ),
            restart_for_update: false,
            view: None,
            transition: VisibilityTransition::new(options.motion),
            worker: None,
            icon_worker: None,
            foreground_observer: None,
            tray: None,
            visible: false,
            binding: None,
            options,
            pending_action: None,
            settings,
            close_after_save: false,
            auto_save: AutoSave::default(),
            catalog,
            targets: HashMap::new(),
            batch,
            generation: 0,
            ready_generation: None,
            pending_accept: None,
            searched_query: String::new(),
            catalog_ready: options.probe,
            selected_identifier: None,
            missing_icons: MissingIcons::default(),
            exchange_rates: None,
            rate_service: ExchangeRateService::new(rate_source),
            window: HWND::default(),
            command: None,
            history: RecentList::load(settings_folder.as_deref(), RecentKind::COMMANDS),
            launched_applications: RecentList::load(
                settings_folder.as_deref(),
                RecentKind::APPLICATIONS,
            ),
            aliases: ApplicationAliases::default(),
            recent_applications: Arc::from([]),
            windows_recent: WindowsRecent::default(),
            recall: None,
            working_directory: super::commands::home(),
            accept_mode: RunMode::Capture,
            media: media_flow::MediaFlow::default(),
            typed_since_show: false,
            shortcuts_suspended: false,
        }
    }

    pub fn start_worker(&mut self, window: HWND) -> std::io::Result<()> {
        self.window = window;
        self.worker = Some(SearchWorker::new(window, !self.options.probe)?);
        self.icon_worker = Some(IconWorker::new(window, self.options.network)?);
        self.queue_search();
        Ok(())
    }

    pub fn configure_shortcut(
        &mut self,
        window: HWND,
        shortcut: super::settings::Shortcut,
    ) -> Result<(), String> {
        if self
            .binding
            .as_ref()
            .is_some_and(|binding| binding.shortcut == shortcut)
        {
            return Ok(());
        }
        if self.options.probe
            || (self.options.dry_run
                && !std::env::args().any(|argument| argument == "--test-shortcut"))
        {
            return Ok(());
        }
        if self.shortcuts_suspended {
            // A shortcut field is recording: check the shortcut is free, register it later.
            return super::activation::ActivationBinding::install(window, shortcut, 3).map(drop);
        }
        let identifier = if self
            .binding
            .as_ref()
            .is_some_and(|binding| binding.identifier == 1)
        {
            2
        } else {
            1
        };
        let replacement =
            super::activation::ActivationBinding::install(window, shortcut, identifier)?;
        self.binding = Some(replacement);
        if let Some(tray) = &mut self.tray {
            if let Err(error) = tray.set_shortcut(shortcut) {
                eprintln!("Could not update Core's tray tooltip: {error}");
            }
        }
        Ok(())
    }

    /// A newer rate snapshot arrived; re-run a visible query so its answer uses it.
    pub fn receive_exchange_rates(&mut self) {
        self.exchange_rates = self.rate_service.latest();
        if self.visible {
            self.queue_search();
        }
    }

    /// Invalidate in-flight searches and drop pending icon loads.
    fn cancel_background_work(&mut self) {
        self.generation += 1;
        if let Some(worker) = &self.worker {
            worker.cancel(self.generation);
        }
        if let Some(worker) = &self.icon_worker {
            worker.submit(Vec::new());
        }
    }

    pub fn queue_search(&mut self) {
        let Some(view) = self.view.as_ref() else {
            return;
        };
        if view.settings_open() {
            return;
        }
        view.close_power_menu();
        self.generation += 1;
        self.pending_accept = None;
        self.searched_query = view.query();
        let query = self.searched_query.clone();
        self.media_for_query(&query);
        let media = self.media_state();
        if let Some(worker) = &self.worker {
            worker.submit(
                self.generation,
                self.searched_query.clone(),
                SearchContext {
                    catalog: self.catalog.clone(),
                    quicklinks: self.settings.saved.quicklinks.clone(),
                    exchange_rates: self.exchange_rates.clone(),
                    recent_applications: self.recent_applications.clone(),
                    media,
                },
            );
        }
    }

    pub fn receive(&mut self) {
        let Some(worker) = &self.worker else { return };
        let discovery = worker
            .events
            .catalog
            .lock()
            .expect("catalog event lock")
            .take();
        let completion = worker
            .events
            .results
            .lock()
            .expect("search results lock")
            .take();
        if let Some(discovery) = discovery {
            self.publish_catalog(discovery);
        }
        let Some(completion) = completion else { return };
        if completion.generation != self.generation {
            return;
        }
        self.selected_identifier = self.view.as_ref().and_then(|view| {
            self.batch
                .results
                .get(view.selected())
                .map(|result| result.id.clone())
        });
        self.batch = completion.batch;
        self.ready_generation = Some(completion.generation);
        if self.visible || self.options.dry_run || self.options.probe {
            self.render_results();
        }
        if self.pending_accept == Some(self.generation) {
            self.pending_accept = None;
            self.accept();
        }
    }

    fn publish_catalog(&mut self, discovery: Discovery) {
        eprintln!(
            "Discovered {} Start Menu and packaged applications",
            discovery.catalog.len()
        );
        let accepting_current = self.pending_accept == Some(self.generation);
        for warning in &discovery.warnings {
            eprintln!("Application discovery: {warning}");
        }
        self.catalog = discovery.catalog;
        self.targets = discovery.targets;
        self.aliases = discovery.aliases;
        self.catalog_ready = true;
        // New aliases, so Windows' record is resolved again.
        self.windows_recent.invalidate();
        self.refresh_recent_applications();
        self.offer_music_apps();
        self.queue_search();
        if accepting_current {
            self.pending_accept = Some(self.generation);
        }
    }

    pub fn render_results(&self) {
        let Some(view) = &self.view else { return };
        // Before `set_rows`, whose layout then applies terminal mode too.
        self.sync_terminal_view();
        view.set_rows(&self.batch.results);
        if let Some(identifier) = &self.selected_identifier {
            if let Some(index) = self
                .batch
                .results
                .iter()
                .position(|result| &result.id == identifier)
            {
                unsafe {
                    SendMessageW(view.results, LB_SETCURSEL, Some(WPARAM(index)), None);
                }
            }
        }
        self.refresh_footer();
        self.queue_icons();
    }

    fn queue_icons(&self) {
        if !self.visible {
            return;
        }
        let (Some(worker), Some(view)) = (&self.icon_worker, &self.view) else {
            return;
        };
        if view.settings_open() {
            return;
        }
        let now = Instant::now();
        let requests = self
            .batch
            .results
            .iter()
            .filter_map(|result| {
                if view.has_icon(&result.id) || self.missing_icons.is_missing(&result.id, now) {
                    return None;
                }
                let source = match &result.action {
                    Action::LaunchApplication(identifier) => {
                        IconSource::Shell(self.targets.get(identifier)?.clone())
                    }
                    Action::OpenQuicklink(link) => quicklink_icon_source(link)?,
                    _ => return None,
                };
                Some(IconRequest {
                    identifier: result.id.clone(),
                    source,
                })
            })
            .collect();
        worker.submit(requests);
    }

    pub fn receive_icons(&mut self) {
        if let (Some(worker), Some(view)) = (&self.icon_worker, &self.view) {
            if let Some(icons) = worker.take_completed() {
                self.missing_icons.record(&icons, Instant::now());
                view.set_icons(&icons);
            }
        }
    }

    pub fn set_visible(&mut self, window: HWND, visible: bool) {
        if !visible && self.view.as_ref().is_some_and(|view| view.settings_open()) {
            self.flush_settings(window);
        }
        let was_visible = std::mem::replace(&mut self.visible, visible);
        self.pending_accept = None;
        // Opening, not re-showing an open Core: typing so far keeps the bar in place.
        if visible && !was_visible {
            // Before placing the window, so the bar's height is part of the first layout.
            self.media_shown();
        }
        if let Some(view) = &self.view {
            if !visible {
                view.close_power_menu();
            }
            let layout = if visible {
                let foreground = unsafe { GetForegroundWindow() };
                view.position_on_monitor(if foreground.0.is_null() {
                    window
                } else {
                    foreground
                })
            } else if view.settings_open() {
                view.close_settings(self.auto_save.latest(&self.settings.saved).preferences)
            } else {
                Ok(())
            };
            if let Err(error) = layout {
                view.set_footer(&format!("Could not position Core: {error}"));
            }
            view.set_clock_active(visible);
        }
        if visible && self.foreground_observer.is_none() && !self.options.stay_open_for_test {
            match ForegroundObserver::new(window) {
                Ok(observer) => self.foreground_observer = Some(observer),
                Err(error) => eprintln!("Click-away dismissal unavailable: {error}"),
            }
        } else if !visible {
            self.foreground_observer.take();
        }
        if visible {
            self.refresh_recent_applications();
        }
        unsafe {
            self.transition.set_visible(window, visible);
            self.follow_opacity();
            if visible {
                self.rate_service.refresh(window);
                self.update_service.refresh(window);
                self.media_after_show();
                self.queue_search();
                if !self.options.background_for_test {
                    take_foreground(window);
                }
                if let Some(view) = &self.view {
                    let input = view.focus_target();
                    let _ = SetFocus(Some(input));
                    SendMessageW(input, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
                }
            } else {
                self.cancel_background_work();
                self.media_hidden(window);
            }
        }
    }

    /// Advances a fade in or out by one frame.
    pub fn tick_transition(&mut self, window: HWND) {
        self.transition.tick(window);
        self.follow_opacity();
    }

    /// What is drawn beside the window (its corners' fringe) fades with it.
    fn follow_opacity(&self) {
        if let Some(view) = &self.view {
            view.set_opacity(self.transition.opacity());
        }
    }

    pub fn accept(&mut self) {
        if self.view.as_ref().is_some_and(|view| view.settings_open()) {
            return;
        }
        if !self.visible && !self.options.dry_run {
            return;
        }
        // An edit whose change notification was lost (Win32 drops it while state is busy)
        // would otherwise leave results for older text in place.
        if self
            .view
            .as_ref()
            .is_some_and(|view| view.query() != self.searched_query)
        {
            self.queue_search();
        }
        if self.ready_generation != Some(self.generation) {
            // `accept_mode` stays set until the search this accept waits for completes.
            self.pending_accept = Some(self.generation);
            return;
        }
        let Some(view) = self.view.clone() else {
            return;
        };
        let mode = std::mem::replace(&mut self.accept_mode, RunMode::Capture);
        // Terminal mode has no visible rows; the modifiers held with Enter choose the run.
        let chosen = if view.terminal() {
            self.batch.results.iter().find(|result| {
                matches!(result.action, Action::RunCommand { mode: candidate, .. } if candidate == mode)
            })
        } else {
            self.batch.results.get(view.selected())
        };
        let Some(result) = chosen else {
            return;
        };
        let action = result.action.clone();
        if let Action::FillQuery(query) = action {
            if let Err(error) = view.set_query(&query) {
                view.set_footer(&error.to_string());
                return;
            }
            self.queue_search();
            return;
        }
        if let Action::RunCommand {
            command,
            shell,
            mode: RunMode::Capture,
        } = &action
        {
            if !self.options.dry_run || self.options.test_commands {
                self.run_captured(command.clone(), *shell);
                self.clear_command_input();
                return;
            }
        }
        if self.options.dry_run {
            view.set_footer("Verified action for the current query · no side effect");
            return;
        }
        // Media controls keep Core open, so another press can follow.
        if let Action::Media { command, target } = action {
            self.send_media(super::media::MediaAction::Control(command), target);
            return;
        }
        self.pending_action = Some(match action {
            Action::Update => {
                self.accept_update();
                return;
            }
            Action::LaunchApplication(identifier) => {
                let Some(path) = self.targets.get(&identifier).cloned() else {
                    view.set_footer("This application is no longer available");
                    return;
                };
                self.remember_launch(&identifier);
                NativeAction::OpenApplication(path)
            }
            Action::CopyText(text) => NativeAction::CopyText(text),
            Action::OpenUrl(url) => NativeAction::OpenUrl(url),
            Action::OpenQuicklink(link) => NativeAction::OpenQuicklink(link),
            Action::Power(action) => NativeAction::Power(action),
            Action::RevealTaskbar => NativeAction::RevealTaskbar,
            Action::RunCommand {
                command,
                shell,
                mode,
            } => match self.command_action(command, shell, mode) {
                Ok(action) => {
                    self.clear_command_input();
                    action
                }
                Err(error) => {
                    view.set_footer(&error);
                    return;
                }
            },
            Action::OpenRunTarget { target, elevated } => {
                NativeAction::OpenRunTarget { target, elevated }
            }
            Action::FillQuery(_) => {
                unreachable!("query editing is handled before platform dispatch")
            }
            Action::Media { .. } => {
                unreachable!("media controls are sent before platform dispatch")
            }
        });
    }

    /// How the next accept runs a shell command (Enter with or without Ctrl and Shift).
    pub fn set_accept_mode(&mut self, mode: RunMode) {
        self.accept_mode = mode;
    }

    /// Windows' usage record changes as the person works, so each time Core is shown it is
    /// read again if Windows has changed it since. Probes use fixture apps and skip it.
    fn refresh_recent_applications(&mut self) {
        if !self.options.probe && self.catalog_ready {
            self.windows_recent.refresh(&self.aliases);
        }
        self.merge_recent_applications();
    }

    fn merge_recent_applications(&mut self) {
        self.recent_applications = recent_applications::merge(
            &self.launched_applications.entries(),
            self.windows_recent.identifiers(),
        );
    }

    /// Only Core's own list changes, so Windows' record is not read again. The file is written
    /// by `save_launches` once the app has been started.
    fn remember_launch(&mut self, identifier: &str) {
        self.launched_applications.record_unsaved(identifier);
        self.merge_recent_applications();
    }

    /// Runs after each action, so a launch never waits for the disk.
    pub fn save_launches(&mut self) {
        if let Err(error) = self.launched_applications.save_pending() {
            eprintln!("{error}");
        }
    }

    pub fn choose_power(&mut self, action: core_engine::search::PowerAction) {
        let Some(view) = self.view.clone() else {
            return;
        };
        view.close_power_menu();
        if let Err(error) = view.set_query(&format!("@power confirm {}", action.command())) {
            view.set_footer(&format!("Could not show power confirmation: {error}"));
            return;
        }
        unsafe {
            let _ = SetFocus(Some(view.input));
        }
        self.queue_search();
    }
}

/// Windows lets only the process that received the last input take the foreground. The
/// Windows-key hook swallows the key that opens Core, so right after the person has used
/// another app the request can be refused, leaving Core open behind that app. An empty mouse
/// input (it moves nothing) makes Core that process, and the second request succeeds.
unsafe fn take_foreground(window: HWND) {
    if SetForegroundWindow(window).as_bool() {
        return;
    }
    let input = INPUT {
        r#type: INPUT_MOUSE,
        ..Default::default()
    };
    SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    if !SetForegroundWindow(window).as_bool() {
        eprintln!("Windows did not let Core come to the front");
    }
}

/// Websites use their favicon; files and folders use their Windows Shell icon. App links such
/// as `steam://` have neither and keep the ↗ symbol, so they never reach the network.
fn quicklink_icon_source(link: &str) -> Option<IconSource> {
    let target = core_engine::quicklinks::validate_target(link).ok()?;
    if core_engine::quicklinks::app_link_scheme(&target).is_some() {
        return None;
    }
    Some(match WebsiteOrigin::parse(&target) {
        Some(origin) => IconSource::Website(origin),
        None => IconSource::Shell(PathBuf::from(target)),
    })
}

/// `--exchange-rates-file <path>` uses a fixed ECB file and never downloads.
fn exchange_rates_file() -> Option<PathBuf> {
    let mut arguments = std::env::args_os();
    arguments.find(|argument| argument == "--exchange-rates-file")?;
    arguments.next().map(PathBuf::from)
}

fn fixture_applications() -> Vec<Application> {
    [
        "Calculator",
        "Command Prompt",
        "File Explorer",
        "Notepad",
        "PowerShell",
        "Settings",
        "Task Manager",
        "Visual Studio Code",
    ]
    .into_iter()
    .map(|name| Application {
        id: name.into(),
        name: name.into(),
        description: "Shell measurement fixture".into(),
        pinned: false,
        launches: 0,
        aliases: Default::default(),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_website_quicklinks_request_favicons() {
        assert!(matches!(
            quicklink_icon_source("https://example.com"),
            Some(IconSource::Website(_))
        ));
        assert!(matches!(
            quicklink_icon_source(r"C:\Games"),
            Some(IconSource::Shell(_))
        ));
        for link in [
            "steam://rungameid/2379780",
            "spotify:track:abc",
            "shell:startup",
        ] {
            assert!(quicklink_icon_source(link).is_none(), "{link}");
        }
    }
}
