use crate::{
    command::{CommandAction, CommandCategory, CommandResult, FeatureAction},
    destiny::{self, get_weapon_detail, saved_perk_labels, update_favorite},
    ui::lucide_icons::{self, LucideIcon},
};
use gpui::{
    div, img, prelude::*, px, rgb, Context, MouseButton, MouseUpEvent, Render, SharedString, Window,
};
use std::path::PathBuf;

use super::{
    ClearD2WeaponCompare, LauncherPanel, LauncherView, StartD2WeaponCompare,
};
use super::result_list::category_color;

struct D2PerkTooltipContent {
    is_saved: bool,
    name: SharedString,
    body: SharedString,
    from_clarity: bool,
}

impl Render for D2PerkTooltipContent {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let mut tooltip = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(12.))
            .py(px(10.))
            .bg(rgb(0x18181b))
            .border_1()
            .border_color(rgb(0x3f3f46))
            .rounded(px(6.))
            .w(px(248.))
            .max_w(px(248.))
            .shadow_md()
            .child(
                div()
                    .w_full()
                    .text_size(px(13.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(0xf4f4f5))
                    .child(self.name.clone()),
            );

        if self.from_clarity {
            tooltip = tooltip.child(
                div()
                    .text_size(px(10.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(rgb(0xc084fc))
                    .child("Clarity"),
            );
        }

        tooltip
            .child(
                div()
                    .w_full()
                    .text_size(px(11.))
                    .text_color(rgb(0xa1a1aa))
                    .line_height(px(16.))
                    .child(self.body.clone()),
            )
            .child(
                div()
                    .pt(px(4.))
                    .text_size(px(10.))
                    .text_color(rgb(0x71717a))
                    .child(if self.is_saved {
                        "Left-click to remove from saved roll"
                    } else {
                        "Left-click to save to roll"
                    }),
            )
    }
}

fn d2_perk_tooltip_body(perk: &destiny::Perk) -> String {
    if perk.tooltip_text.is_empty() {
        destiny::perk_tooltip_text(perk)
    } else {
        perk.tooltip_text.clone()
    }
}

fn d2_perk_element_id(perk: &destiny::Perk) -> u32 {
    if perk.hash != 0 {
        perk.hash
    } else {
        perk.name
            .bytes()
            .fold(0u32, |acc, byte| acc.wrapping_mul(31).wrapping_add(byte as u32))
    }
}



pub(super) const D2_SEARCH_WEAPON_ICON_SIZE: f32 = 48.;
const D2_DETAIL_HEADER_NAME_SIZE: f32 = 18.;
const D2_DETAIL_HEADER_NAME_LINE_HEIGHT: f32 = 22.;
const D2_DETAIL_HEADER_META_SIZE: f32 = 11.;
const D2_DETAIL_HEADER_META_LINE_HEIGHT: f32 = 14.;
const D2_DETAIL_HEADER_TEXT_GAP: f32 = 2.;

fn d2_detail_header_icon_size() -> f32 {
    D2_DETAIL_HEADER_NAME_LINE_HEIGHT + D2_DETAIL_HEADER_TEXT_GAP + D2_DETAIL_HEADER_META_LINE_HEIGHT
}

const D2_WEAPON_STAT_BAR_HEIGHT: f32 = 18.;
const D2_WEAPON_STAT_BAR_GAP: f32 = 4.;
const D2_WEAPON_STAT_BAR_WIDTH: f32 = 136.;

fn d2_weapon_stat_bar_fill_ratio(value: i32) -> f32 {
    (value as f32 / 100.0).clamp(0.0, 1.0)
}
fn render_d2_weapon_stat_bar(stat: &destiny::WeaponStat) -> gpui::AnyElement {
    let fill_ratio = d2_weapon_stat_bar_fill_ratio(stat.value);
    let fill_width = px(D2_WEAPON_STAT_BAR_WIDTH * fill_ratio);
    let value_label = stat.value.to_string();

    div()
        .relative()
        .w(px(D2_WEAPON_STAT_BAR_WIDTH))
        .h(px(D2_WEAPON_STAT_BAR_HEIGHT))
        .rounded(px(3.))
        .overflow_hidden()
        .bg(rgb(0x18181b))
        .border_1()
        .border_color(rgb(0x27272a))
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .bottom_0()
                .w(fill_width)
                .bg(rgb(0x52525b)),
        )
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .flex()
                .items_center()
                .justify_between()
                .px(px(6.))
                .child(
                    div()
                        .text_size(px(10.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgb(0xf4f4f5))
                        .child(stat.name.clone()),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(0xffffff))
                        .child(value_label),
                ),
        )
        .into_any_element()
}

fn d2_weapon_stats_panel_height(stat_count: usize) -> f32 {
    const HEADER: f32 = 18.;
    const META_ROW: f32 = 28.;
    const ROW: f32 = D2_WEAPON_STAT_BAR_HEIGHT + D2_WEAPON_STAT_BAR_GAP;
    const EMPTY_MESSAGE: f32 = 18.;

    HEADER
        + META_ROW
        + if stat_count == 0 {
            EMPTY_MESSAGE
        } else {
            ROW * stat_count as f32
        }
}

fn d2_weapon_detail_height(max_perks: usize, stat_count: usize) -> f32 {
    const HEADER_HEIGHT: f32 = 56.;
    const FOOTER_HEIGHT: f32 = 34.;
    const VERTICAL_PADDING: f32 = 24.;
    const SECTION_GAPS: f32 = 16.;
    const PERK_SECTION_PADDING: f32 = 4.;
    const COLUMN_LABEL_HEIGHT: f32 = 11.;
    const PERK_ROW_HEIGHT: f32 = 40.;

    let perk_column_height = if max_perks == 0 {
        0.
    } else {
        COLUMN_LABEL_HEIGHT + PERK_ROW_HEIGHT * max_perks as f32
    };
    let body_height = perk_column_height.max(d2_weapon_stats_panel_height(stat_count));

    HEADER_HEIGHT
        + SECTION_GAPS
        + PERK_SECTION_PADDING
        + body_height
        + SECTION_GAPS
        + FOOTER_HEIGHT
        + VERTICAL_PADDING
}

fn render_d2_weapon_stats_panel(weapon: &destiny::DestinyWeapon) -> gpui::AnyElement {
    let card_icons = destiny_weapon_card_icons_for_weapon(weapon);
    let mut meta_row = div().flex().items_center().gap(px(6.)).min_h(px(24.));

    if let Some(path) = card_icons.damage_icon {
        meta_row = meta_row.child(img(path).w(px(16.)).h(px(16.)));
    }
    if let Some(label) = weapon.damage_type.clone() {
        meta_row = meta_row.child(
            div()
                .text_size(px(10.))
                .text_color(rgb(0xd4d4d8))
                .child(label),
        );
    }
    if let Some(path) = card_icons.ammo_icon {
        meta_row = meta_row.child(img(path).w(px(16.)).h(px(16.)));
    }
    if let Some(label) = weapon.ammo_type.clone() {
        meta_row = meta_row.child(
            div()
                .text_size(px(10.))
                .text_color(rgb(0xd4d4d8))
                .child(label),
        );
    }

    let mut panel = div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .w(px(152.))
        .flex_shrink_0()
        .pl(px(12.))
        .border_l_1()
        .border_color(rgb(0x27272a))
        .child(
            div()
                .text_size(px(10.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(0x71717a))
                .child("STATS"),
        )
        .child(meta_row);

    if weapon.stats.is_empty() {
        panel = panel.child(
            div()
                .text_size(px(10.))
                .text_color(rgb(0x52525b))
                .line_height(px(14.))
                .child("Stats load after the next manifest refresh."),
        );
    } else {
        for stat in &weapon.stats {
            panel = panel.child(render_d2_weapon_stat_bar(stat));
        }
    }

    panel.into_any_element()
}

#[derive(Clone, Default)]
pub(super) struct DestinyWeaponPortraitPaths {
    weapon_icon: Option<PathBuf>,
    /// Bottom strip shadow (`watermark-layer.png`).
    season_strip_shadow: Option<PathBuf>,
    /// Small top-left season badge (`secondaryBackground`, then `iconWatermark`).
    season_corner_icon: Option<PathBuf>,
}

#[derive(Clone, Default)]
struct DestinyWeaponCardIcons {
    damage_icon: Option<PathBuf>,
    ammo_icon: Option<PathBuf>,
}

impl DestinyWeaponPortraitPaths {
    pub(super) fn has_any_icon(&self) -> bool {
        self.weapon_icon.is_some()
            || self.season_strip_shadow.is_some()
            || self.season_corner_icon.is_some()
    }
}

fn destiny_weapon_portrait_for_weapon(weapon: &destiny::DestinyWeapon) -> DestinyWeaponPortraitPaths {
    use crate::destiny::{
        request_icon_download, weapon_icon_if_cached, weapon_season_banner_if_cached,
        weapon_season_banner_shadow_if_cached, weapon_season_watermark_if_cached,
    };

    for path in [
        weapon.icon_path.as_deref(),
        weapon.season_banner_overlay_path.as_deref(),
        weapon.season_banner_shadow_path.as_deref(),
        weapon.season_watermark_path.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        request_icon_download(path);
    }

    let season_corner_icon = weapon
        .season_banner_overlay_path
        .as_deref()
        .and_then(destiny::icon_if_cached)
        .or_else(|| weapon_season_banner_if_cached(weapon))
        .or_else(|| {
            weapon
                .season_watermark_path
                .as_deref()
                .and_then(destiny::icon_if_cached)
        })
        .or_else(|| weapon_season_watermark_if_cached(weapon));

    DestinyWeaponPortraitPaths {
        weapon_icon: weapon
            .icon_path
            .as_deref()
            .and_then(destiny::icon_if_cached)
            .or_else(|| weapon_icon_if_cached(weapon)),
        season_strip_shadow: weapon
            .season_banner_shadow_path
            .as_deref()
            .and_then(destiny::icon_if_cached)
            .or_else(|| weapon_season_banner_shadow_if_cached(weapon))
            .filter(|_| season_corner_icon.is_some()),
        season_corner_icon,
    }
}

fn destiny_weapon_card_icons_for_weapon(weapon: &destiny::DestinyWeapon) -> DestinyWeaponCardIcons {
    use crate::destiny::{
        request_icon_download, weapon_ammo_icon_if_cached, weapon_damage_icon_if_cached,
    };

    destiny::ensure_bundled_ammo_icons();

    if let Some(path) = weapon.damage_type_icon_path.as_deref() {
        request_icon_download(path);
    }

    DestinyWeaponCardIcons {
        damage_icon: weapon
            .damage_type_icon_path
            .as_deref()
            .and_then(destiny::icon_if_cached)
            .or_else(|| weapon_damage_icon_if_cached(weapon)),
        ammo_icon: weapon_ammo_icon_if_cached(weapon),
    }
}

fn destiny_weapon_card_icons_for_result(result: &CommandResult) -> DestinyWeaponCardIcons {
    if result.icon_path.is_some() {
        return DestinyWeaponCardIcons::default();
    }
    if let CommandAction::Feature(FeatureAction::OpenDestinyWeapon { weapon_hash }) = &result.action
    {
        if let Some((weapon, _)) = destiny::get_weapon_detail(*weapon_hash) {
            return destiny_weapon_card_icons_for_weapon(&weapon);
        }
    }

    DestinyWeaponCardIcons::default()
}
pub(super) fn destiny_weapon_portrait_for_result(result: &CommandResult) -> DestinyWeaponPortraitPaths {
    if result.icon_path.is_some() {
        return DestinyWeaponPortraitPaths {
            weapon_icon: result.icon_path.clone(),
            ..Default::default()
        };
    }

    if let CommandAction::Feature(FeatureAction::OpenDestinyWeapon { weapon_hash }) = &result.action
    {
        if let Some((weapon, _)) = destiny::get_weapon_detail(*weapon_hash) {
            return destiny_weapon_portrait_for_weapon(&weapon);
        }
    }

    DestinyWeaponPortraitPaths {
        weapon_icon: result.icon_path.clone(),
        ..Default::default()
    }
}

/// Small season badge in the top-left corner (DIM: ~11px on a 50px tile).
fn d2_season_corner_icon_size(icon_size: f32) -> f32 {
    (icon_size * 0.23).clamp(10., 20.)
}

/// Bottom season strip — only the shadow layer, clipped to the lower edge of the icon.
fn d2_season_strip_height(icon_size: f32) -> f32 {
    (icon_size * 0.28).clamp(10., 26.)
}

/// Shift the watermark-layer up so the visible band hugs the bottom edge (not the icon midpoint).
fn d2_season_strip_image_lift(icon_size: f32) -> f32 {
    (icon_size * 0.20).clamp(6., 18.)
}

fn render_destiny_season_strip_shadow(path: PathBuf, icon_size: f32) -> gpui::AnyElement {
    let strip_height = d2_season_strip_height(icon_size);
    let image_lift = d2_season_strip_image_lift(icon_size);
    let icon_px = px(icon_size);
    div()
        .absolute()
        .bottom_0()
        .left_0()
        .w_full()
        .h(px(strip_height))
        .overflow_hidden()
        .child(
            div()
                .absolute()
                .bottom(px(image_lift))
                .left_0()
                .w_full()
                .h(icon_px)
                .child(img(path).w_full().h_full()),
        )
        .into_any_element()
}

fn render_destiny_overlay_icon(path: PathBuf, size: f32) -> gpui::AnyElement {
    let size_px = px(size);
    div()
        .size(size_px)
        .overflow_hidden()
        .child(img(path).w_full().h_full())
        .into_any_element()
}

pub(super) fn render_destiny_weapon_portrait(
    paths: DestinyWeaponPortraitPaths,
    icon_size: f32,
    category: CommandCategory,
) -> gpui::AnyElement {
    let icon_px = px(icon_size);
    let corner_icon_size = d2_season_corner_icon_size(icon_size);
    let fallback_color = category_color(&category);

    let weapon_icon = paths
        .weapon_icon
        .map(|path| {
            img(path)
                .w_full()
                .h_full()
                .with_fallback(move || {
                    lucide_icons::render_lucide_badge(LucideIcon::Swords, icon_size, fallback_color)
                        .into_any_element()
                })
                .into_any_element()
        })
        .unwrap_or_else(|| {
            lucide_icons::render_lucide_badge(LucideIcon::Swords, icon_size, fallback_color)
        });

    let mut icon_layers = div()
        .relative()
        .w_full()
        .h_full()
        .child(weapon_icon);

    if let Some(shadow) = paths.season_strip_shadow {
        icon_layers = icon_layers.child(render_destiny_season_strip_shadow(shadow, icon_size));
    }

    if let Some(corner_icon) = paths.season_corner_icon {
        icon_layers = icon_layers.child(
            div()
                .absolute()
                .top(px(1.))
                .left(px(1.))
                .size(px(corner_icon_size))
                .overflow_hidden()
                .child(img(corner_icon).w_full().h_full()),
        );
    }

    div()
        .flex()
        .flex_none()
        .w(icon_px)
        .h(icon_px)
        .overflow_hidden()
        .rounded_md()
        .border_1()
        .border_color(rgb(0x3f3f46))
        .bg(rgb(0x18181b))
        .child(icon_layers)
        .into_any_element()
}


fn render_d2_compare_stats_panel(
    primary: &destiny::DestinyWeapon,
    compare: Option<&destiny::DestinyWeapon>,
) -> gpui::AnyElement {
    let stat_names: Vec<String> = primary.stats.iter().map(|stat| stat.name.clone()).collect();

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
    stat_name: &str,
    primary_value: i32,
    compare_value: Option<i32>,
) -> gpui::Div {
    let delta_text = compare_value.map(|compare| {
        let delta = primary_value - compare;
        if delta > 0 {
            format!(" (+{delta})")
        } else if delta < 0 {
            format!(" ({delta})")
        } else {
            " (=)".to_string()
        }
    });

    div()
        .flex()
        .items_center()
        .justify_between()
        .rounded_sm()
        .bg(rgb(0x050505))
        .px(px(10.))
        .py(px(6.))
        .child(
            div()
                .text_size(px(12.))
                .text_color(rgb(0xd4d4d8))
                .child(stat_name.to_string()),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(rgb(0xffffff))
                .child(match (compare_value, delta_text) {
                    (Some(compare), Some(delta)) => {
                        format!("{primary_value} vs {compare}{delta}")
                    }
                    _ => primary_value.to_string(),
                }),
        )
}
impl LauncherView {
    pub(super) fn render_d2_weapon_detail(
        &self,
        weapon_hash: u32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {

        



        let (weapon, favorite) = match get_weapon_detail(weapon_hash) {
            Some(pair) => pair,

            None => {

                return div()

                    .p(px(16.))

                    .text_color(rgb(0x9ca3af))

                    .child("Weapon not found in cache. Try running a @d2 search after the manifest finishes downloading.")

                    .into_any_element();

            }

        };
        let compare_hash = self.compare_weapon_hash;
        let compare_weapon = compare_hash.and_then(|hash| get_weapon_detail(hash).map(|(w, _)| w));
        destiny::prefetch_weapon_icons(weapon_hash);

        let weapon_portrait = render_destiny_weapon_portrait(
            destiny_weapon_portrait_for_weapon(&weapon),
            d2_detail_header_icon_size(),
            CommandCategory::Destiny,
        );


        let season_badge = destiny::weapon_season_label(&weapon);


        let favorite_filled = favorite.favorited;

        let roles_text = if favorite.roles.is_empty() {
            "no roles yet".to_string()
        } else {
            favorite.roles.join(" • ")
        };
        let saved_perks = saved_perk_labels(&weapon, &favorite);
        let saved_perks_text = if saved_perks.is_empty() {
            "hover perks for details · left-click to save".to_string()
        } else {
            saved_perks.join(" • ")
        };


        let max_perks = weapon
            .perk_columns
            .iter()
            .map(|column| column.perks.len())
            .max()
            .unwrap_or(0);

        let mut root = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(12.))
            .w_full()
            .min_h(px(d2_weapon_detail_height(max_perks, weapon.stats.len())))
            .text_color(rgb(0xe4e4e7))
            .child(

                // Header: icon + name + season + favorite toggle
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .child(weapon_portrait)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(D2_DETAIL_HEADER_TEXT_GAP))
                            .h(px(d2_detail_header_icon_size()))
                            .justify_center()
                            .child(
                                div()
                                    .text_size(px(D2_DETAIL_HEADER_NAME_SIZE))
                                    .line_height(px(D2_DETAIL_HEADER_NAME_LINE_HEIGHT))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(weapon.name.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .text_size(px(D2_DETAIL_HEADER_META_SIZE))
                                    .line_height(px(D2_DETAIL_HEADER_META_LINE_HEIGHT))
                                    .text_color(rgb(0xa1a1aa))
                                    .child(season_badge.clone())
                                    .child("•")
                                    .child(weapon.archetype.clone().unwrap_or_default()),
                            ),
                    )
                    .child(
                        // Favorite heart + roles

                        div()

                            .ml_auto()

                            .flex()

                            .flex_col()

                            .items_end()

                            .gap(px(2.))

                            .child(
                                div()
                                    .id("d2-fav-toggle")
                                    .px(px(8.))
                                    .py(px(2.))
                                    .rounded(px(4.))
                                    .cursor_pointer()
                                    .child(lucide_icons::render_lucide_icon(
                                        if favorite_filled {
                                            LucideIcon::Heart
                                        } else {
                                            LucideIcon::HeartOff
                                        },
                                        18.,
                                        if favorite_filled {
                                            rgb(0xf472b6)
                                        } else {
                                            rgb(0xa1a1aa)
                                        },
                                        favorite_filled,
                                    ))
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(move |launcher, _event: &MouseUpEvent, _window, cx| {

                                            update_favorite(weapon_hash, |fav| {

                                                fav.favorited = !fav.favorited;

                                            });

                                            let current_query = launcher.text_input.read(cx).content().to_string();

                                            launcher.rebuild_results(&current_query);

                                            cx.notify();

                                        }),

                                    ),

                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(rgb(0x71717a))
                                    .child(format!("roles: {}", roles_text)),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(rgb(0xa78bfa))
                                    .child(format!("saved: {}", saved_perks_text)),
                            ),
                    ),
            );

        // Perk columns — hover for overlay tooltip, left-click saves
        let column_elements: Vec<_> = weapon
            .perk_columns
            .iter()
            .map(|column| {
                let perk_icons: Vec<_> = column
                    .perks
                    .iter()
                    .map(|perk| {
                        self.render_d2_perk_icon(
                            perk,
                            weapon_hash,
                            favorite.saved_perk_hashes.contains(&perk.hash),
                            cx,
                        )
                    })
                    .collect();
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.))
                    .min_w(px(36.))
                    .children(perk_icons)
                    .into_any_element()
            })
            .collect();

        let perk_section = div()
            .flex()
            .flex_row()
            .items_start()
            .gap(px(10.))
            .children(column_elements);

        root = root.child(
            div()
                .flex()
                .flex_row()
                .items_start()
                .gap(px(12.))
                .pt(px(4.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .child(perk_section),
                )
                .child(render_d2_weapon_stats_panel(&weapon)),
        );

        if compare_weapon.is_some() {
            root = root.child(render_d2_compare_stats_panel(&weapon, compare_weapon.as_ref()));
        }

        let footer_hint = if compare_weapon.is_some() {
            "C compare · X clear compare · Esc back"
        } else {
            "Hover perks for details · Left-click to save · C compare · Esc back"
        };

        root = root.child(
            div()
                .pt(px(8.))
                .text_size(px(10.))
                .text_color(rgb(0x52525b))
                .child(footer_hint),
        );

        root.into_any_element()
    }

    pub(super) fn render_d2_perk_icon(
        &self,
        perk: &destiny::Perk,
        weapon_hash: u32,
        is_saved: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let perk_hash = perk.hash;
        let perk_element_id = d2_perk_element_id(perk);
        let tooltip_body = d2_perk_tooltip_body(perk);
        let tooltip_from_clarity = perk
            .clarity_description
            .as_ref()
            .is_some_and(|text| !text.trim().is_empty());
        let tooltip_name = SharedString::from(perk.name.clone());
        let tooltip_body = SharedString::from(tooltip_body);

        if let Some(icon_path) = perk.icon_path.as_deref() {
            destiny::request_icon_download(icon_path);
        }

        let icon_element = perk
            .icon_path
            .as_deref()
            .and_then(destiny::icon_if_cached)
            .map(|local_path| {
                img(local_path)
                    .w(px(32.))
                    .h(px(32.))
                    .rounded_sm()
                    .into_any_element()
            })
            .unwrap_or_else(|| {
                div()
                    .w(px(32.))
                    .h(px(32.))
                    .rounded_sm()
                    .bg(rgb(0x27272a))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(10.))
                    .text_color(rgb(0xa1a1aa))
                    .child(
                        perk.name
                            .chars()
                            .next()
                            .unwrap_or('?')
                            .to_uppercase()
                            .to_string(),
                    )
                    .into_any_element()
            });

        const PERK_ICON_SIZE: f32 = 36.;

        div()
            .id(("d2-perk-icon", perk_element_id))
            .hoverable_tooltip(move |_window, cx| {
                cx.new(|_cx| D2PerkTooltipContent {
                    name: tooltip_name.clone(),
                    body: tooltip_body.clone(),
                    from_clarity: tooltip_from_clarity,
                    is_saved,
                })
                .into()
            })
            .cursor_pointer()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |_launcher, _: &MouseUpEvent, _window, cx| {
                    destiny::toggle_saved_weapon_perk_multi(weapon_hash, perk_hash);
                    cx.notify();
                }),
            )
            .flex()
            .items_center()
            .justify_center()
            .size(px(PERK_ICON_SIZE))
            .rounded_full()
            .when(is_saved, |perk_shell| perk_shell.bg(rgb(0x2563eb)))
            .when(!is_saved, |perk_shell| {
                perk_shell.hover(|style| style.bg(rgb(0x27272a)))
            })
            .child(icon_element)
            .into_any_element()
    }

    
    pub(super) fn start_d2_weapon_compare(
        &mut self,
        _: &StartD2WeaponCompare,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

    pub(super) fn clear_d2_weapon_compare(
        &mut self,
        _: &ClearD2WeaponCompare,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.compare_weapon_hash = None;
        self.d2_compare_picking = false;
        self.d2_compare_primary_hash = None;
        cx.notify();
    }
}
