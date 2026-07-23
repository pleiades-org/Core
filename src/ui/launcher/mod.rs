use crate::{
    action_executor::{
        execute_result_action_with_context, ActionExecutionContext, ExecutedAction,
    },
    app_index::ApplicationIndex,
    clipboard_history,
    command::{BuiltInAction, CommandAction, CommandCategory, CommandResult, FeatureAction},
    ui_flow::{track_accept_result, track_execute_result},
    command_router::{
        clipboard_browse_filter_from_query, file_search_scope_from_query,
        is_clipboard_browse_query, CommandRouter,
    },
    launcher_services::{DefaultLauncherServices, LauncherServices},
    custom_commands,
    file_index::FileIndex,
    focus,
    settings::LauncherSettings,
    startup::set_launch_at_startup,
    terminal::{
        default_terminal_directory, detect_shell_profiles, parse_command_scope,
        parse_directory_change_target, resolve_directory_change_target, spawn_terminal_command,
        ShellProfile, TerminalOutputKind, TerminalProcessEvent,
    },
    tray_icon::{start_tray_icon_event_loop, TrayIconEvent},
    ui::{
        lucide_icons::{self, LucideIcon},
        platform_window::{hide_platform_window, show_platform_window},
        text_input::{bind_text_input_keys, TextInput},
    },
    window_management,
};
use chrono::{DateTime, Local};
use gpui::{
    actions, div, img, prelude::*, px, rgb, rgba, size, App, AssetSource, AsyncWindowContext,
    Bounds, ClipboardEntry, ClipboardItem, Context, Entity, FocusHandle, Focusable, KeyBinding,
    MouseButton, MouseUpEvent, ObjectFit, ScrollHandle, SharedString, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions,
};
use std::{
    borrow::Cow,
    fs,
    path::{Path, PathBuf},
    sync::{mpsc::Receiver, Arc},
    time::{Duration, Instant, SystemTime},
};

const FOCUS_LOSS_HIDE_GRACE_PERIOD: Duration = Duration::from_millis(350);
const SEARCH_DEBOUNCE_DEFAULT_MS: u64 = 28;
/// File scopes scan a large index — wait for typing to settle before searching.
const SEARCH_DEBOUNCE_FILE_MS: u64 = 160;
/// Fixed browse row height so lists scroll instead of compressing every item into the viewport.
const BROWSE_ROW_HEIGHT: f32 = 52.;
const BROWSE_THUMB_SIZE: f32 = 36.;
const HOME_INPUT_PLACEHOLDER: &str = "";
pub(super) const SETTINGS_INPUT_PLACEHOLDER: &str = "Filter settings...";
const TERMINAL_INPUT_PLACEHOLDER: &str = "Terminal command...";
const MAX_TERMINAL_OUTPUT_LINES: usize = 1_000;
const MAX_RECENT_RESULTS: usize = 8;

enum LauncherPanel {
    Home,
    TerminalShellPicker { command_text: String },
    TerminalSession(TerminalSession),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(super) enum SettingsSection {
    #[default]
    General,
    Search,
    Appearance,
    Shortcuts,
    Expansions,
    Commands,
    Advanced,
}

impl SettingsSection {
    fn label(self) -> &'static str {
        match self {
            SettingsSection::General => "General",
            SettingsSection::Search => "Search",
            SettingsSection::Appearance => "Appearance",
            SettingsSection::Shortcuts => "Shortcuts",
            SettingsSection::Expansions => "Expansions",
            SettingsSection::Commands => "Commands",
            SettingsSection::Advanced => "Advanced",
        }
    }

    fn icon(self) -> crate::ui::lucide_icons::LucideIcon {
        use crate::ui::lucide_icons::LucideIcon;
        match self {
            SettingsSection::General => LucideIcon::Settings,
            SettingsSection::Search => LucideIcon::Search,
            SettingsSection::Appearance => LucideIcon::Monitor,
            SettingsSection::Shortcuts => LucideIcon::Keyboard,
            SettingsSection::Expansions => LucideIcon::TextQuote,
            SettingsSection::Commands => LucideIcon::Terminal,
            SettingsSection::Advanced => LucideIcon::FileCog,
        }
    }

    fn keywords(self) -> &'static [&'static str] {
        match self {
            SettingsSection::General => &[
                "general",
                "hotkey",
                "startup",
                "clipboard",
                "web",
                "launch",
            ],
            SettingsSection::Search => &["search", "index", "apps", "files", "start menu"],
            SettingsSection::Appearance => &["appearance", "blur", "position", "display", "theme"],
            SettingsSection::Shortcuts => &["shortcuts", "hotkeys", "keyboard", "bindings"],
            SettingsSection::Expansions => &[
                "expansions",
                "aliases",
                "snippets",
                "quicklinks",
                "links",
            ],
            SettingsSection::Commands => &["commands", "shell", "custom", "terminal"],
            SettingsSection::Advanced => &[
                "advanced",
                "timezone",
                "currency",
                "config",
                "diagnostics",
            ],
        }
    }

    fn all() -> &'static [SettingsSection] {
        &[
            SettingsSection::General,
            SettingsSection::Search,
            SettingsSection::Appearance,
            SettingsSection::Shortcuts,
            SettingsSection::Expansions,
            SettingsSection::Commands,
            SettingsSection::Advanced,
        ]
    }
}

pub(super) const SETTINGS_SIDEBAR_WIDTH: f32 = 176.;
pub(super) const SETTINGS_PANEL_HEIGHT: f32 = 480.;
pub(super) const LAUNCHER_RESULTS_HEIGHT: f32 = 400.;

#[derive(Clone, Debug, Default)]
struct RegisteredHotkeys {
    launcher: Option<RegisteredHotkey>,
    command_hotkeys: Vec<RegisteredCommandHotkey>,
}

#[derive(Clone, Debug)]
struct RegisteredHotkey {
    id: u32,
    display_text: String,
}

#[derive(Clone, Debug)]
struct RegisteredCommandHotkey {
    id: u32,
    display_text: String,
    query: String,
}

struct TerminalSession {
    shell_profile: ShellProfile,
    working_directory: PathBuf,
    output_lines: Vec<TerminalOutputLine>,
    scroll_handle: ScrollHandle,
    is_running: bool,
    event_receiver: Option<Receiver<TerminalProcessEvent>>,
}

struct TerminalOutputLine {
    kind: TerminalOutputKind,
    text: String,
}

actions!(
    launcher,
    [
        AcceptResult,
        CopyResult,
        MoveSelectionUp,
        MoveSelectionDown,
        MoveSelectionPageUp,
        MoveSelectionPageDown,
        MoveSelectionFirst,
        MoveSelectionLast,
        DismissLauncher,
        QuitApplication,
    ]
);

pub struct LauncherView {
    text_input: Entity<TextInput>,
    quicklink_keyword_input: Entity<TextInput>,
    quicklink_target_input: Entity<TextInput>,
    alias_keyword_input: Entity<TextInput>,
    alias_expands_to_input: Entity<TextInput>,
    snippet_keyword_input: Entity<TextInput>,
    snippet_body_input: Entity<TextInput>,
    hotkey_binding_input: Entity<TextInput>,
    hotkey_query_input: Entity<TextInput>,
    custom_name_input: Entity<TextInput>,
    custom_command_input: Entity<TextInput>,
    timezone_input: Entity<TextInput>,
    currency_input: Entity<TextInput>,
    focus_handle: FocusHandle,
    services: DefaultLauncherServices,
    settings: LauncherSettings,
    available_shells: Vec<ShellProfile>,
    panel: LauncherPanel,
    results: Vec<CommandResult>,
    recent_results: Vec<CommandResult>,
    selected_index: usize,
    is_file_search_view: bool,
    is_clipboard_browse_view: bool,
    browse_results_scroll_handle: ScrollHandle,
    registered_hotkeys: RegisteredHotkeys,
    is_settings_open: bool,
    settings_section: SettingsSection,
    is_launcher_visible: bool,
    last_visibility_change_at: Instant,
    last_recorded_clipboard_text: Option<String>,
    last_external_foreground_window: Option<isize>,
    settings_search_query: String,
    search_debounce_generation: u64,
    pending_search_query: Option<String>,
    last_built_query: String,
    search_bar_hovered: bool,
    pub(super) spotify_title: Option<String>,
    pub(super) spotify_artist: Option<String>,
    pub(super) spotify_closed: bool,
    pub(super) spotify_volume: f32,
    pub window_handle: Option<WindowHandle<LauncherView>>,
}

mod result_list;
mod settings_panel;

use result_list::{compact_display_text, result_row_background};

impl LauncherView {
    fn new(
        text_input: Entity<TextInput>,
        settings: LauncherSettings,
        application_index: ApplicationIndex,
        file_index: FileIndex,
        registered_hotkeys: RegisteredHotkeys,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&text_input, |launcher, text_input, cx| {
            let query = text_input.read(cx).content().to_string();

            // Auto-expand bangs on space press (e.g. "!yt " -> "YouTube | ")
            let bangs_to_expand = [
                ("!g", "Google | "),
                ("!yt", "YouTube | "),
                ("!w", "Wikipedia | "),
                ("!wiki", "Wikipedia | "),
                ("!gh", "GitHub | "),
                ("!d", "DuckDuckGo | "),
            ];

            let mut expanded = None;
            for (bang, replacement) in bangs_to_expand {
                if query == format!("{} ", bang) {
                    expanded = Some(replacement.to_string());
                    break;
                }
            }

            if let Some(replaced) = expanded {
                text_input.update(cx, |input, cx| {
                    input.set_content(replaced, cx);
                });
                return;
            }

            if launcher.is_settings_open {
                launcher.settings_search_query = query;
                cx.notify();
                return;
            }
            launcher.schedule_search_rebuild(query, cx);
        })
        .detach();

        let quicklink_keyword_input = cx.new(|cx| TextInput::new_compact("Keyword", cx));
        let quicklink_target_input = cx.new(|cx| TextInput::new_compact("URL or Path", cx));
        let alias_keyword_input = cx.new(|cx| TextInput::new_compact("Keyword", cx));
        let alias_expands_to_input = cx.new(|cx| TextInput::new_compact("Expands to", cx));
        let snippet_keyword_input = cx.new(|cx| TextInput::new_compact("Keyword", cx));
        let snippet_body_input = cx.new(|cx| TextInput::new_compact("Snippet text", cx));
        let hotkey_binding_input = cx.new(|cx| TextInput::new_compact("Ctrl+Alt+N", cx));
        let hotkey_query_input = cx.new(|cx| TextInput::new_compact("@note Scratch", cx));
        let custom_name_input = cx.new(|cx| TextInput::new_compact("Name", cx));
        let custom_command_input = cx.new(|cx| TextInput::new_compact("Shell command", cx));
        let timezone_seed = settings.local_timezone.clone();
        let timezone_input = cx.new(|cx| {
            let mut input = TextInput::new_compact("Europe/London", cx);
            input.set_content(timezone_seed, cx);
            input
        });
        let currency_seed = settings.home_currency.clone().unwrap_or_default();
        let currency_input = cx.new(|cx| {
            let mut input = TextInput::new_compact("USD", cx);
            input.set_content(currency_seed, cx);
            input
        });

        let services = DefaultLauncherServices::new(CommandRouter::new(
            settings.clone(),
            application_index,
            file_index,
        ));
        let results = services.search("");
        let available_shells = ordered_shell_profiles(
            detect_shell_profiles(),
            settings.preferred_terminal_profile.as_deref(),
        );

        let launcher = Self {
            text_input,
            quicklink_keyword_input,
            quicklink_target_input,
            alias_keyword_input,
            alias_expands_to_input,
            snippet_keyword_input,
            snippet_body_input,
            hotkey_binding_input,
            hotkey_query_input,
            custom_name_input,
            custom_command_input,
            timezone_input,
            currency_input,
            focus_handle: cx.focus_handle(),
            services,
            settings,
            available_shells,
            panel: LauncherPanel::Home,
            results,
            recent_results: Vec::new(),
            selected_index: 0,
            is_file_search_view: false,
            is_clipboard_browse_view: false,
            browse_results_scroll_handle: ScrollHandle::new(),
            registered_hotkeys,
            is_settings_open: false,
            settings_section: SettingsSection::default(),
            is_launcher_visible: true,
            last_visibility_change_at: Instant::now(),
            last_recorded_clipboard_text: None,
            last_external_foreground_window: None,
            settings_search_query: String::new(),
            search_debounce_generation: 0,
            pending_search_query: None,
            last_built_query: String::new(),
            search_bar_hovered: false,
            spotify_title: None,
            spotify_artist: None,
            spotify_closed: false,
            spotify_volume: crate::media_tools::get_system_volume().unwrap_or(0.5),
            window_handle: None,
        };

        // Start background polling loop for Spotify/media status and system volume
        cx.spawn(async move |this, cx| {
            loop {
                let media_info = cx.background_executor().spawn(async move {
                    (
                        crate::media_tools::read_now_playing(),
                        crate::media_tools::get_system_volume().unwrap_or(0.5),
                    )
                }).await;

                let result = this.update(cx, |launcher, cx| {
                    let (now_playing, volume) = media_info;
                    if let Some((title, artist)) = now_playing {
                        launcher.spotify_title = Some(title);
                        launcher.spotify_artist = Some(artist);
                    } else {
                        launcher.spotify_title = None;
                        launcher.spotify_artist = None;
                    }
                    launcher.spotify_volume = volume;
                    cx.notify();
                });
                if result.is_err() {
                    break;
                }

                cx.background_executor()
                    .timer(Duration::from_secs(2))
                    .await;
            }
        })
        .detach();

        launcher
    }

    fn schedule_search_rebuild(&mut self, query: String, cx: &mut Context<Self>) {
        if matches!(&self.panel, LauncherPanel::TerminalSession(_)) {
            return;
        }

        if let Some(command_text) = parse_command_scope(&query) {
            let was_shell_picker = matches!(self.panel, LauncherPanel::TerminalShellPicker { .. });
            self.panel = LauncherPanel::TerminalShellPicker { command_text };
            self.results.clear();
            if !was_shell_picker {
                self.selected_index = 0;
            } else {
                self.selected_index = self
                    .selected_index
                    .min(self.available_shells.len().saturating_sub(1));
            }
            cx.notify();
            return;
        }

        // Enter browse chrome immediately for files/clipboard. File index search is debounced.
        let is_file_query = is_file_search_query(&query);
        let is_clipboard_query = is_clipboard_browse_query(&query);
        if is_file_query || is_clipboard_query {
            self.is_file_search_view = is_file_query;
            self.is_clipboard_browse_view = is_clipboard_query && !is_file_query;
            self.panel = LauncherPanel::Home;
            if is_file_query && file_scope_search_text(&query).is_empty() {
                self.rebuild_results(&query);
                self.pending_search_query = None;
                cx.notify();
                return;
            }
            if is_clipboard_query && !is_file_query {
                // Clipboard history is small — refresh immediately.
                self.rebuild_results(&query);
                self.pending_search_query = None;
                cx.notify();
                return;
            }
            // Keep previous file results visible while the debounced search settles.
            cx.notify();
        } else {
            self.is_file_search_view = false;
            self.is_clipboard_browse_view = false;
        }

        self.pending_search_query = Some(query.clone());
        self.search_debounce_generation = self.search_debounce_generation.wrapping_add(1);
        let generation = self.search_debounce_generation;
        let delay_ms = if is_file_query {
            SEARCH_DEBOUNCE_FILE_MS
        } else {
            SEARCH_DEBOUNCE_DEFAULT_MS
        };

        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(delay_ms))
                .await;

            this.update(cx, |launcher, cx| {
                if launcher.search_debounce_generation != generation {
                    return;
                }
                let Some(query) = launcher.pending_search_query.take() else {
                    return;
                };
                launcher.apply_search_query(&query);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn apply_search_query(&mut self, query: &str) {
        self.panel = LauncherPanel::Home;
        self.is_file_search_view = is_file_search_query(query);
        self.is_clipboard_browse_view =
            is_clipboard_browse_query(query) && !self.is_file_search_view;
        self.rebuild_results(query);
    }

    fn is_browse_view(&self) -> bool {
        self.is_file_search_view || self.is_clipboard_browse_view
    }

    /// Keep the selected file/clipboard row in view during keyboard navigation.
    pub(super) fn ensure_browse_selection_visible(&self) {
        if !self.is_browse_view() {
            return;
        }

        let visual_index = self
            .results
            .iter()
            .enumerate()
            .filter(|(_, result)| {
                if self.is_file_search_view {
                    matches!(result.category, CommandCategory::File)
                } else {
                    matches!(result.category, CommandCategory::Clipboard)
                }
            })
            .position(|(result_index, _)| result_index == self.selected_index);

        if let Some(visual_index) = visual_index {
            self.browse_results_scroll_handle.scroll_to_item(visual_index);
        }
    }

    pub fn apply_launcher_window_geometry(&self, window: &mut Window) {
        let scale = window.scale_factor();
        let (x, y, w, h) = self.geometry_for_display(1.0, scale);
        self.set_window_geometry_with_resize(window, x, y, w, h);
        set_dwm_corners(window, self.settings.display_position);
    }

    fn set_window_position(&self, window: &mut Window, left: i32, top: i32) {
        set_launcher_window_position(window, left, top);
    }

    /// Full geometry set including GPUI resize — use only for initial placement, not per-frame animation.
    fn set_window_geometry_with_resize(&self, window: &mut Window, left: i32, top: i32, width: i32, height: i32) {
        let scale = window.scale_factor();
        let logical_w = width as f32 / scale;
        let logical_h = height as f32 / scale;
        window.resize(size(px(logical_w), px(logical_h)));
        set_launcher_window_position(window, left, top);
    }

    fn geometry_for_display(&self, t: f32, scale_factor: f32) -> (i32, i32, i32, i32) {
        use crate::settings::DisplayPosition;
        let (work_left, work_top, work_width, work_height) = get_primary_work_area();
        let max_width = (720.0 * scale_factor).round() as i32;
        let max_height = (520.0 * scale_factor).round() as i32;

        match self.settings.display_position {
            DisplayPosition::Center => {
                let left = work_left + (work_width - max_width) / 2;
                let top = work_top + (work_height - max_height) / 2;
                (left, top, max_width, max_height)
            }
            DisplayPosition::Top => {
                let left = work_left + (work_width - max_width) / 2;
                let top = work_top - (max_height as f32 * (1.0 - t)) as i32;
                (left, top, max_width, max_height)
            }
            DisplayPosition::Bottom => {
                let left = work_left + (work_width - max_width) / 2;
                let top = work_top + work_height - (max_height as f32 * t) as i32;
                (left, top, max_width, max_height)
            }
            DisplayPosition::Left => {
                let left = work_left - (max_width as f32 * (1.0 - t)) as i32;
                let top = work_top + (work_height - max_height) / 2;
                (left, top, max_width, max_height)
            }
            DisplayPosition::Right => {
                let left = work_left + work_width - (max_width as f32 * t) as i32;
                let top = work_top + (work_height - max_height) / 2;
                (left, top, max_width, max_height)
            }
        }
    }

    pub fn show_launcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.last_external_foreground_window = window_management::active_window_handle();
        self.is_launcher_visible = true;
        self.last_visibility_change_at = Instant::now();
        window.focus(&self.text_input.focus_handle(cx), cx);
        window.activate_window();
        cx.activate(true);

        use crate::settings::DisplayPosition;
        if self.settings.display_position == DisplayPosition::Center {
            self.apply_launcher_window_geometry(window);
            show_platform_window(window);
            cx.notify();
        } else {
            let scale = window.scale_factor();
            let (start_x, start_y, start_w, start_h) = self.geometry_for_display(0.0, scale);
            self.set_window_geometry_with_resize(window, start_x, start_y, start_w, start_h);
            show_platform_window(window);

            let window_handle = self.window_handle.clone().unwrap();
            window.spawn(cx, async move |cx: &mut gpui::AsyncWindowContext| {
                let duration = std::time::Duration::from_millis(150);
                let frames = 15;
                let frame_dur = duration / frames;
                for i in 1..=frames {
                    cx.background_executor().timer(frame_dur).await;
                    let t = i as f32 / frames as f32;
                    // Ease out cubic
                    let ease_t = 1.0 - (1.0 - t).powi(3);
                    let _ = window_handle.update(cx, |launcher, window, _| {
                        let scale = window.scale_factor();
                        let (x, y, _, _) = launcher.geometry_for_display(ease_t, scale);
                        launcher.set_window_position(window, x, y);
                    });
                }
                let _ = window_handle.update(cx, |launcher, window, _| {
                    // Force GPUI viewport to re-sync after ShowWindow(SW_SHOW)
                    // may have sent WM_SIZE that clobbered the initial resize.
                    launcher.apply_launcher_window_geometry(window);
                });
            }).detach();
            cx.notify();
        }
    }

    fn hide_launcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.last_visibility_change_at = Instant::now();
        self.is_settings_open = false;

        use crate::settings::DisplayPosition;
        if self.settings.display_position == DisplayPosition::Center {
            self.is_launcher_visible = false;
            hide_platform_window(window);
            cx.notify();
        } else {
            let window_handle = self.window_handle.clone().unwrap();
            window.spawn(cx, async move |cx: &mut gpui::AsyncWindowContext| {
                let duration = std::time::Duration::from_millis(150);
                let frames = 15;
                let frame_dur = duration / frames;
                for i in 1..=frames {
                    cx.background_executor().timer(frame_dur).await;
                    let t = i as f32 / frames as f32;
                    // Ease in cubic
                    let ease_t = 1.0 - t.powi(3); // goes from 1.0 down to 0.0
                    let _ = window_handle.update(cx, |launcher, window, _| {
                        let scale = window.scale_factor();
                        let (x, y, _, _) = launcher.geometry_for_display(ease_t, scale);
                        launcher.set_window_position(window, x, y);
                    });
                }
                let _ = window_handle.update(cx, |launcher, window, cx| {
                    launcher.is_launcher_visible = false;
                    hide_platform_window(window);
                    cx.notify();
                });
            }).detach();
            cx.notify();
        }
    }

    fn should_hide_for_focus_loss(&self, window: &Window) -> bool {
        self.is_launcher_visible
            && !window.is_window_active()
            && self.last_visibility_change_at.elapsed() >= FOCUS_LOSS_HIDE_GRACE_PERIOD
    }

    fn rebuild_results(&mut self, query: &str) {
        self.last_built_query = query.to_string();
        self.results = if query.trim().is_empty() {
            self.home_results()
        } else {
            self.services.search(query)
        };

        if self.results.is_empty() {
            self.selected_index = 0;
        } else {
            self.selected_index = self.selected_index.min(self.results.len() - 1);
        }
        self.ensure_browse_selection_visible();
    }

    fn home_results(&self) -> Vec<CommandResult> {
        if self.recent_results.is_empty() {
            self.services.search("")
        } else {
            self.recent_results.clone()
        }
    }

    fn accept_action(&mut self, _: &AcceptResult, window: &mut Window, cx: &mut Context<Self>) {
        match &self.panel {
            LauncherPanel::Home => self.accept_selected_result(window, cx),
            LauncherPanel::TerminalShellPicker { .. } => self.accept_selected_shell(cx),
            LauncherPanel::TerminalSession(_) => self.accept_terminal_input(cx),
        }
    }

    fn copy_action(&mut self, _: &CopyResult, _window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(&self.panel, LauncherPanel::Home) {
            return;
        }
        self.copy_selected_result(cx);
    }

    fn copy_selected_result(&mut self, cx: &mut Context<Self>) {
        let Some(selected_result) = self.results.get(self.selected_index) else {
            return;
        };
        if selected_result.copy_text.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(selected_result.copy_text.clone()));
        cx.notify();
    }

    fn accept_selected_result(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected_result) = self.results.get(self.selected_index).cloned() else {
            return;
        };

        self.accept_result(
            selected_result,
            self.last_external_foreground_window,
            window,
            cx,
        );
    }

    fn accept_hotkey_query(
        &mut self,
        query: &str,
        target_window_handle: Option<isize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selected_result) = self
            .services
            .search(query)
            .into_iter()
            .find(|result| !matches!(result.action, CommandAction::None))
        else {
            return;
        };

        self.accept_result(selected_result, target_window_handle, window, cx);
    }

    fn accept_result(
        &mut self,
        selected_result: CommandResult,
        target_window_handle: Option<isize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        track_accept_result(&selected_result);
        self.record_recent_usage(&selected_result);

        match &selected_result.action {
            CommandAction::CopyToClipboard(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                self.hide_launcher(window, cx);
            }
            CommandAction::BuiltIn(BuiltInAction::OpenSettings) => {
                self.enter_settings_mode(cx);
            }
            CommandAction::BuiltIn(BuiltInAction::ReloadApplications) => {
                self.services.router_mut().reload_application_index();
                let query = self.text_input.read(cx).content().to_string();
                self.rebuild_results(&query);
            }
            CommandAction::BuiltIn(BuiltInAction::Quit) => {
                cx.quit();
            }
            _ => {
                track_execute_result(&selected_result);
                match execute_result_action_with_context(
                &selected_result,
                &ActionExecutionContext {
                    target_window_handle,
                },
            ) {
                Ok(ExecutedAction::Launched(_)) => {
                    self.hide_launcher(window, cx);
                }
                Ok(ExecutedAction::OpenedSettings) => {
                    self.hide_launcher(window, cx);
                }
                Ok(ExecutedAction::Copied(text)) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    self.hide_launcher(window, cx);
                }
                Ok(ExecutedAction::CopiedImage(image)) => {
                    cx.write_to_clipboard(ClipboardItem::new_image(&image));
                    self.hide_launcher(window, cx);
                }
                Ok(ExecutedAction::ReloadApplications) => {
                    self.services.router_mut().reload_application_index();
                }
                Ok(ExecutedAction::Quit) => cx.quit(),
                Ok(ExecutedAction::Nothing) => {}
                Err(_) => {}
            }
            }
        }

        cx.notify();
    }

    fn record_recent_usage(&mut self, selected_result: &CommandResult) {
        let _ = self.services.record_usage(selected_result);
        if matches!(selected_result.action, CommandAction::None) {
            return;
        }

        self.recent_results.retain(|recent_result| {
            recent_result.title != selected_result.title
                || recent_result.category != selected_result.category
                || recent_result.copy_text != selected_result.copy_text
        });

        self.recent_results.insert(0, selected_result.clone());
        if self.recent_results.len() > MAX_RECENT_RESULTS {
            self.recent_results.truncate(MAX_RECENT_RESULTS);
        }
    }

    fn accept_selected_shell(&mut self, cx: &mut Context<Self>) {
        let command_text = match &self.panel {
            LauncherPanel::TerminalShellPicker { command_text } => command_text.trim().to_string(),
            _ => return,
        };

        let Some(shell_profile) = self.available_shells.get(self.selected_index).cloned() else {
            return;
        };

        self.settings.preferred_terminal_profile = Some(shell_profile.preference_key());
        self.available_shells = ordered_shell_profiles(
            self.available_shells.clone(),
            self.settings.preferred_terminal_profile.as_deref(),
        );
        self.save_settings();
        self.start_terminal_session(shell_profile, command_text, cx);
    }

    fn accept_terminal_input(&mut self, cx: &mut Context<Self>) {
        let command_text = self.text_input.read(cx).content().trim().to_string();
        if command_text.is_empty() {
            return;
        }

        self.run_command_in_terminal(command_text, cx);
        self.text_input
            .update(cx, |text_input, cx| text_input.reset(cx));
    }

    fn start_terminal_session(
        &mut self,
        shell_profile: ShellProfile,
        command_text: String,
        cx: &mut Context<Self>,
    ) {
        self.panel = LauncherPanel::TerminalSession(TerminalSession {
            shell_profile,
            working_directory: default_terminal_directory(),
            output_lines: Vec::new(),
            scroll_handle: ScrollHandle::new(),
            is_running: false,
            event_receiver: None,
        });
        self.selected_index = 0;
        self.text_input.update(cx, |text_input, cx| {
            text_input.set_placeholder(TERMINAL_INPUT_PLACEHOLDER, cx);
            text_input.reset(cx);
        });

        if !command_text.trim().is_empty() {
            self.run_command_in_terminal(command_text, cx);
        }
    }

    fn run_command_in_terminal(&mut self, command_text: String, cx: &mut Context<Self>) {
        let LauncherPanel::TerminalSession(session) = &mut self.panel else {
            return;
        };

        if session.is_running {
            push_terminal_output_line(
                &mut session.output_lines,
                TerminalOutputKind::Status,
                "A command is already running.",
            );
            cx.notify();
            return;
        }

        if let Some(target_text) = parse_directory_change_target(&command_text) {
            change_terminal_directory(session, &command_text, &target_text);
            session.scroll_handle.scroll_to_bottom();
            cx.notify();
            return;
        }

        let event_receiver = spawn_terminal_command(
            session.shell_profile.clone(),
            command_text,
            session.working_directory.clone(),
        );
        session.is_running = true;
        session.event_receiver = Some(event_receiver);
        session.scroll_handle.scroll_to_bottom();
        cx.notify();
    }

    fn dismiss(&mut self, _: &DismissLauncher, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_settings_open || !matches!(&self.panel, LauncherPanel::Home) {
            self.return_home(cx);
            return;
        }

        self.hide_launcher(window, cx);
    }

    fn return_home(&mut self, cx: &mut Context<Self>) {
        self.is_settings_open = false;
        self.settings_search_query.clear();
        self.panel = LauncherPanel::Home;
        self.is_file_search_view = false;
        self.is_clipboard_browse_view = false;
        self.text_input.update(cx, |text_input, cx| {
            text_input.set_placeholder(HOME_INPUT_PLACEHOLDER, cx);
            text_input.reset(cx);
        });
        self.rebuild_results("");
        self.selected_index = 0;
        cx.notify();
    }

    fn quit_application(
        &mut self,
        _: &QuitApplication,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.quit();
    }

    fn accept_mouse_terminal_shell(
        &mut self,
        shell_index: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_index = shell_index;
        self.accept_selected_shell(cx);
    }

    fn record_current_clipboard(&mut self, cx: &mut Context<Self>) {
        if !self.settings.clipboard_history_enabled {
            return;
        }

        let Some(clipboard_item) = cx.read_from_clipboard() else {
            return;
        };

        for clipboard_entry in clipboard_item.entries() {
            if let ClipboardEntry::Image(image) = clipboard_entry {
                let _ = clipboard_history::record_clipboard_image(
                    &image.bytes,
                    image_format_extension(image.format),
                );
            }
        }

        if let Some(clipboard_text) = clipboard_item.text() {
            if self.last_recorded_clipboard_text.as_deref() == Some(clipboard_text.as_str()) {
                return;
            }

            if clipboard_history::record_clipboard_text(&clipboard_text).is_ok() {
                self.last_recorded_clipboard_text = Some(clipboard_text);
            }
        }
    }

    fn poll_terminal_events(&mut self, cx: &mut Context<Self>) {
        let LauncherPanel::TerminalSession(session) = &mut self.panel else {
            return;
        };

        let Some(event_receiver) = session.event_receiver.take() else {
            return;
        };

        let mut should_keep_receiver = true;
        let mut handled_event = false;
        while let Ok(event) = event_receiver.try_recv() {
            handled_event = true;
            match event {
                TerminalProcessEvent::Output { kind, text } => {
                    push_terminal_output_line(&mut session.output_lines, kind, &text);
                }
                TerminalProcessEvent::Completed { exit_code } => {
                    let status_text = match exit_code {
                        Some(code) => format!("Process exited with code {code}."),
                        None => "Process exited.".to_string(),
                    };
                    push_terminal_output_line(
                        &mut session.output_lines,
                        TerminalOutputKind::Status,
                        &status_text,
                    );
                    session.is_running = false;
                    should_keep_receiver = false;
                }
                TerminalProcessEvent::Failed(error) => {
                    push_terminal_output_line(
                        &mut session.output_lines,
                        TerminalOutputKind::StandardError,
                        &error,
                    );
                    session.is_running = false;
                    should_keep_receiver = false;
                }
            }
        }

        if should_keep_receiver {
            session.event_receiver = Some(event_receiver);
        }

        if handled_event {
            session.scroll_handle.scroll_to_bottom();
            cx.notify();
        }
    }

    fn reload_application_index_from_settings(&mut self) {
        let application_index = if self.settings.index_start_menu {
            ApplicationIndex::load_from_windows_start_menu()
        } else {
            ApplicationIndex::default()
        };
        self.services
            .router_mut()
            .replace_application_index(application_index);
    }

    fn reload_file_index_from_settings(&mut self) {
        let file_index = if self.settings.index_user_files {
            FileIndex::load_from_user_directories()
        } else {
            FileIndex::default()
        };
        self.services.router_mut().replace_file_index(file_index);
    }

}

impl Focusable for LauncherView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for LauncherView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {

        // Outer corner radii
        let tl_out = px(12.);
        let tr_out = px(12.);
        let bl_out = px(12.);
        let br_out = px(12.);

        // Inner corner radii (slightly smaller for concentric layout)
        let tl_in = px(11.);
        let tr_in = px(11.);
        let bl_in = px(11.);
        let br_in = px(11.);

        div()
            .key_context("Launcher")
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::accept_action))
            .on_action(cx.listener(Self::copy_action))
            .on_action(cx.listener(Self::move_selection_up))
            .on_action(cx.listener(Self::move_selection_down))
            .on_action(cx.listener(Self::move_selection_page_up))
            .on_action(cx.listener(Self::move_selection_page_down))
            .on_action(cx.listener(Self::move_selection_first))
            .on_action(cx.listener(Self::move_selection_last))
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::quit_application))
            .size_full()
            .bg(crate::ui::theme::colors::border_glow())
            .p(px(1.))
            .overflow_hidden()
            .rounded_tl(tl_out)
            .rounded_tr(tr_out)
            .rounded_bl(bl_out)
            .rounded_br(br_out)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .size_full()
                    .bg(launcher_background_color(&self.settings))
                    .text_color(crate::ui::theme::colors::text_primary())
                    .overflow_hidden()
                    .rounded_tl(tl_in)
                    .rounded_tr(tr_in)
                    .rounded_bl(bl_in)
                    .rounded_br(br_in)
                    .child(if !self.is_launcher_visible {
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h(px(0.))
                            .child(self.render_search_container(false, cx))
                            .into_any_element()
                    } else if self.is_settings_open {
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h(px(0.))
                            .child(self.render_search_container(false, cx))
                            .child(self.render_settings_menu(cx))
                            .into_any_element()
                    } else if self.is_browse_view() && matches!(&self.panel, LauncherPanel::Home) {
                        let browse_body = if self.is_file_search_view {
                            self.render_file_results(cx).into_any_element()
                        } else {
                            self.render_clipboard_results(cx).into_any_element()
                        };
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h(px(0.))
                            .child(self.render_search_container(false, cx))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_h(px(0.))
                                    .child(browse_body),
                            )
                            .into_any_element()
                    } else {
                        let panel = match &self.panel {
                            LauncherPanel::Home => self.render_home_panel(cx).into_any_element(),
                            LauncherPanel::TerminalShellPicker { command_text } => self
                                .render_terminal_shell_picker(command_text, cx)
                                .into_any_element(),
                            LauncherPanel::TerminalSession(session) => {
                                self.render_terminal_session(session).into_any_element()
                            }
                        };

                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h(px(0.))
                            .child(self.render_search_container(false, cx))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_h(px(0.))
                                    .child(panel),
                            )
                            .into_any_element()
                    })
            )
    }
}

impl LauncherView {
    fn render_search_container(&self, at_bottom: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let hovered = self.search_bar_hovered;

        let settings_button = div()
            .flex()
            .items_center()
            .justify_center()
            .size(px(26.))
            .rounded_md()
            .hover(|style| {
                style
                    .bg(crate::ui::theme::surface_overlay_mid())
                    .cursor_pointer()
            })
            .child(lucide_icons::render_lucide_icon(
                LucideIcon::Settings,
                14.,
                if self.is_settings_open {
                    crate::ui::theme::colors::accent_soft()
                } else {
                    crate::ui::theme::colors::text_muted()
                },
                false,
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |launcher, _: &MouseUpEvent, _window, cx| {
                    if launcher.is_settings_open {
                        launcher.is_settings_open = false;
                        launcher.text_input.update(cx, |text_input, cx| {
                            text_input.set_placeholder(HOME_INPUT_PLACEHOLDER, cx);
                            text_input.reset(cx);
                        });
                        launcher.rebuild_results("");
                        cx.notify();
                    } else {
                        launcher.enter_settings_mode(cx);
                    }
                }),
            );

        let hotkey_badge = if !hovered && !self.is_settings_open {
            let hotkey_text = self
                .registered_hotkeys
                .launcher
                .as_ref()
                .map(|hotkey| hotkey.display_text.clone())
                .unwrap_or_else(|| "Alt+Space".to_string());
            Some(
                div()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .px(px(6.))
                            .py(px(2.))
                            .rounded(px(4.))
                            .bg(crate::ui::theme::surface_overlay_low())
                            .text_size(px(10.))
                            .text_color(crate::ui::theme::colors::text_muted())
                            .child(hotkey_text),
                    ),
            )
        } else {
            None
        };

        let mut container = div()
            .id("search-container")
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .px(px(16.))
            .py(px(12.))
            .on_hover(cx.listener(|this, is_hovered: &bool, _window, cx| {
                this.search_bar_hovered = *is_hovered;
                cx.notify();
            }));

        if at_bottom {
            container = container.border_t_1().border_color(rgba(0xffffff08));
        } else {
            container = container.border_b_1().border_color(rgba(0xffffff08));
        }

        container
            .child(div().flex_1().child(self.text_input.clone()))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .children(hotkey_badge)
                    .child(settings_button),
            )
    }

    fn render_file_results(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::ui::theme::{self, colors, type_scale};

        let query = self.text_input.read(cx).content().to_string();
        let scope = file_search_scope_from_query(&query);
        let scope_title = scope
            .as_ref()
            .map(|scope| scope.short_label())
            .unwrap_or_else(|| "Files".to_string());
        let scope_filter = scope
            .as_ref()
            .map(|scope| scope.label())
            .unwrap_or_else(|| "Browse".to_string());

        let file_results = self
            .results
            .iter()
            .enumerate()
            .filter(|(_, result)| matches!(result.category, CommandCategory::File))
            .collect::<Vec<_>>();
        let info_message = self
            .results
            .iter()
            .find(|result| matches!(result.category, CommandCategory::Help))
            .map(|result| (result.title.clone(), result.subtitle.clone()));

        let selected_file_result = self
            .results
            .get(self.selected_index)
            .filter(|result| matches!(result.category, CommandCategory::File));

        div()
            .flex()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .overflow_hidden()
            .child(
                div()
                    .w(px(320.))
                    .flex_none()
                    .h_full()
                    .flex()
                    .flex_col()
                    .min_h(px(0.))
                    .overflow_hidden()
                    .border_r_1()
                    .border_color(theme::border_subtle())
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_between()
                            .px(px(14.))
                            .py(px(10.))
                            .child(
                                div()
                                    .text_size(px(type_scale::BODY_SM))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(colors::text_secondary())
                                    .child(scope_title),
                            )
                            .child(
                                div()
                                    .px(px(8.))
                                    .py(px(3.))
                                    .rounded_md()
                                    .bg(theme::surface_overlay_low())
                                    .text_size(px(type_scale::LABEL))
                                    .text_color(colors::text_muted())
                                    .child(scope_filter),
                            ),
                    )
                    .child(
                        div()
                            .id("file-results-list")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h(px(0.))
                            .gap(px(2.))
                            .px(px(8.))
                            .pb(px(10.))
                            .overflow_hidden()
                            .overflow_y_scroll()
                            .track_scroll(&self.browse_results_scroll_handle)
                            .when(file_results.is_empty(), |list| {
                                let (title, subtitle) = info_message.unwrap_or_else(|| {
                                    (
                                        "No files found".to_string(),
                                        "Try @files, @pdf, @images, or another extension"
                                            .to_string(),
                                    )
                                });
                                list.child(crate::ui::browse_views::browse_empty_state(
                                    Some(LucideIcon::FolderOpen),
                                    title,
                                    subtitle,
                                ))
                            })
                            .children(file_results.into_iter().map(|(result_index, result)| {
                                self.render_file_result_row(
                                    result,
                                    result_index,
                                    result_index == self.selected_index,
                                    cx,
                                )
                            })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .min_h(px(0.))
                    .h_full()
                    .overflow_hidden()
                    .child(self.render_file_preview(selected_file_result)),
            )
    }

    fn render_clipboard_results(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::ui::theme::{self, colors, type_scale};

        let query = self.text_input.read(cx).content().to_string();
        let filter = clipboard_browse_filter_from_query(&query);
        let filter_label = if filter.trim().is_empty() {
            "All items".to_string()
        } else {
            compact_display_text(&filter, 28)
        };

        let clipboard_results = self
            .results
            .iter()
            .enumerate()
            .filter(|(_, result)| matches!(result.category, CommandCategory::Clipboard))
            .collect::<Vec<_>>();
        let info_message = self
            .results
            .iter()
            .find(|result| matches!(result.category, CommandCategory::Help))
            .map(|result| (result.title.clone(), result.subtitle.clone()));

        let selected = self
            .results
            .get(self.selected_index)
            .filter(|result| matches!(result.category, CommandCategory::Clipboard));

        div()
            .flex()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .overflow_hidden()
            .child(
                div()
                    .w(px(320.))
                    .flex_none()
                    .h_full()
                    .flex()
                    .flex_col()
                    .min_h(px(0.))
                    .overflow_hidden()
                    .border_r_1()
                    .border_color(theme::border_subtle())
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_between()
                            .px(px(14.))
                            .py(px(10.))
                            .child(
                                div()
                                    .text_size(px(type_scale::BODY_SM))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(colors::text_secondary())
                                    .child("Clipboard"),
                            )
                            .child(
                                div()
                                    .px(px(8.))
                                    .py(px(3.))
                                    .rounded_md()
                                    .bg(theme::surface_overlay_low())
                                    .text_size(px(type_scale::LABEL))
                                    .text_color(colors::text_muted())
                                    .child(filter_label),
                            ),
                    )
                    .child(
                        div()
                            .id("clipboard-results-list")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h(px(0.))
                            .gap(px(2.))
                            .px(px(8.))
                            .pb(px(10.))
                            .overflow_hidden()
                            .overflow_y_scroll()
                            .track_scroll(&self.browse_results_scroll_handle)
                            .when(clipboard_results.is_empty(), |list| {
                                let (title, subtitle) = info_message.unwrap_or_else(|| {
                                    (
                                        "Clipboard is empty".to_string(),
                                        "Copy text, links, colors, or images to populate history"
                                            .to_string(),
                                    )
                                });
                                list.child(crate::ui::browse_views::browse_empty_state(
                                    Some(LucideIcon::Clipboard),
                                    title,
                                    subtitle,
                                ))
                            })
                            .children(clipboard_results.into_iter().map(
                                |(result_index, result)| {
                                    self.render_clipboard_result_row(
                                        result,
                                        result_index,
                                        result_index == self.selected_index,
                                        cx,
                                    )
                                },
                            )),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .min_h(px(0.))
                    .h_full()
                    .overflow_hidden()
                    .child(self.render_clipboard_preview(selected)),
            )
    }

    fn render_file_result_row(
        &self,
        result: &CommandResult,
        result_index: usize,
        is_selected: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        use crate::ui::theme::{colors, type_scale};

        let title = result.title.clone();
        let subtitle = file_path_from_result(result)
            .and_then(|path| path.parent())
            .map(format_display_path)
            .unwrap_or_else(|| result.subtitle.clone());

        // flex_none keeps rows at natural height so the list scrolls instead of compressing items.
        div()
            .id(("file-result-row", result_index))
            .w_full()
            .flex()
            .flex_none()
            .flex_shrink_0()
            .items_center()
            .gap(px(10.))
            .h(px(BROWSE_ROW_HEIGHT))
            .px(px(10.))
            .rounded_md()
            .overflow_hidden()
            .bg(result_row_background(is_selected))
            .hover(|style| {
                let style = style.cursor_pointer();
                if is_selected {
                    style
                } else {
                    style.bg(crate::ui::theme::result_row_hover_background())
                }
            })
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |launcher, _: &MouseUpEvent, window, cx| {
                    launcher.accept_mouse_result(result_index, window, cx);
                }),
            )
            .child(file_thumbnail_for_result(result, BROWSE_THUMB_SIZE))
            .child(browse_row_text(title, subtitle, type_scale::BODY_LG, colors::text_primary()))
            .into_any_element()
    }

    fn render_clipboard_result_row(
        &self,
        result: &CommandResult,
        result_index: usize,
        is_selected: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        use crate::ui::theme::{colors, type_scale};

        let title = result.title.clone();
        let subtitle = result.subtitle.clone();
        let leading = clipboard_leading_for_result(result, BROWSE_THUMB_SIZE);

        let mut row = div()
            .id(("clipboard-result-row", result_index))
            .w_full()
            .flex()
            .flex_none()
            .flex_shrink_0()
            .items_center()
            .gap(px(10.))
            .h(px(BROWSE_ROW_HEIGHT))
            .px(px(10.))
            .rounded_md()
            .overflow_hidden()
            .bg(result_row_background(is_selected))
            .hover(|style| {
                let style = style.cursor_pointer();
                if is_selected {
                    style
                } else {
                    style.bg(crate::ui::theme::result_row_hover_background())
                }
            })
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |launcher, _: &MouseUpEvent, window, cx| {
                    launcher.accept_mouse_result(result_index, window, cx);
                }),
            );

        if let Some(leading) = leading {
            row = row.child(leading);
        }

        row.child(browse_row_text(
            title,
            subtitle,
            type_scale::BODY_LG,
            colors::text_primary(),
        ))
        .into_any_element()
    }

    fn render_file_preview(
        &self,
        selected_file_result: Option<&CommandResult>,
    ) -> gpui::AnyElement {
        use crate::ui::theme::{self, colors, type_scale};

        let Some(selected_file_result) = selected_file_result else {
            return div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .h_full()
                .text_color(colors::text_muted())
                .child("No file selected")
                .into_any_element();
        };

        let file_path = file_path_from_result(selected_file_result);
        let metadata = file_path.and_then(|path| fs::metadata(path).ok());
        let file_name = selected_file_result.title.clone();
        let parent_path = file_path
            .and_then(|path| path.parent())
            .map(format_display_path)
            .unwrap_or_default();
        let file_type = file_path
            .map(|path| file_type_label(path))
            .unwrap_or_else(|| "File".to_string());
        let file_size = metadata
            .as_ref()
            .map(|metadata| format_file_size(metadata.len()))
            .unwrap_or_else(|| "Unknown".to_string());
        let created_at = metadata
            .as_ref()
            .and_then(|metadata| metadata.created().ok())
            .map(format_system_time)
            .unwrap_or_else(|| "Unknown".to_string());
        let modified_at = metadata
            .as_ref()
            .and_then(|metadata| metadata.modified().ok())
            .map(format_system_time)
            .unwrap_or_else(|| "Unknown".to_string());

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .px(px(16.))
            .py(px(12.))
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .w_full()
                    .h(px(180.))
                    .rounded_md()
                    .bg(theme::elevated_surface_background())
                    .border_1()
                    .border_color(theme::card_border())
                    .overflow_hidden()
                    .child(large_file_preview(selected_file_result)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(type_scale::BODY_SM))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(colors::text_muted())
                            .child("Metadata"),
                    )
                    .child(file_metadata_row("Name", file_name))
                    .child(file_metadata_row("Where", parent_path))
                    .child(file_metadata_row("Type", file_type))
                    .child(file_metadata_row("Size", file_size))
                    .child(file_metadata_row("Created", created_at))
                    .child(file_metadata_row("Modified", modified_at)),
            )
            .into_any_element()
    }

    fn render_clipboard_preview(
        &self,
        selected: Option<&CommandResult>,
    ) -> gpui::AnyElement {
        use crate::ui::theme::{self, colors, type_scale};

        let Some(selected) = selected else {
            return div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .h_full()
                .text_color(colors::text_muted())
                .child("No item selected")
                .into_any_element();
        };

        let image_path = clipboard_image_path(selected);
        let has_image = image_path.is_some();
        let color = clipboard_history::parse_clipboard_color(&selected.copy_text)
            .or_else(|| clipboard_history::parse_clipboard_color(&selected.title));

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .px(px(16.))
            .py(px(12.))
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .w_full()
                    .h(px(180.))
                    .rounded_md()
                    .bg(theme::elevated_surface_background())
                    .border_1()
                    .border_color(theme::card_border())
                    .overflow_hidden()
                    .child(if let Some(path) = image_path {
                        img(path)
                            .w_full()
                            .h_full()
                            .object_fit(ObjectFit::Contain)
                            .with_fallback(|| {
                                extension_badge("IMG", 120., 44.).into_any_element()
                            })
                            .into_any_element()
                    } else if let Some(color_value) = color {
                        div()
                            .size_full()
                            .bg(rgb(color_value))
                            .into_any_element()
                    } else {
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap(px(8.))
                            .size_full()
                            .px(px(16.))
                            .child(lucide_icons::render_lucide_icon(
                                LucideIcon::Clipboard,
                                28.,
                                colors::text_faint(),
                                false,
                            ))
                            .child(
                                div()
                                    .text_size(px(type_scale::BODY_SM))
                                    .text_color(colors::text_muted())
                                    .text_center()
                                    .child(compact_display_text(&selected.title, 72)),
                            )
                            .into_any_element()
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(type_scale::BODY_SM))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(colors::text_muted())
                            .child("Details"),
                    )
                    .child(file_metadata_row("Preview", selected.title.clone()))
                    .child(file_metadata_row("Kind", selected.subtitle.clone()))
                    .when(!selected.copy_text.is_empty() && !has_image, |details| {
                        details.child(file_metadata_row(
                            "Content",
                            compact_display_text(&selected.copy_text, 96),
                        ))
                    }),
            )
            .into_any_element()
    }

    fn render_terminal_shell_picker(
        &self,
        command_text: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let command_text = command_text.to_string();
        let shell_count = self.available_shells.len();

        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .px(px(10.))
            .py(px(10.))
            .h(px(360.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .pb(px(8.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(0xffffff))
                            .child("Choose terminal"),
                    )
                    .child(div().text_size(px(12.)).text_color(rgb(0xd9d9d9)).child(
                        if command_text.is_empty() {
                            "Type a command after @cmd".to_string()
                        } else {
                            command_text.clone()
                        },
                    )),
            )
            .when(shell_count == 0, |container| {
                container.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .h_full()
                        .text_color(rgb(0x9ca3af))
                        .child("No supported terminal was found"),
                )
            })
            .children(self.available_shells.iter().enumerate().map(
                |(shell_index, shell_profile)| {
                    let is_selected = shell_index == self.selected_index;
                    let shell_name = shell_profile.display_name.clone();

                    div()
                        .id(("terminal-shell-row", shell_index))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .px(px(10.))
                        .py(px(8.))
                        .rounded_sm()
                        .bg(if is_selected {
                            rgb(0x010101)
                        } else {
                            rgb(0x000000)
                        })
                        .hover(|style| style.bg(rgb(0x010101)).cursor_pointer())
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |launcher, _: &MouseUpEvent, window, cx| {
                                launcher.accept_mouse_terminal_shell(shell_index, window, cx);
                            }),
                        )
                        .child(fallback_icon(">", rgb(0x22c55e)))
                        .child(
                            div().w_full().child(
                                div()
                                    .text_size(px(15.))
                                    .text_color(rgb(0xffffff))
                                    .child(shell_name),
                            ),
                        )
                },
            ))
    }

    fn render_terminal_session(&self, session: &TerminalSession) -> impl IntoElement {
        let shell_name = session.shell_profile.display_name.clone();
        let working_directory = format_display_path(&session.working_directory);
        let is_running = session.is_running;

        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .px(px(10.))
            .py(px(10.))
            .h(px(360.))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .rounded_sm()
                    .bg(rgb(0x050505))
                    .border_1()
                    .border_color(rgb(0x171717))
                    .px(px(10.))
                    .py(px(8.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(rgb(0xffffff))
                                    .child(shell_name),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(0xd9d9d9))
                                    .child(working_directory),
                            ),
                    )
                    .when(is_running, |header| {
                        header.child(
                            div()
                                .px(px(8.))
                                .py(px(3.))
                                .rounded_sm()
                                .bg(rgb(0x052e16))
                                .text_size(px(12.))
                                .text_color(rgb(0x22c55e))
                                .child("Running"),
                        )
                    }),
            )
            .child(
                div()
                    .id("terminal-output")
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .h_full()
                    .overflow_y_scroll()
                    .rounded_sm()
                    .bg(rgb(0x050505))
                    .border_1()
                    .border_color(rgb(0x171717))
                    .px(px(12.))
                    .py(px(10.))
                    .font_family("Consolas")
                    .text_size(px(12.))
                    .track_scroll(&session.scroll_handle)
                    .children(session.output_lines.iter().map(|output_line| {
                        div()
                            .text_color(terminal_output_color(output_line.kind))
                            .child(output_line.text.clone())
                    })),
            )
    }

}

struct FileAssets;

impl AssetSource for FileAssets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        std::fs::read(path)
            .map(Cow::Owned)
            .map(Some)
            .map_err(Into::into)
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(std::fs::read_dir(path)?
            .filter_map(|entry| {
                Some(SharedString::from(
                    entry.ok()?.path().to_string_lossy().into_owned(),
                ))
            })
            .collect())
    }
}

fn purple_app_icon_image() -> Arc<image::RgbaImage> {
    const CORE_ICON_PNG: &[u8] = include_bytes!("../../../assets/Core.png");
    let fallback = Arc::new(image::RgbaImage::from_pixel(
        64,
        64,
        image::Rgba([0x7c, 0x3a, 0xed, 0xff]),
    ));
    Arc::new(
        image::load_from_memory(CORE_ICON_PNG)
            .ok()
            .and_then(|img| Some(img.into_rgba8()))
            .unwrap_or_else(|| (*fallback).clone()),
    )
}

fn window_background_appearance(settings: &LauncherSettings) -> WindowBackgroundAppearance {
    if settings.backdrop_blur_enabled {
        WindowBackgroundAppearance::Blurred
    } else {
        WindowBackgroundAppearance::Transparent
    }
}

fn launcher_background_color(settings: &LauncherSettings) -> gpui::Rgba {
    crate::ui::theme::launcher_background(settings.backdrop_blur_enabled)
}

fn image_format_extension(image_format: gpui::ImageFormat) -> &'static str {
    match image_format {
        gpui::ImageFormat::Png => "png",
        gpui::ImageFormat::Jpeg => "jpg",
        gpui::ImageFormat::Webp => "webp",
        gpui::ImageFormat::Gif => "gif",
        gpui::ImageFormat::Svg => "svg",
        gpui::ImageFormat::Bmp => "bmp",
        gpui::ImageFormat::Tiff => "tiff",
        gpui::ImageFormat::Ico => "ico",
        gpui::ImageFormat::Pnm => "pnm",
    }
}

pub fn run() {
    gpui_platform::application()
        .with_assets(FileAssets)
        .run(|cx: &mut App| {
            bind_launcher_keys(cx);
            bind_text_input_keys(cx);

            let (tray_event_sender, tray_event_receiver) = std::sync::mpsc::channel();
            start_tray_icon_event_loop(tray_event_sender);

            let settings = LauncherSettings::load_or_create();
            let _ = set_launch_at_startup(settings.launch_at_startup);
            let application_index = if settings.index_start_menu {
                ApplicationIndex::load_from_windows_start_menu()
            } else {
                ApplicationIndex::default()
            };
            let file_index = if settings.index_user_files {
                FileIndex::load_from_user_directories()
            } else {
                FileIndex::default()
            };
            let registered_hotkeys = register_global_hotkeys(&settings);
            let window_bounds = Bounds::centered(None, size(px(720.0), px(520.0)), cx);

            let window_handle = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(window_bounds)),
                        kind: WindowKind::Floating,
                        titlebar: None,
                        is_resizable: false,
                        window_background: window_background_appearance(&settings),
                        icon: Some(purple_app_icon_image()),
                        ..Default::default()
                    },
                    |window, cx| {
                        let text_input = cx.new(|cx| TextInput::new(HOME_INPUT_PLACEHOLDER, cx).borderless());
                        cx.new(|cx| {
                            cx.observe_window_activation(
                                window,
                                |launcher: &mut LauncherView, window, cx| {
                                    if launcher.should_hide_for_focus_loss(window) {
                                        launcher.hide_launcher(window, cx);
                                    }
                                },
                            )
                            .detach();

                            LauncherView::new(
                                text_input,
                                settings,
                                application_index,
                                file_index,
                                registered_hotkeys.clone(),
                                cx,
                            )
                        })
                    },
                )
                .expect("failed to open Core Launcher window");

            window_handle
                .update(cx, move |launcher, window, cx| {
                    launcher.window_handle = Some(window_handle.clone());
                    launcher.show_launcher(window, cx);
                    start_global_hotkey_poll(window_handle, window, cx);
                    start_tray_icon_event_poll(window_handle, window, cx, tray_event_receiver);
                    start_focus_loss_poll(window_handle, window, cx);
                    start_terminal_event_poll(window_handle, window, cx);
                    start_productivity_event_poll(window_handle, window, cx);
                })
                .ok();

            cx.activate(true);
        });
}

fn browse_row_text(
    title: String,
    subtitle: String,
    title_size: f32,
    title_color: gpui::Rgba,
) -> gpui::Div {
    use crate::ui::theme::{colors, type_scale};

    div()
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(2.))
        .min_w(px(0.))
        .flex_1()
        .overflow_hidden()
        .child(
            div()
                .w_full()
                .min_w(px(0.))
                .overflow_hidden()
                .truncate()
                .text_size(px(title_size))
                .text_color(title_color)
                .child(title),
        )
        .child(
            div()
                .w_full()
                .min_w(px(0.))
                .overflow_hidden()
                .truncate()
                .text_size(px(type_scale::LABEL))
                .text_color(colors::text_faint())
                .child(subtitle),
        )
}

fn clipped_thumbnail(size: f32, child: gpui::AnyElement) -> gpui::AnyElement {
    div()
        .size(px(size))
        .flex_none()
        .rounded_md()
        .overflow_hidden()
        .child(child)
        .into_any_element()
}

fn file_thumbnail_for_result(result: &CommandResult, size: f32) -> gpui::AnyElement {
    if let Some(path) = file_path_from_result(result).filter(|path| path_is_image(path)) {
        let fallback_label = extension_label_for_path(path);
        return clipped_thumbnail(
            size,
            img(path.clone())
                .size(px(size))
                .object_fit(ObjectFit::Cover)
                .with_fallback(move || {
                    extension_badge(&fallback_label, size, 9.).into_any_element()
                })
                .into_any_element(),
        );
    }

    let label = file_path_from_result(result)
        .map(|path| extension_label_for_path(path))
        .unwrap_or_else(|| "FILE".to_string());
    extension_badge(&label, size, if label.len() > 3 { 8. } else { 10. }).into_any_element()
}

fn large_file_preview(result: &CommandResult) -> gpui::AnyElement {
    if let Some(path) = file_path_from_result(result).filter(|path| path_is_image(path)) {
        let fallback_label = extension_label_for_path(path);
        return img(path.clone())
            .w_full()
            .h_full()
            .object_fit(ObjectFit::Contain)
            .with_fallback(move || extension_badge(&fallback_label, 120., 36.).into_any_element())
            .into_any_element();
    }

    let label = file_path_from_result(result)
        .map(|path| extension_label_for_path(path))
        .unwrap_or_else(|| "FILE".to_string());
    extension_badge(&label, 120., if label.len() > 4 { 28. } else { 36. }).into_any_element()
}

/// Leading media for clipboard rows. Text/link items intentionally have no icon.
fn clipboard_leading_for_result(result: &CommandResult, size: f32) -> Option<gpui::AnyElement> {
    if let Some(path) = clipboard_image_path(result) {
        return Some(clipped_thumbnail(
            size,
            img(path)
                .size(px(size))
                .object_fit(ObjectFit::Cover)
                .with_fallback(move || extension_badge("IMG", size, 9.).into_any_element())
                .into_any_element(),
        ));
    }

    if let Some(color_value) = clipboard_history::parse_clipboard_color(&result.copy_text)
        .or_else(|| clipboard_history::parse_clipboard_color(&result.title))
    {
        return Some(
            div()
                .size(px(size))
                .flex_none()
                .rounded_md()
                .border_1()
                .border_color(crate::ui::theme::border_subtle())
                .bg(rgb(color_value))
                .into_any_element(),
        );
    }

    None
}

fn extension_badge(label: &str, size: f32, text_size: f32) -> gpui::Div {
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .bg(rgb(0xf4f4f5))
        .text_color(rgb(0x155e75))
        .text_size(px(text_size))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(label.to_string())
}

fn extension_label_for_path(path: &Path) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_uppercase())
        .filter(|extension| !extension.is_empty())
        .unwrap_or_else(|| "FILE".to_string())
}

fn path_is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "ico" | "tif" | "tiff" | "heic"
                    | "avif"
            )
        })
}

fn clipboard_image_path(result: &CommandResult) -> Option<PathBuf> {
    if let Some(path) = result.icon_path.clone() {
        return Some(path);
    }
    match &result.action {
        CommandAction::Feature(FeatureAction::CopyClipboardImage { image_path }) => {
            Some(image_path.clone())
        }
        _ => None,
    }
}

fn file_metadata_row(label: &'static str, value: String) -> gpui::Div {
    use crate::ui::theme::{colors, type_scale};
    let compact_value = compact_display_text(&value, 48);

    div()
        .flex()
        .justify_between()
        .items_center()
        .gap(px(12.))
        .py(px(5.))
        .child(
            div()
                .text_size(px(type_scale::BODY_SM))
                .text_color(colors::text_faint())
                .child(label),
        )
        .child(
            div()
                .text_size(px(type_scale::BODY_SM))
                .text_color(colors::text_secondary())
                .text_align(gpui::TextAlign::Right)
                .child(compact_value),
        )
}

fn file_path_from_result(result: &CommandResult) -> Option<&PathBuf> {
    match &result.action {
        CommandAction::OpenPath(path) if matches!(result.category, CommandCategory::File) => {
            Some(path)
        }
        _ => None,
    }
}

fn file_type_label(path: &Path) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!("{} file", extension.to_uppercase()))
        .unwrap_or_else(|| "File".to_string())
}

fn format_display_path(path: &Path) -> String {
    let path_text = path.display().to_string();
    let Some(user_profile) = std::env::var_os("USERPROFILE").map(PathBuf::from) else {
        return path_text;
    };

    path.strip_prefix(&user_profile)
        .ok()
        .map(|relative_path| format!("~\\{}", relative_path.display()))
        .unwrap_or(path_text)
}

fn format_file_size(size_in_bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    match size_in_bytes {
        0..=999 => format!("{size_in_bytes} bytes"),
        _ if (size_in_bytes as f64) < MIB => format!("{:.1} KB", size_in_bytes as f64 / KIB),
        _ if (size_in_bytes as f64) < GIB => format!("{:.1} MB", size_in_bytes as f64 / MIB),
        _ => format!("{:.1} GB", size_in_bytes as f64 / GIB),
    }
}

fn format_system_time(system_time: SystemTime) -> String {
    let datetime: DateTime<Local> = system_time.into();
    let now = Local::now();

    if datetime.date_naive() == now.date_naive() {
        format!("Today at {}", datetime.format("%H:%M:%S"))
    } else {
        datetime.format("%b %d, %Y at %H:%M:%S").to_string()
    }
}

fn is_file_search_query(query: &str) -> bool {
    file_search_scope_from_query(query).is_some()
        || crate::command_router::is_implicit_file_browse_query(query)
}

/// Text after an `@files` / `@mp4` / … scope tag. Empty means "idle, waiting for input".
fn file_scope_search_text(query: &str) -> String {
    let Some(scope_query) = query.trim().strip_prefix('@') else {
        return query.trim().to_string();
    };
    let mut parts = scope_query.split_whitespace();
    let _scope_tag = parts.next();
    parts.collect::<Vec<_>>().join(" ")
}

fn fallback_icon(label: &'static str, color: gpui::Rgba) -> gpui::Div {
    div()
        .size(px(28.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .bg(rgb(0x010101))
        .text_color(color)
        .text_size(px(13.))
        .child(label)
}

fn bind_launcher_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", AcceptResult, None),
        KeyBinding::new("return", AcceptResult, None),
        KeyBinding::new("ctrl-enter", CopyResult, None),
        KeyBinding::new("cmd-enter", CopyResult, None),
        KeyBinding::new("up", MoveSelectionUp, None),
        KeyBinding::new("down", MoveSelectionDown, None),
        KeyBinding::new("shift-tab", MoveSelectionUp, None),
        KeyBinding::new("tab", MoveSelectionDown, None),
        KeyBinding::new("ctrl-p", MoveSelectionUp, None),
        KeyBinding::new("ctrl-n", MoveSelectionDown, None),
        KeyBinding::new("pageup", MoveSelectionPageUp, None),
        KeyBinding::new("pagedown", MoveSelectionPageDown, None),
        KeyBinding::new("ctrl-home", MoveSelectionFirst, None),
        KeyBinding::new("ctrl-end", MoveSelectionLast, None),
        KeyBinding::new("escape", DismissLauncher, None),
        KeyBinding::new("ctrl-q", QuitApplication, None),
        KeyBinding::new("cmd-q", QuitApplication, None),
    ]);
}

fn terminal_output_color(output_kind: TerminalOutputKind) -> gpui::Rgba {
    match output_kind {
        TerminalOutputKind::Command => rgb(0x22c55e),
        TerminalOutputKind::StandardOutput => rgb(0xffffff),
        TerminalOutputKind::StandardError => rgb(0xef4444),
        TerminalOutputKind::Status => rgb(0xd9d9d9),
    }
}

fn ordered_shell_profiles(
    mut shell_profiles: Vec<ShellProfile>,
    preferred_terminal_profile: Option<&str>,
) -> Vec<ShellProfile> {
    let Some(preferred_terminal_profile) = preferred_terminal_profile else {
        return shell_profiles;
    };

    shell_profiles.sort_by_key(|shell_profile| {
        if shell_profile.preference_key() == preferred_terminal_profile {
            0
        } else {
            1
        }
    });
    shell_profiles
}

fn change_terminal_directory(session: &mut TerminalSession, command_text: &str, target_text: &str) {
    push_terminal_output_line(
        &mut session.output_lines,
        TerminalOutputKind::Command,
        &format!(
            "{} {}> {}",
            session.shell_profile.display_name,
            session.working_directory.display(),
            command_text.trim()
        ),
    );

    let target_directory = resolve_directory_change_target(&session.working_directory, target_text);
    if !target_directory.is_dir() {
        push_terminal_output_line(
            &mut session.output_lines,
            TerminalOutputKind::StandardError,
            &format!("Directory not found: {}", target_directory.display()),
        );
        return;
    }

    session.working_directory = target_directory.canonicalize().unwrap_or(target_directory);
    push_terminal_output_line(
        &mut session.output_lines,
        TerminalOutputKind::Status,
        &format!(
            "Directory changed to {}",
            session.working_directory.display()
        ),
    );
}

fn push_terminal_output_line(
    output_lines: &mut Vec<TerminalOutputLine>,
    output_kind: TerminalOutputKind,
    output_text: &str,
) {
    if output_text.is_empty() {
        output_lines.push(TerminalOutputLine {
            kind: output_kind,
            text: String::new(),
        });
    } else {
        for output_line in output_text.replace("\r\n", "\n").split('\n') {
            output_lines.push(TerminalOutputLine {
                kind: output_kind,
                text: output_line.to_string(),
            });
        }
    }

    if output_lines.len() > MAX_TERMINAL_OUTPUT_LINES {
        let removed_line_count = output_lines.len() - MAX_TERMINAL_OUTPUT_LINES;
        output_lines.drain(0..removed_line_count);
    }
}

#[cfg(target_os = "windows")]
fn register_global_hotkeys(settings: &LauncherSettings) -> RegisteredHotkeys {
    use global_hotkey::GlobalHotKeyManager;
    use std::collections::HashSet;

    let Ok(manager) = GlobalHotKeyManager::new() else {
        return RegisteredHotkeys::default();
    };

    let mut registered_hotkeys = RegisteredHotkeys::default();
    let mut registered_ids = HashSet::new();

    if settings.hotkey_enabled {
        if let Some(launcher_hotkey) = register_hotkey(&manager, &settings.hotkey) {
            registered_ids.insert(launcher_hotkey.id);
            registered_hotkeys.launcher = Some(launcher_hotkey);
        }
    }

    for configured_hotkey in custom_commands::configured_query_hotkeys(settings) {
        let Some((hotkey, display_text)) = parse_hotkey_text(&configured_hotkey.hotkey) else {
            continue;
        };
        let hotkey_id = hotkey.id();

        if registered_ids.contains(&hotkey_id) || manager.register(hotkey).is_err() {
            continue;
        }

        registered_ids.insert(hotkey_id);
        registered_hotkeys
            .command_hotkeys
            .push(RegisteredCommandHotkey {
                id: hotkey_id,
                display_text,
                query: configured_hotkey.query,
            });
    }

    if registered_hotkeys.launcher.is_some() || !registered_hotkeys.command_hotkeys.is_empty() {
        Box::leak(Box::new(manager));
    }

    registered_hotkeys
}

#[cfg(target_os = "windows")]
fn register_hotkey(
    manager: &global_hotkey::GlobalHotKeyManager,
    hotkey_text: &str,
) -> Option<RegisteredHotkey> {
    let (hotkey, display_text) = parse_hotkey_text(hotkey_text)?;
    manager.register(hotkey).ok()?;

    Some(RegisteredHotkey {
        id: hotkey.id(),
        display_text,
    })
}

#[cfg(target_os = "windows")]
fn parse_hotkey_text(hotkey_text: &str) -> Option<(global_hotkey::hotkey::HotKey, String)> {
    let display_text = hotkey_text.trim().to_string();
    if display_text.is_empty() {
        return None;
    }

    let hotkey = display_text.parse::<global_hotkey::hotkey::HotKey>().ok()?;
    Some((hotkey, display_text))
}

#[cfg(not(target_os = "windows"))]
fn register_global_hotkeys(_settings: &LauncherSettings) -> RegisteredHotkeys {
    RegisteredHotkeys::default()
}

fn start_focus_loss_poll(
    window_handle: WindowHandle<LauncherView>,
    window: &mut Window,
    cx: &mut Context<LauncherView>,
) {
    window
        .spawn(cx, async move |async_window_cx: &mut AsyncWindowContext| {
            loop {
                async_window_cx
                    .background_executor()
                    .timer(Duration::from_millis(100))
                    .await;

                let _ = window_handle.update(async_window_cx, |launcher, window, cx| {
                    if launcher.should_hide_for_focus_loss(window) {
                        launcher.hide_launcher(window, cx);
                    }
                });
            }
        })
        .detach();
}

fn start_terminal_event_poll(
    window_handle: WindowHandle<LauncherView>,
    window: &mut Window,
    cx: &mut Context<LauncherView>,
) {
    window
        .spawn(cx, async move |async_window_cx: &mut AsyncWindowContext| {
            loop {
                async_window_cx
                    .background_executor()
                    .timer(Duration::from_millis(80))
                    .await;

                let _ = window_handle.update(async_window_cx, |launcher, _window, cx| {
                    launcher.poll_terminal_events(cx);
                });
            }
        })
        .detach();
}

fn start_productivity_event_poll(
    window_handle: WindowHandle<LauncherView>,
    window: &mut Window,
    cx: &mut Context<LauncherView>,
) {
    window
        .spawn(cx, async move |async_window_cx: &mut AsyncWindowContext| {
            loop {
                async_window_cx
                    .background_executor()
                    .timer(Duration::from_millis(900))
                    .await;

                let _ = window_handle.update(async_window_cx, |launcher, _window, cx| {
                    launcher.record_current_clipboard(cx);
                    focus::enforce_active_focus_session();
                });
            }
        })
        .detach();
}

fn start_tray_icon_event_poll(
    window_handle: WindowHandle<LauncherView>,
    window: &mut Window,
    cx: &mut Context<LauncherView>,
    tray_event_receiver: Receiver<TrayIconEvent>,
) {
    window
        .spawn(cx, async move |async_window_cx: &mut AsyncWindowContext| {
            loop {
                async_window_cx
                    .background_executor()
                    .timer(Duration::from_millis(120))
                    .await;

                while let Ok(tray_event) = tray_event_receiver.try_recv() {
                    match tray_event {
                        TrayIconEvent::ShowLauncher => {
                            let _ =
                                window_handle.update(async_window_cx, |launcher, window, cx| {
                                    launcher.show_launcher(window, cx);
                                });
                        }
                        TrayIconEvent::OpenSettings => {
                            let _ =
                                window_handle.update(async_window_cx, |launcher, _window, cx| {
                                    launcher.enter_settings_mode(cx);
                                });
                        }
                        TrayIconEvent::QuitApplication => {
                            let _ = window_handle.update(async_window_cx, |_launcher, _window, cx| {
                                cx.quit();
                            });
                        }
                    }
                }
            }
        })
        .detach();
}

#[cfg(target_os = "windows")]
fn start_global_hotkey_poll(
    window_handle: WindowHandle<LauncherView>,
    window: &mut Window,
    cx: &mut Context<LauncherView>,
) {
    use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
    use std::collections::HashSet;

    window
        .spawn(cx, async move |async_window_cx: &mut AsyncWindowContext| {
            let mut pressed_hotkeys = HashSet::new();
            let mut last_toggle_at: Option<Instant> = None;
            const TOGGLE_COOLDOWN: Duration = Duration::from_millis(200);

            loop {
                async_window_cx
                    .background_executor()
                    .timer(Duration::from_millis(40))
                    .await;

                while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                    match event.state {
                        HotKeyState::Pressed => {
                            if !pressed_hotkeys.insert(event.id) {
                                continue;
                            }

                            let now = Instant::now();
                            if let Some(last_time) = last_toggle_at {
                                if now.duration_since(last_time) < TOGGLE_COOLDOWN {
                                    continue;
                                }
                            }

                            let _ = window_handle.update(async_window_cx, |launcher, window, cx| {
                                if launcher
                                    .registered_hotkeys
                                    .launcher
                                    .as_ref()
                                    .is_some_and(|hotkey| hotkey.id == event.id)
                                {
                                    last_toggle_at = Some(now);
                                    if launcher.is_launcher_visible {
                                        launcher.hide_launcher(window, cx);
                                    } else {
                                        launcher.show_launcher(window, cx);
                                    }
                                    return;
                                }

                                if let Some(command_hotkey) = launcher
                                    .registered_hotkeys
                                    .command_hotkeys
                                    .iter()
                                    .find(|hotkey| hotkey.id == event.id)
                                    .cloned()
                                {
                                    last_toggle_at = Some(now);
                                    let target_window_handle =
                                        window_management::active_window_handle();
                                    launcher.accept_hotkey_query(
                                        &command_hotkey.query,
                                        target_window_handle,
                                        window,
                                        cx,
                                    );
                                }
                            });
                        }
                        HotKeyState::Released => {
                            pressed_hotkeys.remove(&event.id);
                        }
                    }
                }
            }
        })
        .detach();
}

#[cfg(not(target_os = "windows"))]
fn start_global_hotkey_poll(
    _window_handle: WindowHandle<LauncherView>,
    _window: &mut Window,
    _cx: &mut Context<LauncherView>,
) {
}



#[cfg(target_os = "windows")]
fn get_primary_work_area() -> (i32, i32, i32, i32) {
    use windows::Win32::{
        Foundation::POINT,
        Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY},
    };

    let monitor = unsafe {
        MonitorFromPoint(
            POINT { x: 0, y: 0 },
            MONITOR_DEFAULTTOPRIMARY,
        )
    };
    if monitor.0.is_null() {
        return fallback();
    }

    let mut monitor_info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let succeeded = unsafe { GetMonitorInfoW(monitor, &mut monitor_info).as_bool() };
    if !succeeded {
        return fallback();
    }

    let rect = monitor_info.rcWork;
    (rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top)
}

#[cfg(target_os = "windows")]
fn fallback() -> (i32, i32, i32, i32) {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    let width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    (0, 0, width.max(1280), height.max(800))
}

#[cfg(not(target_os = "windows"))]
fn get_primary_work_area() -> (i32, i32, i32, i32) {
    (0, 0, 1280, 800)
}

#[cfg(target_os = "windows")]
fn set_launcher_window_position(window: &Window, left: i32, top: i32) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOREDRAW, SWP_NOSIZE, SWP_NOZORDER},
    };

    if let Ok(window_handle) = HasWindowHandle::window_handle(window) {
        if let RawWindowHandle::Win32(win32_window_handle) = window_handle.as_raw() {
            unsafe {
                let hwnd = HWND(win32_window_handle.hwnd.get() as *mut std::ffi::c_void);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    left,
                    top,
                    0,
                    0,
                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOREDRAW,
                );
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn set_launcher_window_position(_window: &Window, _left: i32, _top: i32) {}

#[cfg(target_os = "windows")]
fn set_dwm_corners(window: &Window, _display_position: crate::settings::DisplayPosition) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::{
        Foundation::HWND,
        Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND},
    };

    if let Ok(window_handle) = HasWindowHandle::window_handle(window) {
        if let RawWindowHandle::Win32(win32_window_handle) = window_handle.as_raw() {
            unsafe {
                let preference = DWMWCP_ROUND.0 as i32;
                let _ = DwmSetWindowAttribute(
                    HWND(win32_window_handle.hwnd.get() as *mut std::ffi::c_void),
                    DWMWA_WINDOW_CORNER_PREFERENCE,
                    &preference as *const i32 as *const std::ffi::c_void,
                    std::mem::size_of::<i32>() as u32,
                );
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn set_dwm_corners(_window: &Window, _display_position: crate::settings::DisplayPosition) {}

