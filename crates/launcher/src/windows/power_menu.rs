use super::{
    button_hover, painting,
    theme::{scale, Fonts, Palette},
    view::child,
};
use core_engine::search::PowerAction;
use std::cell::Cell;
use windows::{
    core::w,
    Win32::{
        Foundation::*,
        UI::{
            Controls::{DRAWITEMSTRUCT, ODS_FOCUS, ODS_SELECTED},
            Input::KeyboardAndMouse::{GetFocus, SetFocus},
            WindowsAndMessaging::*,
        },
    },
};

pub const POWER_ID: usize = 105;
const PANEL_ID: usize = 106;
const SLEEP_ID: usize = 107;
const RESTART_ID: usize = 108;
const SHUTDOWN_ID: usize = 109;

pub fn action(identifier: usize) -> Option<PowerAction> {
    match identifier {
        SLEEP_ID => Some(PowerAction::Sleep),
        RESTART_ID => Some(PowerAction::Restart),
        SHUTDOWN_ID => Some(PowerAction::ShutDown),
        _ => None,
    }
}

/// Inline child controls keep the menu inside Core's focus/dismissal boundary.
pub struct PowerMenu {
    panel: HWND,
    buttons: [(usize, HWND); 3],
    open: Cell<bool>,
}

impl PowerMenu {
    pub fn create(parent: HWND, instance: HINSTANCE) -> windows::core::Result<Self> {
        let panel = unsafe {
            child(
                parent,
                instance,
                w!("STATIC"),
                w!("Power options"),
                WINDOW_STYLE(windows::Win32::System::SystemServices::SS_OWNERDRAW.0)
                    | WS_CLIPSIBLINGS,
                PANEL_ID,
            )?
        };
        let mut menu = Self {
            panel,
            buttons: [
                (SLEEP_ID, HWND::default()),
                (RESTART_ID, HWND::default()),
                (SHUTDOWN_ID, HWND::default()),
            ],
            open: Cell::new(false),
        };
        for (identifier, button) in &mut menu.buttons {
            let label = match *identifier {
                SLEEP_ID => w!("Sleep"),
                RESTART_ID => w!("Restart"),
                _ => w!("Power off"),
            };
            *button = unsafe {
                child(
                    parent,
                    instance,
                    w!("BUTTON"),
                    label,
                    WINDOW_STYLE(BS_OWNERDRAW as u32) | WS_TABSTOP | WS_CLIPSIBLINGS,
                    *identifier,
                )?
            };
            button_hover::install(*button)?;
        }
        menu.show(false);
        Ok(menu)
    }
    pub fn is_open(&self) -> bool {
        self.open.get()
    }
    pub fn move_focus(&self, backwards: bool) {
        let focused = unsafe { GetFocus() };
        let current = self
            .buttons
            .iter()
            .position(|(_, control)| *control == focused)
            .unwrap_or(0);
        let next = if backwards {
            (current + self.buttons.len() - 1) % self.buttons.len()
        } else {
            (current + 1) % self.buttons.len()
        };
        unsafe {
            let _ = SetFocus(Some(self.buttons[next].1));
        }
    }
    pub fn show(&self, open: bool) {
        self.open.set(open);
        for control in [
            self.panel,
            self.buttons[0].1,
            self.buttons[1].1,
            self.buttons[2].1,
        ] {
            unsafe {
                let _ = ShowWindow(control, if open { SW_SHOWNA } else { SW_HIDE });
            }
        }
        if open {
            unsafe {
                let _ = SetFocus(Some(self.buttons[0].1));
            }
        }
    }
    pub fn layout(&self, width: i32, height: i32, dpi: u32) -> windows::core::Result<()> {
        let top = height - scale(188, dpi);
        let panel = [(self.panel, width - scale(72, dpi), top, 48, 136)];
        let buttons = self
            .buttons
            .iter()
            .enumerate()
            .map(|(index, (_, control))| {
                (
                    *control,
                    width - scale(66, dpi),
                    top + scale(8 + index as i32 * 40, dpi),
                    36,
                    40,
                )
            });
        for (control, left, top, width, height) in panel.into_iter().chain(buttons) {
            unsafe {
                SetWindowPos(
                    control,
                    Some(HWND_TOP),
                    left,
                    top,
                    scale(width, dpi),
                    scale(height, dpi),
                    SWP_NOACTIVATE,
                )?;
            }
        }
        Ok(())
    }
    pub fn draw(&self, item: &DRAWITEMSTRUCT, dpi: u32, fonts: Fonts, palette: Palette) -> bool {
        if item.CtlID as usize == PANEL_ID {
            painting::fill(item.hDC, &item.rcItem, palette.background);
            // Nested flat fills form a restrained inset shadow without blur surfaces or timers.
            painting::rounded(item.hDC, &item.rcItem, scale(9, dpi), palette.selected);
            let mut inset = item.rcItem;
            inset.left += scale(2, dpi);
            inset.top += scale(3, dpi);
            inset.right -= scale(2, dpi);
            inset.bottom -= scale(2, dpi);
            painting::rounded(item.hDC, &inset, scale(7, dpi), palette.background);
            return true;
        }
        let glyph = match item.CtlID as usize {
            SLEEP_ID => "\u{e708}",
            RESTART_ID => "\u{e72c}",
            SHUTDOWN_ID => "\u{e7e8}",
            _ => return false,
        };
        icon_button(item, glyph, dpi, fonts, palette);
        true
    }
}

pub fn icon_button(item: &DRAWITEMSTRUCT, glyph: &str, dpi: u32, fonts: Fonts, palette: Palette) {
    let color = if button_hover::is_hovered(item.hwndItem) {
        palette.accent
    } else {
        palette.text
    };
    tinted_icon_button(item, glyph, color, dpi, fonts, palette);
}

/// An icon button whose glyph has the colour its owner chose, for a button that shows a state.
pub fn tinted_icon_button(
    item: &DRAWITEMSTRUCT,
    glyph: &str,
    color: COLORREF,
    dpi: u32,
    fonts: Fonts,
    palette: Palette,
) {
    painting::fill(item.hDC, &item.rcItem, palette.background);
    let hovered = button_hover::is_hovered(item.hwndItem);
    if hovered || item.itemState.0 & (ODS_SELECTED.0 | ODS_FOCUS.0) != 0 {
        painting::rounded(item.hDC, &item.rcItem, scale(6, dpi), palette.selected);
    }
    let mut area = item.rcItem;
    area.left += ((area.right - area.left) - scale(20, dpi)) / 2;
    painting::text(item.hDC, glyph, area, fonts.icon, color);
}

impl Drop for PowerMenu {
    fn drop(&mut self) {
        for control in [
            self.panel,
            self.buttons[0].1,
            self.buttons[1].1,
            self.buttons[2].1,
        ] {
            unsafe {
                if IsWindow(Some(control)).as_bool() {
                    let _ = DestroyWindow(control);
                }
            }
        }
    }
}
