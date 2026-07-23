use gpui::{div, prelude::*, px, AnyElement, IntoElement, ParentElement, Rgba, Styled};

use super::{
    lucide_icons::{self, LucideIcon},
    theme::{self, colors, type_scale},
};

pub const BROWSE_LIST_PANE_WIDTH: f32 = 420.;
pub const BROWSE_PREVIEW_PANE_WIDTH: f32 = 500.;
pub const BROWSE_PREVIEW_BODY_HEIGHT: f32 = 220.;

pub const LAUNCHER_SURFACE: u32 = colors::SURFACE;
pub const LAUNCHER_SURFACE_BLUR: u32 = colors::SURFACE_BLUR;

pub fn launcher_background(backdrop_blur_enabled: bool) -> Rgba {
    theme::launcher_background(backdrop_blur_enabled)
}

pub fn surface_overlay_low() -> Rgba {
    theme::surface_overlay_low()
}

pub fn surface_overlay_mid() -> Rgba {
    theme::surface_overlay_mid()
}

pub fn surface_muted() -> Rgba {
    theme::surface_muted()
}

pub fn result_row_background(is_selected: bool) -> Rgba {
    theme::result_row_background(is_selected)
}

pub fn result_row_hover_background() -> Rgba {
    theme::result_row_hover_background()
}

pub fn result_row_badge_background() -> Rgba {
    theme::result_row_badge_background()
}

pub fn border_subtle() -> Rgba {
    theme::border_subtle()
}

pub fn input_field_background(focused: bool) -> Rgba {
    theme::input_field_background(focused)
}

pub fn input_field_border(focused: bool) -> Rgba {
    theme::input_field_border(focused)
}

pub fn elevated_surface_background() -> Rgba {
    theme::elevated_surface_background()
}

pub fn elevated_surface_border() -> Rgba {
    theme::border_subtle()
}

pub fn panel_surface_background() -> Rgba {
    theme::panel_surface_background()
}

pub fn icon_button_background() -> Rgba {
    theme::icon_button_background()
}

pub fn icon_button_hover_background() -> Rgba {
    theme::icon_button_hover_background()
}

pub fn settings_sidebar_background() -> Rgba {
    theme::settings_sidebar_background()
}

pub fn settings_row_background() -> Rgba {
    theme::settings_row_background()
}

pub fn settings_row_hover_background() -> Rgba {
    theme::settings_row_hover_background()
}

pub fn settings_row_selected_background() -> Rgba {
    theme::settings_row_selected_background()
}

pub fn browse_scope_bar(
    section_label: impl Into<String>,
    filter_label: impl Into<String>,
) -> impl IntoElement {
    let section_label = section_label.into();
    let filter_label = filter_label.into();
    let has_section_label = !section_label.is_empty();

    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_between()
        .px(px(12.))
        .py(px(6.))
        .when(has_section_label, |bar| {
            bar.child(
                div()
                    .text_size(px(type_scale::BODY_SM))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(colors::text_secondary())
                    .child(section_label),
            )
        })
        .when(!has_section_label, |bar| bar.child(div()))
        .child(browse_filter_pill(filter_label))
}

pub fn browse_filter_pill(label: impl Into<String>) -> impl IntoElement {
    div()
        .px(px(10.))
        .py(px(5.))
        .rounded_md()
        .bg(surface_overlay_low())
        .border_1()
        .border_color(border_subtle())
        .text_size(px(type_scale::LABEL))
        .text_color(colors::text_secondary())
        .child(label.into())
}

pub fn browse_split_pane(left: AnyElement, right: AnyElement) -> impl IntoElement {
    div()
        .flex()
        .flex_1()
        .min_h(px(0.))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .h_full()
                .flex()
                .flex_col()
                .min_h(px(0.))
                .border_r_1()
                .border_color(border_subtle())
                .child(left),
        )
        .child(
            div()
                .flex_none()
                .w(px(BROWSE_PREVIEW_PANE_WIDTH))
                .h_full()
                .overflow_hidden()
                .flex()
                .flex_col()
                .child(right),
        )
}

pub fn browse_empty_state(
    icon: Option<LucideIcon>,
    title: impl Into<String>,
    subtitle: impl Into<String>,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .h_full()
        .w_full()
        .px(px(24.))
        .when(icon.is_some(), |state| {
            state.child(lucide_icons::render_lucide_icon(
                icon.unwrap(),
                28.,
                colors::text_disabled(),
                false,
            ))
        })
        .child(
            div()
                .text_size(px(type_scale::BODY_LG))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(colors::text_secondary())
                .child(title.into()),
        )
        .child(
            div()
                .text_size(px(type_scale::BODY_SM))
                .text_color(colors::text_faint())
                .text_center()
                .child(subtitle.into()),
        )
        .into_any()
}

pub fn browse_preview_section_label(label: impl Into<String>) -> impl IntoElement {
    div()
        .pb(px(6.))
        .text_size(px(type_scale::BODY_SM))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(colors::text_muted())
        .child(label.into())
}

pub fn browse_metadata_row(label: impl Into<String>, value: impl Into<String>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .py(px(5.))
        .child(
            div()
                .text_size(px(type_scale::BODY_SM))
                .text_color(colors::text_faint())
                .child(label.into()),
        )
        .child(
            div()
                .text_size(px(type_scale::BODY_SM))
                .text_color(colors::text_secondary())
                .text_align(gpui::TextAlign::Right)
                .child(value.into()),
        )
}

pub fn browse_action_hint(keys: impl Into<String>, action: impl Into<String>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .px(px(6.))
                .py(px(2.))
                .rounded_md()
                .bg(surface_overlay_low())
                .border_1()
                .border_color(border_subtle())
                .text_size(px(type_scale::CAPTION))
                .text_color(colors::text_secondary())
                .child(keys.into()),
        )
        .child(
            div()
                .text_size(px(type_scale::LABEL))
                .text_color(colors::text_faint())
                .child(action.into()),
        )
}

pub fn browse_action_bar(primary: AnyElement, hints: Vec<AnyElement>) -> impl IntoElement {
    div()
        .id("launcher-action-bar")
        .w_full()
        .flex()
        .flex_none()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .mt_auto()
        .px(px(14.))
        .py(px(8.))
        .min_h(px(36.))
        .bg(elevated_surface_background())
        .border_t_1()
        .border_color(border_subtle())
        .child(
            div()
                .flex_none()
                .min_w(px(0.))
                .overflow_hidden()
                .child(primary),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(12.))
                .children(hints),
        )
}

pub fn browse_primary_action(label: impl Into<String>) -> impl IntoElement {
    div()
        .text_size(px(type_scale::BODY_SM))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(colors::text_secondary())
        .child(label.into())
}

pub fn browse_action_button(
    label: impl Into<String>,
    icon: LucideIcon,
    accent: u32,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(type_scale::BODY_SM))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(colors::text_secondary())
        .hover(|style| style.text_color(colors::text_primary()).cursor_pointer())
        .child(lucide_icons::render_lucide_icon(
            icon,
            12.,
            gpui::rgb(accent),
            false,
        ))
        .child(label.into())
}

pub fn result_category_label(label: impl Into<String>) -> impl IntoElement {
    div()
        .flex_none()
        .text_size(px(type_scale::LABEL))
        .text_color(colors::text_faint())
        .child(label.into())
}

pub fn primary_button(label: impl Into<String>) -> gpui::Div {
    div()
        .px(px(10.))
        .py(px(5.))
        .rounded_sm()
        .bg(colors::accent())
        .text_color(colors::text_primary())
        .text_size(px(type_scale::BODY_SM))
        .hover(|style| style.bg(colors::accent_hover()).cursor_pointer())
        .child(label.into())
}
