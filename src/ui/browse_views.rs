use gpui::{div, prelude::*, px, rgb, rgba, AnyElement, IntoElement, ParentElement, Rgba, Styled};

use super::lucide_icons::{self, LucideIcon};

pub const BROWSE_LIST_PANE_WIDTH: f32 = 420.;
pub const BROWSE_PREVIEW_PANE_WIDTH: f32 = 500.;
pub const BROWSE_PREVIEW_BODY_HEIGHT: f32 = 220.;

pub const LAUNCHER_SURFACE: u32 = 0x090909;
pub const LAUNCHER_SURFACE_BLUR: u32 = 0x090909f7;

pub fn launcher_background(backdrop_blur_enabled: bool) -> Rgba {
    if backdrop_blur_enabled {
        rgba(LAUNCHER_SURFACE_BLUR)
    } else {
        rgb(LAUNCHER_SURFACE)
    }
}

pub fn surface_overlay_low() -> Rgba {
    rgba(0xffffff0a)
}

pub fn surface_overlay_mid() -> Rgba {
    rgba(0xffffff12)
}

pub fn surface_muted() -> Rgba {
    surface_overlay_low()
}

pub fn result_row_background(is_selected: bool) -> Rgba {
    if is_selected {
        result_row_hover_background()
    } else {
        rgba(0x00000000)
    }
}

pub fn result_row_hover_background() -> Rgba {
    surface_overlay_mid()
}

pub fn result_row_badge_background() -> Rgba {
    rgba(0xffffff10)
}

pub fn border_subtle() -> Rgba {
    rgba(0xffffff14)
}

pub fn input_field_background(focused: bool) -> Rgba {
    if focused {
        surface_overlay_mid()
    } else {
        surface_overlay_low()
    }
}

pub fn input_field_border(focused: bool) -> Rgba {
    if focused {
        rgba(0xffffff20)
    } else {
        rgba(0x00000000)
    }
}

pub fn elevated_surface_background() -> Rgba {
    surface_overlay_low()
}

pub fn elevated_surface_border() -> Rgba {
    border_subtle()
}

pub fn panel_surface_background() -> Rgba {
    surface_overlay_low()
}

pub fn icon_button_background() -> Rgba {
    rgba(0x00000000)
}

pub fn icon_button_hover_background() -> Rgba {
    surface_overlay_mid()
}

pub fn settings_sidebar_background() -> Rgba {
    rgba(0x00000000)
}

pub fn settings_row_background() -> Rgba {
    rgba(0x00000000)
}

pub fn settings_row_hover_background() -> Rgba {
    surface_overlay_mid()
}

pub fn settings_row_selected_background() -> Rgba {
    surface_overlay_mid()
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
                    .text_size(px(12.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(0xe4e4e7))
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
        .text_size(px(11.))
        .text_color(rgb(0xd4d4d8))
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
                rgb(0x52525b),
                false,
            ))
        })
        .child(
            div()
                .text_size(px(14.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(rgb(0xd4d4d8))
                .child(title.into()),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(rgb(0x71717a))
                .text_center()
                .child(subtitle.into()),
        )
        .into_any()
}

pub fn browse_preview_section_label(label: impl Into<String>) -> impl IntoElement {
    div()
        .pb(px(6.))
        .text_size(px(12.))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(rgb(0xa1a1aa))
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
                .text_size(px(12.))
                .text_color(rgb(0x71717a))
                .child(label.into()),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(rgb(0xf4f4f5))
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
                .text_size(px(10.))
                .text_color(rgb(0xd4d4d8))
                .child(keys.into()),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(rgb(0x71717a))
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
        .bg(surface_overlay_low())
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
        .text_size(px(12.))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(rgb(0xf4f4f5))
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
        .text_size(px(12.))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(rgb(0xf4f4f5))
        .hover(|style| style.text_color(rgb(0xffffff)).cursor_pointer())
        .child(lucide_icons::render_lucide_icon(icon, 12., rgb(accent), false))
        .child(label.into())
}

pub fn result_category_label(label: impl Into<String>) -> impl IntoElement {
    div()
        .flex_none()
        .text_size(px(11.))
        .text_color(rgb(0x71717a))
        .child(label.into())
}