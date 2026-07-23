use crate::{
    command::{CommandAction, CommandCategory, CommandResult},
    ui::{
        browse_views::{
            browse_action_bar, browse_action_hint, browse_empty_state, browse_primary_action,
        },
        lucide_icons::LucideIcon,
        theme::{self, colors, category_theme},
    },
};
use gpui::{
    div, img, prelude::*, px, rgb, rgba, Context, MouseButton, MouseUpEvent, Window,
};
use super::{
    LauncherPanel, LauncherView, MoveSelectionDown, MoveSelectionFirst, MoveSelectionLast,
    MoveSelectionPageDown, MoveSelectionPageUp, MoveSelectionUp,
};

impl LauncherView {
    pub(super) fn move_selection_up(
        &mut self,
        _: &MoveSelectionUp,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selectable_item_count() > 0 && self.selected_index > 0 {
            self.selected_index -= 1;
            self.ensure_browse_selection_visible();
            cx.notify();
        }
    }

    pub(super) fn move_selection_down(
        &mut self,
        _: &MoveSelectionDown,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_index + 1 < self.selectable_item_count() {
            self.selected_index += 1;
            self.ensure_browse_selection_visible();
            cx.notify();
        }
    }

    pub(super) fn move_selection_page_up(
        &mut self,
        _: &MoveSelectionPageUp,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_selection_by(-5, cx);
    }

    pub(super) fn move_selection_page_down(
        &mut self,
        _: &MoveSelectionPageDown,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_selection_by(5, cx);
    }

    pub(super) fn move_selection_first(
        &mut self,
        _: &MoveSelectionFirst,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selectable_item_count() > 0 && self.selected_index != 0 {
            self.selected_index = 0;
            self.ensure_browse_selection_visible();
            cx.notify();
        }
    }

    pub(super) fn move_selection_last(
        &mut self,
        _: &MoveSelectionLast,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selectable_item_count = self.selectable_item_count();
        if selectable_item_count > 0 && self.selected_index + 1 != selectable_item_count {
            self.selected_index = selectable_item_count - 1;
            self.ensure_browse_selection_visible();
            cx.notify();
        }
    }

    pub(super) fn move_selection_by(&mut self, offset: isize, cx: &mut Context<Self>) {
        let selectable_item_count = self.selectable_item_count();
        if selectable_item_count == 0 {
            return;
        }

        let last_selectable_index = selectable_item_count - 1;
        let next_selected_index = self
            .selected_index
            .saturating_add_signed(offset)
            .min(last_selectable_index);
        if next_selected_index != self.selected_index {
            self.selected_index = next_selected_index;
            self.ensure_browse_selection_visible();
            cx.notify();
        }
    }

    pub(super) fn selectable_item_count(&self) -> usize {
        match &self.panel {
            LauncherPanel::Home => self.results.len(),
            LauncherPanel::TerminalShellPicker { .. } => self.available_shells.len(),
            LauncherPanel::TerminalSession(_) => 0,
        }
    }

    pub(super) fn accept_mouse_result(
        &mut self,
        result_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_index = result_index;
        self.ensure_browse_selection_visible();
        self.accept_selected_result(window, cx);
    }

    pub(super) fn render_home_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut panel = div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .child(self.render_results(cx));

        if let Some(spotify_bar) = self.render_spotify_bar(cx) {
            panel = panel.child(spotify_bar);
        }

        panel = panel.child(self.render_launcher_action_bar(cx));

        if self.spotify_closed {
            panel = panel.child(
                div()
                    .absolute()
                    .bottom(px(42.))
                    .right(px(14.))
                    .child(
                        div()
                            .id("expand-spotify")
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(20.))
                            .rounded_md()
                            .bg(rgba(0xffffff0d))
                            .border_1()
                            .border_color(rgba(0xffffff15))
                            .hover(|style| style.bg(rgba(0xffffff20)).cursor_pointer())
                            .child(crate::ui::lucide_icons::render_lucide_icon(
                                crate::ui::lucide_icons::LucideIcon::ChevronUp,
                                12.,
                                rgb(0x1db954),
                                false,
                            ))
                            .on_mouse_up(
                                gpui::MouseButton::Left,
                                cx.listener(move |launcher, _: &gpui::MouseUpEvent, _window, cx| {
                                    launcher.spotify_closed = false;
                                    cx.notify();
                                }),
                            ),
                    )
            );
        }

        panel
    }

    pub(super) fn render_spotify_bar(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.spotify_closed {
            return None;
        }

        let display_text = if let Some(title) = &self.spotify_title {
            if let Some(art) = &self.spotify_artist {
                if art.is_empty() {
                    title.clone()
                } else {
                    format!("{title} · {art}")
                }
            } else {
                title.clone()
            }
        } else {
            "Not playing".to_string()
        };

        Some(
            div()
                .id("spotify-bar")
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .px(px(14.))
                .py(px(6.))
                .bg(rgba(0xffffff06))
                .border_t_1()
                .border_color(rgba(0xffffff10))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .flex_1()
                        .min_w(px(0.))
                        .child(crate::ui::lucide_icons::render_lucide_icon(
                            crate::ui::lucide_icons::LucideIcon::Music,
                            14.,
                            rgb(0x1db954), // Spotify Green
                            false,
                        ))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(0xd4d4d8))
                                .text_ellipsis()
                                .child(display_text),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(16.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .rounded_md()
                                        .hover(|style| style.bg(rgba(0xffffff10)).cursor_pointer())
                                        .child(crate::ui::lucide_icons::render_hoverable_lucide_icon(
                                            crate::ui::lucide_icons::LucideIcon::Volume2,
                                            12.,
                                            rgb(0x9ca3af),
                                            false,
                                        ))
                                        .on_mouse_up(
                                            gpui::MouseButton::Left,
                                            cx.listener(move |launcher, _: &gpui::MouseUpEvent, _window, cx| {
                                                let current = launcher.spotify_volume;
                                                let new_vol = if current > 0.0 { 0.0 } else { 0.35 };
                                                crate::media_tools::set_system_volume(new_vol);
                                                launcher.spotify_volume = new_vol;
                                                cx.notify();
                                            }),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(2.))
                                        .children((1..=10).map(|i| {
                                            let target_vol = i as f32 * 0.1;
                                            let active = self.spotify_volume >= (target_vol - 0.05);
                                            let bar_color = if active {
                                                rgb(0x1db954)
                                            } else {
                                                rgba(0xffffff20)
                                            };
                                            div()
                                                .id(("volume-tick", i as usize))
                                                .w(px(3.))
                                                .h(px(10.))
                                                .rounded_sm()
                                                .bg(bar_color)
                                                .hover(|style| style.bg(rgb(0xffffff)).cursor_pointer())
                                                .on_mouse_up(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(move |launcher, _: &gpui::MouseUpEvent, _window, cx| {
                                                        crate::media_tools::set_system_volume(target_vol);
                                                        launcher.spotify_volume = target_vol;
                                                        cx.notify();
                                                    }),
                                                )
                                        })),
                                )
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(10.))
                                .child(
                                    div()
                                        .rounded_md()
                                        .hover(|style| style.bg(rgba(0xffffff10)).cursor_pointer())
                                        .child(crate::ui::lucide_icons::render_hoverable_lucide_icon(
                                            crate::ui::lucide_icons::LucideIcon::SkipBack,
                                            12.,
                                            rgb(0x9ca3af),
                                            false,
                                        ))
                                        .on_mouse_up(
                                            gpui::MouseButton::Left,
                                            cx.listener(move |_launcher, _: &gpui::MouseUpEvent, _window, cx| {
                                                let _ = crate::system_controls::execute_system_control(
                                                    &crate::command::SystemControlCommand::MediaPrevious,
                                                );
                                                cx.spawn(async move |this, cx| {
                                                    cx.background_executor().timer(std::time::Duration::from_millis(300)).await;
                                                    let now_playing = crate::media_tools::read_now_playing();
                                                    this.update(cx, |launcher, cx| {
                                                        if let Some((t, a)) = now_playing {
                                                            launcher.spotify_title = Some(t);
                                                            launcher.spotify_artist = Some(a);
                                                        }
                                                        cx.notify();
                                                    }).ok();
                                                }).detach();
                                            }),
                                        ),
                                )
                                .child(
                                    div()
                                        .rounded_md()
                                        .hover(|style| style.bg(rgba(0xffffff10)).cursor_pointer())
                                        .child(crate::ui::lucide_icons::render_hoverable_lucide_icon(
                                            crate::ui::lucide_icons::LucideIcon::Play,
                                            12.,
                                            rgb(0xf4f4f5),
                                            false,
                                        ))
                                        .on_mouse_up(
                                            gpui::MouseButton::Left,
                                            cx.listener(move |_launcher, _: &gpui::MouseUpEvent, _window, cx| {
                                                let _ = crate::system_controls::execute_system_control(
                                                    &crate::command::SystemControlCommand::MediaPlayPause,
                                                );
                                                cx.spawn(async move |this, cx| {
                                                    cx.background_executor().timer(std::time::Duration::from_millis(300)).await;
                                                    let now_playing = crate::media_tools::read_now_playing();
                                                    this.update(cx, |launcher, cx| {
                                                        if let Some((t, a)) = now_playing {
                                                            launcher.spotify_title = Some(t);
                                                            launcher.spotify_artist = Some(a);
                                                        }
                                                        cx.notify();
                                                    }).ok();
                                                }).detach();
                                            }),
                                        ),
                                )
                                .child(
                                    div()
                                        .rounded_md()
                                        .hover(|style| style.bg(rgba(0xffffff10)).cursor_pointer())
                                        .child(crate::ui::lucide_icons::render_hoverable_lucide_icon(
                                            crate::ui::lucide_icons::LucideIcon::SkipForward,
                                            12.,
                                            rgb(0x9ca3af),
                                            false,
                                        ))
                                        .on_mouse_up(
                                            gpui::MouseButton::Left,
                                            cx.listener(move |_launcher, _: &gpui::MouseUpEvent, _window, cx| {
                                                let _ = crate::system_controls::execute_system_control(
                                                    &crate::command::SystemControlCommand::MediaNext,
                                                );
                                                cx.spawn(async move |this, cx| {
                                                    cx.background_executor().timer(std::time::Duration::from_millis(300)).await;
                                                    let now_playing = crate::media_tools::read_now_playing();
                                                    this.update(cx, |launcher, cx| {
                                                        if let Some((t, a)) = now_playing {
                                                            launcher.spotify_title = Some(t);
                                                            launcher.spotify_artist = Some(a);
                                                        }
                                                        cx.notify();
                                                    }).ok();
                                                }).detach();
                                            }),
                                        ),
                                )
                        )
                        .child(
                            div()
                                .rounded_md()
                                .hover(|style| style.bg(rgba(0xffffff10)).cursor_pointer())
                                .child(crate::ui::lucide_icons::render_hoverable_lucide_icon(
                                    crate::ui::lucide_icons::LucideIcon::ChevronDown,
                                    12.,
                                    rgb(0x9ca3af),
                                    false,
                                ))
                                .on_mouse_up(
                                    gpui::MouseButton::Left,
                                    cx.listener(move |launcher, _: &gpui::MouseUpEvent, _window, cx| {
                                        launcher.spotify_closed = true;
                                        cx.notify();
                                    }),
                                ),
                        )
                )
                .into_any_element()
        )
    }

    pub(super) fn render_launcher_action_bar(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.results.get(self.selected_index);
        let primary_label = selected
            .map(|result| primary_action_label(result))
            .unwrap_or_else(|| "Search".to_string());
        let can_copy = selected.is_some_and(|result| !result.copy_text.is_empty());

        let mut hints = vec![
            browse_action_hint("Enter", primary_label.clone()).into_any_element(),
            browse_action_hint("Esc", "Dismiss").into_any_element(),
        ];
        if can_copy {
            hints.insert(
                1,
                browse_action_hint("Ctrl+Enter", "Copy").into_any_element(),
            );
        }

        browse_action_bar(browse_primary_action(primary_label).into_any_element(), hints)
    }

    pub(super) fn render_results(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let result_count = self.results.len();

        div()
            .flex()
            .flex_col()
            .overflow_hidden()
            .gap(px(4.))
            .pl(px(5.))
            .pr(px(8.))
            .py(px(10.))
            .flex_1()
            .min_h(px(0.))
            .max_h(px(super::LAUNCHER_RESULTS_HEIGHT))
            .when(result_count == 0, |container| {
                container.child(browse_empty_state(
                    Some(LucideIcon::Search),
                    "No confident result",
                    "Try a different query or scope",
                ))
            })
            .children(
                self.results
                    .iter()
                    .enumerate()
                    .map(|(result_index, result)| {
                        let is_selected = result_index == self.selected_index;

                        if matches!(result.category, CommandCategory::Calculation)
                            || result.calculation_display.is_some()
                        {
                            self.render_calculation_result(result, result_index, is_selected, cx)
                        } else {
                            self.render_standard_result(result, result_index, is_selected, cx)
                        }
                    }),
            )
    }

    pub(super) fn render_standard_result(
        &self,
        result: &CommandResult,
        result_index: usize,
        is_selected: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let has_at_prefix = result.title.starts_with('@');
        let raw_title = if has_at_prefix {
            result.title.strip_prefix('@').unwrap_or(&result.title).to_string()
        } else {
            result.title.clone()
        };
        let title = compact_display_text(&raw_title, 72);
        let subtitle = compact_display_text(&result.subtitle, 86);
        let explanation = compact_display_text(&result.explanation.clone().unwrap_or_default(), 96);
        let is_application = matches!(result.category, CommandCategory::Application);
        let shows_subtitle = should_show_subtitle(result);
        let shows_explanation = shows_subtitle && !explanation.is_empty();
        
        let category_label = if has_at_prefix {
            "Function"
        } else {
            match &result.category {
                CommandCategory::Application => "App",
                CommandCategory::BuiltIn
                | CommandCategory::System
                | CommandCategory::WindowManagement
                | CommandCategory::DevTools
                | CommandCategory::Git => "Command",
                CommandCategory::Note => "Function",
                CommandCategory::Web | CommandCategory::Quicklink => "Link",
                CommandCategory::Calculation => "Calc",
                _ => category_label(&result.category),
            }
        };

        let (badge_text, badge_bg, badge_border) = category_badge_theme(&result.category, has_at_prefix);

        div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                // Selection indicator line
                div()
                    .w(px(3.))
                    .h(px(18.))
                    .rounded_full()
                    .bg(if is_selected { rgb(0xa78bfa) } else { rgba(0x00000000) })
            )
            .child(
                div()
                    .id(("result-row", result_index))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .pl(px(4.))
                    .pr(px(8.))
                    .py(px(8.))
                    .flex_1()
                    .rounded_md()
                    .bg(if is_selected { rgba(0xffffff0d) } else { rgba(0x00000000) })
                    .border_1()
                    .border_color(if is_selected { rgba(0xffffff14) } else { rgba(0x00000000) })
                    .hover(|style| {
                        let style = style.cursor_pointer();
                        if is_selected {
                            style
                        } else {
                            style.bg(rgba(0xffffff06))
                        }
                    })
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |launcher, _: &MouseUpEvent, window, cx| {
                            launcher.accept_mouse_result(result_index, window, cx);
                        }),
                    )
                    .child(render_result_icon(result))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .w_full()
                            .child(
                                div()
                                    .text_size(if is_application { px(15.) } else { px(14.) })
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(rgb(0xffffff))
                                    .child(title),
                            )
                            .when(shows_subtitle, |result_text| {
                                result_text.child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(rgb(0xa1a1aa))
                                        .child(subtitle),
                                )
                            })
                            .when(shows_explanation, |result_text| {
                                result_text.child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(rgb(0x71717a))
                                        .child(explanation),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_none()
                            .px(px(8.))
                            .py(px(2.))
                            .rounded_full()
                            .bg(badge_bg)
                            .border_1()
                            .border_color(badge_border)
                            .text_color(badge_text)
                            .text_size(px(10.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(category_label),
                    )
            )
            .into_any_element()
    }



    pub(super) fn render_calculation_result(
        &self,
        result: &CommandResult,
        result_index: usize,
        is_selected: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let calculation_display = result.calculation_display.as_ref();
        let expression = calculation_display
            .map(|display| display.expression.clone())
            .unwrap_or_else(|| result.subtitle.clone());
        let answer = calculation_display
            .map(|display| display.result.clone())
            .unwrap_or_else(|| result.title.clone());
        let kind_label = calculation_display
            .map(|display| display.kind_label.clone())
            .unwrap_or_else(|| "Calculation".to_string());
        let result_label = calculation_display
            .map(|display| display.result_label.clone())
            .unwrap_or_else(|| "Result".to_string());

        let display_title = match result.category {
            CommandCategory::Calculation => "Calculator",
            CommandCategory::Context => {
                if result.title.contains(':') {
                    "Time"
                } else {
                    "Date"
                }
            }
            _ => "Calculator",
        };

        div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                // Selection indicator line
                div()
                    .w(px(3.))
                    .h(px(18.))
                    .rounded_full()
                    .bg(if is_selected { rgb(0xa78bfa) } else { rgba(0x00000000) })
            )
            .child(
                div()
                    .id(("result-row", result_index))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .pl(px(4.))
                    .pr(px(8.))
                    .py(px(7.))
                    .flex_1()
                    .rounded_md()
                    .bg(if is_selected { rgba(0xffffff0d) } else { rgba(0x00000000) })
                    .border_1()
                    .border_color(if is_selected { rgba(0xffffff14) } else { rgba(0x00000000) })
                    .hover(|style| {
                        let style = style.cursor_pointer();
                        if is_selected {
                            style
                        } else {
                            style.bg(rgba(0xffffff06))
                        }
                    })
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |launcher, _: &MouseUpEvent, window, cx| {
                            launcher.accept_mouse_result(result_index, window, cx);
                        }),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(0xd9d9d9))
                            .child(display_title),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .h(px(118.))
                            .rounded_sm()
                            .bg(rgba(0xffffff06))
                            .border_1()
                            .border_color(rgba(0xffffff0a))
                            .px(px(18.))
                            .child(calculation_side(expression, kind_label, false))
                            .child(
                                div().flex().items_center().gap(px(14.)).child(
                                    div()
                                        .text_size(px(28.))
                                        .text_color(rgb(0xffffff))
                                        .child("->"),
                                ),
                            )
                            .child(calculation_side(answer, result_label, true)),
                    )
            )
            .into_any_element()
    }

    pub(super) fn render_onboarding_panel(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        use crate::settings::DisplayPosition;
        use crate::ui::browse_views::{border_subtle, surface_muted, surface_overlay_low};
        use crate::ui::lucide_icons::{render_lucide_icon, LucideIcon};
        use crate::ui::theme::{colors, launcher_background, type_scale};

        let current_position = self.settings.display_position;
        let current_hotkey = self.settings.hotkey.clone();
        let hotkey_enabled = self.settings.hotkey_enabled;

        div()
            .id("onboarding-panel")
            .flex()
            .flex_col()
            .flex_1()
            .size_full()
            .bg(launcher_background(self.settings.backdrop_blur_enabled))
            .overflow_y_scroll()
            .p(px(24.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .text_center()
                    .mb(px(20.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(52.))
                            .h(px(52.))
                            .rounded_full()
                            .bg(surface_overlay_low())
                            .border_1()
                            .border_color(border_subtle())
                            .mb(px(12.))
                            .child(render_lucide_icon(
                                LucideIcon::Zap,
                                26.,
                                colors::accent(),
                                false,
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(type_scale::TITLE_LG))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(colors::text_primary())
                            .child("Welcome to Core Launcher"),
                    )
                    .child(
                        div()
                            .text_size(px(type_scale::BODY_LG))
                            .text_color(colors::text_muted())
                            .mt(px(4.))
                            .child("Customize your screen position, global reveal hotkey, and defaults"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p(px(16.))
                    .mb(px(16.))
                    .bg(surface_overlay_low())
                    .border_1()
                    .border_color(border_subtle())
                    .rounded(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .mb(px(8.))
                            .child(render_lucide_icon(
                                LucideIcon::Monitor,
                                18.,
                                colors::accent(),
                                false,
                            ))
                            .child(
                                div()
                                    .text_size(px(type_scale::TITLE))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(colors::text_primary())
                                    .child("1. Launcher Screen Position"),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(type_scale::CAPTION))
                            .text_color(colors::text_muted())
                            .mb(px(12.))
                            .child("Choose where Core Launcher anchors on your screen (moves live)"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(8.))
                            .children([
                                (DisplayPosition::Center, "Center"),
                                (DisplayPosition::Top, "Top Screen"),
                                (DisplayPosition::Bottom, "Bottom Dock"),
                                (DisplayPosition::Left, "Left Sidebar"),
                                (DisplayPosition::Right, "Right Sidebar"),
                            ].into_iter().map(|(pos, label)| {
                                let is_selected = current_position == pos;
                                div()
                                    .flex()
                                    .items_center()
                                    .px(px(14.))
                                    .py(px(8.))
                                    .rounded(px(8.))
                                    .cursor_pointer()
                                    .bg(if is_selected { result_row_background(true) } else { surface_muted() })
                                    .border_1()
                                    .border_color(if is_selected { colors::accent() } else { border_subtle() })
                                    .text_color(if is_selected { colors::text_primary() } else { colors::text_muted() })
                                    .on_mouse_up(MouseButton::Left, cx.listener(move |this, _, window, cx| {
                                        this.select_display_position(pos, window, cx);
                                    }))
                                    .child(
                                        div()
                                            .text_size(px(type_scale::BODY_LG))
                                            .font_weight(if is_selected { gpui::FontWeight::SEMIBOLD } else { gpui::FontWeight::NORMAL })
                                            .child(label)
                                    )
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p(px(16.))
                    .mb(px(16.))
                    .bg(surface_overlay_low())
                    .border_1()
                    .border_color(border_subtle())
                    .rounded(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .mb(px(8.))
                            .child(render_lucide_icon(
                                LucideIcon::Keyboard,
                                18.,
                                colors::accent(),
                                false,
                            ))
                            .child(
                                div()
                                    .text_size(px(type_scale::TITLE))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(colors::text_primary())
                                    .child("2. Global Reveal Shortcut"),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(type_scale::CAPTION))
                            .text_color(colors::text_muted())
                            .mb(px(12.))
                            .child("Press this key shortcut anywhere to open or hide Core Launcher"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .children(["Alt+Space", "Ctrl+Space", "Ctrl+Shift+Space", "Super+Space"].into_iter().map(|preset| {
                                let is_selected = hotkey_enabled && current_hotkey.eq_ignore_ascii_case(preset);
                                let preset_str = preset.to_string();

                                div()
                                    .flex()
                                    .items_center()
                                    .px(px(12.))
                                    .py(px(6.))
                                    .rounded(px(6.))
                                    .cursor_pointer()
                                    .bg(if is_selected { result_row_background(true) } else { surface_muted() })
                                    .border_1()
                                    .border_color(if is_selected { colors::accent() } else { border_subtle() })
                                    .text_color(if is_selected { colors::text_primary() } else { colors::text_muted() })
                                    .on_mouse_up(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                                        this.settings.hotkey = preset_str.clone();
                                        this.settings.hotkey_enabled = true;
                                        this.save_settings();
                                        #[cfg(target_os = "windows")]
                                        {
                                            this.registered_hotkeys = super::register_global_hotkeys(&this.settings);
                                        }
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .text_size(px(type_scale::CAPTION))
                                            .font_weight(if is_selected { gpui::FontWeight::SEMIBOLD } else { gpui::FontWeight::NORMAL })
                                            .child(preset)
                                    )
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p(px(16.))
                    .mb(px(20.))
                    .bg(surface_overlay_low())
                    .border_1()
                    .border_color(border_subtle())
                    .rounded(px(12.))
                    .child(
                        div()
                            .text_size(px(type_scale::TITLE))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(colors::text_primary())
                            .mb(px(12.))
                            .child("Core Launcher Features"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .child(render_onboarding_feature_row(LucideIcon::Search, "Instant Search", "Search applications, files, settings, and web shortcuts"))
                            .child(render_onboarding_feature_row(LucideIcon::Calculator, "Smart Math & Unit Conversion", "Evaluates calculations, currency rates, relative dates, and hex"))
                            .child(render_onboarding_feature_row(LucideIcon::Clipboard, "Local Storage Clipboard History", "Searchable local history for text, color codes, and image captures"))
                            .child(render_onboarding_feature_row(LucideIcon::Terminal, "Embedded Terminal & Dev Tools", "PowerShell terminal sessions, git commands, JSON formatters, and QR codes"))
                    ),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .items_center()
                    .pt(px(16.))
                    .border_t_1()
                    .border_color(border_subtle())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .px(px(24.))
                            .py(px(10.))
                            .rounded(px(8.))
                            .bg(colors::accent())
                            .text_color(colors::text_primary())
                            .font_weight(gpui::FontWeight::BOLD)
                            .cursor_pointer()
                            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                                this.settings.onboarding_completed = true;
                                this.save_settings();
                                this.is_onboarding_open = false;
                                cx.notify();
                            }))
                            .child("Get Started →"),
                    ),
            )
    }
}

fn render_onboarding_feature_row(
    icon: LucideIcon,
    title: &'static str,
    description: &'static str,
) -> impl IntoElement {
    use crate::ui::browse_views::surface_muted;
    use crate::ui::lucide_icons::render_lucide_icon;
    use crate::ui::theme::{colors, type_scale};

    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .w(px(32.))
                .h(px(32.))
                .rounded(px(8.))
                .bg(surface_muted())
                .child(render_lucide_icon(icon, 16., colors::accent(), false)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(type_scale::BODY_LG))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(colors::text_primary())
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(type_scale::CAPTION))
                        .text_color(colors::text_muted())
                        .child(description),
                ),
        )
}

fn category_icon_theme(category: &CommandCategory) -> (gpui::Rgba, gpui::Rgba, gpui::Rgba) {
    let theme = category_theme(category);
    (theme.accent, theme.icon_bg, theme.icon_border)
}

fn category_badge_theme(
    category: &CommandCategory,
    has_at_prefix: bool,
) -> (gpui::Rgba, gpui::Rgba, gpui::Rgba) {
    if has_at_prefix {
        return (
            colors::accent_soft(),
            rgba(0x7c3aed14),
            rgba(0x7c3aed2d),
        );
    }
    let theme = category_theme(category);
    (theme.accent, theme.badge_bg, theme.badge_border)
}

pub(super) fn render_result_icon(result: &CommandResult) -> gpui::AnyElement {
    let icon = LucideIcon::for_category(&result.category);

    if let Some(icon_path) = result.icon_path.clone() {
        let fallback_cat = result.category.clone();
        return img(icon_path)
            .size(px(28.))
            .rounded_sm()
            .with_fallback(move || render_result_fallback_icon_themed(icon, &fallback_cat))
            .into_any_element();
    }

    render_result_fallback_icon_themed(icon, &result.category)
}

fn render_result_fallback_icon_themed(icon: LucideIcon, category: &CommandCategory) -> gpui::AnyElement {
    use crate::ui::lucide_icons::render_lucide_icon;
    let (icon_color, bg_color, border_color) = category_icon_theme(category);
    div()
        .size(px(28.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .bg(bg_color)
        .border_1()
        .border_color(border_color)
        .child(render_lucide_icon(icon, 14.0, icon_color, false))
        .into_any_element()
}

pub(super) fn should_show_subtitle(result: &CommandResult) -> bool {
    !matches!(result.category, CommandCategory::Application) && !result.subtitle.is_empty()
}

pub(super) fn calculation_side(primary_text: String, badge_text: String, is_answer: bool) -> gpui::Div {
    let primary_text = compact_calculation_text(&primary_text, is_answer);
    let primary_text_size = calculation_primary_text_size(&primary_text, is_answer);

    div()
        .w(px(260.))
        .h_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(14.))
        .child(calculation_primary_text(
            primary_text,
            primary_text_size,
            is_answer,
        ))
        .child(
            div()
                .px(px(8.))
                .py(px(3.))
                .rounded_sm()
                .bg(rgb(0x242424))
                .text_size(px(12.))
                .text_color(rgb(0xffffff))
                .child(badge_text),
        )
}

pub(super) fn calculation_primary_text(
    primary_text: String,
    fallback_text_size: f32,
    is_answer: bool,
) -> gpui::Div {
    let primary_lines = primary_text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let is_multiline = primary_lines.len() > 1;

    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(if is_multiline { 4. } else { 0. }))
        .children(
            primary_lines
                .into_iter()
                .enumerate()
                .map(move |(line_index, line_text)| {
                    let line_text_size = calculation_primary_line_text_size(
                        &line_text,
                        fallback_text_size,
                        is_multiline,
                        is_answer,
                        line_index,
                    );
                    div()
                        .text_size(px(line_text_size))
                        .text_color(if is_multiline && line_index == 0 {
                            rgb(0xd9d9d9)
                        } else {
                            rgb(0xffffff)
                        })
                        .child(line_text)
                }),
        )
}

pub(super) fn compact_calculation_text(primary_text: &str, is_answer: bool) -> String {
    let max_line_length = if is_answer { 30 } else { 36 };
    primary_text
        .lines()
        .map(|line| compact_display_text(line.trim(), max_line_length))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn calculation_primary_line_text_size(
    line_text: &str,
    fallback_text_size: f32,
    is_multiline: bool,
    is_answer: bool,
    line_index: usize,
) -> f32 {
    if !is_multiline {
        return fallback_text_size;
    }

    let line_length = line_text.chars().count();
    if line_index == 0 {
        if line_length > 28 {
            17.
        } else {
            19.
        }
    } else if is_answer {
        26.
    } else {
        22.
    }
}

pub(super) fn calculation_primary_text_size(primary_text: &str, is_answer: bool) -> f32 {
    let text_length = primary_text.chars().count();

    if text_length > 34 {
        17.
    } else if text_length > 24 {
        20.
    } else if is_answer {
        30.
    } else {
        26.
    }
}

pub(super) fn compact_display_text(value: &str, max_length: usize) -> String {
    let normalized_value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized_value.chars().count() <= max_length {
        return normalized_value;
    }

    format!(
        "{}...",
        normalized_value
            .chars()
            .take(max_length.saturating_sub(3))
            .collect::<String>()
    )
}

pub(super) fn category_color(category: &CommandCategory) -> gpui::Rgba {
    theme::category_color(category)
}

pub(super) fn category_label(category: &CommandCategory) -> &'static str {
    theme::category_label(category)
}

pub(super) fn result_row_background(is_selected: bool) -> gpui::Rgba {
    theme::result_row_background(is_selected)
}

fn primary_action_label(result: &CommandResult) -> String {
    match &result.action {
        CommandAction::CopyToClipboard(_) => "Copy".to_string(),
        CommandAction::Feature(_) => match result.category {
            CommandCategory::Calculation => "Copy result".to_string(),
            _ => "Run".to_string(),
        },
        CommandAction::BuiltIn(_) => "Open".to_string(),
        CommandAction::OpenPath(_) | CommandAction::RunProgram { .. } => "Open".to_string(),
        CommandAction::OpenUrl(_) => "Open link".to_string(),
        CommandAction::None if matches!(result.category, CommandCategory::Calculation) => {
            "Copy result".to_string()
        }
        CommandAction::None => "Select".to_string(),
    }
}

