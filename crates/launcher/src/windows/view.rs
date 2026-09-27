use super::{
    application_icon::ApplicationIcon,
    button_hover,
    icon_worker::LoadedIcon,
    painting::{self, DisplayRow, ResultKind},
    power_menu::{PowerMenu, POWER_ID},
    search_layout::SearchLayout,
    settings::{
        layout as settings_layout,
        page::{SettingsAction, SettingsPage, COLOR_ID, SHORTCUT_ID, STATUS_ID},
        Preferences,
    },
    theme::{self, Fonts, Palette},
    wide, window_placement,
};
use core_engine::{search::SearchResult, RECENT_APPLICATION_LIMIT, VISIBLE_RESULT_LIMIT};
use std::{
    cell::{Cell, OnceCell, RefCell},
    sync::Arc,
};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::SystemServices::{SS_CENTER, SS_ENDELLIPSIS},
        UI::{
            Controls::{
                SetWindowTheme, DRAWITEMSTRUCT, EM_GETSEL, EM_SETCUEBANNER, EM_SETLIMITTEXT,
                EM_SETSEL, NMCUSTOMDRAW, NM_CUSTOMDRAW, ODS_SELECTED,
            },
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::*,
        },
    },
};

mod app_grid;
mod command_prompt;
mod console;
mod footer;
mod icon_cache;
mod output;
mod paint;
mod power;
mod preference_changes;
mod settings_bridge;

use icon_cache::IconCache;
use preference_changes::PreferenceChanges;

pub use footer::CLOCK_TIMER;
pub use output::OUTPUT_ID;

pub use super::theme::scale;
pub const INPUT_ID: usize = 100;
pub const RESULTS_ID: usize = 101;
pub const FOOTER_ID: usize = 102;
pub const SETTINGS_ID: usize = 104;
/// Top of the section label ("APPLICATIONS"); from here down, the window shows the results.
const SECTION_LABEL_TOP: i32 = 83;
/// Rows, or the larger recently used grid.
const ROW_LIMIT: usize = if RECENT_APPLICATION_LIMIT > VISIBLE_RESULT_LIMIT {
    RECENT_APPLICATION_LIMIT
} else {
    VISIBLE_RESULT_LIMIT
};

/// What the window's size and control positions depend on; layout is skipped when unchanged.
#[derive(Clone, Copy, PartialEq, Eq)]
struct LayoutKey {
    dpi: u32,
    count: usize,
    row_height: i32,
    /// Logical height of the command output, or zero when it is hidden.
    output_height: i32,
    terminal: bool,
    grid: bool,
    /// The settings page has its own size and controls.
    settings: bool,
}

impl LayoutKey {
    const STALE: Self = Self {
        dpi: 0,
        count: usize::MAX,
        row_height: 0,
        output_height: 0,
        terminal: false,
        grid: false,
        settings: false,
    };
}

/// Native edit/list controls retain text input and accessibility; painting is read-only.
pub struct View {
    pub input: HWND,
    pub results: HWND,
    pub footer: HWND,
    footer_text: RefCell<String>,
    clock: HWND,
    clock_active: Cell<bool>,
    /// The console window, which shows command output.
    output: HWND,
    console: Box<console::ConsoleView>,
    output_visible: Cell<bool>,
    /// The query is a shell command: output replaces the result rows.
    terminal: Cell<bool>,
    /// The recently used app under the pointer.
    grid_hover: Cell<Option<usize>>,
    /// Tiles in the rows that fit on screen; the rest are not shown or reachable.
    grid_visible: Cell<usize>,
    /// A `/` was typed: the box holds a command and the `/` is hidden.
    command_mode: Cell<bool>,
    parent: HWND,
    power_button: HWND,
    power_menu: OnceCell<PowerMenu>,
    settings_button: HWND,
    background: Cell<HBRUSH>,
    selected_brush: Cell<HBRUSH>,
    palette: Cell<Palette>,
    preferences: Cell<Preferences>,
    screen: Cell<window_placement::ScreenArea>,
    monitor_reference: Cell<HWND>,
    settings_page: OnceCell<SettingsPage>,
    settings_open: Cell<bool>,
    fonts: Cell<Fonts>,
    dpi: Cell<u32>,
    width: Cell<i32>,
    height: Cell<i32>,
    /// The window's screen rectangle from the last layout, which corner clipping needs.
    bounds: Cell<RECT>,
    /// The shape of the window region that is set, if any.
    clip_shape: Cell<Option<window_placement::ClipShape>>,
    layout_key: Cell<LayoutKey>,
    rows: RefCell<Vec<DisplayRow>>,
    /// Icons shown recently, for results that come back.
    icon_cache: RefCell<IconCache>,
}

impl View {
    /// Starts with the saved preferences, so the first appearance is applied and laid out once.
    pub fn create(
        parent: HWND,
        instance: HINSTANCE,
        preferences: Preferences,
    ) -> windows::core::Result<Self> {
        let console = console::ConsoleView::create(parent, instance, OUTPUT_ID)?;
        let mut view = Self {
            input: HWND::default(),
            results: HWND::default(),
            footer: HWND::default(),
            footer_text: RefCell::new("Type to search".into()),
            clock: HWND::default(),
            clock_active: Cell::new(false),
            output: console.window(),
            console,
            output_visible: Cell::new(false),
            terminal: Cell::new(false),
            command_mode: Cell::new(false),
            grid_hover: Cell::new(None),
            grid_visible: Cell::new(0),
            power_button: HWND::default(),
            power_menu: OnceCell::new(),
            parent,
            settings_button: HWND::default(),
            background: Cell::new(HBRUSH::default()),
            selected_brush: Cell::new(HBRUSH::default()),
            palette: Cell::new(Palette::default()),
            preferences: Cell::new(preferences),
            screen: Cell::new(window_placement::ScreenArea::default()),
            monitor_reference: Cell::new(parent),
            settings_page: OnceCell::new(),
            settings_open: Cell::new(false),
            fonts: Cell::new(Fonts::default()),
            dpi: Cell::new(96),
            width: Cell::new(0),
            height: Cell::new(0),
            bounds: Cell::new(RECT::default()),
            clip_shape: Cell::new(None),
            layout_key: Cell::new(LayoutKey::STALE),
            rows: RefCell::new(Vec::new()),
            icon_cache: RefCell::new(IconCache::default()),
        };
        unsafe {
            view.input = child(
                parent,
                instance,
                w!("EDIT"),
                w!(""),
                WINDOW_STYLE(ES_AUTOHSCROLL as u32) | WS_TABSTOP,
                INPUT_ID,
            )?;
            view.results = child(
                parent,
                instance,
                w!("LISTBOX"),
                w!(""),
                WS_CLIPSIBLINGS
                    | WINDOW_STYLE(
                        (LBS_NOTIFY | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS | LBS_NOINTEGRALHEIGHT)
                            as u32,
                    ),
                RESULTS_ID,
            )?;
            view.footer = child(
                parent,
                instance,
                w!("STATIC"),
                w!("Type to search"),
                WINDOW_STYLE(SS_ENDELLIPSIS.0),
                FOOTER_ID,
            )?;
            view.clock = child(
                parent,
                instance,
                w!("STATIC"),
                w!(""),
                WINDOW_STYLE(SS_CENTER.0),
                footer::CLOCK_ID,
            )?;
            // Tab order follows window order: the output comes after the footer, as it did
            // when it was created there.
            SetWindowPos(
                view.output,
                Some(view.footer),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )?;
            view.console.set_prompt(view.input);
            view.power_button = child(
                parent,
                instance,
                w!("BUTTON"),
                w!("Power options"),
                WINDOW_STYLE(BS_OWNERDRAW as u32) | WS_TABSTOP,
                POWER_ID,
            )?;
            view.settings_button = child(
                parent,
                instance,
                w!("BUTTON"),
                w!("Settings"),
                WINDOW_STYLE(BS_OWNERDRAW as u32) | WS_TABSTOP,
                SETTINGS_ID,
            )?;
            button_hover::install(view.power_button)?;
            button_hover::install(view.settings_button)?;
            SendMessageW(view.input, EM_SETLIMITTEXT, Some(WPARAM(4096)), None);
            let cue = wide(command_prompt::SEARCH_CUE);
            SendMessageW(
                view.input,
                EM_SETCUEBANNER,
                Some(WPARAM(1)),
                Some(LPARAM(cue.as_ptr() as isize)),
            );
        }
        view.set_fonts(unsafe { GetDpiForWindow(parent) })?;
        // Saved preferences that cannot be applied (a display that cannot be read) must not
        // stop Core from starting; it opens with the defaults instead.
        if let Err(error) = view.apply(preferences, PreferenceChanges::ALL) {
            eprintln!("Could not apply saved appearance: {error}");
            view.apply(Preferences::default(), PreferenceChanges::ALL)?;
        }
        Ok(view)
    }

    pub fn set_dpi(&self, dpi: u32) -> windows::core::Result<()> {
        self.set_fonts(dpi)?;
        self.layout()
    }

    fn set_fonts(&self, dpi: u32) -> windows::core::Result<()> {
        let fonts = Fonts::create(dpi)?;
        let previous = self.fonts.replace(fonts);
        self.dpi.set(dpi);
        unsafe {
            for (control, font) in [
                (self.input, fonts.input),
                (self.results, fonts.title),
                (self.footer, fonts.detail),
                (self.clock, fonts.detail),
                (self.power_button, fonts.detail),
                (self.settings_button, fonts.detail),
                (self.output, fonts.mono),
            ] {
                SendMessageW(
                    control,
                    WM_SETFONT,
                    Some(WPARAM(font.0 as usize)),
                    Some(LPARAM(0)),
                );
            }
            SendMessageW(
                self.results,
                LB_SETITEMHEIGHT,
                Some(WPARAM(0)),
                Some(LPARAM(scale(theme::ROW_HEIGHT, dpi) as isize)),
            );
        }
        previous.delete();
        Ok(())
    }

    fn layout(&self) -> windows::core::Result<()> {
        let key = self.current_layout_key();
        let previous = self.layout_key.get();
        let settings_open = self.settings_open.get();
        if previous == key {
            // Only what the rows show may have changed.
            self.invalidate_below_search(settings_open);
            return Ok(());
        }
        let LayoutKey {
            dpi,
            count,
            row_height,
            output_height,
            terminal,
            grid,
            ..
        } = key;
        let row_count = count.max(1) as i32;
        let preferences = self.preferences.get();
        let spacing = scale(preferences.edge_spacing.logical(), dpi);
        let area = self.screen.get().placement(spacing);
        let settings_dpi = settings_layout::fitting_dpi(dpi, area);
        let logical_height = if settings_open {
            settings_layout::PAGE_HEIGHT
        } else {
            let body = match (terminal, output_height) {
                (true, 0) => 0,
                (true, output) => output + theme::OUTPUT_GAP,
                (false, _) if grid => {
                    self.fit_grid(count, area, dpi) * theme::GRID_TILE_HEIGHT + theme::GRID_GAP
                }
                (false, _) => row_count * row_height,
            };
            let height = theme::RESULTS_TOP + body + theme::FOOTER_HEIGHT;
            if self.power_menu_open() {
                height.max(280)
            } else {
                height
            }
        };
        let (width, height) = if settings_open {
            (
                scale(settings_layout::PAGE_WIDTH, settings_dpi),
                scale(logical_height, settings_dpi),
            )
        } else {
            (scale(theme::WIDTH, dpi), scale(logical_height, dpi))
        };
        let bounds = window_placement::bounds(area, width, height, preferences.position);
        let client_width = bounds.right - bounds.left;
        let client_height = bounds.bottom - bounds.top;
        // The search box row keeps its pixels unless its width, scale or page changed.
        let keeps_search_row = !settings_open
            && previous != LayoutKey::STALE
            && previous.dpi == dpi
            && self.width.get() == client_width;
        self.width.set(client_width);
        self.height.set(client_height);
        self.bounds.set(bounds);
        let search = SearchLayout::new(client_width, client_height, dpi);
        unsafe {
            SendMessageW(
                self.results,
                LB_SETITEMHEIGHT,
                Some(WPARAM(0)),
                Some(LPARAM(scale(row_height, dpi) as isize)),
            );
            SetWindowPos(
                self.parent,
                None,
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )?;
            self.clip()?;
            if settings_open {
                self.settings_page
                    .get()
                    .expect("open settings page")
                    .layout(settings_dpi)?;
            }
            let controls = [
                (self.input, search.input),
                (self.settings_button, search.settings),
                (self.results, search.results),
                (self.footer, search.footer),
                (self.clock, search.clock),
                (self.power_button, search.power),
                (self.output, search.output),
            ];
            // One batch moves every control together, with a single repaint.
            let mut batch = BeginDeferWindowPos(controls.len() as i32)?;
            for (control, area) in controls {
                // Moved controls repaint rather than reuse pixels from where they were.
                batch = DeferWindowPos(
                    batch,
                    control,
                    None,
                    area.left,
                    area.top,
                    (area.right - area.left).max(0),
                    (area.bottom - area.top).max(0),
                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOCOPYBITS,
                )?;
            }
            EndDeferWindowPos(batch)?;
            let _ = ShowWindow(
                self.results,
                if settings_open || terminal || grid || self.rows.borrow().is_empty() {
                    SW_HIDE
                } else {
                    SW_SHOWNA
                },
            );
        }
        if keeps_search_row {
            self.invalidate_below_search(false);
        } else {
            unsafe {
                let _ = InvalidateRect(Some(self.parent), None, false);
            }
        }
        // A closed menu is hidden; opening it lays out again first.
        if let Some(menu) = self.power_menu.get().filter(|menu| menu.is_open()) {
            menu.layout(client_width, client_height, dpi)?;
        }
        self.layout_key.set(key);
        Ok(())
    }

    /// What the window's size and control positions depend on right now.
    fn current_layout_key(&self) -> LayoutKey {
        let dpi = self.dpi.get();
        let settings_open = self.settings_open.get();
        let terminal = self.terminal.get() && !settings_open;
        // Terminal mode shows output in place of rows, so rows do not affect its size.
        let count = if terminal {
            0
        } else {
            self.rows.borrow().len()
        };
        let row_height = if self
            .rows
            .borrow()
            .first()
            .is_some_and(|row| painting::is_answer(row.kind))
        {
            theme::ANSWER_HEIGHT
        } else {
            theme::ROW_HEIGHT
        };
        let grid = self.grid();
        let output_height = if terminal && self.output_visible.get() {
            self.fitted_output_height()
        } else {
            0
        };
        LayoutKey {
            dpi,
            count,
            row_height,
            output_height,
            terminal,
            grid,
            settings: settings_open,
        }
    }

    /// Repaints everything below the search box: the section label, the rows or tiles, the
    /// output and the footer. The settings page is repainted whole.
    fn invalidate_below_search(&self, settings_open: bool) {
        let area = painting::rectangle(
            0,
            scale(SECTION_LABEL_TOP, self.dpi.get()),
            self.width.get(),
            self.height.get(),
        );
        unsafe {
            let _ = InvalidateRect(
                Some(self.parent),
                (!settings_open).then_some(&area as *const RECT),
                false,
            );
        }
    }

    /// Lays out again after a change `set_rows` does not make, such as entering terminal mode.
    pub fn relayout(&self) {
        if let Err(error) = self.layout() {
            self.set_footer(&format!("Could not lay out Core: {error}"));
        }
    }

    /// Updates only what differs from the current preferences.
    pub fn apply_preferences(&self, preferences: Preferences) -> windows::core::Result<()> {
        self.apply(
            preferences,
            PreferenceChanges::between(self.preferences.get(), preferences),
        )
    }

    fn apply(
        &self,
        preferences: Preferences,
        changes: PreferenceChanges,
    ) -> windows::core::Result<()> {
        if changes.palette {
            let palette = Palette::for_background(preferences.background);
            let background = unsafe { CreateSolidBrush(palette.background) };
            if background.0.is_null() {
                return Err(windows::core::Error::from_win32());
            }
            let selected = unsafe { CreateSolidBrush(palette.selected) };
            if selected.0.is_null() {
                unsafe {
                    let _ = DeleteObject(background.into());
                }
                return Err(windows::core::Error::from_win32());
            }
            for old in [
                self.background.replace(background),
                self.selected_brush.replace(selected),
            ] {
                if !old.0.is_null() {
                    unsafe {
                        let _ = DeleteObject(old.into());
                    }
                }
            }
            self.palette.set(palette);
            self.console.set_palette(palette);
        }
        self.preferences.set(preferences);
        if changes.theme {
            let palette = self.palette.get();
            // Dark scroll bars for the command output on dark backgrounds.
            let theme = if palette.is_dark() {
                w!("DarkMode_Explorer")
            } else {
                w!("Explorer")
            };
            unsafe {
                let _ = SetWindowTheme(self.output, theme, PCWSTR::null());
            }
            if let Some(page) = self.settings_page.get() {
                page.apply_theme(palette);
            }
        }
        if changes.screen {
            self.screen.set(super::displays::screen_area(
                preferences.display,
                self.monitor_reference.get(),
            )?);
        }
        if changes.layout {
            self.invalidate_layout();
            self.layout()?;
        } else if changes.corners {
            // Rounding changes neither the window's size nor any control's position.
            self.clip()?;
            unsafe {
                let _ = InvalidateRect(Some(self.parent), None, false);
            }
        }
        if changes.palette {
            // Controls take their colors from the brushes, so every one repaints.
            unsafe {
                let _ = RedrawWindow(
                    Some(self.parent),
                    None,
                    None,
                    RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_ERASE,
                );
            }
        }
        Ok(())
    }

    /// Rounds the corners that float; corners touching a screen edge stay square.
    /// An unchanged shape keeps the current region instead of replacing and redrawing it.
    fn clip(&self) -> windows::core::Result<()> {
        let dpi = self.dpi.get();
        let preferences = self.preferences.get();
        let spacing = scale(preferences.edge_spacing.logical(), dpi);
        let shape = window_placement::ClipShape::new(
            self.bounds.get(),
            self.screen.get().edges(spacing),
            scale(preferences.corner_radius.logical(), dpi),
        );
        if self.clip_shape.get() == Some(shape) {
            return Ok(());
        }
        // Forget the old shape first: a failure leaves the region unknown, so it is retried.
        self.clip_shape.set(None);
        window_placement::clip_to_edges(self.parent, shape)?;
        self.clip_shape.set(Some(shape));
        Ok(())
    }

    /// Runs each time Core is shown. Layout runs again only on another screen area, but
    /// everything is repainted: controls that were moved or resized while Core was hidden may
    /// otherwise show the blank pixels they had then.
    pub fn position_on_monitor(&self, reference: HWND) -> windows::core::Result<()> {
        self.monitor_reference.set(reference);
        let screen = super::displays::screen_area(self.preferences.get().display, reference)?;
        if self.screen.replace(screen) != screen {
            self.invalidate_layout();
        }
        self.layout()?;
        unsafe {
            let _ = RedrawWindow(
                Some(self.parent),
                None,
                None,
                RDW_INVALIDATE | RDW_ALLCHILDREN,
            );
        }
        Ok(())
    }

    fn invalidate_layout(&self) {
        self.layout_key.set(LayoutKey::STALE);
    }

    /// The search box text, with the hidden `/` restored in command mode.
    pub fn query(&self) -> String {
        let text = control_text(self.input);
        if self.command_mode.get() {
            format!("/{text}")
        } else {
            text
        }
    }

    pub fn set_rows(&self, results: &[SearchResult]) {
        let mut rows: Vec<DisplayRow> = results
            .iter()
            .take(ROW_LIMIT)
            .map(DisplayRow::new)
            .collect();
        // The same results again (an exchange-rate refresh, a repeated search): the list,
        // its selection and the window already show them.
        if same_rows(&self.rows.borrow(), &rows)
            && self.layout_key.get() == self.current_layout_key()
        {
            // As a reset would, clear the hover; the pointer's next move restores it.
            self.leave_grid();
            return;
        }
        self.grid_hover.set(None);
        let previous = self.rows.take();
        let mut cache = self.icon_cache.borrow_mut();
        for row in &mut rows {
            // An icon shown recently comes back at once; the rest are asked for.
            row.icon = cache.get(&row.identifier).or_else(|| {
                previous
                    .iter()
                    .find(|old| old.identifier == row.identifier)
                    .and_then(|old| old.icon.clone())
            });
        }
        drop(cache);
        *self.rows.borrow_mut() = rows;
        unsafe {
            SendMessageW(self.results, WM_SETREDRAW, Some(WPARAM(0)), None);
            SendMessageW(self.results, LB_RESETCONTENT, None, None);
            for result in results.iter().take(ROW_LIMIT) {
                let title = wide(&result.title);
                SendMessageW(
                    self.results,
                    LB_ADDSTRING,
                    None,
                    Some(LPARAM(title.as_ptr() as isize)),
                );
            }
            SendMessageW(self.results, LB_SETCURSEL, Some(WPARAM(0)), None);
            SendMessageW(self.results, WM_SETREDRAW, Some(WPARAM(1)), None);
            let _ = InvalidateRect(Some(self.results), None, false);
        }
        if let Err(error) = self.layout() {
            self.set_footer(&format!("Could not lay out Core: {error}"));
        }
    }

    pub fn set_footer(&self, text: &str) {
        if *self.footer_text.borrow() == text {
            return;
        }
        let wide_text = wide(text);
        if let Err(error) = unsafe { SetWindowTextW(self.footer, PCWSTR(wide_text.as_ptr())) } {
            eprintln!("Could not update launcher status: {error}");
            return;
        }
        self.footer_text.borrow_mut().clear();
        self.footer_text.borrow_mut().push_str(text);
    }

    pub fn set_icons(&self, icons: &[LoadedIcon]) {
        let mut cache = self.icon_cache.borrow_mut();
        for loaded in icons {
            if let Some(icon) = &loaded.icon {
                cache.insert(loaded.identifier.clone(), icon.clone());
            }
        }
        drop(cache);
        let mut changed = Vec::new();
        for (index, row) in self.rows.borrow_mut().iter_mut().enumerate() {
            if let Some(loaded) = icons
                .iter()
                .find(|loaded| loaded.identifier == row.identifier)
            {
                if !same_icon(&row.icon, &loaded.icon) {
                    changed.push(index);
                }
                row.icon = loaded.icon.clone();
            }
        }
        // Only rows whose icon changed repaint. A failed icon for a row that has none, or an
        // icon it already shows, changes nothing.
        let grid = self.grid();
        for index in changed {
            // In the grid, tiles are painted by the window itself; the hidden list never repaints.
            if grid {
                self.invalidate_tile(index);
            } else {
                self.invalidate_row(index);
            }
        }
    }

    fn invalidate_row(&self, index: usize) {
        let mut area = RECT::default();
        unsafe {
            if SendMessageW(
                self.results,
                LB_GETITEMRECT,
                Some(WPARAM(index)),
                Some(LPARAM(&mut area as *mut RECT as isize)),
            )
            .0 != LB_ERR as isize
            {
                let _ = InvalidateRect(Some(self.results), Some(&area), false);
            }
        }
    }

    pub fn has_icon(&self, identifier: &str) -> bool {
        self.rows
            .borrow()
            .iter()
            .any(|row| &*row.identifier == identifier && row.icon.is_some())
    }

    pub fn selected(&self) -> usize {
        unsafe {
            SendMessageW(self.results, LB_GETCURSEL, None, None)
                .0
                .max(0) as usize
        }
    }

    pub fn move_selection(&self, delta: isize) {
        unsafe {
            let count = SendMessageW(self.results, LB_GETCOUNT, None, None).0;
            if count <= 0 {
                return;
            }
            let next = (self.selected() as isize + delta).rem_euclid(count);
            SendMessageW(
                self.results,
                LB_SETCURSEL,
                Some(WPARAM(next as usize)),
                None,
            );
        }
    }
}

impl Drop for View {
    fn drop(&mut self) {
        self.fonts.get().delete();
        unsafe {
            let _ = DeleteObject(self.background.get().into());
            let _ = DeleteObject(self.selected_brush.get().into());
        }
    }
}

/// Rows that draw the same: same results, titles, details and kinds. Icons are carried over.
fn same_rows(current: &[DisplayRow], next: &[DisplayRow]) -> bool {
    current.len() == next.len()
        && current.iter().zip(next).all(|(current, next)| {
            current.identifier == next.identifier
                && current.title == next.title
                && current.detail == next.detail
                && current.kind == next.kind
        })
}

fn same_icon(current: &Option<Arc<ApplicationIcon>>, next: &Option<Arc<ApplicationIcon>>) -> bool {
    match (current, next) {
        (Some(current), Some(next)) => Arc::ptr_eq(current, next),
        (None, None) => true,
        _ => false,
    }
}

pub(super) fn control_text(window: HWND) -> String {
    unsafe {
        let length = GetWindowTextLengthW(window).max(0) as usize;
        let mut buffer = vec![0; length + 1];
        let copied = GetWindowTextW(window, &mut buffer).max(0) as usize;
        String::from_utf16_lossy(&buffer[..copied])
    }
}

pub(super) unsafe fn child(
    parent: HWND,
    instance: HINSTANCE,
    class: PCWSTR,
    text: PCWSTR,
    style: WINDOW_STYLE,
    identifier: usize,
) -> windows::core::Result<HWND> {
    CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        text,
        WS_CHILD | WS_VISIBLE | style,
        0,
        0,
        1,
        1,
        Some(parent),
        Some(HMENU(identifier as *mut _)),
        Some(instance),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_footer_text_does_not_send_another_native_update() {
        let _serial = crate::windows::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let instance = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }
            .unwrap()
            .into();
        let parent = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!(""),
                WS_POPUP,
                0,
                0,
                100,
                100,
                None,
                None,
                Some(instance),
                None,
            )
        }
        .unwrap();
        let view = View::create(parent, instance, Preferences::default()).unwrap();
        view.set_footer("Esc to stop");
        assert_eq!(control_text(view.footer), "Esc to stop");
        unsafe { SetWindowTextW(view.footer, w!("native sentinel")) }.unwrap();
        view.set_footer("Esc to stop");
        assert_eq!(control_text(view.footer), "native sentinel");
        view.set_footer("");
        assert_eq!(control_text(view.footer), "");
        view.set_footer("Finished");
        assert_eq!(control_text(view.footer), "Finished");
        unsafe { DestroyWindow(parent) }.unwrap();
    }

    #[test]
    fn cached_icons_come_back_without_reloading_and_evictions_release_them() {
        use windows::Win32::System::Threading::{
            GetCurrentProcess, GetGuiResources, GR_GDIOBJECTS, GR_USEROBJECTS,
        };
        let _serial = crate::windows::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let handles = || unsafe {
            (
                GetGuiResources(GetCurrentProcess(), GR_USEROBJECTS),
                GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS),
            )
        };
        let instance = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }
            .unwrap()
            .into();
        let parent = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!(""),
                WS_POPUP,
                0,
                0,
                100,
                100,
                None,
                None,
                Some(instance),
                None,
            )
        }
        .unwrap();
        let view = View::create(parent, instance, Preferences::default()).unwrap();
        let results = |range: std::ops::Range<usize>| -> Vec<SearchResult> {
            range
                .map(|index| SearchResult {
                    kind: ResultKind::Application,
                    id: format!("app:{index}").into(),
                    title: format!("App {index}").into(),
                    description: "Programs".into(),
                    action: Action::LaunchApplication(format!("app:{index}").into()),
                })
                .collect()
        };
        // A new icon handle for each result, as the worker delivers after loading one.
        let loaded = |results: &[SearchResult]| -> Vec<LoadedIcon> {
            results
                .iter()
                .map(|result| {
                    let icon = unsafe { CopyIcon(LoadIconW(None, IDI_APPLICATION).unwrap()) };
                    LoadedIcon::new(
                        result.id.clone(),
                        ApplicationIcon::from_handle(icon.unwrap()).map(Arc::new),
                    )
                })
                .collect()
        };
        let empty = handles();
        // Load a screen at a time, far more icons than the cache keeps.
        for start in (0..400).step_by(VISIBLE_RESULT_LIMIT) {
            let screen = results(start..start + VISIBLE_RESULT_LIMIT);
            view.set_rows(&screen);
            view.set_icons(&loaded(&screen));
        }
        let full = handles();
        assert!(full.0 <= empty.0 + icon_cache::CAPACITY as u32, "{full:?}");
        // Results typed again show their icons at once, without loading new handles.
        view.set_rows(&results(300..308));
        assert!((300..308).all(|index| view.has_icon(&format!("app:{index}"))));
        assert_eq!(handles(), full);
        // Evicted ones are asked for again.
        view.set_rows(&results(0..8));
        assert!(!view.has_icon("app:0"));
        // Every icon handle goes with the view; its fonts and brushes are deleted too.
        drop(view);
        let released = handles();
        assert_eq!(released.0, empty.0);
        assert!(released.1 < empty.1);
        unsafe { DestroyWindow(parent) }.unwrap();
    }

    use core_engine::search::{Action, ResultKind};

    fn row(kind: ResultKind, id: &str, title: &str, description: &str) -> DisplayRow {
        DisplayRow::new(&SearchResult {
            kind,
            id: id.into(),
            title: title.into(),
            description: description.into(),
            action: Action::CopyText(title.into()),
        })
    }

    fn rows() -> Vec<DisplayRow> {
        vec![
            row(
                ResultKind::Application,
                "app:1",
                "Code",
                r"C:\Programs\Code",
            ),
            row(
                ResultKind::Conversion,
                "convert",
                "5 USD",
                "€4.60 · rates of today",
            ),
        ]
    }

    #[test]
    fn identical_results_are_the_same_rows_whatever_their_icons() {
        let mut current = rows();
        assert!(same_rows(&current, &rows()));
        assert!(same_rows(&[], &[]));
        // Icons are carried over from the current rows, so a missing one is not a change.
        current[0].icon = None;
        assert!(same_rows(&current, &rows()));
        assert!(same_icon(&None, &None));
    }

    #[test]
    fn any_difference_in_id_title_detail_kind_or_count_is_a_change() {
        let current = rows();
        for changed in [
            row(
                ResultKind::Application,
                "app:2",
                "Code",
                r"C:\Programs\Code",
            ),
            row(
                ResultKind::Application,
                "app:1",
                "Code - Insiders",
                r"C:\Programs\Code",
            ),
            row(
                ResultKind::Application,
                "app:1",
                "Code",
                r"C:\Programs\Tools",
            ),
            // Same app among recent tiles: drawn as a grid instead of a row.
            row(ResultKind::Recent, "app:1", "Code", r"C:\Programs\Code"),
        ] {
            let mut next = rows();
            next[0] = changed;
            assert!(!same_rows(&current, &next));
        }
        // A new exchange rate changes only the detail.
        let mut refreshed = rows();
        refreshed[1] = row(
            ResultKind::Conversion,
            "convert",
            "5 USD",
            "€4.61 · rates of today",
        );
        assert!(!same_rows(&current, &refreshed));
        assert!(!same_rows(&current, &current[..1]));
        assert!(!same_rows(&[], &current));
    }
}
