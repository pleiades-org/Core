use super::{
    control_style::{self, Look},
    layout::{self, *},
    quicklink_table::QuicklinkTable,
    slider::{self, SlideStage},
    BackgroundColor, CornerRadius, DisplayChoice, EdgeSpacing, Preferences, ScreenPosition,
    SettingsDocument, Shortcut,
};
use crate::windows::{
    button_hover, painting,
    theme::{scale, Fonts, Palette},
    view::{child, control_text},
    wide,
};
use core_engine::search::ShellKind;
use std::cell::{Cell, RefCell};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        UI::{
            Controls::{
                SetWindowTheme, DRAWITEMSTRUCT, EM_SETLIMITTEXT, NMCUSTOMDRAW, ODS_FOCUS,
                ODS_SELECTED,
            },
            WindowsAndMessaging::*,
        },
    },
};

pub const COLOR_ID: usize = 200;
const BLACK_ID: usize = 201;
const CHARCOAL_ID: usize = 202;
const LIGHT_ID: usize = 203;
const POSITION_FIRST_ID: usize = 210;
pub const RETRY_ID: usize = 220;
pub const DONE_ID: usize = 221;
pub const STATUS_ID: usize = 222;
const COLOR_LABEL_ID: usize = 223;
const POSITION_LABEL_ID: usize = 224;
const APPEARANCE_CATEGORY_ID: usize = 230;
const BEHAVIOUR_CATEGORY_ID: usize = 231;
const QUICKLINKS_CATEGORY_ID: usize = 232;
pub const SHORTCUT_ID: usize = 240;
const WINDOWS_KEY_ID: usize = 241;
const DISPLAY_ID: usize = 242;
const STARTUP_ID: usize = 243;
const SHORTCUT_LABEL_ID: usize = 244;
const DISPLAY_LABEL_ID: usize = 245;
const SHORTCUT_HELP_ID: usize = 246;
const SHELL_ID: usize = 247;
const CLEAR_HISTORY_ID: usize = 248;
const COMMANDS_LABEL_ID: usize = 249;

/// The display and shell dropdowns, whose own keys (Enter, Esc, arrows) must reach them.
pub fn is_dropdown(identifier: usize) -> bool {
    matches!(identifier, DISPLAY_ID | SHELL_ID)
}
const RADIUS_LABEL_ID: usize = 250;
const RADIUS_ID: usize = 251;
const RADIUS_VALUE_ID: usize = 252;
const SPACING_LABEL_ID: usize = 253;
const SPACING_ID: usize = 254;
const SPACING_VALUE_ID: usize = 255;
/// Clicking the track moves corner rounding by 4 px and edge spacing by 8 px.
const RADIUS_PAGE: u32 = 4;
const SPACING_PAGE: u32 = 8;
const SLIDER_WIDTH: i32 = 290;
const SLIDER_HEIGHT: i32 = 30;
const VALUE_HEIGHT: i32 = 20;
const SPACING_LEFT: i32 = layout::CONTENT_LEFT + 314;
const COLOR_INPUT_WIDTH: i32 = 126;
const SHORTCUT_INPUT_WIDTH: i32 = 318;
const WINDOWS_KEY_LEFT: i32 = 356;
const WINDOWS_KEY_WIDTH: i32 = 236;
const DONE_WIDTH: i32 = 138;
/// Dropdowns and switch rows share the shortcut field's width.
const FIELD_WIDTH: i32 = 342;
const CLEAR_HISTORY_WIDTH: i32 = 236;
/// A dropdown's height includes its open list; the closed field sizes itself to its font.
const DROPDOWN_LIST_HEIGHT: i32 = 260;
/// Centres a closed dropdown in a 40 px row, as tall as the text fields beside it.
const DROPDOWN_OFFSET: i32 = 4;
const RETRY_WIDTH: i32 = 144;
const FOOTER_GAP: i32 = 12;
/// Widths of the rounded boxes drawn behind the color and shortcut text boxes.
const COLOR_FIELD_WIDTH: i32 = 154;
const SHORTCUT_FIELD_WIDTH: i32 = 342;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Appearance,
    Behaviour,
    Quicklinks,
}

pub enum SettingsAction {
    Edit,
    Change,
    Retry,
    Done,
    ClearHistory,
    None,
}

pub struct SettingsPage {
    pub color: HWND,
    quicklinks: QuicklinkTable,
    controls: Vec<(usize, HWND)>,
    position: Cell<ScreenPosition>,
    section: Cell<Section>,
    display: Cell<DisplayChoice>,
    displays: RefCell<Vec<crate::windows::displays::Display>>,
    /// The display each item of the display dropdown selects.
    display_choices: RefCell<Vec<DisplayChoice>>,
    startup: Cell<bool>,
    shell: Cell<ShellKind>,
    dpi: Cell<u32>,
    fonts: Cell<Fonts>,
    save_failed: Cell<bool>,
}

impl SettingsPage {
    pub fn scroll_quicklinks(&self, command: u16, wheel: Option<i16>) {
        if self.section.get() == Section::Quicklinks {
            self.quicklinks.scroll(command, wheel);
        }
    }

    pub fn advance_quicklink_tab(&self, identifier: usize, backwards: bool) -> bool {
        self.section.get() == Section::Quicklinks
            && self.quicklinks.advance_tab(identifier, backwards)
    }

    pub fn create(parent: HWND, instance: HINSTANCE) -> windows::core::Result<Self> {
        let mut page = Self {
            color: HWND::default(),
            quicklinks: QuicklinkTable::create(parent, instance)?,
            controls: Vec::new(),
            position: Cell::new(ScreenPosition::Center),
            section: Cell::new(Section::Appearance),
            display: Cell::new(DisplayChoice::Active),
            displays: RefCell::new(Vec::new()),
            display_choices: RefCell::new(Vec::new()),
            startup: Cell::new(false),
            shell: Cell::new(ShellKind::Default),
            dpi: Cell::new(0),
            fonts: Cell::new(Fonts::default()),
            save_failed: Cell::new(false),
        };
        page.add(
            parent,
            instance,
            "Background color (hex)",
            w!("STATIC"),
            WINDOW_STYLE(0),
            COLOR_LABEL_ID,
        )?;
        page.color = page.add(
            parent,
            instance,
            "Background color (hex)",
            w!("EDIT"),
            WINDOW_STYLE(ES_AUTOHSCROLL as u32) | WS_TABSTOP,
            COLOR_ID,
        )?;
        unsafe {
            SendMessageW(page.color, EM_SETLIMITTEXT, Some(WPARAM(7)), None);
        }
        for (identifier, label) in [
            (BLACK_ID, "OLED black"),
            (CHARCOAL_ID, "Charcoal"),
            (LIGHT_ID, "Light"),
        ] {
            page.add_button(parent, instance, label, identifier)?;
        }
        page.add(
            parent,
            instance,
            "Screen position",
            w!("STATIC"),
            WINDOW_STYLE(0),
            POSITION_LABEL_ID,
        )?;
        for (index, position) in ScreenPosition::ALL.into_iter().enumerate() {
            page.add_button(parent, instance, position.name(), POSITION_FIRST_ID + index)?;
        }
        page.add_sliders(parent, instance)?;
        page.add_button(parent, instance, "Appearance", APPEARANCE_CATEGORY_ID)?;
        page.add_button(parent, instance, "Behaviour", BEHAVIOUR_CATEGORY_ID)?;
        page.add_button(parent, instance, "Quicklinks", QUICKLINKS_CATEGORY_ID)?;
        // Switch text carries the state ("…: On") for screen readers; only the name is drawn.
        page.add_button(parent, instance, "Use Windows key: Off", WINDOWS_KEY_ID)?;
        page.add_dropdown(parent, instance, "Display", DISPLAY_ID)?;
        page.add_button(parent, instance, "Start with Windows: Off", STARTUP_ID)?;
        page.add_dropdown(parent, instance, "Commands run in", SHELL_ID)?;
        page.add_button(parent, instance, "Clear command history", CLEAR_HISTORY_ID)?;
        let shortcut = page.add(
            parent,
            instance,
            "Shortcut",
            w!("EDIT"),
            WINDOW_STYLE(ES_AUTOHSCROLL as u32) | WS_TABSTOP,
            SHORTCUT_ID,
        )?;
        unsafe {
            SendMessageW(shortcut, EM_SETLIMITTEXT, Some(WPARAM(64)), None);
        }
        for (identifier, label) in [
            (SHORTCUT_LABEL_ID, "Open Core shortcut"),
            (DISPLAY_LABEL_ID, "Display"),
            (COMMANDS_LABEL_ID, "Commands run in"),
            (
                SHORTCUT_HELP_ID,
                "Example: Ctrl+Alt+Space. Win replaces Start when tapped.",
            ),
        ] {
            page.add(
                parent,
                instance,
                label,
                w!("STATIC"),
                WINDOW_STYLE(0),
                identifier,
            )?;
        }
        page.add_button(parent, instance, "Done", DONE_ID)?;
        page.add_button(parent, instance, "Retry save", RETRY_ID)?;
        page.add(
            parent,
            instance,
            "",
            w!("STATIC"),
            WINDOW_STYLE(0),
            STATUS_ID,
        )?;
        page.show(false);
        Ok(page)
    }

    fn add_sliders(&mut self, parent: HWND, instance: HINSTANCE) -> windows::core::Result<()> {
        slider::register()?;
        for (label_id, label, slider_id, value_id, maximum, page) in [
            (
                RADIUS_LABEL_ID,
                "Corner rounding",
                RADIUS_ID,
                RADIUS_VALUE_ID,
                u32::from(CornerRadius::MAX),
                RADIUS_PAGE,
            ),
            (
                SPACING_LABEL_ID,
                "Screen edge spacing",
                SPACING_ID,
                SPACING_VALUE_ID,
                u32::from(EdgeSpacing::MAX),
                SPACING_PAGE,
            ),
        ] {
            self.add(
                parent,
                instance,
                label,
                w!("STATIC"),
                WINDOW_STYLE(0),
                label_id,
            )?;
            // The trackbar's text is its accessible name.
            let control = self.add(
                parent,
                instance,
                label,
                slider::CLASS,
                WINDOW_STYLE(slider::STYLE) | WS_TABSTOP,
                slider_id,
            )?;
            slider::configure(control, maximum, page);
            self.add(
                parent,
                instance,
                "",
                w!("STATIC"),
                WINDOW_STYLE(0),
                value_id,
            )?;
        }
        Ok(())
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
        button_hover::install(button)?;
        Ok(())
    }

    /// A native dropdown list: its chevron and open list make the choice obvious, and it works
    /// with the keyboard and screen readers.
    fn add_dropdown(
        &mut self,
        parent: HWND,
        instance: HINSTANCE,
        label: &str,
        identifier: usize,
    ) -> windows::core::Result<()> {
        self.add(
            parent,
            instance,
            label,
            w!("COMBOBOX"),
            WINDOW_STYLE(CBS_DROPDOWNLIST as u32) | WS_VSCROLL | WS_TABSTOP,
            identifier,
        )?;
        Ok(())
    }

    /// Dark or light dropdowns to match the background.
    pub fn apply_theme(&self, palette: Palette) {
        let theme = if palette.is_dark() {
            w!("DarkMode_CFD")
        } else {
            w!("CFD")
        };
        for identifier in [DISPLAY_ID, SHELL_ID] {
            unsafe {
                let _ = SetWindowTheme(self.control(identifier), theme, PCWSTR::null());
            }
        }
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

    pub fn reset(&self, document: SettingsDocument) {
        self.quicklinks.reset(&document.quicklinks);
        let settings = document.preferences;
        self.section.set(Section::Appearance);
        self.position.set(settings.position);
        self.display.set(settings.display);
        self.startup.set(settings.startup);
        self.shell.set(settings.shell);
        self.set_text(self.control(SHORTCUT_ID), &settings.shortcut.to_string());
        match crate::windows::displays::connected() {
            Ok(displays) => *self.displays.borrow_mut() = displays,
            Err(error) => eprintln!("Could not enumerate settings displays: {error}"),
        }
        self.set_text(self.color, &settings.background.to_string());
        slider::set_position(
            self.control(RADIUS_ID),
            settings.corner_radius.logical() as u32,
        );
        slider::set_position(
            self.control(SPACING_ID),
            settings.edge_spacing.logical() as u32,
        );
        self.update_slider_values();
        self.set_save_error(false);
        self.status("Changes save automatically on this PC.");
        self.update_position_names();
        self.update_behaviour_names();
        self.fill_dropdowns();
    }

    pub fn draft(&self) -> Result<SettingsDocument, String> {
        Ok(SettingsDocument {
            preferences: Preferences {
                background: BackgroundColor::parse(&control_text(self.color))?,
                position: self.position.get(),
                shortcut: Shortcut::parse(&control_text(self.control(SHORTCUT_ID)))?,
                display: self.display.get(),
                startup: self.startup.get(),
                shell: self.shell.get(),
                corner_radius: CornerRadius::new(slider::position(self.control(RADIUS_ID))),
                edge_spacing: EdgeSpacing::new(slider::position(self.control(SPACING_ID))),
            },
            quicklinks: self.quicklinks.draft()?,
        })
    }

    pub fn focus_target(&self) -> HWND {
        match self.section.get() {
            Section::Appearance => self.color,
            Section::Behaviour => self.control(SHORTCUT_ID),
            Section::Quicklinks => self.quicklinks.focus_target(),
        }
    }

    pub fn command(&self, identifier: usize, notification: u32) -> SettingsAction {
        if matches!(identifier, RADIUS_ID | SPACING_ID) {
            return self.slide(identifier, SlideStage::from_code(notification));
        }
        if let Some(typing) = self.quicklinks.edit(identifier, notification) {
            return if typing {
                SettingsAction::Edit
            } else {
                SettingsAction::Change
            };
        }
        if matches!(identifier, COLOR_ID | SHORTCUT_ID) && notification == EN_CHANGE {
            if identifier == SHORTCUT_ID {
                // Typing "Win" turns the Windows-key switch on, and anything else turns it off.
                self.update_behaviour_names();
            }
            return SettingsAction::Edit;
        }
        if matches!(identifier, DISPLAY_ID | SHELL_ID) {
            return if notification == CBN_SELCHANGE {
                self.choose(identifier);
                SettingsAction::Change
            } else {
                SettingsAction::None
            };
        }
        if notification != BN_CLICKED {
            return SettingsAction::None;
        }
        match identifier {
            APPEARANCE_CATEGORY_ID | BEHAVIOUR_CATEGORY_ID | QUICKLINKS_CATEGORY_ID => {
                self.section.set(match identifier {
                    APPEARANCE_CATEGORY_ID => Section::Appearance,
                    BEHAVIOUR_CATEGORY_ID => Section::Behaviour,
                    _ => Section::Quicklinks,
                });
                self.show(true);
                unsafe {
                    let _ = RedrawWindow(
                        Some(GetParent(self.color).unwrap_or_default()),
                        None,
                        None,
                        RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_ERASE,
                    );
                }
                SettingsAction::None
            }
            WINDOWS_KEY_ID => {
                if self.windows_key() {
                    let default = Shortcut::default().to_string();
                    self.set_text(self.control(SHORTCUT_ID), &default);
                    self.status(&format!("Core opens with {default}."));
                } else {
                    self.set_text(self.control(SHORTCUT_ID), "Win");
                    self.status("Windows-key tapping replaces Start. Win combinations still work.");
                }
                self.update_behaviour_names();
                SettingsAction::Change
            }
            STARTUP_ID => {
                self.startup.set(!self.startup.get());
                self.update_behaviour_names();
                SettingsAction::Change
            }
            CLEAR_HISTORY_ID => SettingsAction::ClearHistory,
            RETRY_ID => SettingsAction::Retry,
            DONE_ID => SettingsAction::Done,
            BLACK_ID | CHARCOAL_ID | LIGHT_ID => {
                self.set_text(
                    self.color,
                    match identifier {
                        BLACK_ID => "#000000",
                        CHARCOAL_ID => "#181818",
                        _ => "#F5F5F5",
                    },
                );
                SettingsAction::Change
            }
            identifier
                if (POSITION_FIRST_ID..POSITION_FIRST_ID + ScreenPosition::ALL.len())
                    .contains(&identifier) =>
            {
                self.position
                    .set(ScreenPosition::ALL[identifier - POSITION_FIRST_ID]);
                self.update_position_names();
                SettingsAction::Change
            }
            _ => SettingsAction::None,
        }
    }

    /// Radius previews while dragging. Spacing waits for release: moving the window under a
    /// horizontal drag would shift the thumb and feed back into the value.
    fn slide(&self, identifier: usize, stage: SlideStage) -> SettingsAction {
        self.update_slider_values();
        match stage {
            SlideStage::Dragging if identifier == SPACING_ID => SettingsAction::None,
            SlideStage::Dragging | SlideStage::Stepped => SettingsAction::Edit,
            SlideStage::Finished => SettingsAction::Change,
        }
    }

    fn update_slider_values(&self) {
        let radius = slider::position(self.control(RADIUS_ID));
        self.set_text(
            self.control(RADIUS_VALUE_ID),
            &if radius == 0 {
                "Square corners".to_owned()
            } else {
                format!("{radius} px")
            },
        );
        let spacing = slider::position(self.control(SPACING_ID));
        self.set_text(
            self.control(SPACING_VALUE_ID),
            &if spacing == 0 {
                "Automatic · stays above the taskbar".to_owned()
            } else {
                format!("{spacing} px from the screen edge")
            },
        );
    }

    pub fn custom_draw(&self, draw: &NMCUSTOMDRAW, palette: Palette) -> Option<LRESULT> {
        matches!(draw.hdr.idFrom, RADIUS_ID | SPACING_ID)
            .then(|| slider::custom_draw(draw, self.dpi.get(), palette))
    }

    fn update_position_names(&self) {
        for (index, position) in ScreenPosition::ALL.into_iter().enumerate() {
            let label = if position == self.position.get() {
                format!("{} (selected)", position.name())
            } else {
                position.name().into()
            };
            self.set_text(self.control(POSITION_FIRST_ID + index), &label);
        }
    }

    pub fn show(&self, visible: bool) {
        self.quicklinks
            .show(visible && self.section.get() == Section::Quicklinks);
        for (identifier, control) in &self.controls {
            let in_section = match *identifier {
                APPEARANCE_CATEGORY_ID
                | BEHAVIOUR_CATEGORY_ID
                | QUICKLINKS_CATEGORY_ID
                | DONE_ID
                | STATUS_ID => true,
                RETRY_ID => self.save_failed.get(),
                SHORTCUT_ID..=COMMANDS_LABEL_ID => self.section.get() == Section::Behaviour,
                _ => self.section.get() == Section::Appearance,
            };
            unsafe {
                let _ = ShowWindow(
                    *control,
                    if visible && in_section {
                        SW_SHOWNA
                    } else {
                        SW_HIDE
                    },
                );
            }
        }
    }

    /// Switch text, which screen readers announce; the switches repaint to match.
    fn update_behaviour_names(&self) {
        for (identifier, name, on) in [
            (STARTUP_ID, "Start with Windows", self.startup.get()),
            (WINDOWS_KEY_ID, "Use Windows key", self.windows_key()),
        ] {
            let text = format!("{name}: {}", if on { "On" } else { "Off" });
            let control = self.control(identifier);
            if control_text(control) != text {
                self.set_text(control, &text);
                unsafe {
                    let _ = InvalidateRect(Some(control), None, false);
                }
            }
        }
    }

    fn windows_key(&self) -> bool {
        control_text(self.control(SHORTCUT_ID))
            .trim()
            .eq_ignore_ascii_case("win")
    }

    /// Lists the choices, with the saved one selected. A saved display that is not connected
    /// stays listed, so the setting is not silently lost.
    fn fill_dropdowns(&self) {
        let mut choices = vec![(
            DisplayChoice::Active,
            "Active display (where you are working)".to_owned(),
        )];
        choices.extend(
            self.displays
                .borrow()
                .iter()
                .map(|display| (display.choice, display.label.clone())),
        );
        let current = self.display.get();
        if !choices.iter().any(|(choice, _)| *choice == current) {
            choices.push((current, format!("{current} · disconnected; using active")));
        }
        let selected = choices
            .iter()
            .position(|(choice, _)| *choice == current)
            .unwrap_or(0);
        let labels: Vec<String> = choices.iter().map(|(_, label)| label.clone()).collect();
        self.set_items(DISPLAY_ID, &labels, selected);
        *self.display_choices.borrow_mut() =
            choices.into_iter().map(|(choice, _)| choice).collect();
        let shells: Vec<String> = ShellKind::ALL
            .iter()
            .map(|shell| match shell {
                ShellKind::Default => "Default shell (Windows Terminal's default)".to_owned(),
                shell => shell.label().to_owned(),
            })
            .collect();
        let shell = ShellKind::ALL
            .iter()
            .position(|shell| *shell == self.shell.get())
            .unwrap_or(0);
        self.set_items(SHELL_ID, &shells, shell);
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

    /// A dropdown choice was made.
    fn choose(&self, identifier: usize) {
        let index = unsafe { SendMessageW(self.control(identifier), CB_GETCURSEL, None, None) }.0;
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        match identifier {
            DISPLAY_ID => {
                if let Some(choice) = self.display_choices.borrow().get(index) {
                    self.display.set(*choice);
                }
            }
            _ => {
                if let Some(shell) = ShellKind::ALL.get(index) {
                    self.shell.set(*shell);
                }
            }
        }
    }

    pub fn set_save_error(&self, failed: bool) {
        self.save_failed.set(failed);
        unsafe {
            let _ = ShowWindow(
                self.control(RETRY_ID),
                if failed { SW_SHOWNA } else { SW_HIDE },
            );
        }
    }

    pub fn status(&self, message: &str) {
        self.set_text(self.control(STATUS_ID), message);
    }

    fn set_text(&self, control: HWND, text: &str) {
        let text = wide(text);
        if let Err(error) = unsafe { SetWindowTextW(control, PCWSTR(text.as_ptr())) } {
            eprintln!("Could not update settings control: {error}");
        }
    }

    fn control(&self, identifier: usize) -> HWND {
        self.controls
            .iter()
            .find(|(candidate, _)| *candidate == identifier)
            .expect("settings control exists")
            .1
    }

    pub fn layout(&self, dpi: u32) -> windows::core::Result<()> {
        // Controls still reference the old fonts until `place_controls` sends WM_SETFONT,
        // so delete them only afterwards.
        let retired = if self.dpi.get() == dpi {
            None
        } else {
            let fonts = Fonts::create(dpi)?;
            self.dpi.set(dpi);
            Some(self.fonts.replace(fonts))
        };
        let placed = self.place_controls(dpi);
        if let Some(fonts) = retired {
            fonts.delete();
        }
        placed
    }

    fn place_controls(&self, dpi: u32) -> windows::core::Result<()> {
        let fonts = self.fonts.get();
        let content_left = layout::CONTENT_LEFT;
        let content_width = layout::CONTENT_RIGHT - content_left;
        for (identifier, control) in &self.controls {
            let (left, top, width, height) = match *identifier {
                COLOR_LABEL_ID => (content_left, FIRST_LABEL_TOP, LABEL_WIDTH, LABEL_HEIGHT),
                POSITION_LABEL_ID => (content_left, POSITION_LABEL_TOP, LABEL_WIDTH, 24),
                COLOR_ID => (
                    content_left + INPUT_INSET,
                    FIRST_INPUT_TOP,
                    COLOR_INPUT_WIDTH,
                    INPUT_HEIGHT,
                ),
                BLACK_ID | CHARCOAL_ID | LIGHT_ID => (
                    content_left + SWATCH_LEFT + (*identifier - BLACK_ID) as i32 * SWATCH_PITCH,
                    FIRST_ROW_TOP,
                    SWATCH_WIDTH,
                    BUTTON_HEIGHT,
                ),
                RADIUS_LABEL_ID => (content_left, SLIDER_LABEL_TOP, SLIDER_WIDTH, LABEL_HEIGHT),
                RADIUS_ID => (content_left, SLIDER_TOP, SLIDER_WIDTH, SLIDER_HEIGHT),
                RADIUS_VALUE_ID => (content_left, SLIDER_VALUE_TOP, SLIDER_WIDTH, VALUE_HEIGHT),
                SPACING_LABEL_ID => (SPACING_LEFT, SLIDER_LABEL_TOP, SLIDER_WIDTH, LABEL_HEIGHT),
                SPACING_ID => (SPACING_LEFT, SLIDER_TOP, SLIDER_WIDTH, SLIDER_HEIGHT),
                SPACING_VALUE_ID => (SPACING_LEFT, SLIDER_VALUE_TOP, SLIDER_WIDTH, VALUE_HEIGHT),
                DONE_ID => (
                    layout::CONTENT_RIGHT - DONE_WIDTH,
                    layout::FOOTER_TOP,
                    DONE_WIDTH,
                    BUTTON_HEIGHT,
                ),
                RETRY_ID => (
                    layout::CONTENT_RIGHT - DONE_WIDTH - FOOTER_GAP - RETRY_WIDTH,
                    layout::FOOTER_TOP,
                    RETRY_WIDTH,
                    BUTTON_HEIGHT,
                ),
                STATUS_ID => (content_left, STATUS_TOP, content_width, BUTTON_HEIGHT),
                APPEARANCE_CATEGORY_ID | BEHAVIOUR_CATEGORY_ID | QUICKLINKS_CATEGORY_ID => (
                    SIDEBAR_INSET,
                    SIDEBAR_FIRST_TOP
                        + (*identifier - APPEARANCE_CATEGORY_ID) as i32 * SIDEBAR_PITCH,
                    layout::SIDEBAR_WIDTH - SIDEBAR_INSET,
                    SIDEBAR_BUTTON_HEIGHT,
                ),
                SHORTCUT_LABEL_ID => (content_left, FIRST_LABEL_TOP, LABEL_WIDTH, LABEL_HEIGHT),
                SHORTCUT_ID => (
                    content_left + INPUT_INSET,
                    FIRST_INPUT_TOP,
                    SHORTCUT_INPUT_WIDTH,
                    INPUT_HEIGHT,
                ),
                WINDOWS_KEY_ID => (
                    content_left + WINDOWS_KEY_LEFT,
                    FIRST_ROW_TOP,
                    WINDOWS_KEY_WIDTH,
                    BUTTON_HEIGHT,
                ),
                SHORTCUT_HELP_ID => (content_left, SHORTCUT_HELP_TOP, content_width, 26),
                DISPLAY_LABEL_ID => (content_left, DISPLAY_LABEL_TOP, LABEL_WIDTH, LABEL_HEIGHT),
                DISPLAY_ID => (
                    content_left,
                    DISPLAY_TOP + DROPDOWN_OFFSET,
                    FIELD_WIDTH,
                    DROPDOWN_LIST_HEIGHT,
                ),
                STARTUP_ID => (content_left, STARTUP_TOP, FIELD_WIDTH, BUTTON_HEIGHT),
                COMMANDS_LABEL_ID => (content_left, COMMANDS_LABEL_TOP, LABEL_WIDTH, LABEL_HEIGHT),
                SHELL_ID => (
                    content_left,
                    COMMANDS_TOP + DROPDOWN_OFFSET,
                    FIELD_WIDTH,
                    DROPDOWN_LIST_HEIGHT,
                ),
                CLEAR_HISTORY_ID => (
                    content_left + FIELD_WIDTH + 16,
                    COMMANDS_TOP,
                    CLEAR_HISTORY_WIDTH,
                    BUTTON_HEIGHT,
                ),
                position => {
                    let index = position - POSITION_FIRST_ID;
                    (
                        content_left + (index % POSITION_COLUMNS) as i32 * POSITION_COLUMN_PITCH,
                        POSITION_GRID_TOP + (index / POSITION_COLUMNS) as i32 * POSITION_ROW_PITCH,
                        POSITION_WIDTH,
                        POSITION_HEIGHT,
                    )
                }
            };
            unsafe {
                let font = if matches!(
                    *identifier,
                    COLOR_ID
                        | COLOR_LABEL_ID
                        | POSITION_LABEL_ID
                        | SHORTCUT_ID
                        | SHORTCUT_LABEL_ID
                        | DISPLAY_LABEL_ID
                        | COMMANDS_LABEL_ID
                        | DISPLAY_ID
                        | SHELL_ID
                        | RADIUS_LABEL_ID
                        | SPACING_LABEL_ID
                ) {
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
                SetWindowPos(
                    *control,
                    None,
                    scale(left, dpi),
                    scale(top, dpi),
                    scale(width, dpi),
                    scale(height, dpi),
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )?;
            }
        }
        self.quicklinks.layout(dpi, fonts)?;
        Ok(())
    }

    pub fn paint(&self, context: HDC, palette: Palette) {
        let dpi = self.dpi.get();
        let fonts = self.fonts.get();
        let (heading, description) = match self.section.get() {
            Section::Quicklinks => (
                "Quicklinks",
                "Add a link and name. A blank row follows each completed row.",
            ),
            Section::Appearance => (
                "Appearance",
                "Colors, shape and where Core appears on your screen.",
            ),
            Section::Behaviour => (
                "Behaviour",
                "How Core opens, starts with Windows and runs / commands.",
            ),
        };
        for (label, left, top, bottom, font, color) in [
            ("Settings", 28, HEADING_TOP, 58, fonts.input, palette.text),
            (
                "CORE",
                28,
                DESCRIPTION_TOP,
                82,
                fonts.detail,
                palette.secondary,
            ),
            (
                heading,
                layout::CONTENT_LEFT,
                HEADING_TOP,
                60,
                fonts.input,
                palette.text,
            ),
            (
                description,
                layout::CONTENT_LEFT,
                DESCRIPTION_TOP,
                84,
                fonts.detail,
                palette.secondary,
            ),
            (
                "Saves automatically",
                28,
                layout::FOOTER_TOP + 16,
                layout::FOOTER_TOP + 40,
                fonts.detail,
                palette.secondary,
            ),
        ] {
            painting::text(
                context,
                label,
                painting::rectangle(
                    scale(left, dpi),
                    scale(top, dpi),
                    scale(
                        if left < layout::SIDEBAR_WIDTH {
                            layout::SIDEBAR_WIDTH
                        } else {
                            layout::CONTENT_RIGHT
                        },
                        dpi,
                    ),
                    scale(bottom, dpi),
                ),
                font,
                color,
            );
        }
        if self.section.get() == Section::Quicklinks {
            self.quicklinks.paint(context, dpi, fonts, palette);
            return;
        }
        painting::rounded(
            context,
            &layout::area(
                layout::CONTENT_LEFT,
                FIRST_ROW_TOP,
                if self.section.get() == Section::Appearance {
                    COLOR_FIELD_WIDTH
                } else {
                    SHORTCUT_FIELD_WIDTH
                },
                BUTTON_HEIGHT,
                dpi,
            ),
            scale(8, dpi),
            palette.selected,
        );
    }

    pub fn draw_button(&self, item: &DRAWITEMSTRUCT, palette: Palette) -> bool {
        if self
            .quicklinks
            .draw_button(item, self.dpi.get(), self.fonts.get(), palette)
        {
            return true;
        }
        let identifier = item.CtlID as usize;
        if !self
            .controls
            .iter()
            .any(|(candidate, _)| *candidate == identifier)
        {
            return false;
        }
        let dpi = self.dpi.get();
        let fonts = self.fonts.get();
        let position = identifier
            .checked_sub(POSITION_FIRST_ID)
            .and_then(|index| ScreenPosition::ALL.get(index))
            .copied();
        let chosen = position == Some(self.position.get());
        let active = button_hover::is_hovered(item.hwndItem)
            || item.itemState.0 & (ODS_SELECTED.0 | ODS_FOCUS.0) != 0;
        let look = Look {
            dpi,
            fonts,
            palette,
        };
        let label = control_text(item.hwndItem);
        match identifier {
            STARTUP_ID | WINDOWS_KEY_ID => {
                // Text is "Name: On"; the switch shows the state.
                let name = label.split(':').next().unwrap_or(&label);
                let on = if identifier == STARTUP_ID {
                    self.startup.get()
                } else {
                    self.windows_key()
                };
                control_style::draw_toggle(item.hDC, item.rcItem, name, on, active, look);
                return true;
            }
            BLACK_ID | CHARCOAL_ID | LIGHT_ID | CLEAR_HISTORY_ID | DONE_ID | RETRY_ID => {
                let swatch = match identifier {
                    BLACK_ID => Some(COLORREF(0x0000_0000)),
                    CHARCOAL_ID => Some(COLORREF(0x0018_1818)),
                    LIGHT_ID => Some(COLORREF(0x00F5_F5F5)),
                    _ => None,
                };
                control_style::draw_action(item.hDC, item.rcItem, &label, swatch, active, look);
                return true;
            }
            _ => {}
        }
        let emphasized = chosen
            || (identifier == APPEARANCE_CATEGORY_ID && self.section.get() == Section::Appearance)
            || (identifier == BEHAVIOUR_CATEGORY_ID && self.section.get() == Section::Behaviour)
            || (identifier == QUICKLINKS_CATEGORY_ID && self.section.get() == Section::Quicklinks)
            || active;
        painting::fill(item.hDC, &item.rcItem, palette.background);
        if emphasized {
            painting::rounded(item.hDC, &item.rcItem, scale(8, dpi), palette.selected);
        }
        if let Some(position) = position {
            super::position_preview::draw(item, position, emphasized, dpi, fonts, palette);
            return true;
        }
        let mut area = item.rcItem;
        area.left += scale(16, dpi);
        painting::text(
            item.hDC,
            &label,
            area,
            fonts.detail,
            if emphasized {
                palette.text
            } else {
                palette.secondary
            },
        );
        true
    }
}

impl Drop for SettingsPage {
    fn drop(&mut self) {
        for (_, control) in &self.controls {
            unsafe {
                if IsWindow(Some(*control)).as_bool() {
                    let _ = DestroyWindow(*control);
                }
            }
        }
        self.fonts.get().delete();
    }
}
