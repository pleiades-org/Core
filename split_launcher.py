#!/usr/bin/env python3
"""Split gpui_app.rs into launcher submodules by function name."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SRC = ROOT / "src" / "ui" / "launcher" / "_source.rs"
LAUNCHER = ROOT / "src" / "ui" / "launcher"

SETTINGS_METHODS = {
    "open_settings_menu",
    "close_settings_menu",
    "quit_from_settings",
    "open_settings_file",
    "open_quicklinks_file",
    "open_snippets_file",
    "reload_applications",
    "toggle_hotkey_enabled",
    "toggle_launch_at_startup",
    "toggle_app_indexing",
    "toggle_file_indexing",
    "toggle_web_search",
    "toggle_backdrop_blur",
    "toggle_clipboard_history",
    "save_settings",
    "render_settings_editor_sections",
    "render_settings_menu",
}

RESULT_LIST_METHODS = {
    "move_selection_up",
    "move_selection_down",
    "move_selection_page_up",
    "move_selection_page_down",
    "move_selection_first",
    "move_selection_last",
    "move_selection_by",
    "selectable_item_count",
    "accept_mouse_result",
    "render_results",
    "render_standard_result",
    "render_calculation_result",
}

DESTINY_METHODS = {
    "render_d2_weapon_detail",
    "render_d2_perk_icon",
}

SETTINGS_FREE = {
    "registered_command_hotkey_summary",
    "setting_toggle_row",
    "settings_info_row",
    "settings_hotkey_rows",
    "settings_editor_section",
    "settings_editor_row",
    "compact_settings_value",
    "toggle_pill",
    "settings_button",
    "danger_settings_button",
}

RESULT_LIST_FREE = {
    "render_result_icon",
    "should_show_subtitle",
    "calculation_side",
    "calculation_primary_text",
    "compact_calculation_text",
    "calculation_primary_line_text_size",
    "calculation_primary_text_size",
    "compact_display_text",
    "category_color",
    "category_label",
    "result_row_background",
    "result_row_border_color",
}

DESTINY_FREE = {
    "render_d2_compare_stats_panel",
    "render_d2_weapon_stat_bar",
    "d2_perk_element_id",
    "d2_detail_header_icon_size",
    "d2_weapon_stat_bar_fill_ratio",
    "render_d2_weapon_stat_bar",
    "d2_weapon_stats_panel_height",
    "d2_weapon_detail_height",
    "render_d2_weapon_stats_panel",
    "destiny_weapon_portrait_for_weapon",
    "destiny_weapon_card_icons_for_weapon",
    "destiny_weapon_card_icons_for_result",
    "destiny_weapon_portrait_for_result",
    "d2_season_corner_icon_size",
    "d2_season_strip_height",
    "d2_season_strip_image_lift",
    "render_destiny_season_strip_shadow",
    "render_destiny_overlay_icon",
    "render_destiny_weapon_portrait",
}

METHOD_START = re.compile(r"^    (?:pub(?:\(\s*super\s*\))? )?fn ([a-zA-Z0-9_]+)\(")
FREE_FN_START = re.compile(r"^fn ([a-zA-Z0-9_]+)\(")

RESULT_LIST_IMPORTS = """\
use crate::command::{BuiltInAction, CommandCategory, CommandResult};
use gpui::{
    div, img, prelude::*, px, rgb, Context, MouseButton, MouseUpEvent, Window,
};
use super::{
    fallback_icon, file_icon_for_result, LauncherPanel, LauncherView, MoveSelectionDown,
    MoveSelectionFirst, MoveSelectionLast, MoveSelectionPageDown, MoveSelectionPageUp,
    MoveSelectionUp,
};
"""

SETTINGS_IMPORTS = """\
use crate::{
    action_executor::execute_result_action,
    command::{BuiltInAction, CommandResult},
    quicklinks, settings::settings_file_path, snippets, startup::set_launch_at_startup,
};
use gpui::{div, prelude::*, px, rgb, App, Context, MouseButton, MouseUpEvent, Window};
use super::{
    LauncherSettings, LauncherView, RegisteredHotkeys, SettingsEditorRow,
    window_background_appearance,
};
use super::result_list::compact_display_text;
"""

DESTINY_IMPORTS = """\
use super::LauncherView;
"""


def find_matching_brace(lines: list[str], start_idx: int) -> int:
    depth = 0
    for i in range(start_idx, len(lines)):
        for ch in lines[i]:
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    return i
    raise ValueError(f"No matching brace from line {start_idx + 1}")


def find_block_end(lines: list[str], start_idx: int) -> int:
    for i in range(start_idx, len(lines)):
        if "{" in lines[i]:
            return find_matching_brace(lines, i)
    return start_idx


def split_impl_launcher_view(block: list[str]) -> list[tuple[str, list[str]]]:
    methods: list[tuple[str, list[str]]] = []
    i = 1
    while i < len(block) - 1:
        line = block[i]
        m = METHOD_START.match(line)
        if m:
            end = find_matching_brace(block, i)
            methods.append((m.group(1), block[i : end + 1]))
            i = end + 1
            continue
        i += 1
    return methods


def route_method(name: str) -> str:
    if name in SETTINGS_METHODS:
        return "settings_panel"
    if name in RESULT_LIST_METHODS:
        return "result_list"
    if name in DESTINY_METHODS:
        return "destiny_detail"
    return "mod"


def route_free(name: str) -> str:
    if name in SETTINGS_FREE:
        return "settings_panel"
    if name in RESULT_LIST_FREE:
        return "result_list"
    if name in DESTINY_FREE:
        return "destiny_detail"
    return "mod"


def pub_super_method(method: str) -> str:
    if "    pub fn " in method:
        return method
    return method.replace("    fn ", "    pub(super) fn ", 1)


def pub_super_free_fn(chunk: str) -> str:
    if chunk.startswith("fn "):
        return chunk.replace("fn ", "pub(super) fn ", 1)
    return chunk


def wrap_impl(imports: str, methods: list[str]) -> str:
    if not methods:
        return imports
    body = "".join(pub_super_method(m) for m in methods)
    return imports + "\nimpl LauncherView {\n" + body + "}\n"


def wrap_free(imports: str, methods: list[str]) -> str:
    if not methods:
        return imports
    return imports + "\n" + "".join(methods)


def rebuild(lines: list[str]) -> None:
    first_impl = next(i for i, l in enumerate(lines) if l.startswith("impl LauncherView {"))
    header = lines[:first_impl]

    impl1_methods: list[str] = []
    impl2_methods: list[str] = []
    trait_impls: list[str] = []
    tail_chunks: list[str] = []
    settings_methods: list[str] = []
    result_methods: list[str] = []
    destiny_methods: list[str] = []
    settings_free: list[str] = []
    result_free: list[str] = []
    destiny_free: list[str] = []

    launcher_impl_count = 0
    i = first_impl
    while i < len(lines):
        line = lines[i]
        if line.startswith("impl LauncherView {"):
            launcher_impl_count += 1
            end = find_matching_brace(lines, i)
            target = impl1_methods if launcher_impl_count == 1 else impl2_methods
            for name, method_lines in split_impl_launcher_view(lines[i : end + 1]):
                chunk = "".join(method_lines) + "\n"
                dest = route_method(name)
                if dest == "settings_panel":
                    settings_methods.append(chunk)
                elif dest == "result_list":
                    result_methods.append(chunk)
                elif dest == "destiny_detail":
                    destiny_methods.append(chunk)
                else:
                    target.append(chunk)
            i = end + 1
            continue

        m = FREE_FN_START.match(line)
        if m:
            end = find_block_end(lines, i)
            chunk = "".join(lines[i : end + 1]) + "\n"
            dest = route_free(m.group(1))
            if dest == "settings_panel":
                settings_free.append(chunk)
            elif dest == "result_list":
                result_free.append(chunk)
            elif dest == "destiny_detail":
                destiny_free.append(chunk)
            else:
                tail_chunks.append(chunk)
            i = end + 1
            continue

        handled = False
        for prefix, dest_list in [
            ("impl Focusable for LauncherView", trait_impls),
            ("impl Render for LauncherView", trait_impls),
            ("struct FileAssets", tail_chunks),
            ("impl AssetSource for FileAssets", tail_chunks),
            ("pub fn run(", tail_chunks),
            ("struct D2PerkTooltipContent", destiny_free),
            ("impl Render for D2PerkTooltipContent", destiny_free),
            ("struct DestinyWeaponPortraitPaths", destiny_free),
            ("struct DestinyWeaponCardIcons", destiny_free),
            ("impl DestinyWeaponPortraitPaths", destiny_free),
        ]:
            if line.startswith(prefix):
                end = find_block_end(lines, i) if "{" in "".join(lines[i : i + 4]) else i
                dest_list.append("".join(lines[i : end + 1]) + "\n")
                i = end + 1
                handled = True
                break
        if handled:
            continue

        if line.startswith("#[cfg"):
            cfg_end = i
            while cfg_end + 1 < len(lines) and not (
                lines[cfg_end + 1].startswith("fn ")
                or lines[cfg_end + 1].startswith("impl ")
                or lines[cfg_end + 1].startswith("struct ")
            ):
                cfg_end += 1
            if cfg_end + 1 < len(lines):
                end = find_block_end(lines, cfg_end + 1)
                tail_chunks.append("".join(lines[i : end + 1]) + "\n")
                i = end + 1
                continue

        if line.strip():
            raise ValueError(f"Unhandled top-level line {i + 1}: {line.strip()}")
        i += 1

    mod_lines = list(header)
    mod_lines.extend(
        [
            "mod destiny_detail;\n",
            "mod result_list;\n",
            "mod settings_panel;\n\n",
            "use result_list::{compact_display_text, result_row_background, result_row_border_color};\n\n",
        ]
    )
    mod_lines.append("impl LauncherView {\n")
    mod_lines.extend(impl1_methods)
    mod_lines.append("}\n\n")
    mod_lines.extend(trait_impls)
    if impl2_methods:
        mod_lines.append("impl LauncherView {\n")
        mod_lines.extend(impl2_methods)
        mod_lines.append("}\n\n")
    mod_lines.extend(tail_chunks)

    LAUNCHER.mkdir(parents=True, exist_ok=True)

    (LAUNCHER / "mod.rs").write_text("".join(mod_lines), encoding="utf-8")
    (LAUNCHER / "settings_panel.rs").write_text(
        wrap_free(SETTINGS_IMPORTS, settings_free)
        + wrap_impl("", settings_methods),
        encoding="utf-8",
    )
    (LAUNCHER / "result_list.rs").write_text(
        wrap_impl(RESULT_LIST_IMPORTS, result_methods)
        + "".join(pub_super_free_fn(c) for c in result_free),
        encoding="utf-8",
    )
    (LAUNCHER / "destiny_detail.rs").write_text(
        wrap_impl(DESTINY_IMPORTS, destiny_methods)
        + "".join(destiny_free),
        encoding="utf-8",
    )

    for name in ("mod.rs", "settings_panel.rs", "result_list.rs", "destiny_detail.rs"):
        path = LAUNCHER / name
        count = len(path.read_text(encoding="utf-8").splitlines())
        print(f"  {path.name}: {count} lines")


if __name__ == "__main__":
    lines = SRC.read_text(encoding="utf-8").splitlines(keepends=True)
    rebuild(lines)