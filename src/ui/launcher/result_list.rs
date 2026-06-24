use crate::{
    command::{CommandAction, CommandCategory, CommandResult},
    destiny,
    ui::{
        browse_views::{
            browse_action_bar, browse_action_hint, browse_primary_action,
            browse_scope_bar, browse_empty_state,
        },
        lucide_icons::LucideIcon,
    },
};
use gpui::{
    div, img, prelude::*, px, rgb, Context, MouseButton, MouseUpEvent, Window,
};
use super::{
    destiny_detail::{
        destiny_weapon_portrait_for_result, render_destiny_weapon_portrait,
        D2_SEARCH_WEAPON_ICON_SIZE,
    },
    fallback_icon, LauncherPanel, LauncherView, MoveSelectionDown,
    MoveSelectionFirst, MoveSelectionLast, MoveSelectionPageDown, MoveSelectionPageUp,
    MoveSelectionUp,
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
            cx.notify();
        }
    }

    pub(super) fn selectable_item_count(&self) -> usize {
        match &self.panel {
            LauncherPanel::Home => self.results.len(),
            LauncherPanel::TerminalShellPicker { .. } => self.available_shells.len(),
            LauncherPanel::TerminalSession(_) | LauncherPanel::D2WeaponDetail { .. } => 0,
        }
    }

    pub(super) fn accept_mouse_result(
        &mut self,
        result_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_index = result_index;
        self.accept_selected_result(window, cx);
    }

    pub(super) fn render_home_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.text_input.read(cx).content().to_string();
        let d2_scope_query = d2_scope_query_suffix(&query);
        let filter_pills = d2_scope_query
            .as_deref()
            .map(destiny::d2_query_filter_pills)
            .unwrap_or_default();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .when(d2_scope_query.is_some(), |panel| {
                let scope_query = d2_scope_query.clone().unwrap_or_default();
                let filter_label = if filter_pills.is_empty() {
                    if scope_query.trim().is_empty() {
                        "all weapons".to_string()
                    } else {
                        scope_query
                    }
                } else {
                    filter_pills.join(" · ")
                };
                panel.child(browse_scope_bar("Destiny 2", filter_label))
            })
            .child(self.render_results(cx))
            .child(self.render_launcher_action_bar(cx))
    }

    pub(super) fn render_launcher_action_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.text_input.read(cx).content().to_string();
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
        if d2_scope_query_suffix(&query).is_some() {
            hints.push(
                browse_action_hint("Tab", "Autocomplete").into_any_element(),
            );
        }

        browse_action_bar(browse_primary_action(primary_label).into_any_element(), hints)
    }

    pub(super) fn render_results(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let result_count = self.results.len();

        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .px(px(10.))
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
            )
    }

    pub(super) fn render_standard_result(
        &self,
        result: &CommandResult,
        result_index: usize,
        is_selected: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let title = compact_display_text(&result.title, 72);
        let subtitle = compact_display_text(&result.subtitle, 86);
        let explanation = compact_display_text(&result.explanation.clone().unwrap_or_default(), 96);
        let is_application = matches!(result.category, CommandCategory::Application);
        let shows_subtitle = should_show_subtitle(result);
        let shows_explanation = shows_subtitle && !explanation.is_empty();
        let category_label = category_label(&result.category);

        div()
            .id(("result-row", result_index))
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(10.))
            .py(px(8.))
            .rounded_sm()
            .bg(result_row_background(is_selected))
            .border_1()
            .border_color(result_row_border_color(result, is_selected))
            .hover(|style| style.bg(rgb(0x010101)).cursor_pointer())
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
                    .gap(px(2.))
                    .w_full()
                    .child(
                        div()
                            .text_size(if is_application { px(16.) } else { px(15.) })
                            .text_color(rgb(0xffffff))
                            .child(title),
                    )
                    .when(shows_subtitle, |result_text| {
                        result_text.child(
                            div()
                                .text_size(px(12.))
                                .text_color(rgb(0xd9d9d9))
                                .child(subtitle),
                        )
                    })
                    .when(shows_explanation, |result_text| {
                        result_text.child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(0xd9d9d9))
                                .child(explanation),
                        )
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .px(px(7.))
                    .py(px(3.))
                    .rounded_sm()
                    .bg(rgb(0x050505))
                    .text_color(category_color(&result.category))
                    .text_size(px(10.))
                    .child(category_label),
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

        div()
            .id(("result-row", result_index))
            .flex()
            .flex_col()
            .gap(px(8.))
            .px(px(8.))
            .py(px(7.))
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
                    launcher.accept_mouse_result(result_index, window, cx);
                }),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(rgb(0xd9d9d9))
                    .child("Calculator"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(118.))
                    .rounded_sm()
                    .bg(rgb(0x050505))
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
            .into_any_element()
    }

    pub(super) fn render_d2_manifest_download(
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
}
pub(super) fn render_result_icon(result: &CommandResult) -> gpui::AnyElement {
    if matches!(result.category, CommandCategory::Destiny) {
        let portrait = destiny_weapon_portrait_for_result(result);
        if portrait.has_any_icon() {
            return render_destiny_weapon_portrait(
                portrait,
                D2_SEARCH_WEAPON_ICON_SIZE,
                result.category.clone(),
            );
        }
    }

    if let Some(icon_path) = result.icon_path.clone() {
        return img(icon_path)
            .size(px(28.))
            .rounded_sm()
            .with_fallback(|| fallback_icon("A", rgb(0x123322)).into_any_element())
            .into_any_element();
    }

    let label = match &result.category {
        CommandCategory::Calculation => "=",
        CommandCategory::Application => "A",
        CommandCategory::File => "F",
        CommandCategory::BuiltIn => "S",
        CommandCategory::Web => "W",
        CommandCategory::Help => "?",
        CommandCategory::Note => "N",
        CommandCategory::Focus => "F",
        CommandCategory::Clipboard => "C",
        CommandCategory::WindowManagement => "W",
        CommandCategory::Snippet => "T",
        CommandCategory::Quicklink => "Q",
        CommandCategory::Calendar => "D",
        CommandCategory::System => "P",
        CommandCategory::Emoji => ":",
        CommandCategory::Destiny => "D2",
        CommandCategory::Context => "Ctx",
        CommandCategory::DevTools => "Dev",
        CommandCategory::Git => "Git",
        CommandCategory::Package => "Pkg",
        CommandCategory::Lookup => "Lu",
        CommandCategory::Media => "Md",
        CommandCategory::Network => "Nt",
    };

    fallback_icon(label, category_color(&result.category)).into_any_element()
}

pub(super) fn should_show_subtitle(result: &CommandResult) -> bool {
    matches!(
        result.category,
        CommandCategory::Calculation
            | CommandCategory::File
            | CommandCategory::BuiltIn
            | CommandCategory::Help
            | CommandCategory::Note
            | CommandCategory::Focus
            | CommandCategory::Clipboard
            | CommandCategory::WindowManagement
            | CommandCategory::Snippet
            | CommandCategory::Quicklink
            | CommandCategory::Calendar
            | CommandCategory::System
            | CommandCategory::Emoji
            | CommandCategory::Destiny
            | CommandCategory::Context
            | CommandCategory::DevTools
            | CommandCategory::Git
            | CommandCategory::Package
            | CommandCategory::Lookup
            | CommandCategory::Media
            | CommandCategory::Network
    ) && !result.subtitle.is_empty()
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
    match category {
        CommandCategory::Calculation => rgb(0x22c55e),
        CommandCategory::Application => rgb(0x38bdf8),
        CommandCategory::File => rgb(0xffffff),
        CommandCategory::BuiltIn => rgb(0xf59e0b),
        CommandCategory::Web => rgb(0xa78bfa),
        CommandCategory::Help => rgb(0x64748b),
        CommandCategory::Note => rgb(0xfacc15),
        CommandCategory::Focus => rgb(0xef4444),
        CommandCategory::Clipboard => rgb(0x14b8a6),
        CommandCategory::WindowManagement => rgb(0x60a5fa),
        CommandCategory::Snippet => rgb(0xf472b6),
        CommandCategory::Quicklink => rgb(0x2dd4bf),
        CommandCategory::Calendar => rgb(0x818cf8),
        CommandCategory::System => rgb(0xe5e7eb),
        CommandCategory::Emoji => rgb(0xfb7185),
        CommandCategory::Destiny => rgb(0x7c3aed),
        CommandCategory::Context => rgb(0x94a3b8),
        CommandCategory::DevTools => rgb(0xfbbf24),
        CommandCategory::Git => rgb(0xf97316),
        CommandCategory::Package => rgb(0x34d399),
        CommandCategory::Lookup => rgb(0x60a5fa),
        CommandCategory::Media => rgb(0xe879f9),
        CommandCategory::Network => rgb(0x4ade80),
    }
}

pub(super) fn category_label(category: &CommandCategory) -> &'static str {
    match category {
        CommandCategory::Calculation => "Calc",
        CommandCategory::Application => "App",
        CommandCategory::File => "File",
        CommandCategory::BuiltIn => "Core",
        CommandCategory::Web => "Web",
        CommandCategory::Help => "Help",
        CommandCategory::Note => "Note",
        CommandCategory::Focus => "Focus",
        CommandCategory::Clipboard => "Clip",
        CommandCategory::WindowManagement => "Window",
        CommandCategory::Snippet => "Snippet",
        CommandCategory::Quicklink => "Link",
        CommandCategory::Calendar => "Calendar",
        CommandCategory::System => "System",
        CommandCategory::Emoji => "Emoji",
        CommandCategory::Destiny => "D2",
        CommandCategory::Context => "Ctx",
        CommandCategory::DevTools => "Dev",
        CommandCategory::Git => "Git",
        CommandCategory::Package => "Pkg",
        CommandCategory::Lookup => "Lookup",
        CommandCategory::Media => "Media",
        CommandCategory::Network => "Net",
    }
}

pub(super) fn result_row_background(is_selected: bool) -> gpui::Rgba {
    if is_selected {
        rgb(0x050505)
    } else {
        rgb(0x000000)
    }
}

pub(super) fn result_row_border_color(result: &CommandResult, is_selected: bool) -> gpui::Rgba {
    if is_selected {
        return category_color(&result.category);
    }

    match result.category {
        CommandCategory::Note
        | CommandCategory::Clipboard
        | CommandCategory::File
        | CommandCategory::Calendar => rgb(0x171717),
        _ => rgb(0x050505),
    }
}

fn d2_scope_query_suffix(query: &str) -> Option<String> {
    let trimmed = query.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower == "@d2" {
        return Some(String::new());
    }
    if let Some(suffix) = lower.strip_prefix("@d2 ") {
        return Some(suffix.to_string());
    }
    None
}

fn primary_action_label(result: &CommandResult) -> String {
    match &result.action {
        CommandAction::CopyToClipboard(_) => "Copy".to_string(),
        CommandAction::Feature(_) => match result.category {
            CommandCategory::Destiny => "View weapon".to_string(),
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

