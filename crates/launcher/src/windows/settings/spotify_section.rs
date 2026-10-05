//! Spotify setup uses Core's existing settings controls, fonts and palette.
use super::{
    control_style::{self, Look},
    layout,
    page::SettingsAction,
    SpotifySettings,
};
use crate::windows::{
    button_hover, painting,
    theme::{scale, Fonts, Palette},
    view::{child, control_text},
    wide,
};
use std::cell::Cell;
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::Gdi::{InvalidateRect, HDC},
        UI::{
            Controls::{DRAWITEMSTRUCT, EM_SETLIMITTEXT, ODS_FOCUS, ODS_SELECTED},
            WindowsAndMessaging::*,
        },
    },
};

pub const BACK_ID: usize = 340;
const ENABLE_ID: usize = 341;
const CLIENT_LABEL_ID: usize = 342;
pub const CLIENT_ID: usize = 343;
const CONNECT_ID: usize = 344;
const DISCONNECT_ID: usize = 345;
const SETUP_ID: usize = 346;
const STATUS_ID: usize = 347;
const HELP_FIRST_ID: usize = 348;
const HELP_LINES: usize = 4;
/// Whether the now-playing bar's volume slider sets Spotify's own volume.
const VOLUME_ID: usize = HELP_FIRST_ID + HELP_LINES;
const FIELD_TOP: i32 = 188;
const FIELD_WIDTH: i32 = layout::CONTENT_RIGHT - layout::CONTENT_LEFT;
/// The two switches share the first row: song search, then the bar's volume beside it.
const SWITCH_TOP: i32 = 104;
const ENABLE_WIDTH: i32 = 380;
const SWITCH_GAP: i32 = 12;

pub struct SpotifySection {
    controls: Vec<(usize, HWND)>,
    enabled: Cell<bool>,
    volume: Cell<bool>,
}

impl SpotifySection {
    pub fn create(parent: HWND, instance: HINSTANCE) -> windows::core::Result<Self> {
        let mut section = Self {
            controls: Vec::new(),
            enabled: Cell::new(false),
            volume: Cell::new(false),
        };
        // Switch text carries the state for screen readers; only the name is drawn.
        section.add_button(parent, instance, ENABLE_ID, "Spotify song search: Off")?;
        section.add_button(
            parent,
            instance,
            VOLUME_ID,
            "Now playing bar's volume slider sets Spotify's volume: Off",
        )?;
        section.add(
            parent,
            instance,
            CLIENT_LABEL_ID,
            "Spotify app Client ID",
            w!("STATIC"),
            WINDOW_STYLE(0),
        )?;
        let input = section.add(
            parent,
            instance,
            CLIENT_ID,
            "",
            w!("EDIT"),
            WINDOW_STYLE(ES_AUTOHSCROLL as u32) | WS_TABSTOP,
        )?;
        unsafe {
            SendMessageW(input, EM_SETLIMITTEXT, Some(WPARAM(32)), None);
        }
        for (identifier, label) in [
            (CONNECT_ID, "Connect Spotify"),
            (DISCONNECT_ID, "Disconnect"),
            (SETUP_ID, "Create Spotify app"),
        ] {
            section.add_button(parent, instance, identifier, label)?;
        }
        section.add(
            parent,
            instance,
            STATUS_ID,
            "Connect once, then use @song followed by a song or artist.",
            w!("STATIC"),
            WINDOW_STYLE(0),
        )?;
        let redirect_help = format!(
            "Add this Redirect URI: {}",
            crate::windows::spotify::REDIRECT_URI
        );
        for (offset, label) in [
            "Spotify Premium is required. Search text is sent only for @song.",
            "Create a Web API app in Spotify's dashboard and copy its Client ID.",
            redirect_help.as_str(),
            "For other accounts, add them to your app's allowed users (up to 5).",
        ]
        .into_iter()
        .enumerate()
        {
            section.add(
                parent,
                instance,
                HELP_FIRST_ID + offset,
                label,
                w!("STATIC"),
                WINDOW_STYLE(0),
            )?;
        }
        section.add_button(parent, instance, BACK_ID, "Back to Music")?;
        Ok(section)
    }

    fn add_button(
        &mut self,
        parent: HWND,
        instance: HINSTANCE,
        identifier: usize,
        label: &str,
    ) -> windows::core::Result<()> {
        let button = self.add(
            parent,
            instance,
            identifier,
            label,
            w!("BUTTON"),
            WINDOW_STYLE(BS_OWNERDRAW as u32) | WS_TABSTOP,
        )?;
        button_hover::install(button)
    }

    fn add(
        &mut self,
        parent: HWND,
        instance: HINSTANCE,
        identifier: usize,
        label: &str,
        class: PCWSTR,
        style: WINDOW_STYLE,
    ) -> windows::core::Result<HWND> {
        let text = wide(label);
        let control = unsafe {
            child(
                parent,
                instance,
                class,
                PCWSTR(text.as_ptr()),
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
            .map(|(_, control)| *control)
            .unwrap_or_default()
    }

    pub fn reset(&self, settings: &SpotifySettings) {
        self.enabled.set(settings.enabled);
        self.volume.set(settings.volume);
        self.set_text(CLIENT_ID, &settings.client_id);
        self.update_toggles();
    }

    pub fn draft(&self) -> Result<SpotifySettings, String> {
        let settings = SpotifySettings {
            enabled: self.enabled.get(),
            client_id: control_text(self.control(CLIENT_ID)).trim().to_owned(),
            volume: self.volume.get(),
        };
        settings.validate()?;
        Ok(settings)
    }

    pub fn status(&self, text: &str) {
        self.set_text(STATUS_ID, text);
    }
    pub fn focus_target(&self) -> HWND {
        self.control(ENABLE_ID)
    }

    fn set_text(&self, identifier: usize, text: &str) {
        if let Err(error) =
            unsafe { SetWindowTextW(self.control(identifier), PCWSTR(wide(text).as_ptr())) }
        {
            eprintln!("Could not update Spotify settings: {error}");
        }
    }

    /// Switch text, which screen readers announce; the switches repaint to match.
    fn update_toggles(&self) {
        for (identifier, name, on) in [
            (ENABLE_ID, "Spotify song search", self.enabled.get()),
            (
                VOLUME_ID,
                "Now playing bar's volume slider sets Spotify's volume",
                self.volume.get(),
            ),
        ] {
            self.set_text(
                identifier,
                &format!("{name}: {}", if on { "On" } else { "Off" }),
            );
            unsafe {
                let _ = InvalidateRect(Some(self.control(identifier)), None, false);
            }
        }
    }

    pub fn command(&self, identifier: usize, notification: u32) -> Option<SettingsAction> {
        if !self
            .controls
            .iter()
            .any(|(candidate, _)| *candidate == identifier)
        {
            return None;
        }
        if identifier == CLIENT_ID && notification == EN_CHANGE {
            return Some(SettingsAction::Edit);
        }
        if notification != BN_CLICKED {
            return Some(SettingsAction::None);
        }
        Some(match identifier {
            ENABLE_ID => {
                self.enabled.set(!self.enabled.get());
                self.update_toggles();
                SettingsAction::Change
            }
            VOLUME_ID => {
                self.volume.set(!self.volume.get());
                self.update_toggles();
                SettingsAction::Change
            }
            CONNECT_ID => SettingsAction::ConnectSpotify,
            DISCONNECT_ID => SettingsAction::DisconnectSpotify,
            SETUP_ID => SettingsAction::SpotifySetup,
            _ => SettingsAction::None,
        })
    }

    pub fn show(&self, visible: bool) {
        for (_, control) in &self.controls {
            unsafe {
                let _ = ShowWindow(*control, if visible { SW_SHOWNA } else { SW_HIDE });
            }
        }
    }

    fn area(identifier: usize) -> (i32, i32, i32, i32) {
        let left = layout::CONTENT_LEFT;
        match identifier {
            ENABLE_ID => (left, SWITCH_TOP, ENABLE_WIDTH, layout::BUTTON_HEIGHT),
            VOLUME_ID => (
                left + ENABLE_WIDTH + SWITCH_GAP,
                SWITCH_TOP,
                FIELD_WIDTH - ENABLE_WIDTH - SWITCH_GAP,
                layout::BUTTON_HEIGHT,
            ),
            CLIENT_LABEL_ID => (left, 158, FIELD_WIDTH, 22),
            CLIENT_ID => (
                left + layout::INPUT_INSET,
                FIELD_TOP + 6,
                FIELD_WIDTH - 2 * layout::INPUT_INSET,
                layout::INPUT_HEIGHT,
            ),
            CONNECT_ID => (left, 242, 210, 40),
            DISCONNECT_ID => (left + 222, 242, 160, 40),
            SETUP_ID => (left + 394, 242, 210, 40),
            STATUS_ID => (left, 294, FIELD_WIDTH, 38),
            BACK_ID => (left, 422, 170, 40),
            identifier => (
                left,
                334 + (identifier - HELP_FIRST_ID) as i32 * 20,
                FIELD_WIDTH,
                20,
            ),
        }
    }

    pub fn layout(&self, dpi: u32, fonts: Fonts, new_fonts: bool) -> windows::core::Result<()> {
        for (identifier, control) in &self.controls {
            let (left, top, width, height) = Self::area(*identifier);
            unsafe {
                if new_fonts {
                    SendMessageW(
                        *control,
                        WM_SETFONT,
                        Some(WPARAM(fonts.detail.0 as usize)),
                        Some(LPARAM(0)),
                    );
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
            }
        }
        Ok(())
    }

    pub fn paint(&self, context: HDC, dpi: u32, palette: Palette) {
        painting::rounded(
            context,
            &layout::area(layout::CONTENT_LEFT, FIELD_TOP, FIELD_WIDTH, 40, dpi),
            scale(8, dpi),
            palette.selected,
        );
    }

    pub fn draw(&self, item: &DRAWITEMSTRUCT, look: Look) -> bool {
        let identifier = item.CtlID as usize;
        if !matches!(
            identifier,
            ENABLE_ID | VOLUME_ID | CONNECT_ID | DISCONNECT_ID | SETUP_ID | BACK_ID
        ) {
            return false;
        }
        let active = button_hover::is_hovered(item.hwndItem)
            || item.itemState.0 & (ODS_FOCUS.0 | ODS_SELECTED.0) != 0;
        let switch = match identifier {
            ENABLE_ID => Some(("Spotify song search", self.enabled.get())),
            VOLUME_ID => Some(("Spotify volume", self.volume.get())),
            _ => None,
        };
        match switch {
            Some((name, on)) => {
                control_style::draw_toggle(item.hDC, item.rcItem, name, on, active, look)
            }
            None => control_style::draw_action(
                item.hDC,
                item.rcItem,
                &control_text(item.hwndItem),
                None,
                active,
                look,
            ),
        }
        true
    }
}

impl Drop for SpotifySection {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spotify_controls_fit_the_existing_settings_page_without_covering_the_footer() {
        for identifier in BACK_ID..=VOLUME_ID {
            assert!(identifier > super::super::quicklink_table::SCROLL_ID);
            assert!(identifier > super::super::music_section::LAST_ID);
            let (left, top, width, height) = SpotifySection::area(identifier);
            assert!(left >= layout::CONTENT_LEFT && left + width <= layout::CONTENT_RIGHT);
            assert!(top >= layout::FIRST_LABEL_TOP && top + height < layout::FOOTER_TOP);
        }
        // The volume switch sits beside song search, as tall, without touching it.
        let (enable_left, enable_top, enable_width, enable_height) =
            SpotifySection::area(ENABLE_ID);
        let (volume_left, volume_top, volume_width, volume_height) =
            SpotifySection::area(VOLUME_ID);
        assert!(volume_left > enable_left + enable_width);
        assert_eq!((volume_top, volume_height), (enable_top, enable_height));
        assert_eq!(volume_left + volume_width, layout::CONTENT_RIGHT);
        let (_, status_top, _, status_height) = SpotifySection::area(STATUS_ID);
        let (_, help_top, _, _) = SpotifySection::area(HELP_FIRST_ID);
        assert!(status_top + status_height <= help_top);
    }
}
