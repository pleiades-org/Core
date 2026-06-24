#!/usr/bin/env python3
"""Apply UX feature integrations to gpui_app.rs."""
from pathlib import Path

ROOT = Path(__file__).resolve().parent
TARGET = ROOT / "src" / "ui" / "launcher" / "mod.rs"
text = TARGET.read_text(encoding="utf-8")

replacements = [
    (
        """    command::{BuiltInAction, CommandAction, CommandCategory, CommandResult},
    command_router::{file_search_scope_from_query, CommandRouter},
    custom_commands,
    file_index::FileIndex,
    focus, quicklinks,""",
        """    command::{BuiltInAction, CommandAction, CommandCategory, CommandResult, FeatureAction},
    command_router::{file_search_scope_from_query, CommandRouter},
    custom_commands, destiny,
    file_index::FileIndex,
    focus, quicklinks, recent_usage,""",
    ),
    (
        "    ui::text_input::{bind_text_input_keys, TextInput},",
        """    ui::{
        browse_views::{browse_empty_state, border_subtle},
        lucide_icons::{self, LucideIcon},
        text_input::{bind_text_input_keys, TextInput},
    },""",
    ),
    (
        """enum LauncherPanel {
    Home,
    TerminalShellPicker { command_text: String },
    TerminalSession(TerminalSession),
}""",
        """enum LauncherPanel {
    Home,
    TerminalShellPicker { command_text: String },
    TerminalSession(TerminalSession),
    D2WeaponDetail { weapon_hash: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum SettingsSection {
    #[default]
    General,
    Indexing,
    Hotkeys,
    Notes,
    Aliases,
    CustomCommands,
    Quicklinks,
    Snippets,
    Advanced,
}

impl SettingsSection {
    fn label(self) -> &'static str {
        match self {
            SettingsSection::General => "General",
            SettingsSection::Indexing => "Indexing",
            SettingsSection::Hotkeys => "Hotkeys",
            SettingsSection::Notes => "Notes",
            SettingsSection::Aliases => "Aliases",
            SettingsSection::CustomCommands => "Commands",
            SettingsSection::Quicklinks => "Quicklinks",
            SettingsSection::Snippets => "Snippets",
            SettingsSection::Advanced => "Advanced",
        }
    }

    fn icon(self) -> LucideIcon {
        match self {
            SettingsSection::General => LucideIcon::Settings,
            SettingsSection::Indexing => LucideIcon::Search,
            SettingsSection::Hotkeys => LucideIcon::Keyboard,
            SettingsSection::Notes => LucideIcon::StickyNote,
            SettingsSection::Aliases => LucideIcon::TextQuote,
            SettingsSection::CustomCommands => LucideIcon::Terminal,
            SettingsSection::Quicklinks => LucideIcon::Link,
            SettingsSection::Snippets => LucideIcon::StickyNote,
            SettingsSection::Advanced => LucideIcon::FileCog,
        }
    }

    fn all() -> &'static [SettingsSection] {
        &[
            SettingsSection::General,
            SettingsSection::Indexing,
            SettingsSection::Hotkeys,
            SettingsSection::Notes,
            SettingsSection::Aliases,
            SettingsSection::CustomCommands,
            SettingsSection::Quicklinks,
            SettingsSection::Snippets,
            SettingsSection::Advanced,
        ]
    }
}""",
    ),
    (
        """        QuitApplication,
    ]
);""",
        """        QuitApplication,
        StartD2WeaponCompare,
        ClearD2WeaponCompare,
    ]
);""",
    ),
    (
        """pub struct LauncherView {
    text_input: Entity<TextInput>,
    focus_handle: FocusHandle,
    router: CommandRouter,
    settings: LauncherSettings,
    available_shells: Vec<ShellProfile>,
    panel: LauncherPanel,
    results: Vec<CommandResult>,
    recent_results: Vec<CommandResult>,
    selected_index: usize,
    is_file_search_view: bool,
    registered_hotkeys: RegisteredHotkeys,
    is_settings_open: bool,
    is_launcher_visible: bool,
    last_visibility_change_at: Instant,
    last_recorded_clipboard_text: Option<String>,
    last_external_foreground_window: Option<isize>,
}""",
        """pub struct LauncherView {
    text_input: Entity<TextInput>,
    settings_search: Entity<TextInput>,
    focus_handle: FocusHandle,
    router: CommandRouter,
    settings: LauncherSettings,
    available_shells: Vec<ShellProfile>,
    panel: LauncherPanel,
    results: Vec<CommandResult>,
    selected_index: usize,
    is_file_search_view: bool,
    registered_hotkeys: RegisteredHotkeys,
    is_settings_open: bool,
    settings_section: SettingsSection,
    settings_search_query: String,
    compare_weapon_hash: Option<u32>,
    d2_compare_picking: bool,
    d2_compare_primary_hash: Option<u32>,
    last_manifest_poll: Instant,
    is_launcher_visible: bool,
    last_visibility_change_at: Instant,
    last_recorded_clipboard_text: Option<String>,
    last_external_foreground_window: Option<isize>,
}""",
    ),
]

for old, new in replacements:
    if old not in text:
        raise SystemExit(f"Missing expected block:\n{old[:120]}...")
    text = text.replace(old, new, 1)

# new()
old_new = """        let router = CommandRouter::new(settings.clone(), application_index, file_index);
        let results = router.search("");
        let available_shells = ordered_shell_profiles(
            detect_shell_profiles(),
            settings.preferred_terminal_profile.as_deref(),
        );

        Self {
            text_input,
            focus_handle: cx.focus_handle(),
            router,
            settings,
            available_shells,
            panel: LauncherPanel::Home,
            results,
            recent_results: Vec::new(),
            selected_index: 0,
            is_file_search_view: false,
            registered_hotkeys,
            is_settings_open: false,
            is_launcher_visible: true,
            last_visibility_change_at: Instant::now(),
            last_recorded_clipboard_text: None,
            last_external_foreground_window: None,
        }"""

new_new = """        let settings_search = cx.new(|cx| TextInput::new_compact("Search settings...", cx));
        cx.observe(&settings_search, |launcher, settings_search, cx| {
            launcher.settings_search_query = settings_search.read(cx).content().to_string();
            cx.notify();
        })
        .detach();

        let router = CommandRouter::new(settings.clone(), application_index, file_index);
        let results = recent_usage::home_results(MAX_RECENT_RESULTS);
        let available_shells = ordered_shell_profiles(
            detect_shell_profiles(),
            settings.preferred_terminal_profile.as_deref(),
        );

        Self {
            text_input,
            settings_search,
            focus_handle: cx.focus_handle(),
            router,
            settings,
            available_shells,
            panel: LauncherPanel::Home,
            results,
            selected_index: 0,
            is_file_search_view: false,
            registered_hotkeys,
            is_settings_open: false,
            settings_section: SettingsSection::default(),
            settings_search_query: String::new(),
            compare_weapon_hash: None,
            d2_compare_picking: false,
            d2_compare_primary_hash: None,
            last_manifest_poll: Instant::now(),
            is_launcher_visible: true,
            last_visibility_change_at: Instant::now(),
            last_recorded_clipboard_text: None,
            last_external_foreground_window: None,
        }"""

if old_new not in text:
    raise SystemExit("new() block missing")
text = text.replace(old_new, new_new, 1)

more = [
    (
        "        if matches!(&self.panel, LauncherPanel::TerminalSession(_)) {",
        "        if matches!(\n            &self.panel,\n            LauncherPanel::TerminalSession(_) | LauncherPanel::D2WeaponDetail { .. }\n        ) {",
    ),
    (
        """    fn home_results(&self) -> Vec<CommandResult> {
        if self.recent_results.is_empty() {
            self.router.search("")
        } else {
            self.recent_results.clone()
        }
    }""",
        """    fn home_results(&self) -> Vec<CommandResult> {
        let mut results = recent_usage::home_results(MAX_RECENT_RESULTS);
        if let Some(progress) = destiny::current_manifest_progress() {
            if progress.percent < 1.0 {
                let subtitle = format!(
                    "{} - {:.0}% {}",
                    progress.stage,
                    progress.percent * 100.0,
                    progress.message
                );
                let mut manifest_result =
                    CommandResult::informational("Downloading Destiny Manifest", subtitle);
                manifest_result.category = CommandCategory::Destiny;
                results.insert(0, manifest_result);
            }
        }
        results
    }""",
    ),
    (
        """            LauncherPanel::TerminalSession(_) => self.accept_terminal_input(cx),
        }
    }""",
        """            LauncherPanel::TerminalSession(_) => self.accept_terminal_input(cx),
            LauncherPanel::D2WeaponDetail { .. } => cx.notify(),
        }
    }""",
    ),
    (
        """            CommandAction::BuiltIn(BuiltInAction::Quit) => {
                cx.quit();
            }
            _ => match execute_result_action_with_context(""",
        """            CommandAction::BuiltIn(BuiltInAction::Quit) => {
                cx.quit();
            }
            CommandAction::Feature(FeatureAction::OpenDestinyWeapon { weapon_hash }) => {
                if self.d2_compare_picking {
                    self.compare_weapon_hash = Some(*weapon_hash);
                    self.d2_compare_picking = false;
                    if let Some(primary_hash) = self.d2_compare_primary_hash.take() {
                        self.panel = LauncherPanel::D2WeaponDetail { weapon_hash: primary_hash };
                        self.results.clear();
                        self.text_input.update(cx, |text_input, cx| {
                            text_input.set_placeholder(HOME_INPUT_PLACEHOLDER, cx);
                            text_input.reset(cx);
                        });
                    }
                    cx.notify();
                    return;
                }
                self.panel = LauncherPanel::D2WeaponDetail { weapon_hash: *weapon_hash };
                self.compare_weapon_hash = None;
                self.results.clear();
                self.selected_index = 0;
                destiny::prefetch_weapon_icons(*weapon_hash);
                cx.notify();
                return;
            }
            CommandAction::Feature(FeatureAction::ClearRecentUsage) => {
                let _ = recent_usage::clear_recent_usage();
                if self.text_input.read(cx).content().trim().is_empty() {
                    self.rebuild_results("");
                }
                cx.notify();
                return;
            }
            CommandAction::Feature(FeatureAction::PinRecentUsageItem { item_id }) => {
                let _ = recent_usage::pin_usage_item(item_id);
                if self.text_input.read(cx).content().trim().is_empty() {
                    self.rebuild_results("");
                }
                cx.notify();
                return;
            }
            _ => match execute_result_action_with_context(""",
    ),
    (
        """    fn record_recent_usage(&mut self, selected_result: &CommandResult) {
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
    }""",
        """    fn record_recent_usage(&mut self, selected_result: &CommandResult) {
        let _ = recent_usage::record_usage(selected_result);
    }

    fn start_d2_weapon_compare(&mut self, _: &StartD2WeaponCompare, cx: &mut Context<Self>) {
        let LauncherPanel::D2WeaponDetail { weapon_hash } = self.panel else {
            return;
        };
        self.d2_compare_primary_hash = Some(weapon_hash);
        self.d2_compare_picking = true;
        self.panel = LauncherPanel::Home;
        self.text_input.update(cx, |text_input, cx| {
            text_input.set_content("@d2 ".to_string(), cx);
        });
        self.rebuild_results("@d2 ");
        self.selected_index = 0;
        cx.notify();
    }

    fn clear_d2_weapon_compare(&mut self, _: &ClearD2WeaponCompare, cx: &mut Context<Self>) {
        self.compare_weapon_hash = None;
        self.d2_compare_picking = false;
        self.d2_compare_primary_hash = None;
        cx.notify();
    }

    fn clear_recent_usage_from_settings(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = recent_usage::clear_recent_usage();
        if self.text_input.read(cx).content().trim().is_empty() {
            self.rebuild_results("");
        }
        cx.notify();
    }

    fn select_settings_section(
        &mut self,
        section: SettingsSection,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_section = section;
        cx.notify();
    }

    fn filtered_settings_sections(&self) -> Vec<SettingsSection> {
        let query = self.settings_search_query.trim().to_lowercase();
        SettingsSection::all()
            .iter()
            .copied()
            .filter(|section| query.is_empty() || section.label().to_lowercase().contains(&query))
            .collect()
    }""",
    ),
    (
        """            LauncherPanel::TerminalSession(_) => 0,
        }
    }""",
        """            LauncherPanel::TerminalSession(_) => 0,
            LauncherPanel::D2WeaponDetail { .. } => 0,
        }
    }""",
    ),
    (
        """    fn return_home(&mut self, cx: &mut Context<Self>) {
        self.is_settings_open = false;
        self.panel = LauncherPanel::Home;
        self.is_file_search_view = false;""",
        """    fn return_home(&mut self, cx: &mut Context<Self>) {
        self.is_settings_open = false;
        self.panel = LauncherPanel::Home;
        self.compare_weapon_hash = None;
        self.d2_compare_picking = false;
        self.d2_compare_primary_hash = None;
        self.is_file_search_view = false;""",
    ),
]

for old, new in more:
    if old not in text:
        raise SystemExit(f"Missing block: {old[:80]}...")
    text = text.replace(old, new, 1)

# Render updates
render_old = """impl Render for LauncherView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("Launcher")
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::accept_action))
            .on_action(cx.listener(Self::move_selection_up))
            .on_action(cx.listener(Self::move_selection_down))
            .on_action(cx.listener(Self::move_selection_page_up))
            .on_action(cx.listener(Self::move_selection_page_down))
            .on_action(cx.listener(Self::move_selection_first))
            .on_action(cx.listener(Self::move_selection_last))
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::quit_application))"""

render_new = """impl Render for LauncherView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.last_manifest_poll.elapsed() >= Duration::from_millis(250) {
            self.last_manifest_poll = Instant::now();
            if destiny::current_manifest_progress().is_some()
                && self.text_input.read(cx).content().trim().is_empty()
            {
                self.rebuild_results("");
            }
        }

        div()
            .key_context("Launcher")
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::accept_action))
            .on_action(cx.listener(Self::move_selection_up))
            .on_action(cx.listener(Self::move_selection_down))
            .on_action(cx.listener(Self::move_selection_page_up))
            .on_action(cx.listener(Self::move_selection_page_down))
            .on_action(cx.listener(Self::move_selection_first))
            .on_action(cx.listener(Self::move_selection_last))
            .on_action(cx.listener(Self::start_d2_weapon_compare))
            .on_action(cx.listener(Self::clear_d2_weapon_compare))
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::quit_application))"""

if render_old not in text:
    raise SystemExit("render block missing")
text = text.replace(render_old, render_new, 1)

panel_old = """                let panel = match &self.panel {
                    LauncherPanel::Home => self.render_results(cx).into_any_element(),
                    LauncherPanel::TerminalShellPicker { command_text } => self
                        .render_terminal_shell_picker(command_text, cx)
                        .into_any_element(),
                    LauncherPanel::TerminalSession(session) => {
                        self.render_terminal_session(session).into_any_element()
                    }
                };"""

panel_new = """                let panel = match &self.panel {
                    LauncherPanel::Home => self.render_results(cx).into_any_element(),
                    LauncherPanel::TerminalShellPicker { command_text } => self
                        .render_terminal_shell_picker(command_text, cx)
                        .into_any_element(),
                    LauncherPanel::TerminalSession(session) => {
                        self.render_terminal_session(session).into_any_element()
                    }
                    LauncherPanel::D2WeaponDetail { weapon_hash } => self
                        .render_d2_weapon_detail(*weapon_hash, cx)
                        .into_any_element(),
                };"""

if panel_old not in text:
    raise SystemExit("panel match missing")
text = text.replace(panel_old, panel_new, 1)

settings_menu_old = """    fn render_settings_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let command_hotkey_summary = registered_command_hotkey_summary(&self.registered_hotkeys);

        div()
            .id("settings-menu")
            .flex()
            .flex_col()
            .gap(px(8.))
            .px(px(12.))
            .py(px(10.))
            .h(px(360.))
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .pb(px(4.))
                    .child(
                        div()
                            .text_size(px(18.))
                            .text_color(rgb(0xffffff))
                            .child("Settings"),
                    )
                    .child(
                        settings_button("Close")
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::close_settings_menu)),
                    ),
            )"""

settings_menu_new = """    fn render_settings_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let sections = self.filtered_settings_sections();
        div()
            .flex()
            .flex_col()
            .w(px(168.))
            .flex_none()
            .h_full()
            .px(px(8.))
            .py(px(10.))
            .gap(px(4.))
            .border_r_1()
            .border_color(border_subtle())
            .children(sections.into_iter().map(|section| {
                let is_selected = self.settings_section == section;
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .py(px(8.))
                    .rounded_md()
                    .cursor_pointer()
                    .text_size(px(13.))
                    .text_color(if is_selected { rgb(0xffffff) } else { rgb(0xa1a1aa) })
                    .bg(if is_selected { rgb(0x111111) } else { rgb(0x000000) })
                    .child(lucide_icons::render_lucide_icon(section.icon(), 14., rgb(0xa1a1aa), false))
                    .child(section.label())
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |launcher, event, window, cx| {
                            launcher.select_settings_section(section, event, window, cx);
                        }),
                    )
            }))
    }

    fn render_settings_section_content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        match self.settings_section {
            SettingsSection::General => div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(setting_toggle_row(
                    "Global hotkey",
                    &self.settings.hotkey,
                    self.settings.hotkey_enabled,
                    cx.listener(Self::toggle_hotkey_enabled),
                ))
                .child(setting_toggle_row(
                    "Launch at startup",
                    "Start Core Launcher when Windows signs in",
                    self.settings.launch_at_startup,
                    cx.listener(Self::toggle_launch_at_startup),
                ))
                .child(setting_toggle_row(
                    "Web fallback",
                    "Show search-the-web result",
                    self.settings.show_web_search_result,
                    cx.listener(Self::toggle_web_search),
                ))
                .child(setting_toggle_row(
                    "Backdrop blur",
                    "Use the OS blurred window backdrop",
                    self.settings.backdrop_blur_enabled,
                    cx.listener(Self::toggle_backdrop_blur),
                ))
                .child(setting_toggle_row(
                    "Clipboard history",
                    "Store searchable local clipboard text",
                    self.settings.clipboard_history_enabled,
                    cx.listener(Self::toggle_clipboard_history),
                )),
            SettingsSection::Indexing => div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(setting_toggle_row(
                    "Start Menu apps",
                    &format!("{} indexed", self.router.indexed_application_count()),
                    self.settings.index_start_menu,
                    cx.listener(Self::toggle_app_indexing),
                ))
                .child(setting_toggle_row(
                    "User files",
                    &format!("{} indexed", self.router.indexed_file_count()),
                    self.settings.index_user_files,
                    cx.listener(Self::toggle_file_indexing),
                )),
            SettingsSection::Hotkeys => div().child(self.render_settings_editor_sections()),
            SettingsSection::Notes => div()
                .text_size(px(12.))
                .text_color(rgb(0xa1a1aa))
                .child("Launcher notes use @note. Quick notes open a pinned scratch pad."),
            SettingsSection::Aliases
            | SettingsSection::CustomCommands
            | SettingsSection::Quicklinks
            | SettingsSection::Snippets => div().child(self.render_settings_editor_sections()),
            SettingsSection::Advanced => {
                let command_hotkey_summary = registered_command_hotkey_summary(&self.registered_hotkeys);
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(settings_info_row("Command hotkeys", &command_hotkey_summary))
                    .child(settings_info_row("Local timezone", &self.settings.local_timezone))
                    .child(settings_info_row(
                        "Config file",
                        &settings_file_path().display().to_string(),
                    ))
                    .child(
                        settings_button("Clear recent usage")
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::clear_recent_usage_from_settings)),
                    )
            }
        }
    }

    fn render_settings_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("settings-menu")
            .flex()
            .flex_row()
            .h(px(360.))
            .child(self.render_settings_sidebar(cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .px(px(12.))
                    .py(px(10.))
                    .gap(px(8.))
                    .child(self.settings_search.clone())
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .text_color(rgb(0xffffff))
                                    .child(self.settings_section.label()),
                            )
                            .child(
                                settings_button("Close")
                                    .on_mouse_up(MouseButton::Left, cx.listener(Self::close_settings_menu)),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .overflow_y_scroll()
                            .child(self.render_settings_section_content(cx)),
                    )
            )"""

# Remove duplicate toggle rows from old settings menu - replace from first child after header to before editor sections
if settings_menu_old not in text:
    raise SystemExit("settings menu header missing")

# Find end of old settings menu toggles - replace whole function body start
text = text.replace(settings_menu_old, settings_menu_new, 1)

# Remove duplicate content from old settings menu (toggles through editor sections before buttons)
dup_start = text.find("""            .child(setting_toggle_row(
                "Global hotkey",""")
dup_end = text.find("""            .child(self.render_settings_editor_sections())""")
if dup_start != -1 and dup_end != -1:
    # remove duplicate block between settings_menu_new end and buttons
    buttons_marker = """            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .pt(px(8.))
                    .child(
                        settings_button("Open config")"""
    if buttons_marker in text:
        dup_block_start = text.find("""            .child(setting_toggle_row(
                "Global hotkey",""", dup_start)
        if dup_block_start != -1:
            text = text[:dup_block_start] + "            " + text[text.find(buttons_marker):]

# render_results manifest + empty state
results_empty_old = """            .when(result_count == 0, |container| {
                container.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .h_full()
                        .text_color(rgb(0x9ca3af))
                        .child("No confident result"),
                )
            })
            .children(
                self.results
                    .iter()
                    .enumerate()
                    .map(|(result_index, result)| {
                        let is_selected = result_index == self.selected_index;

                        if matches!(result.category, CommandCategory::Calculation) {
                            self.render_calculation_result(result, result_index, is_selected, cx)
                        } else {
                            self.render_standard_result(result, result_index, is_selected, cx)
                        }
                    }),
            )"""

results_empty_new = """            .when(result_count == 0, |container| {
                container.child(
                    browse_empty_state(
                        Some(LucideIcon::Search),
                        "No confident result",
                        "Try a different query or scope",
                    ),
                )
            })
            .children(
                self.results
                    .iter()
                    .enumerate()
                    .map(|(result_index, result)| {
                        let is_selected = result_index == self.selected_index;
                        if result.category == CommandCategory::Destiny
                            && result.title == "Downloading Destiny Manifest"
                        {
                            self.render_d2_manifest_download(result, is_selected)
                                .into_any_element()
                        } else if matches!(result.category, CommandCategory::Calculation) {
                            self.render_calculation_result(result, result_index, is_selected, cx)
                        } else {
                            self.render_standard_result(result, result_index, is_selected, cx)
                        }
                    }),
            )"""

if results_empty_old not in text:
    raise SystemExit("render_results block missing")
text = text.replace(results_empty_old, results_empty_new, 1)

file_preview_old = """            return div()
                .flex()
                .items_center()
                .justify_center()
                .h_full()
                .w_full()
                .text_color(rgb(0x9ca3af))
                .child("No file selected")
                .into_any_element();"""

file_preview_new = """            return browse_empty_state(
                Some(LucideIcon::FolderOpen),
                "Select a file",
                "Arrow keys move the selection, Enter opens the highlighted file",
            );"""

text = text.replace(file_preview_old, file_preview_new, 1)

# category matches
text = text.replace(
    "        CommandCategory::Emoji => \"=\",",
    "        CommandCategory::Emoji => \":\",\n        CommandCategory::Destiny => \"D2\",\n        CommandCategory::Context => \"Ctx\",",
)
text = text.replace(
    "        CommandCategory::Emoji => rgb(0xfb7185),\n    }",
    "        CommandCategory::Emoji => rgb(0xfb7185),\n        CommandCategory::Destiny => rgb(0x7c3aed),\n        CommandCategory::Context => rgb(0x94a3b8),\n    }",
)
text = text.replace(
    "        CommandCategory::Emoji => \"Emoji\",\n    }",
    "        CommandCategory::Emoji => \"Emoji\",\n        CommandCategory::Destiny => \"D2\",\n        CommandCategory::Context => \"Ctx\",\n    }",
)

# bind_launcher_keys home/end
text = text.replace(
    '        KeyBinding::new("ctrl-home", MoveSelectionFirst, None),',
    '        KeyBinding::new("home", MoveSelectionFirst, None),\n        KeyBinding::new("ctrl-home", MoveSelectionFirst, None),',
)
text = text.replace(
    '        KeyBinding::new("ctrl-end", MoveSelectionLast, None),',
    '        KeyBinding::new("end", MoveSelectionLast, None),\n        KeyBinding::new("ctrl-end", MoveSelectionLast, None),',
)

# tray events
text = text.replace(
    """                    match tray_event {
                        TrayIconEvent::ShowLauncher => {
                            let _ =
                                window_handle.update(async_window_cx, |launcher, window, cx| {
                                    launcher.show_launcher(window, cx);
                                });
                        }
                    }""",
    """                    match tray_event {
                        TrayIconEvent::ShowLauncher => {
                            let _ =
                                window_handle.update(async_window_cx, |launcher, window, cx| {
                                    launcher.show_launcher(window, cx);
                                });
                        }
                        TrayIconEvent::OpenSettings => {
                            let _ =
                                window_handle.update(async_window_cx, |launcher, _window, cx| {
                                    launcher.is_settings_open = true;
                                    cx.notify();
                                });
                        }
                        TrayIconEvent::QuitApplication => {
                            async_window_cx.quit();
                        }
                    }""",
)

# destiny boot in run()
text = text.replace(
    """            let settings = LauncherSettings::load_or_create();
            let _ = set_launch_at_startup(settings.launch_at_startup);""",
    """            let settings = LauncherSettings::load_or_create();
            let _ = set_launch_at_startup(settings.launch_at_startup);
            if settings.bungie_api_key.as_ref().map_or(false, |k| !k.trim().is_empty()) {
                destiny::mark_d2_configured();
                let api_key = settings.bungie_api_key.clone();
                std::thread::spawn(move || {
                    destiny::update_manifest_if_needed(api_key);
                });
            }""",
)

# D2 helper functions before bind_launcher_keys
d2_helpers = '''
    fn render_d2_manifest_download(
        &self,
        result: &CommandResult,
        is_selected: bool,
    ) -> impl IntoElement {
        let progress = destiny::current_manifest_progress().unwrap_or_default();
        let pct = progress.percent.clamp(0.0, 1.0);
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .px(px(10.))
            .py(px(8.))
            .rounded_sm()
            .bg(result_row_background(is_selected))
            .child(div().text_size(px(14.)).child(result.title.clone()))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(0xa1a1aa))
                    .child(format!("{} - {:.0}%", progress.stage, pct * 100.0)),
            )
            .child(
                div()
                    .w(px(340.))
                    .h(px(6.))
                    .bg(rgb(0x27272a))
                    .rounded(px(2.))
                    .child(
                        div()
                            .w(px(340.0 * pct))
                            .h_full()
                            .bg(rgb(0x7c3aed))
                            .rounded(px(2.)),
                    ),
            )
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(rgb(0x71717a))
                    .child(progress.message.clone()),
            )
    }

    fn render_d2_weapon_detail(
        &self,
        weapon_hash: u32,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let compare_hash = self.compare_weapon_hash;
        let (weapon, _) = match destiny::get_weapon_detail(weapon_hash) {
            Some(pair) => pair,
            None => {
                return div()
                    .p(px(16.))
                    .text_color(rgb(0x9ca3af))
                    .child("Weapon not found in cache.")
                    .into_any_element();
            }
        };
        let compare_weapon = compare_hash.and_then(|h| destiny::get_weapon_detail(h).map(|(w, _)| w));

        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .px(px(12.))
            .py(px(10.))
            .h(px(360.))
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(weapon.name.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(rgb(0x71717a))
                            .child(if compare_weapon.is_some() {
                                "C to compare · X to clear compare · Esc back"
                            } else {
                                "C to compare with another weapon · Esc back"
                            }),
                    ),
            )
            .child(render_d2_compare_stats_panel(&weapon, compare_weapon.as_ref()))
            .into_any_element()
    }
'''

insert_before = "    fn render_settings_editor_sections(&self) -> gpui::AnyElement {"
if insert_before not in text:
    raise SystemExit("insert point for d2 helpers missing")
text = text.replace(insert_before, d2_helpers + "\n" + insert_before)

free_helpers = '''
fn render_d2_compare_stats_panel(
    primary: &destiny::DestinyWeapon,
    compare: Option<&destiny::DestinyWeapon>,
) -> gpui::AnyElement {
    let stat_names: Vec<String> = primary
        .stats
        .iter()
        .map(|stat| stat.name.clone())
        .collect();

    let mut panel = div().flex().flex_col().gap(px(6.));
    if stat_names.is_empty() {
        return panel
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(0x71717a))
                    .child("Stats load after the next manifest refresh."),
            )
            .into_any_element();
    }

    if let Some(compare_weapon) = compare {
        panel = panel.child(
            div()
                .flex()
                .gap(px(12.))
                .text_size(px(11.))
                .text_color(rgb(0xa1a1aa))
                .child(primary.name.clone())
                .child("vs")
                .child(compare_weapon.name.clone()),
        );
    }

    for stat_name in stat_names {
        let primary_value = primary
            .stats
            .iter()
            .find(|stat| stat.name == stat_name)
            .map(|stat| stat.value)
            .unwrap_or(0);
        let compare_value = compare
            .and_then(|weapon| weapon.stats.iter().find(|stat| stat.name == stat_name))
            .map(|stat| stat.value);

        panel = panel.child(render_d2_stat_compare_row(
            &stat_name,
            primary_value,
            compare_value,
        ));
    }

    panel.into_any_element()
}

fn render_d2_stat_compare_row(
    name: &str,
    primary_value: i32,
    compare_value: Option<i32>,
) -> gpui::AnyElement {
    const BAR_WIDTH: f32 = 140.;
    let primary_width = px(BAR_WIDTH * (primary_value as f32 / 100.0).clamp(0.0, 1.0));

    let mut row = div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(10.))
                .text_color(rgb(0xd4d4d8))
                .child(name.to_string()),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(render_d2_stat_bar(primary_width, primary_value, rgb(0x7c3aed)))
                .when(compare_value.is_some(), |bar_row| {
                    let compare_value = compare_value.unwrap();
                    let compare_width =
                        px(BAR_WIDTH * (compare_value as f32 / 100.0).clamp(0.0, 1.0));
                    bar_row.child(render_d2_stat_bar(compare_width, compare_value, rgb(0x38bdf8)))
                }),
        );

    row.into_any_element()
}

fn render_d2_stat_bar(width: gpui::Pixels, value: i32, color: gpui::Rgba) -> gpui::AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .w(px(140.))
                .h(px(8.))
                .bg(rgb(0x18181b))
                .rounded(px(2.))
                .child(div().w(width).h_full().bg(color).rounded(px(2.))),
        )
        .child(
            div()
                .text_size(px(10.))
                .text_color(rgb(0xffffff))
                .child(value.to_string()),
        )
        .into_any_element()
}

'''

text = text.replace("fn bind_launcher_keys(cx: &mut App) {", free_helpers + "\nfn bind_launcher_keys(cx: &mut App) {")

# bind compare keys in D2 detail context - add to bind_launcher_keys
text = text.replace(
    '        KeyBinding::new("escape", DismissLauncher, None),',
    '        KeyBinding::new("c", StartD2WeaponCompare, Some("D2WeaponDetail")),\n        KeyBinding::new("x", ClearD2WeaponCompare, Some("D2WeaponDetail")),\n        KeyBinding::new("escape", DismissLauncher, None),',
)

# Add key_context for D2 detail panel in render
text = text.replace(
    """                    LauncherPanel::D2WeaponDetail { weapon_hash } => self
                        .render_d2_weapon_detail(*weapon_hash, cx)
                        .into_any_element(),""",
    """                    LauncherPanel::D2WeaponDetail { weapon_hash } => div()
                        .key_context("D2WeaponDetail")
                        .child(self.render_d2_weapon_detail(*weapon_hash, cx))
                        .into_any_element(),""",
)

TARGET.write_text(text, encoding="utf-8")
print(f"Updated {TARGET}")