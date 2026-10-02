//! Settings → Music: which player Core prefers, the now-playing bar, and media shortcuts that
//! are recorded by pressing them and work either while Core is open or in every app.
use super::{
    control_style::{self, Look},
    layout::{self, BUTTON_HEIGHT, LABEL_HEIGHT},
    page::SettingsAction,
    shortcut_recorder::{self, SHORTCUT_RECORDED},
    MediaShortcutAction, MusicApp, MusicSettings, Shortcut, ShortcutScope,
};
use crate::windows::{
    button_hover, painting,
    theme::{scale, Fonts, Palette},
    view::{child, control_text},
    wide,
};
use core_engine::media::PriorityMode;
use std::cell::{Cell, RefCell};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::Gdi::{InvalidateRect, HDC},
        UI::{
            Controls::{SetWindowTheme, DRAWITEMSTRUCT, ODS_FOCUS, ODS_SELECTED},
            WindowsAndMessaging::*,
        },
    },
};

const PRIORITY_LABEL_ID: usize = 260;
const PRIORITY_ID: usize = 261;
const BAR_ID: usize = 262;
const APPS_LABEL_ID: usize = 263;
const APPS_ID: usize = 264;
const UP_ID: usize = 265;
const DOWN_ID: usize = 266;
const IGNORE_ID: usize = 267;
const SHORTCUTS_LABEL_ID: usize = 268;
/// One of each per media shortcut, in `MediaShortcutAction::ALL` order.
const ACTION_LABEL_FIRST: usize = 270;
const RECORDER_FIRST: usize = 274;
const SCOPE_FIRST: usize = 278;
pub const FIRST_ID: usize = 260;
pub const LAST_ID: usize = 289;

// Layout in 96-DPI pixels: settings on the left, the app list on the right, shortcuts below.
const LEFT: i32 = layout::CONTENT_LEFT;
const LEFT_WIDTH: i32 = 342;
const RIGHT: i32 = LEFT + LEFT_WIDTH + 16;
const RIGHT_WIDTH: i32 = layout::CONTENT_RIGHT - RIGHT;
const LABEL_TOP: i32 = layout::FIRST_LABEL_TOP;
const ROW_TOP: i32 = 130;
const BAR_TOP: i32 = 180;
const LIST_HEIGHT: i32 = 92;
const LIST_ROW_HEIGHT: i32 = 26;
const LIST_BUTTON_TOP: i32 = 228;
const LIST_BUTTON_HEIGHT: i32 = 36;
const LIST_BUTTON_GAP: i32 = 6;
const SHORTCUTS_LABEL_TOP: i32 = 276;
const SHORTCUT_TOP: i32 = 300;
const SHORTCUT_PITCH: i32 = 44;
const ACTION_WIDTH: i32 = 140;
const RECORDER_LEFT: i32 = LEFT + 148;
const RECORDER_BOX_WIDTH: i32 = 240;
const SCOPE_LEFT: i32 = RECORDER_LEFT + RECORDER_BOX_WIDTH + 12;
const DROPDOWN_LIST_HEIGHT: i32 = 260;
const DROPDOWN_OFFSET: i32 = 4;

pub fn is_dropdown(identifier: usize) -> bool {
    identifier == PRIORITY_ID || scope_slot(identifier).is_some()
}

/// The shortcut fields, which are drawn as text boxes and record keys.
pub fn is_recorder(identifier: usize) -> bool {
    recorder_slot(identifier).is_some()
}

fn recorder_slot(identifier: usize) -> Option<usize> {
    identifier
        .checked_sub(RECORDER_FIRST)
        .filter(|slot| *slot < MediaShortcutAction::ALL.len())
}

fn scope_slot(identifier: usize) -> Option<usize> {
    identifier
        .checked_sub(SCOPE_FIRST)
        .filter(|slot| *slot < MediaShortcutAction::ALL.len())
}

/// The app list: preferred apps in order, then the others, then ignored apps.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct AppList {
    preferred: Vec<MusicApp>,
    others: Vec<MusicApp>,
    ignored: Vec<MusicApp>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListMove {
    Up,
    Down,
    ToggleIgnored,
}

impl AppList {
    fn new(settings: &MusicSettings, available: &[MusicApp]) -> Self {
        let listed = |app: &MusicApp| {
            settings
                .preferred
                .iter()
                .chain(settings.ignored.iter())
                .any(|listed| listed.key == app.key)
        };
        let mut others: Vec<MusicApp> = Vec::new();
        for app in available {
            if !listed(app) && !others.iter().any(|other| other.key == app.key) {
                others.push(app.clone());
            }
        }
        others.sort_by_key(|app| app.name.to_lowercase());
        Self {
            preferred: settings.preferred.to_vec(),
            others,
            ignored: settings.ignored.to_vec(),
        }
    }

    fn rows(&self) -> Vec<String> {
        let preferred = self
            .preferred
            .iter()
            .enumerate()
            .map(|(index, app)| format!("{}. {}", index + 1, app.name));
        let others = self.others.iter().map(|app| app.name.to_string());
        let ignored = self
            .ignored
            .iter()
            .map(|app| format!("{} · ignored", app.name));
        preferred.chain(others).chain(ignored).collect()
    }

    fn app(&self, row: usize) -> Option<&MusicApp> {
        self.preferred
            .iter()
            .chain(&self.others)
            .chain(&self.ignored)
            .nth(row)
    }

    fn is_ignored(&self, row: usize) -> bool {
        row >= self.preferred.len() + self.others.len() && row < self.len()
    }

    fn len(&self) -> usize {
        self.preferred.len() + self.others.len() + self.ignored.len()
    }

    /// Applies a move to the app in `row`; returns the row it is in afterwards.
    fn apply(&mut self, row: usize, change: ListMove) -> Option<usize> {
        let key = self.app(row)?.key.clone();
        let preferred = self.preferred.iter().position(|app| app.key == key);
        let ignored = self.ignored.iter().position(|app| app.key == key);
        match (change, preferred, ignored) {
            (ListMove::Up, Some(index), _) if index > 0 => self.preferred.swap(index, index - 1),
            (ListMove::Up, None, None) => {
                let index = self.others.iter().position(|app| app.key == key)?;
                let app = self.others.remove(index);
                self.preferred.push(app);
            }
            (ListMove::Down, Some(index), _) if index + 1 < self.preferred.len() => {
                self.preferred.swap(index, index + 1)
            }
            (ListMove::Down, Some(index), _) => {
                let app = self.preferred.remove(index);
                self.others.insert(0, app);
            }
            (ListMove::ToggleIgnored, _, Some(index)) => {
                let app = self.ignored.remove(index);
                self.others.insert(0, app);
            }
            (ListMove::ToggleIgnored, Some(index), None) => {
                let app = self.preferred.remove(index);
                self.ignored.push(app);
            }
            (ListMove::ToggleIgnored, None, None) => {
                let index = self.others.iter().position(|app| app.key == key)?;
                let app = self.others.remove(index);
                self.ignored.push(app);
            }
            _ => return Some(row),
        }
        (0..self.len()).find(|row| self.app(*row).is_some_and(|app| app.key == key))
    }
}

pub struct MusicSection {
    controls: Vec<(usize, HWND)>,
    bar: Cell<bool>,
    apps: RefCell<AppList>,
}

impl MusicSection {
    pub fn create(parent: HWND, instance: HINSTANCE) -> windows::core::Result<Self> {
        let mut section = Self {
            controls: Vec::new(),
            bar: Cell::new(true),
            apps: RefCell::new(AppList::default()),
        };
        let label = WINDOW_STYLE(0);
        section.add(
            parent,
            instance,
            "Which player Core controls",
            w!("STATIC"),
            label,
            PRIORITY_LABEL_ID,
        )?;
        section.add(
            parent,
            instance,
            "Which player Core controls",
            w!("COMBOBOX"),
            WINDOW_STYLE(CBS_DROPDOWNLIST as u32) | WS_VSCROLL | WS_TABSTOP,
            PRIORITY_ID,
        )?;
        // Switch text carries the state ("…: On") for screen readers; only the name is drawn.
        section.add_button(parent, instance, "Now playing bar: On", BAR_ID)?;
        section.add(
            parent,
            instance,
            "Preferred apps · first wins",
            w!("STATIC"),
            label,
            APPS_LABEL_ID,
        )?;
        section.add(
            parent,
            instance,
            "Preferred apps",
            w!("LISTBOX"),
            WS_VSCROLL
                | WS_TABSTOP
                | WINDOW_STYLE(
                    (LBS_NOTIFY | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS | LBS_NOINTEGRALHEIGHT)
                        as u32,
                ),
            APPS_ID,
        )?;
        section.add_button(parent, instance, "Up", UP_ID)?;
        section.add_button(parent, instance, "Down", DOWN_ID)?;
        section.add_button(parent, instance, "Ignore", IGNORE_ID)?;
        section.add(
            parent,
            instance,
            "Media shortcuts · click a box, then press the keys · Backspace clears",
            w!("STATIC"),
            label,
            SHORTCUTS_LABEL_ID,
        )?;
        for (slot, action) in MediaShortcutAction::ALL.into_iter().enumerate() {
            section.add(
                parent,
                instance,
                action.label(),
                w!("STATIC"),
                label,
                ACTION_LABEL_FIRST + slot,
            )?;
            let recorder = section.add(
                parent,
                instance,
                "",
                w!("EDIT"),
                WINDOW_STYLE(ES_AUTOHSCROLL as u32) | WS_TABSTOP,
                RECORDER_FIRST + slot,
            )?;
            shortcut_recorder::install(recorder, true, shortcut_recorder::NOT_SET_CUE)?;
            section.add(
                parent,
                instance,
                &format!("Where {} works", action.label().to_lowercase()),
                w!("COMBOBOX"),
                WINDOW_STYLE(CBS_DROPDOWNLIST as u32) | WS_VSCROLL | WS_TABSTOP,
                SCOPE_FIRST + slot,
            )?;
        }
        Ok(section)
    }

    fn add_button(
        &mut self,
        parent: HWND,
        instance: HINSTANCE,
        label: &str,
        identifier: usize,
    ) -> windows::core::Result<()> {
        let button = self.add(
            parent,
            instance,
            label,
            w!("BUTTON"),
            WINDOW_STYLE(BS_OWNERDRAW as u32) | WS_TABSTOP,
            identifier,
        )?;
        button_hover::install(button)
    }

    fn add(
        &mut self,
        parent: HWND,
        instance: HINSTANCE,
        label: &str,
        class: PCWSTR,
        style: WINDOW_STYLE,
        identifier: usize,
    ) -> windows::core::Result<HWND> {
        let label = wide(label);
        let control = unsafe {
            child(
                parent,
                instance,
                class,
                PCWSTR(label.as_ptr()),
                style,
                identifier,
            )?
        };
        self.controls.push((identifier, control));
        Ok(control)
    }

    fn control(&self, identifier: usize) -> HWND {
        self.controls
            .iter()
            .find(|(candidate, _)| *candidate == identifier)
            .expect("music control exists")
            .1
    }

    /// `available`: music apps found on this PC and players seen this session.
    pub fn reset(&self, settings: &MusicSettings, available: &[MusicApp]) {
        self.bar.set(settings.bar);
        let priority = PriorityMode::ALL
            .iter()
            .position(|mode| *mode == settings.priority)
            .unwrap_or(0);
        let modes: Vec<String> = PriorityMode::ALL
            .iter()
            .map(|mode| mode.label().to_owned())
            .collect();
        self.set_items(PRIORITY_ID, &modes, priority);
        let scopes: Vec<String> = ShortcutScope::ALL
            .iter()
            .map(|scope| scope.label().to_owned())
            .collect();
        for (slot, shortcut) in settings.shortcuts.iter().enumerate() {
            set_text(
                self.control(RECORDER_FIRST + slot),
                &shortcut
                    .shortcut
                    .map(|shortcut| shortcut.to_string())
                    .unwrap_or_default(),
            );
            let scope = ShortcutScope::ALL
                .iter()
                .position(|scope| *scope == shortcut.scope)
                .unwrap_or(0);
            self.set_items(SCOPE_FIRST + slot, &scopes, scope);
        }
        *self.apps.borrow_mut() = AppList::new(settings, available);
        self.fill_list(Some(0));
        self.update_names();
    }

    pub fn draft(&self) -> Result<MusicSettings, String> {
        let selected = |identifier| {
            usize::try_from(unsafe {
                SendMessageW(self.control(identifier), CB_GETCURSEL, None, None).0
            })
            .unwrap_or(0)
        };
        let mut shortcuts = MusicSettings::default().shortcuts;
        for (slot, action) in MediaShortcutAction::ALL.iter().enumerate() {
            let text = control_text(self.control(RECORDER_FIRST + slot));
            shortcuts[slot].shortcut = if text.trim().is_empty() {
                None
            } else {
                Some(
                    Shortcut::parse(&text)
                        .map_err(|error| format!("{}: {error}", action.label()))?,
                )
            };
            shortcuts[slot].scope = ShortcutScope::ALL
                .get(selected(SCOPE_FIRST + slot))
                .copied()
                .unwrap_or_default();
        }
        let apps = self.apps.borrow();
        Ok(MusicSettings {
            priority: PriorityMode::ALL
                .get(selected(PRIORITY_ID))
                .copied()
                .unwrap_or_default(),
            bar: self.bar.get(),
            shortcuts,
            preferred: apps.preferred.clone().into(),
            ignored: apps.ignored.clone().into(),
        })
    }

    pub fn focus_target(&self) -> HWND {
        self.control(PRIORITY_ID)
    }

    /// None when `identifier` is not one of this section's controls.
    pub fn command(&self, identifier: usize, notification: u32) -> Option<SettingsAction> {
        if !(FIRST_ID..=LAST_ID).contains(&identifier) {
            return None;
        }
        if recorder_slot(identifier).is_some() {
            return Some(match notification {
                // Text set by automation applies as typing did; a held "Ctrl+…" does not yet.
                EN_CHANGE if !shortcut_recorder::is_partial(self.control(identifier)) => {
                    SettingsAction::Edit
                }
                SHORTCUT_RECORDED => SettingsAction::Change,

                _ => SettingsAction::None,
            });
        }
        if is_dropdown(identifier) {
            return Some(if notification == CBN_SELCHANGE {
                SettingsAction::Change
            } else {
                SettingsAction::None
            });
        }
        if identifier == APPS_ID {
            if notification == LBN_SELCHANGE {
                self.update_names();
            }
            return Some(SettingsAction::None);
        }
        if notification != BN_CLICKED {
            return Some(SettingsAction::None);
        }
        let change = match identifier {
            BAR_ID => {
                self.bar.set(!self.bar.get());
                self.update_names();
                return Some(SettingsAction::Change);
            }
            UP_ID => ListMove::Up,
            DOWN_ID => ListMove::Down,
            IGNORE_ID => ListMove::ToggleIgnored,
            _ => return Some(SettingsAction::None),
        };
        let Some(row) = self.selected_row() else {
            return Some(SettingsAction::None);
        };
        let moved = self.apps.borrow_mut().apply(row, change);
        self.fill_list(moved);
        self.update_names();
        Some(SettingsAction::Change)
    }

    /// Apps found after Settings opened (discovery finishing, a player starting) join the
    /// list; the selection and the person's order stay.
    pub fn add_available(&self, available: &[MusicApp]) {
        let selected = self.selected_row();
        let selected_key =
            selected.and_then(|row| self.apps.borrow().app(row).map(|app| app.key.clone()));
        let added = {
            let mut apps = self.apps.borrow_mut();
            let before = apps.len();
            for app in available {
                let listed = (0..apps.len())
                    .any(|row| apps.app(row).is_some_and(|listed| listed.key == app.key));
                if !listed {
                    apps.others.push(app.clone());
                }
            }
            apps.others.sort_by_key(|app| app.name.to_lowercase());
            apps.len() != before
        };
        if added {
            let apps = self.apps.borrow();
            let row = selected_key.and_then(|key| {
                (0..apps.len()).find(|row| apps.app(*row).is_some_and(|app| app.key == key))
            });
            drop(apps);
            self.fill_list(row);
            self.update_names();
        }
    }

    fn selected_row(&self) -> Option<usize> {
        usize::try_from(unsafe { SendMessageW(self.control(APPS_ID), LB_GETCURSEL, None, None).0 })
            .ok()
    }

    fn fill_list(&self, selected: Option<usize>) {
        let list = self.control(APPS_ID);
        let rows = self.apps.borrow().rows();
        unsafe {
            SendMessageW(list, WM_SETREDRAW, Some(WPARAM(0)), None);
            SendMessageW(list, LB_RESETCONTENT, None, None);
            for row in &rows {
                let text = wide(row);
                SendMessageW(
                    list,
                    LB_ADDSTRING,
                    None,
                    Some(LPARAM(text.as_ptr() as isize)),
                );
            }
            if let Some(selected) = selected.filter(|row| *row < rows.len()) {
                SendMessageW(list, LB_SETCURSEL, Some(WPARAM(selected)), None);
            }
            SendMessageW(list, WM_SETREDRAW, Some(WPARAM(1)), None);
            let _ = InvalidateRect(Some(list), None, false);
        }
    }

    /// Switch and button text, which screen readers announce; the controls repaint to match.
    fn update_names(&self) {
        let ignored = self
            .selected_row()
            .is_some_and(|row| self.apps.borrow().is_ignored(row));
        for (identifier, text) in [
            (
                BAR_ID,
                format!(
                    "Now playing bar: {}",
                    if self.bar.get() { "On" } else { "Off" }
                ),
            ),
            (
                IGNORE_ID,
                if ignored { "Allow" } else { "Ignore" }.to_owned(),
            ),
        ] {
            let control = self.control(identifier);
            if control_text(control) != text {
                set_text(control, &text);
                unsafe {
                    let _ = InvalidateRect(Some(control), None, false);
                }
            }
        }
    }

    fn set_items(&self, identifier: usize, labels: &[String], selected: usize) {
        let control = self.control(identifier);
        unsafe {
            SendMessageW(control, CB_RESETCONTENT, None, None);
            for label in labels {
                let text = wide(label);
                SendMessageW(
                    control,
                    CB_ADDSTRING,
                    None,
                    Some(LPARAM(text.as_ptr() as isize)),
                );
            }
            SendMessageW(control, CB_SETCURSEL, Some(WPARAM(selected)), None);
        }
    }

    pub fn show(&self, visible: bool) {
        for (_, control) in &self.controls {
            unsafe {
                let _ = ShowWindow(*control, if visible { SW_SHOWNA } else { SW_HIDE });
            }
        }
    }

    /// Dark or light dropdowns and list scroll bar, to match the background.
    pub fn apply_theme(&self, dark: bool) {
        for (identifier, control) in &self.controls {
            let theme = match (is_dropdown(*identifier), *identifier == APPS_ID, dark) {
                (true, _, true) => w!("DarkMode_CFD"),
                (true, _, false) => w!("CFD"),
                (_, true, true) => w!("DarkMode_Explorer"),
                (_, true, false) => w!("Explorer"),
                _ => continue,
            };
            unsafe {
                let _ = SetWindowTheme(*control, theme, PCWSTR::null());
            }
        }
    }

    fn area(identifier: usize) -> (i32, i32, i32, i32) {
        if let Some(slot) = recorder_slot(identifier) {
            let top = SHORTCUT_TOP + slot as i32 * SHORTCUT_PITCH;
            return (
                RECORDER_LEFT + layout::INPUT_INSET,
                top + 6,
                RECORDER_BOX_WIDTH - 2 * layout::INPUT_INSET,
                layout::INPUT_HEIGHT,
            );
        }
        if let Some(slot) = scope_slot(identifier) {
            let top = SHORTCUT_TOP + slot as i32 * SHORTCUT_PITCH;
            return (
                SCOPE_LEFT,
                top + DROPDOWN_OFFSET,
                layout::CONTENT_RIGHT - SCOPE_LEFT,
                DROPDOWN_LIST_HEIGHT,
            );
        }
        if let Some(slot) = identifier
            .checked_sub(ACTION_LABEL_FIRST)
            .filter(|slot| *slot < MediaShortcutAction::ALL.len())
        {
            let top = SHORTCUT_TOP + slot as i32 * SHORTCUT_PITCH;
            return (LEFT, top + 9, ACTION_WIDTH, LABEL_HEIGHT);
        }
        let button_width = (RIGHT_WIDTH - 2 * LIST_BUTTON_GAP) / 3;
        match identifier {
            PRIORITY_LABEL_ID => (LEFT, LABEL_TOP, LEFT_WIDTH, LABEL_HEIGHT),
            PRIORITY_ID => (
                LEFT,
                ROW_TOP + DROPDOWN_OFFSET,
                LEFT_WIDTH,
                DROPDOWN_LIST_HEIGHT,
            ),
            BAR_ID => (LEFT, BAR_TOP, LEFT_WIDTH, BUTTON_HEIGHT),
            APPS_LABEL_ID => (RIGHT, LABEL_TOP, RIGHT_WIDTH, LABEL_HEIGHT),
            APPS_ID => (RIGHT, ROW_TOP, RIGHT_WIDTH, LIST_HEIGHT),
            UP_ID | DOWN_ID | IGNORE_ID => (
                RIGHT + (identifier - UP_ID) as i32 * (button_width + LIST_BUTTON_GAP),
                LIST_BUTTON_TOP,
                button_width,
                LIST_BUTTON_HEIGHT,
            ),
            _ => (
                LEFT,
                SHORTCUTS_LABEL_TOP,
                layout::CONTENT_RIGHT - LEFT,
                LABEL_HEIGHT,
            ),
        }
    }

    /// Fonts are sent only when they were recreated for a new DPI.
    pub fn layout(&self, dpi: u32, fonts: Fonts, new_fonts: bool) -> windows::core::Result<()> {
        for (identifier, control) in &self.controls {
            let (left, top, width, height) = Self::area(*identifier);
            unsafe {
                if new_fonts {
                    let font =
                        if matches!(*identifier, PRIORITY_LABEL_ID | PRIORITY_ID | APPS_LABEL_ID)
                            || is_dropdown(*identifier)
                            || is_recorder(*identifier)
                        {
                            fonts.title
                        } else {
                            fonts.detail
                        };
                    SendMessageW(
                        *control,
                        WM_SETFONT,
                        Some(WPARAM(font.0 as usize)),
                        Some(LPARAM(0)),
                    );
                }
                if *identifier == APPS_ID {
                    SendMessageW(
                        *control,
                        LB_SETITEMHEIGHT,
                        Some(WPARAM(0)),
                        Some(LPARAM(scale(LIST_ROW_HEIGHT, dpi) as isize)),
                    );
                    // A new row height keeps the old scroll offset; start from the first app.
                    SendMessageW(*control, LB_SETTOPINDEX, Some(WPARAM(0)), None);
                }
                SetWindowPos(
                    *control,
                    None,
                    scale(left, dpi),
                    scale(top, dpi),
                    scale(width, dpi),
                    scale(height, dpi),
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )?;
                if new_fonts {
                    let _ = InvalidateRect(Some(*control), None, true);
                }
            }
        }
        Ok(())
    }

    /// The rounded boxes behind the shortcut fields, as behind the other text boxes.
    pub fn paint(&self, context: HDC, dpi: u32, palette: Palette) {
        for slot in 0..MediaShortcutAction::ALL.len() {
            painting::rounded(
                context,
                &layout::area(
                    RECORDER_LEFT,
                    SHORTCUT_TOP + slot as i32 * SHORTCUT_PITCH,
                    RECORDER_BOX_WIDTH,
                    BUTTON_HEIGHT,
                    dpi,
                ),
                scale(8, dpi),
                palette.selected,
            );
        }
    }

    pub fn draw(&self, item: &DRAWITEMSTRUCT, look: Look) -> bool {
        let identifier = item.CtlID as usize;
        let active = button_hover::is_hovered(item.hwndItem)
            || item.itemState.0 & (ODS_SELECTED.0 | ODS_FOCUS.0) != 0;
        match identifier {
            BAR_ID => {
                control_style::draw_toggle(
                    item.hDC,
                    item.rcItem,
                    "Now playing bar",
                    self.bar.get(),
                    active,
                    look,
                );
                true
            }
            UP_ID | DOWN_ID | IGNORE_ID => {
                let label = control_text(item.hwndItem);
                control_style::draw_action(item.hDC, item.rcItem, &label, None, active, look);
                true
            }
            APPS_ID => {
                self.draw_row(item, look);
                true
            }
            _ => false,
        }
    }

    fn draw_row(&self, item: &DRAWITEMSTRUCT, look: Look) {
        let Look {
            dpi,
            fonts,
            palette,
        } = look;
        painting::fill(item.hDC, &item.rcItem, palette.background);
        let Some(text) = self.apps.borrow().rows().get(item.itemID as usize).cloned() else {
            return;
        };
        if item.itemState.0 & ODS_SELECTED.0 != 0 {
            painting::rounded(item.hDC, &item.rcItem, scale(6, dpi), palette.selected);
        }
        let mut area = item.rcItem;
        area.left += scale(10, dpi);
        area.right -= scale(6, dpi);
        let ignored = self.apps.borrow().is_ignored(item.itemID as usize);
        painting::text(
            item.hDC,
            &text,
            area,
            fonts.detail,
            if ignored {
                palette.secondary
            } else {
                palette.text
            },
        );
    }
}

impl Drop for MusicSection {
    fn drop(&mut self) {
        for (_, control) in &self.controls {
            unsafe {
                if IsWindow(Some(*control)).as_bool() {
                    let _ = DestroyWindow(*control);
                }
            }
        }
    }
}

fn set_text(control: HWND, text: &str) {
    let text = wide(text);
    if let Err(error) = unsafe { SetWindowTextW(control, PCWSTR(text.as_ptr())) } {
        eprintln!("Could not update a music setting: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn app(key: &str, name: &str) -> MusicApp {
        MusicApp::new(key, name).unwrap()
    }

    fn list() -> AppList {
        let settings = MusicSettings {
            preferred: Arc::from([app("spotify", "Spotify")]),
            ignored: Arc::from([app("chrome", "Google Chrome")]),
            ..MusicSettings::default()
        };
        AppList::new(
            &settings,
            &[
                app("tidal", "TIDAL"),
                app("spotify", "Spotify"),
                app("applemusic", "Apple Music"),
                app("tidal", "TIDAL"),
            ],
        )
    }

    #[test]
    fn listed_apps_show_preferred_in_order_then_others_then_ignored() {
        assert_eq!(
            list().rows(),
            [
                "1. Spotify",
                "Apple Music",
                "TIDAL",
                "Google Chrome · ignored"
            ]
        );
    }

    #[test]
    fn moving_up_ranks_an_app_and_moving_the_last_down_unranks_it() {
        let mut apps = list();
        assert_eq!(apps.apply(1, ListMove::Up), Some(1));
        assert_eq!(apps.rows()[..2], ["1. Spotify", "2. Apple Music"]);
        assert_eq!(apps.apply(1, ListMove::Up), Some(0));
        assert_eq!(apps.rows()[..2], ["1. Apple Music", "2. Spotify"]);
        assert_eq!(apps.apply(1, ListMove::Down), Some(1));
        assert_eq!(apps.rows()[..2], ["1. Apple Music", "Spotify"]);
        // The first row cannot move further up.
        assert_eq!(apps.apply(0, ListMove::Up), Some(0));
    }

    #[test]
    fn ignoring_takes_an_app_out_of_the_ranking_and_allowing_returns_it() {
        let mut apps = list();
        let row = apps.apply(0, ListMove::ToggleIgnored).unwrap();
        assert!(apps.is_ignored(row));
        assert!(apps.preferred.is_empty());
        let row = apps.apply(row, ListMove::ToggleIgnored).unwrap();
        assert!(!apps.is_ignored(row));
        assert_eq!(apps.others[0].name.as_ref(), "Spotify");
        assert_eq!(apps.apply(99, ListMove::Up), None);
    }
}
