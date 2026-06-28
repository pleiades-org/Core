use crate::{
    action_executor::execute_result_action,
    command::{BuiltInAction, CommandResult},
    quicklinks,
    settings::settings_file_path,
    snippets,
    startup::set_launch_at_startup,
    ui::{
        browse_views::{
            border_subtle, browse_action_bar, browse_action_hint, settings_row_background,
            settings_row_hover_background, settings_row_selected_background,
            settings_sidebar_background,
        },
        lucide_icons::{self, LucideIcon},
    },
    ui_flow::{track_open_settings, track_save_settings},
};
use gpui::{div, prelude::*, px, rgb, App, Context, MouseButton, MouseUpEvent, Window};
use super::{
    LauncherSettings, LauncherView, RegisteredHotkeys, SettingsEditorRow, SettingsSection,
    SETTINGS_INPUT_PLACEHOLDER, SETTINGS_PANEL_HEIGHT, SETTINGS_SIDEBAR_WIDTH,
    window_background_appearance,
};

pub(super) const SETTINGS_FOOTER_HEIGHT: f32 = 36.;
use super::result_list::compact_display_text;

fn registered_command_hotkey_summary(registered_hotkeys: &RegisteredHotkeys) -> String {
    match registered_hotkeys.command_hotkeys.as_slice() {
        [] => "0 registered".to_string(),
        [hotkey] => format!("1 registered: {}", hotkey.display_text),
        [first_hotkey, ..] => format!(
            "{} registered, first {}",
            registered_hotkeys.command_hotkeys.len(),
            first_hotkey.display_text
        ),
    }
}

fn setting_toggle_row(
    title: &'static str,
    subtitle: &str,
    is_enabled: bool,
    on_click: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .rounded_sm()
        .bg(settings_row_background())
        .px(px(12.))
        .py(px(10.))
        .hover(|style| style.bg(settings_row_hover_background()).cursor_pointer())
        .on_mouse_up(MouseButton::Left, on_click)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_color(rgb(0xffffff))
                        .text_size(px(14.))
                        .child(title),
                )
                .child(
                    div()
                        .text_color(rgb(0xd9d9d9))
                        .text_size(px(12.))
                        .child(subtitle.to_string()),
                ),
        )
        .child(toggle_pill(is_enabled))
}

fn settings_info_row(title: &'static str, value: &str) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .rounded_sm()
        .bg(settings_row_background())
        .px(px(12.))
        .py(px(10.))
        .hover(|style| style.bg(settings_row_hover_background()))
        .child(
            div()
                .text_color(rgb(0xffffff))
                .text_size(px(14.))
                .child(title),
        )
        .child(
            div()
                .text_color(rgb(0xd9d9d9))
                .text_size(px(12.))
                .child(value.to_string()),
        )
}

fn settings_hotkey_rows(settings: &LauncherSettings) -> Vec<SettingsEditorRow> {
    let mut rows = vec![SettingsEditorRow {
        title: "Launcher".to_string(),
        subtitle: if settings.hotkey_enabled {
            settings.hotkey.clone()
        } else {
            "Disabled".to_string()
        },
    }];

    rows.extend(
        settings
            .hotkeys
            .iter()
            .take(4)
            .map(|hotkey| SettingsEditorRow {
                title: hotkey.hotkey.clone(),
                subtitle: hotkey.query.clone(),
            }),
    );
    rows.extend(
        settings
            .custom_commands
            .iter()
            .filter_map(|custom_command| {
                Some(SettingsEditorRow {
                    title: custom_command.hotkey.clone()?,
                    subtitle: format!("@custom {}", custom_command.name),
                })
            })
            .take(4),
    );
    rows
}

fn settings_editor_section(
    title: &'static str,
    empty_text: &'static str,
    rows: Vec<SettingsEditorRow>,
) -> gpui::Div {
    let visible_rows = if rows.is_empty() {
        vec![SettingsEditorRow {
            title: "No entries yet".to_string(),
            subtitle: empty_text.to_string(),
        }]
    } else {
        rows
    };

    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .rounded_sm()
        .bg(rgb(0x050505))
        .border_1()
        .border_color(rgb(0x171717))
        .px(px(10.))
        .py(px(9.))
        .child(
            div()
                .text_size(px(12.))
                .text_color(rgb(0xf4f4f5))
                .child(title),
        )
        .children(visible_rows.into_iter().map(settings_editor_row))
}

fn settings_add_form(title: &'static str, form_body: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .rounded_sm()
        .bg(rgb(0x050505))
        .border_1()
        .border_color(rgb(0x171717))
        .px(px(10.))
        .py(px(9.))
        .child(
            div()
                .text_size(px(12.))
                .text_color(rgb(0xf4f4f5))
                .child(title),
        )
        .child(form_body)
}

fn settings_editor_row(row: SettingsEditorRow) -> gpui::Div {
    div()
        .flex()
        .justify_between()
        .items_center()
        .gap(px(12.))
        .rounded_sm()
        .bg(settings_row_background())
        .px(px(9.))
        .py(px(7.))
        .hover(|style| style.bg(settings_row_hover_background()).cursor_pointer())
        .child(
            div()
                .text_size(px(12.))
                .text_color(rgb(0xffffff))
                .child(row.title),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(rgb(0xa1a1aa))
                .child(compact_settings_value(&row.subtitle)),
        )
}

fn compact_settings_value(value: &str) -> String {
    compact_display_text(value, 72)
}

fn toggle_pill(is_enabled: bool) -> impl IntoElement {
    div()
        .w(px(42.))
        .h(px(22.))
        .rounded_full()
        .bg(if is_enabled {
            rgb(0xffffff)
        } else {
            rgb(0x171717)
        })
        .flex()
        .items_center()
        .justify_end()
        .when(!is_enabled, |toggle| toggle.justify_start())
        .px(px(3.))
        .child(div().size(px(14.)).rounded_full().bg(if is_enabled {
            rgb(0x000000)
        } else {
            rgb(0xd9d9d9)
        }))
}

fn settings_section_intro(title: &'static str, subtitle: &'static str) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .pb(px(8.))
        .child(
            div()
                .text_size(px(16.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(0xf4f4f5))
                .child(title),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(rgb(0xa1a1aa))
                .child(subtitle),
        )
}

fn settings_footer_icon_button(
    icon: LucideIcon,
    icon_color: gpui::Rgba,
) -> gpui::Div {
    div()
        .size(px(26.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .bg(gpui::rgba(0x00000000))
        .hover(|style| style.bg(gpui::rgba(0xffffff12)).cursor_pointer())
        .child(lucide_icons::render_lucide_icon(icon, 13., icon_color, false))
}


impl LauncherView {
    pub(super) fn enter_settings_mode(&mut self, cx: &mut Context<Self>) {
        track_open_settings();
        self.is_settings_open = true;
        self.settings_search_query.clear();
        self.text_input.update(cx, |text_input, cx| {
            text_input.set_placeholder(SETTINGS_INPUT_PLACEHOLDER, cx);
            text_input.reset(cx);
        });
        cx.notify();
    }

    pub(super) fn open_settings_menu(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.enter_settings_mode(cx);
    }

    pub(super) fn select_settings_section(
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
            .filter(|section| {
                query.is_empty() || section.label().to_lowercase().contains(&query)
            })
            .collect()
    }

    fn settings_row_matches_search(&self, title: &str, subtitle: &str) -> bool {
        let query = self.settings_search_query.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }
        title.to_lowercase().contains(&query) || subtitle.to_lowercase().contains(&query)
    }

    pub(super) fn close_settings_menu(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.return_home(cx);
    }

    pub(super) fn quit_from_settings(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.quit();
    }

    pub(super) fn open_settings_file(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let settings_result = CommandResult::built_in(
            "Open settings file",
            settings_file_path().display().to_string(),
            BuiltInAction::OpenSettings,
            100,
        );

        let _ = execute_result_action(&settings_result);
        cx.notify();
    }

    pub(super) fn reload_applications(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reload_application_index_from_settings();
        let query = self.text_input.read(cx).content().to_string();
        self.rebuild_results(&query);
        cx.notify();
    }

    pub(super) fn toggle_hotkey_enabled(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.hotkey_enabled = !self.settings.hotkey_enabled;
        self.save_settings();
        cx.notify();
    }

    pub(super) fn toggle_launch_at_startup(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.launch_at_startup = !self.settings.launch_at_startup;
        let _ = set_launch_at_startup(self.settings.launch_at_startup);
        self.save_settings();
        cx.notify();
    }

    pub(super) fn toggle_app_indexing(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.index_start_menu = !self.settings.index_start_menu;
        self.reload_application_index_from_settings();
        let query = self.text_input.read(cx).content().to_string();
        self.rebuild_results(&query);
        self.save_settings();
        cx.notify();
    }

    pub(super) fn toggle_file_indexing(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.index_user_files = !self.settings.index_user_files;
        self.reload_file_index_from_settings();
        let query = self.text_input.read(cx).content().to_string();
        self.rebuild_results(&query);
        self.save_settings();
        cx.notify();
    }

    pub(super) fn toggle_web_search(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.show_web_search_result = !self.settings.show_web_search_result;
        self.services.router_mut().update_settings(self.settings.clone());
        let query = self.text_input.read(cx).content().to_string();
        self.rebuild_results(&query);
        self.save_settings();
        cx.notify();
    }

    pub(super) fn toggle_backdrop_blur(
        &mut self,
        _: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.backdrop_blur_enabled = !self.settings.backdrop_blur_enabled;
        window.set_background_appearance(window_background_appearance(&self.settings));
        self.save_settings();
        cx.notify();
    }

    pub(super) fn toggle_clipboard_history(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.clipboard_history_enabled = !self.settings.clipboard_history_enabled;
        self.save_settings();
        cx.notify();
    }

    pub(super) fn save_settings(&mut self) {
        track_save_settings();
        self.services.router_mut().update_settings(self.settings.clone());
        let _ = self.settings.save();
    }

    fn render_settings_editor_section(
        &self,
        title: &'static str,
        empty_text: &'static str,
        rows: Vec<SettingsEditorRow>,
    ) -> gpui::AnyElement {
        settings_editor_section(title, empty_text, rows).into_any_element()
    }

    pub(super) fn render_settings_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let sections = self.filtered_settings_sections();
        div()
            .id("settings-sidebar")
            .flex()
            .flex_col()
            .w(px(SETTINGS_SIDEBAR_WIDTH))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .pb(px(SETTINGS_FOOTER_HEIGHT))
            .px(px(8.))
            .py(px(10.))
            .gap(px(4.))
            .bg(settings_sidebar_background())
            .border_r_1()
            .border_color(border_subtle())
            .children(sections.into_iter().map(|section| {
                let is_selected = self.settings_section == section;
                div()
                    .id(section.label())
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .py(px(8.))
                    .rounded_md()
                    .cursor_pointer()
                    .text_size(px(13.))
                    .text_color(if is_selected {
                        rgb(0xffffff)
                    } else {
                        rgb(0xa1a1aa)
                    })
                    .bg(if is_selected {
                        settings_row_selected_background()
                    } else {
                        settings_row_background()
                    })
                    .hover(|style| style.bg(settings_row_selected_background()).cursor_pointer())
                    .child(lucide_icons::render_lucide_icon(
                        section.icon(),
                        14.,
                        if is_selected {
                            rgb(0xf4f4f5)
                        } else {
                            rgb(0xa1a1aa)
                        },
                        false,
                    ))
                    .child(section.label())
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |launcher, event, window, cx| {
                            launcher.select_settings_section(section, event, window, cx);
                        }),
                    )
            }))
    }

    pub(super) fn render_settings_section_content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let command_hotkey_summary = registered_command_hotkey_summary(&self.registered_hotkeys);
        let indexed_apps = format!(
            "{} indexed",
            self.services.router().indexed_application_count()
        );
        let indexed_files = format!("{} indexed", self.services.router().indexed_file_count());
        let aliases_count = format!("{} configured", self.settings.aliases.len());
        let custom_count = format!("{} configured", self.settings.custom_commands.len());
        let config_path = settings_file_path().display().to_string();
        let hotkey_rows = settings_hotkey_rows(&self.settings);
        let alias_rows = self
            .settings
            .aliases
            .iter()
            .take(8)
            .map(|alias| SettingsEditorRow {
                title: alias.keyword.clone(),
                subtitle: format!("Expands to {}", alias.expands_to),
            })
            .collect::<Vec<_>>();
        let custom_command_rows = self
            .settings
            .custom_commands
            .iter()
            .take(8)
            .map(|custom_command| SettingsEditorRow {
                title: custom_command.name.clone(),
                subtitle: custom_command.command.clone(),
            })
            .collect::<Vec<_>>();
        let quicklink_rows = quicklinks::configured_quicklinks()
            .into_iter()
            .take(8)
            .map(|quicklink| SettingsEditorRow {
                title: format!(">{}", quicklink.keyword),
                subtitle: quicklink.target,
            })
            .collect::<Vec<_>>();
        let snippet_rows = snippets::configured_snippets()
            .into_iter()
            .take(8)
            .map(|snippet| SettingsEditorRow {
                title: format!(";{}", snippet.keyword),
                subtitle: snippet.title,
            })
            .collect::<Vec<_>>();

        match self.settings_section {
            SettingsSection::General => div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(settings_section_intro(
                    "General",
                    "Launcher behavior, startup, and appearance",
                ))
                .when(
                    self.settings_row_matches_search("Global hotkey", &self.settings.hotkey),
                    |section| {
                        section.child(setting_toggle_row(
                            "Global hotkey",
                            &self.settings.hotkey,
                            self.settings.hotkey_enabled,
                            cx.listener(Self::toggle_hotkey_enabled),
                        ))
                    },
                )
                .when(
                    self.settings_row_matches_search("Launch at startup", "windows signs in"),
                    |section| {
                        section.child(setting_toggle_row(
                            "Launch at startup",
                            "Start Core Launcher when Windows signs in",
                            self.settings.launch_at_startup,
                            cx.listener(Self::toggle_launch_at_startup),
                        ))
                    },
                )
                .when(
                    self.settings_row_matches_search("Web fallback", "search-the-web"),
                    |section| {
                        section.child(setting_toggle_row(
                            "Web fallback",
                            "Show search-the-web result",
                            self.settings.show_web_search_result,
                            cx.listener(Self::toggle_web_search),
                        ))
                    },
                )
                .when(
                    self.settings_row_matches_search("Backdrop blur", "blurred window"),
                    |section| {
                        section.child(setting_toggle_row(
                            "Backdrop blur",
                            "Use the OS blurred window backdrop",
                            self.settings.backdrop_blur_enabled,
                            cx.listener(Self::toggle_backdrop_blur),
                        ))
                    },
                )
                .when(
                    self.settings_row_matches_search("Clipboard history", "clipboard text"),
                    |section| {
                        section.child(setting_toggle_row(
                            "Clipboard history",
                            "Store searchable local clipboard text",
                            self.settings.clipboard_history_enabled,
                            cx.listener(Self::toggle_clipboard_history),
                        ))
                    },
                ),
            SettingsSection::Indexing => div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(settings_section_intro(
                    "Indexing",
                    "Control what Core indexes for search",
                ))
                .when(
                    self.settings_row_matches_search("Start Menu apps", &indexed_apps),
                    |section| {
                        section.child(setting_toggle_row(
                            "Start Menu apps",
                            &indexed_apps,
                            self.settings.index_start_menu,
                            cx.listener(Self::toggle_app_indexing),
                        ))
                    },
                )
                .when(
                    self.settings_row_matches_search("User files", &indexed_files),
                    |section| {
                        section.child(setting_toggle_row(
                            "User files",
                            &indexed_files,
                            self.settings.index_user_files,
                            cx.listener(Self::toggle_file_indexing),
                        ))
                    },
                ),
            SettingsSection::Hotkeys => div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(settings_section_intro(
                    "Hotkeys",
                    "Global launcher hotkey and command shortcuts",
                ))
                .child(self.render_settings_editor_section(
                    "Hotkeys",
                    "Edit in config.toml or add [[hotkeys]] entries",
                    hotkey_rows,
                )),
            SettingsSection::Aliases => div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(settings_section_intro(
                    "Aliases",
                    "First-word query expansions",
                ))
                .child(settings_info_row("Configured", &aliases_count))
                .child(
                    settings_add_form(
                        "Add Alias",
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .w(px(120.))
                                    .child(self.alias_keyword_input.clone())
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .child(self.alias_expands_to_input.clone())
                            )
                            .child(
                                div()
                                    .px(px(10.))
                                    .py(px(5.))
                                    .rounded_sm()
                                    .bg(rgb(0x7c3aed))
                                    .text_color(rgb(0xffffff))
                                    .text_size(px(12.))
                                    .hover(|style| style.bg(rgb(0x6d28d9)).cursor_pointer())
                                    .child("Add")
                                    .on_mouse_up(MouseButton::Left, cx.listener(|this, _, window, cx| {
                                        this.add_alias_from_settings(cx);
                                    }))
                            )
                    )
                )
                .child(self.render_settings_editor_section(
                    "Aliases",
                    "Add aliases in config.toml",
                    alias_rows,
                )),
            SettingsSection::CustomCommands => div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(settings_section_intro(
                    "Commands",
                    "Shell commands with aliases and optional hotkeys",
                ))
                .child(settings_info_row("Configured", &custom_count))
                .child(self.render_settings_editor_section(
                    "Custom commands",
                    "Add [[custom_commands]] entries in config.toml",
                    custom_command_rows,
                )),
            SettingsSection::Quicklinks => div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(settings_section_intro(
                    "Quicklinks",
                    "Keyword shortcuts to URLs and paths",
                ))
                .child(
                    settings_add_form(
                        "Add Quicklink",
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .w(px(120.))
                                    .child(self.quicklink_keyword_input.clone())
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .child(self.quicklink_target_input.clone())
                            )
                            .child(
                                div()
                                    .px(px(10.))
                                    .py(px(5.))
                                    .rounded_sm()
                                    .bg(rgb(0x7c3aed))
                                    .text_color(rgb(0xffffff))
                                    .text_size(px(12.))
                                    .hover(|style| style.bg(rgb(0x6d28d9)).cursor_pointer())
                                    .child("Add")
                                    .on_mouse_up(MouseButton::Left, cx.listener(|this, _, window, cx| {
                                        this.add_quicklink_from_settings(cx);
                                    }))
                            )
                    )
                )
                .child(self.render_settings_editor_section(
                    "Quicklinks",
                    "Use @quicklink keyword = url-or-path",
                    quicklink_rows,
                )),
            SettingsSection::Snippets => div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(settings_section_intro(
                    "Snippets",
                    "Reusable text expansions",
                ))
                .child(
                    settings_add_form(
                        "Add Snippet",
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .w(px(120.))
                                    .child(self.snippet_keyword_input.clone())
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .child(self.snippet_body_input.clone())
                            )
                            .child(
                                div()
                                    .px(px(10.))
                                    .py(px(5.))
                                    .rounded_sm()
                                    .bg(rgb(0x7c3aed))
                                    .text_color(rgb(0xffffff))
                                    .text_size(px(12.))
                                    .hover(|style| style.bg(rgb(0x6d28d9)).cursor_pointer())
                                    .child("Add")
                                    .on_mouse_up(MouseButton::Left, cx.listener(|this, _, window, cx| {
                                        this.add_snippet_from_settings(cx);
                                    }))
                            )
                    )
                )
                .child(self.render_settings_editor_section(
                    "Snippets",
                    "Use @snippet keyword = reusable text",
                    snippet_rows,
                )),
            SettingsSection::Advanced => div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(settings_section_intro(
                    "Advanced",
                    "Diagnostics, config files, and maintenance",
                ))
                .child(settings_info_row("Command hotkeys", &command_hotkey_summary))
                .child(settings_info_row(
                    "Local timezone",
                    &self.settings.local_timezone,
                ))
                .child(settings_info_row("Config file", &config_path)),
        }
    }

    pub(super) fn render_settings_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let primary_actions = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                settings_footer_icon_button(LucideIcon::X, rgb(0xd4d4d8))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::close_settings_menu)),
            )
            .child(
                settings_footer_icon_button(LucideIcon::FileCog, rgb(0xd4d4d8))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::open_settings_file)),
            )
            .child(
                settings_footer_icon_button(LucideIcon::RefreshCw, rgb(0xd4d4d8))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::reload_applications)),
            )
            .child(
                settings_footer_icon_button(LucideIcon::Power, rgb(0xfca5a5))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::quit_from_settings)),
            );

        browse_action_bar(
            primary_actions.into_any_element(),
            vec![browse_action_hint("Esc", "Close settings").into_any_element()],
        )
    }

    pub(super) fn render_settings_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("settings-menu")
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .max_h(px(SETTINGS_PANEL_HEIGHT))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h(px(0.))
                    .pb(px(SETTINGS_FOOTER_HEIGHT))
                    .child(self.render_settings_sidebar(cx))
                    .child(
                        div()
                            .id("settings-content-scroll")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .px(px(14.))
                            .pt(px(12.))
                            .pb(px(8.))
                            .child(self.render_settings_section_content(cx)),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .w_full()
                    .child(self.render_settings_footer(cx)),
            )
    }

    pub(super) fn add_quicklink_from_settings(&mut self, cx: &mut Context<Self>) {
        let keyword = self.quicklink_keyword_input.read(cx).content().to_string();
        let target = self.quicklink_target_input.read(cx).content().to_string();
        if keyword.trim().is_empty() || target.trim().is_empty() {
            return;
        }

        let title = format!("Quicklink to {}", keyword);
        if let Ok(_) = quicklinks::save_quicklink(keyword, title, target) {
            self.quicklink_keyword_input.update(cx, |input, cx| input.reset(cx));
            self.quicklink_target_input.update(cx, |input, cx| input.reset(cx));
            cx.notify();
        }
    }

    pub(super) fn add_alias_from_settings(&mut self, cx: &mut Context<Self>) {
        let keyword = self.alias_keyword_input.read(cx).content().to_string();
        let expands_to = self.alias_expands_to_input.read(cx).content().to_string();
        if keyword.trim().is_empty() || expands_to.trim().is_empty() {
            return;
        }

        let added = self.settings.upsert_alias(
            crate::settings::CommandAliasSetting {
                keyword,
                expands_to,
            },
            None,
        );
        if added {
            self.save_settings();
            self.alias_keyword_input.update(cx, |input, cx| input.reset(cx));
            self.alias_expands_to_input.update(cx, |input, cx| input.reset(cx));
            cx.notify();
        }
    }

    pub(super) fn add_snippet_from_settings(&mut self, cx: &mut Context<Self>) {
        let keyword = self.snippet_keyword_input.read(cx).content().to_string();
        let body = self.snippet_body_input.read(cx).content().to_string();
        if keyword.trim().is_empty() || body.trim().is_empty() {
            return;
        }

        let title = format!("Snippet signature {}", keyword);
        if let Ok(_) = snippets::save_snippet(keyword, title, body) {
            self.snippet_keyword_input.update(cx, |input, cx| input.reset(cx));
            self.snippet_body_input.update(cx, |input, cx| input.reset(cx));
            cx.notify();
        }
    }
}
