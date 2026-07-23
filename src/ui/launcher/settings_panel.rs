use crate::{
    action_executor::execute_result_action,
    command::{CommandAction, CommandCategory, CommandResult},
    quicklinks,
    settings::{
        settings_file_path, CommandAliasSetting, CommandHotkeySetting, CustomCommandSetting,
        DisplayPosition,
    },
    snippets,
    startup::set_launch_at_startup,
    ui::{
        browse_views::{
            border_subtle, browse_action_bar, browse_action_hint, primary_button,
            settings_row_background, settings_row_hover_background,
            settings_row_selected_background, settings_sidebar_background,
        },
        lucide_icons::{self, LucideIcon},
        theme::{self, colors, type_scale},
    },
    ui_flow::{track_open_settings, track_save_settings},
};
use gpui::{div, prelude::*, px, rgb, App, Context, MouseButton, MouseUpEvent, Window};
use super::{
    compact_display_text, LauncherView, RegisteredHotkeys, SettingsSection,
    SETTINGS_INPUT_PLACEHOLDER, SETTINGS_PANEL_HEIGHT, SETTINGS_SIDEBAR_WIDTH,
    window_background_appearance,
};

pub(super) const SETTINGS_FOOTER_HEIGHT: f32 = 36.;

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
                        .text_color(colors::text_primary())
                        .text_size(px(type_scale::BODY_LG))
                        .child(title),
                )
                .child(
                    div()
                        .text_color(colors::text_muted())
                        .text_size(px(type_scale::BODY_SM))
                        .child(subtitle.to_string()),
                ),
        )
        .child(toggle_pill(is_enabled))
}

fn setting_cross_select_row(
    title: &'static str,
    subtitle: &str,
    current_position: DisplayPosition,
    on_select: impl Fn(DisplayPosition, &mut Window, &mut App) + Clone + 'static,
) -> impl IntoElement {
    let cell = move |pos: DisplayPosition, tooltip: &'static str| {
        let is_selected = current_position == pos;
        let on_select = on_select.clone();
        div()
            .id(tooltip)
            .size(px(20.))
            .rounded(px(3.))
            .border_1()
            .border_color(if is_selected {
                colors::accent()
            } else {
                rgb(0x27272a)
            })
            .bg(if is_selected {
                colors::accent()
            } else {
                rgb(0x0a0a0a)
            })
            .hover(|style| {
                if is_selected {
                    style
                } else {
                    style.bg(rgb(0x18181b)).cursor_pointer()
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, window, cx| {
                on_select(pos, window, cx);
            })
    };

    let empty = || div().size(px(20.));

    div()
        .flex()
        .items_center()
        .justify_between()
        .rounded_sm()
        .bg(settings_row_background())
        .px(px(12.))
        .py(px(10.))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_color(colors::text_primary())
                        .text_size(px(type_scale::BODY_LG))
                        .child(title),
                )
                .child(
                    div()
                        .text_color(colors::text_muted())
                        .text_size(px(type_scale::BODY_SM))
                        .child(subtitle.to_string()),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .flex()
                        .gap(px(4.))
                        .child(empty())
                        .child(cell(DisplayPosition::Top, "Top"))
                        .child(empty()),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(4.))
                        .child(cell(DisplayPosition::Left, "Left"))
                        .child(cell(DisplayPosition::Center, "Center"))
                        .child(cell(DisplayPosition::Right, "Right")),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(4.))
                        .child(empty())
                        .child(cell(DisplayPosition::Bottom, "Bottom"))
                        .child(empty()),
                ),
        )
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
                .text_color(colors::text_primary())
                .text_size(px(type_scale::BODY_LG))
                .child(title),
        )
        .child(
            div()
                .text_color(colors::text_muted())
                .text_size(px(type_scale::BODY_SM))
                .child(value.to_string()),
        )
}

fn render_settings_empty_state(message: &str) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .w_full()
        .py(px(20.))
        .px(px(12.))
        .rounded_sm()
        .bg(theme::surface_overlay_low())
        .border_1()
        .border_color(theme::border_muted())
        .child(lucide_icons::render_lucide_icon(
            LucideIcon::CircleQuestionMark,
            16.,
            colors::text_faint(),
            false,
        ))
        .child(
            div()
                .text_size(px(type_scale::LABEL))
                .text_color(colors::text_muted())
                .text_align(gpui::TextAlign::Center)
                .child(message.to_string()),
        )
}

fn settings_card(title: &'static str, body: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .rounded_sm()
        .bg(theme::elevated_surface_background())
        .border_1()
        .border_color(theme::card_border())
        .px(px(10.))
        .py(px(9.))
        .child(
            div()
                .text_size(px(type_scale::BODY_SM))
                .text_color(colors::text_secondary())
                .child(title),
        )
        .child(body)
}

fn settings_list_row(
    title: String,
    subtitle: String,
    on_delete: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
) -> gpui::Div {
    div()
        .flex()
        .justify_between()
        .items_center()
        .gap(px(12.))
        .rounded_sm()
        .bg(settings_row_background())
        .px(px(9.))
        .py(px(7.))
        .hover(|style| style.bg(settings_row_hover_background()))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .min_w(px(0.))
                .flex_1()
                .child(
                    div()
                        .text_size(px(type_scale::BODY_SM))
                        .text_color(colors::text_primary())
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(type_scale::LABEL))
                        .text_color(colors::text_muted())
                        .child(compact_display_text(&subtitle, 72)),
                ),
        )
        .child(
            div()
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .hover(|style| style.bg(theme::surface_overlay_mid()).cursor_pointer())
                .child(lucide_icons::render_lucide_icon(
                    LucideIcon::Trash2,
                    12.,
                    colors::danger(),
                    false,
                ))
                .on_mouse_up(MouseButton::Left, on_delete),
        )
}

fn toggle_pill(is_enabled: bool) -> impl IntoElement {
    div()
        .w(px(42.))
        .h(px(22.))
        .rounded_full()
        .bg(if is_enabled {
            colors::toggle_on_track()
        } else {
            colors::toggle_off_track()
        })
        .flex()
        .items_center()
        .justify_end()
        .when(!is_enabled, |toggle| toggle.justify_start())
        .px(px(3.))
        .child(
            div()
                .size(px(14.))
                .rounded_full()
                .bg(if is_enabled {
                    colors::toggle_on_knob()
                } else {
                    colors::toggle_off_knob()
                }),
        )
}

fn settings_section_intro(title: &'static str, subtitle: &'static str) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .pb(px(8.))
        .child(
            div()
                .text_size(px(type_scale::TITLE))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(colors::text_secondary())
                .child(title),
        )
        .child(
            div()
                .text_size(px(type_scale::BODY_SM))
                .text_color(colors::text_muted())
                .child(subtitle),
        )
}

fn settings_footer_icon_button(icon: LucideIcon, icon_color: gpui::Rgba) -> gpui::Div {
    div()
        .size(px(26.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .bg(gpui::rgba(0x00000000))
        .hover(|style| style.bg(theme::surface_overlay_mid()).cursor_pointer())
        .child(lucide_icons::render_lucide_icon(icon, 13., icon_color, false))
}

fn form_add_row(fields: impl IntoElement, on_add: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(fields)
        .child(
            primary_button("Add").on_mouse_up(MouseButton::Left, on_add),
        )
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
                if query.is_empty() {
                    return true;
                }
                section.label().to_lowercase().contains(&query)
                    || section
                        .keywords()
                        .iter()
                        .any(|keyword| keyword.contains(query.as_str()) || query.contains(*keyword))
            })
            .collect()
    }

    fn settings_row_matches_search(&self, title: &str, subtitle: &str) -> bool {
        let query = self.settings_search_query.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }
        title.to_lowercase().contains(&query)
            || subtitle.to_lowercase().contains(&query)
            || self
                .settings_section
                .keywords()
                .iter()
                .any(|keyword| keyword.contains(query.as_str()))
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
        let path = settings_file_path();
        let settings_result = CommandResult {
            title: "Open settings file".to_string(),
            subtitle: path.display().to_string(),
            copy_text: path.display().to_string(),
            explanation: None,
            icon_path: None,
            calculation_display: None,
            category: CommandCategory::BuiltIn,
            action: CommandAction::OpenPath(path),
            confidence: 100,
        };

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
        self.services
            .router_mut()
            .update_settings(self.settings.clone());
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

    pub(super) fn select_display_position(
        &mut self,
        position: DisplayPosition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.display_position = position;
        self.apply_launcher_window_geometry(window);
        self.save_settings();
        cx.notify();
    }

    pub(super) fn save_settings(&mut self) {
        track_save_settings();
        self.services
            .router_mut()
            .update_settings(self.settings.clone());
        let _ = self.settings.save();
    }

    pub(super) fn render_settings_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let sections = self.filtered_settings_sections();
        div()
            .id("settings-sidebar")
            .flex()
            .flex_col()
            .flex_none()
            .w(px(SETTINGS_SIDEBAR_WIDTH))
            .h_full()
            .px(px(8.))
            .py(px(10.))
            .gap(px(2.))
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
                    .bg(if is_selected {
                        settings_row_selected_background()
                    } else {
                        settings_row_background()
                    })
                    .hover(|style| {
                        style
                            .bg(settings_row_hover_background())
                            .cursor_pointer()
                    })
                    .child(lucide_icons::render_lucide_icon(
                        section.icon(),
                        14.,
                        if is_selected {
                            colors::accent_soft()
                        } else {
                            colors::text_muted()
                        },
                        false,
                    ))
                    .child(
                        div()
                            .text_size(px(type_scale::BODY_SM))
                            .text_color(if is_selected {
                                colors::text_primary()
                            } else {
                                colors::text_muted()
                            })
                            .child(section.label()),
                    )
                    .on_mouse_up(MouseButton::Left, cx.listener(move |launcher, event, window, cx| {
                        launcher.select_settings_section(section, event, window, cx);
                    }))
            }))
    }

    pub(super) fn render_settings_section_content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let command_hotkey_summary = registered_command_hotkey_summary(&self.registered_hotkeys);
        let indexed_apps = format!(
            "{} indexed",
            self.services.router().indexed_application_count()
        );
        let indexed_files = format!("{} indexed", self.services.router().indexed_file_count());
        let config_path = settings_file_path().display().to_string();
        let terminal_pref = self
            .settings
            .preferred_terminal_profile
            .clone()
            .unwrap_or_else(|| "System default".to_string());

        match self.settings_section {
            SettingsSection::General => self.render_general_settings(cx),
            SettingsSection::Search => self.render_search_settings(&indexed_apps, &indexed_files, cx),
            SettingsSection::Appearance => self.render_appearance_settings(cx),
            SettingsSection::Shortcuts => self.render_shortcuts_settings(cx),
            SettingsSection::Expansions => self.render_expansions_settings(cx),
            SettingsSection::Commands => self.render_commands_settings(cx),
            SettingsSection::Advanced => self.render_advanced_settings(
                &command_hotkey_summary,
                &config_path,
                &terminal_pref,
                cx,
            ),
        }
    }

    fn render_general_settings(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(settings_section_intro(
                "General",
                "Launcher behavior, startup, and everyday defaults",
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
                        "Start Core when Windows signs in",
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
                self.settings_row_matches_search("Clipboard history", "clipboard text"),
                |section| {
                    section.child(setting_toggle_row(
                        "Clipboard history",
                        "Store searchable local clipboard text",
                        self.settings.clipboard_history_enabled,
                        cx.listener(Self::toggle_clipboard_history),
                    ))
                },
            )
            .when(
                self.settings_row_matches_search("Welcome & setup", "onboarding screen"),
                |section| {
                    section.child(setting_toggle_row(
                        "Welcome & setup screen",
                        "Replay onboarding walkthrough and screen position setup",
                        false,
                        cx.listener(|this, _, _, cx| {
                            this.is_onboarding_open = true;
                            this.is_settings_open = false;
                            cx.notify();
                        }),
                    ))
                },
            )
            .into_any_element()
    }

    fn render_search_settings(
        &self,
        indexed_apps: &str,
        indexed_files: &str,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(settings_section_intro(
                "Search",
                "Control what Core indexes for universal search",
            ))
            .when(
                self.settings_row_matches_search("Start Menu apps", indexed_apps),
                |section| {
                    section.child(setting_toggle_row(
                        "Start Menu apps",
                        indexed_apps,
                        self.settings.index_start_menu,
                        cx.listener(Self::toggle_app_indexing),
                    ))
                },
            )
            .when(
                self.settings_row_matches_search("User files", indexed_files),
                |section| {
                    section.child(setting_toggle_row(
                        "User files",
                        indexed_files,
                        self.settings.index_user_files,
                        cx.listener(Self::toggle_file_indexing),
                    ))
                },
            )
            .into_any_element()
    }

    fn render_appearance_settings(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(settings_section_intro(
                "Appearance",
                "Window chrome, position, and visual effects",
            ))
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
                self.settings_row_matches_search(
                    "Display position",
                    "launcher position screen center top bottom left right",
                ),
                |section| {
                    let handle = cx.entity().clone();
                    section.child(setting_cross_select_row(
                        "Display position",
                        "Snap the launcher to an edge of the screen",
                        self.settings.display_position,
                        move |pos, window, cx| {
                            let _ = handle.update(cx, |launcher, cx| {
                                launcher.select_display_position(pos, window, cx);
                            });
                        },
                    ))
                },
            )
            .into_any_element()
    }

    fn render_shortcuts_settings(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rows: Vec<_> = self
            .settings
            .hotkeys
            .iter()
            .filter(|hotkey| {
                self.settings_row_matches_search(&hotkey.hotkey, &hotkey.query)
            })
            .map(|hotkey| {
                let binding = hotkey.hotkey.clone();
                let query = hotkey.query.clone();
                let description = if hotkey.description.is_empty() {
                    query.clone()
                } else {
                    format!("{} · {}", hotkey.description, query)
                };
                settings_list_row(binding.clone(), description, cx.listener(move |this, _, _, cx| {
                    this.remove_hotkey_from_settings(&binding, cx);
                }))
            })
            .collect();

        let list = if rows.is_empty() {
            render_settings_empty_state("No command hotkeys yet").into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .children(rows)
                .into_any_element()
        };

        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(settings_section_intro(
                "Shortcuts",
                "Command hotkeys that run a launcher query",
            ))
            .child(settings_info_row(
                "Launcher hotkey",
                if self.settings.hotkey_enabled {
                    &self.settings.hotkey
                } else {
                    "Disabled"
                },
            ))
            .child(settings_card(
                "Add hotkey",
                form_add_row(
                    div()
                        .flex()
                        .flex_1()
                        .items_center()
                        .gap(px(8.))
                        .child(div().w(px(140.)).child(self.hotkey_binding_input.clone()))
                        .child(div().flex_1().child(self.hotkey_query_input.clone())),
                    cx.listener(|this, _, _, cx| this.add_hotkey_from_settings(cx)),
                ),
            ))
            .child(settings_card("Command hotkeys", list))
            .into_any_element()
    }

    fn render_expansions_settings(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let alias_rows: Vec<_> = self
            .settings
            .aliases
            .iter()
            .filter(|alias| {
                self.settings_row_matches_search(&alias.keyword, &alias.expands_to)
            })
            .map(|alias| {
                let keyword = alias.keyword.clone();
                settings_list_row(
                    alias.keyword.clone(),
                    format!("Expands to {}", alias.expands_to),
                    cx.listener(move |this, _, _, cx| {
                        this.remove_alias_from_settings(&keyword, cx);
                    }),
                )
            })
            .collect();

        let quicklink_rows: Vec<_> = quicklinks::configured_quicklinks()
            .into_iter()
            .filter(|quicklink| {
                self.settings_row_matches_search(&quicklink.keyword, &quicklink.target)
            })
            .map(|quicklink| {
                let keyword = quicklink.keyword.clone();
                settings_list_row(
                    format!(">{}", quicklink.keyword),
                    quicklink.target,
                    cx.listener(move |this, _, _, cx| {
                        this.remove_quicklink_from_settings(&keyword, cx);
                    }),
                )
            })
            .collect();

        let snippet_rows: Vec<_> = snippets::configured_snippets()
            .into_iter()
            .filter(|snippet| {
                self.settings_row_matches_search(&snippet.keyword, &snippet.title)
            })
            .map(|snippet| {
                let keyword = snippet.keyword.clone();
                settings_list_row(
                    format!(";{}", snippet.keyword),
                    snippet.title,
                    cx.listener(move |this, _, _, cx| {
                        this.remove_snippet_from_settings(&keyword, cx);
                    }),
                )
            })
            .collect();

        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(settings_section_intro(
                "Expansions",
                "Aliases, snippets, and quicklinks",
            ))
            .child(settings_card(
                "Add alias",
                form_add_row(
                    div()
                        .flex()
                        .flex_1()
                        .items_center()
                        .gap(px(8.))
                        .child(div().w(px(120.)).child(self.alias_keyword_input.clone()))
                        .child(div().flex_1().child(self.alias_expands_to_input.clone())),
                    cx.listener(|this, _, _, cx| this.add_alias_from_settings(cx)),
                ),
            ))
            .child(settings_card(
                "Aliases",
                if alias_rows.is_empty() {
                    render_settings_empty_state("No aliases yet").into_any_element()
                } else {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .children(alias_rows)
                        .into_any_element()
                },
            ))
            .child(settings_card(
                "Add quicklink",
                form_add_row(
                    div()
                        .flex()
                        .flex_1()
                        .items_center()
                        .gap(px(8.))
                        .child(div().w(px(120.)).child(self.quicklink_keyword_input.clone()))
                        .child(div().flex_1().child(self.quicklink_target_input.clone())),
                    cx.listener(|this, _, _, cx| this.add_quicklink_from_settings(cx)),
                ),
            ))
            .child(settings_card(
                "Quicklinks",
                if quicklink_rows.is_empty() {
                    render_settings_empty_state("No quicklinks yet").into_any_element()
                } else {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .children(quicklink_rows)
                        .into_any_element()
                },
            ))
            .child(settings_card(
                "Add snippet",
                form_add_row(
                    div()
                        .flex()
                        .flex_1()
                        .items_center()
                        .gap(px(8.))
                        .child(div().w(px(120.)).child(self.snippet_keyword_input.clone()))
                        .child(div().flex_1().child(self.snippet_body_input.clone())),
                    cx.listener(|this, _, _, cx| this.add_snippet_from_settings(cx)),
                ),
            ))
            .child(settings_card(
                "Snippets",
                if snippet_rows.is_empty() {
                    render_settings_empty_state("No snippets yet").into_any_element()
                } else {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .children(snippet_rows)
                        .into_any_element()
                },
            ))
            .into_any_element()
    }

    fn render_commands_settings(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rows: Vec<_> = self
            .settings
            .custom_commands
            .iter()
            .filter(|command| {
                self.settings_row_matches_search(&command.name, &command.command)
            })
            .map(|command| {
                let name = command.name.clone();
                settings_list_row(
                    command.name.clone(),
                    command.command.clone(),
                    cx.listener(move |this, _, _, cx| {
                        this.remove_custom_command_from_settings(&name, cx);
                    }),
                )
            })
            .collect();

        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(settings_section_intro(
                "Commands",
                "Custom shell commands available via @custom",
            ))
            .child(settings_card(
                "Add command",
                form_add_row(
                    div()
                        .flex()
                        .flex_1()
                        .items_center()
                        .gap(px(8.))
                        .child(div().w(px(140.)).child(self.custom_name_input.clone()))
                        .child(div().flex_1().child(self.custom_command_input.clone())),
                    cx.listener(|this, _, _, cx| this.add_custom_command_from_settings(cx)),
                ),
            ))
            .child(settings_card(
                "Custom commands",
                if rows.is_empty() {
                    render_settings_empty_state("No custom commands yet").into_any_element()
                } else {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .children(rows)
                        .into_any_element()
                },
            ))
            .into_any_element()
    }

    fn render_advanced_settings(
        &self,
        command_hotkey_summary: &str,
        config_path: &str,
        terminal_pref: &str,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(settings_section_intro(
                "Advanced",
                "Locale defaults, diagnostics, and config files",
            ))
            .child(settings_info_row("Command hotkeys", command_hotkey_summary))
            .child(settings_info_row("Preferred terminal", terminal_pref))
            .child(settings_info_row("Config file", config_path))
            .child(settings_card(
                "Locale",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .w(px(90.))
                                    .text_size(px(type_scale::BODY_SM))
                                    .text_color(colors::text_muted())
                                    .child("Timezone"),
                            )
                            .child(div().flex_1().child(self.timezone_input.clone())),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .w(px(90.))
                                    .text_size(px(type_scale::BODY_SM))
                                    .text_color(colors::text_muted())
                                    .child("Currency"),
                            )
                            .child(div().flex_1().child(self.currency_input.clone())),
                    )
                    .child(
                        primary_button("Save locale")
                            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                                this.save_locale_from_settings(cx);
                            })),
                    ),
            ))
            .into_any_element()
    }

    pub(super) fn render_settings_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let primary_actions = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                settings_footer_icon_button(LucideIcon::X, colors::text_secondary())
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::close_settings_menu)),
            )
            .child(
                settings_footer_icon_button(LucideIcon::FileCog, colors::text_secondary())
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::open_settings_file)),
            )
            .child(
                settings_footer_icon_button(LucideIcon::RefreshCw, colors::text_secondary())
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::reload_applications)),
            )
            .child(
                settings_footer_icon_button(LucideIcon::Power, colors::danger())
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
        if quicklinks::save_quicklink(keyword, title, target).is_ok() {
            self.quicklink_keyword_input
                .update(cx, |input, cx| input.reset(cx));
            self.quicklink_target_input
                .update(cx, |input, cx| input.reset(cx));
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
            CommandAliasSetting {
                keyword,
                expands_to,
            },
            None,
        );
        if added {
            self.save_settings();
            self.alias_keyword_input
                .update(cx, |input, cx| input.reset(cx));
            self.alias_expands_to_input
                .update(cx, |input, cx| input.reset(cx));
            cx.notify();
        }
    }

    pub(super) fn add_snippet_from_settings(&mut self, cx: &mut Context<Self>) {
        let keyword = self.snippet_keyword_input.read(cx).content().to_string();
        let body = self.snippet_body_input.read(cx).content().to_string();
        if keyword.trim().is_empty() || body.trim().is_empty() {
            return;
        }

        let title = format!("Snippet {}", keyword);
        if snippets::save_snippet(keyword, title, body).is_ok() {
            self.snippet_keyword_input
                .update(cx, |input, cx| input.reset(cx));
            self.snippet_body_input
                .update(cx, |input, cx| input.reset(cx));
            cx.notify();
        }
    }

    pub(super) fn add_hotkey_from_settings(&mut self, cx: &mut Context<Self>) {
        let hotkey = self.hotkey_binding_input.read(cx).content().to_string();
        let query = self.hotkey_query_input.read(cx).content().to_string();
        if hotkey.trim().is_empty() || query.trim().is_empty() {
            return;
        }

        let added = self.settings.upsert_query_hotkey(
            CommandHotkeySetting {
                hotkey,
                query: query.clone(),
                description: query,
            },
            None,
        );
        if added {
            self.save_settings();
            self.hotkey_binding_input
                .update(cx, |input, cx| input.reset(cx));
            self.hotkey_query_input
                .update(cx, |input, cx| input.reset(cx));
            cx.notify();
        }
    }

    pub(super) fn add_custom_command_from_settings(&mut self, cx: &mut Context<Self>) {
        let name = self.custom_name_input.read(cx).content().to_string();
        let command = self.custom_command_input.read(cx).content().to_string();
        if name.trim().is_empty() || command.trim().is_empty() {
            return;
        }

        let added = self.settings.upsert_custom_command(
            CustomCommandSetting {
                name,
                description: String::new(),
                command,
                aliases: Vec::new(),
                hotkey: None,
                working_directory: None,
            },
            None,
        );
        if added {
            self.save_settings();
            self.custom_name_input
                .update(cx, |input, cx| input.reset(cx));
            self.custom_command_input
                .update(cx, |input, cx| input.reset(cx));
            cx.notify();
        }
    }

    pub(super) fn remove_alias_from_settings(&mut self, keyword: &str, cx: &mut Context<Self>) {
        if self.settings.remove_alias(keyword) {
            self.save_settings();
            cx.notify();
        }
    }

    pub(super) fn remove_hotkey_from_settings(&mut self, hotkey: &str, cx: &mut Context<Self>) {
        if self.settings.remove_query_hotkey(hotkey) {
            self.save_settings();
            cx.notify();
        }
    }

    pub(super) fn remove_custom_command_from_settings(
        &mut self,
        name: &str,
        cx: &mut Context<Self>,
    ) {
        if self.settings.remove_custom_command(name) {
            self.save_settings();
            cx.notify();
        }
    }

    pub(super) fn remove_quicklink_from_settings(&mut self, keyword: &str, cx: &mut Context<Self>) {
        if quicklinks::delete_quicklink(keyword).is_ok() {
            cx.notify();
        }
    }

    pub(super) fn remove_snippet_from_settings(&mut self, keyword: &str, cx: &mut Context<Self>) {
        if snippets::delete_snippet(keyword).is_ok() {
            cx.notify();
        }
    }

    pub(super) fn save_locale_from_settings(&mut self, cx: &mut Context<Self>) {
        let timezone = self.timezone_input.read(cx).content().to_string();
        let currency = self.currency_input.read(cx).content().to_string();

        if !timezone.trim().is_empty() {
            self.settings.local_timezone = timezone.trim().to_string();
        }
        self.settings.home_currency = {
            let trimmed = currency.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_uppercase())
            }
        };
        self.save_settings();
        cx.notify();
    }
}
