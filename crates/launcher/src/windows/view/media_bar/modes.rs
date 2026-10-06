//! Shuffle and repeat on the now-playing bar. While the pointer is over the bar's right side,
//! where the timecode and the buttons are, two more buttons take the timecode's place, left of
//! previous. Each shows how the player's setting stands, and a press changes it. A setting the
//! player does not let Windows change has no button.
use super::*;
use crate::windows::power_menu::tinted_icon_button;
use core_engine::media::{MediaCommand, RepeatMode};
use level_slider::{contains, pointer};
use windows::Win32::UI::{
    Controls::WM_MOUSELEAVE,
    Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT},
    Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
};

const REVEAL_SUBCLASS: usize = 7;
pub const MEDIA_SHUFFLE_ID: usize = 124;
pub const MEDIA_REPEAT_ID: usize = 125;
/// Left to right, with the command each sends.
const BUTTONS: [(usize, MediaCommand); 2] = [
    (MEDIA_SHUFFLE_ID, MediaCommand::Shuffle),
    (MEDIA_REPEAT_ID, MediaCommand::Repeat),
];

pub(in crate::windows::view) const SHUFFLE_GLYPH: &str = "\u{e8b1}";
pub(in crate::windows::view) const REPEAT_GLYPH: &str = "\u{e8ee}";
const REPEAT_ONE_GLYPH: &str = "\u{e8ed}";

// Geometry in 96-DPI pixels. A setting that is on has a dot under its glyph.
const DOT_SIZE: i32 = 4;
const DOT_BOTTOM: i32 = 3;

/// The command a bar button sends; None for a control that is not one of these two.
pub fn mode_command(identifier: usize) -> Option<MediaCommand> {
    BUTTONS
        .iter()
        .find(|(button, _)| *button == identifier)
        .map(|(_, command)| *command)
}

/// The bar's right side in the info control's client pixels: from where the timecode starts to
/// the bar's end, down to where clicks start to seek. The buttons are inside it, so showing
/// them never moves the pointer out of it.
fn zone(client: RECT, dpi: u32) -> RECT {
    let previous = button_areas(client.right - client.left, 0, dpi)[0];
    painting::rectangle(
        client.left + previous.left - scale(TEXT_GAP + TIME_WIDTH, dpi),
        client.top,
        client.right,
        progress_track(client, dpi).top - scale(SEEK_REACH, dpi),
    )
}

/// What the bar's controls and its painting share. The bar owns it in a box, whose address the
/// controls' window procedures keep, and destroys those controls before the box.
pub(super) struct ModeButtons {
    /// Shuffle, then repeat.
    buttons: [HWND; 2],
    /// The bar's info control: the pointer's place is measured in it, and its text makes room.
    info: Cell<HWND>,
    /// What the shown player reports; a setting it does not offer has no button.
    controls: Cell<MediaControls>,
    /// The pointer is over the bar's right side.
    hovered: Cell<bool>,
    bar_visible: Cell<bool>,
    /// Which buttons show now. Both to begin with, as every control is created showing.
    shown: Cell<[bool; 2]>,
    dpi: Cell<u32>,
}

impl ModeButtons {
    pub fn create(parent: HWND, instance: HINSTANCE) -> windows::core::Result<Box<Self>> {
        let mut modes = Box::new(Self {
            buttons: [HWND::default(); 2],
            info: Cell::new(HWND::default()),
            controls: Cell::new(MediaControls::default()),
            hovered: Cell::new(false),
            bar_visible: Cell::new(false),
            shown: Cell::new([true; 2]),
            dpi: Cell::new(96),
        });
        for (button, (identifier, command)) in modes.buttons.iter_mut().zip(BUTTONS) {
            let label = wide(command.label());
            *button = unsafe {
                child(
                    parent,
                    instance,
                    w!("BUTTON"),
                    PCWSTR(label.as_ptr()),
                    WINDOW_STYLE(BS_OWNERDRAW as u32) | WS_TABSTOP,
                    identifier,
                )?
            };
            button_hover::install(*button)?;
        }
        modes.refresh();
        Ok(modes)
    }

    /// Follows the pointer over `info`, the bar's info control, over `others`, the buttons that
    /// are always there, and over these two.
    pub fn watch(&self, info: HWND, others: &[HWND]) -> windows::core::Result<()> {
        self.info.set(info);
        for control in others.iter().chain(&self.buttons).chain([&info]) {
            unsafe {
                SetWindowSubclass(
                    *control,
                    Some(reveal_proc),
                    REVEAL_SUBCLASS,
                    self as *const Self as usize,
                )
                .ok()?;
            }
        }
        Ok(())
    }

    pub fn set_dpi(&self, dpi: u32) {
        self.dpi.set(dpi);
    }

    pub fn buttons(&self) -> [HWND; 2] {
        self.buttons
    }

    /// The player's settings as they stand now; the default for no player, which offers none.
    pub fn set_controls(&self, controls: MediaControls) {
        if self.controls.replace(controls) == controls {
            return;
        }
        for (button, (_, command)) in self.buttons.iter().zip(BUTTONS) {
            let label = controls
                .mode_label(command)
                .unwrap_or_else(|| command.label());
            if control_text(*button) != label {
                let label = wide(label);
                unsafe {
                    let _ = SetWindowTextW(*button, PCWSTR(label.as_ptr()));
                }
            }
            unsafe {
                let _ = InvalidateRect(Some(*button), None, false);
            }
        }
        self.refresh();
    }

    /// The bar shows or hides. A hidden bar has no pointer over it.
    pub fn show(&self, visible: bool) {
        self.bar_visible.set(visible);
        if !visible {
            self.hovered.set(false);
        }
        self.refresh();
    }

    /// Core is hiding: the pointer no longer holds the buttons.
    pub fn hide(&self) {
        self.hovered.set(false);
        self.refresh();
    }

    /// Where the bar's text must end while buttons show: the left edge of the first of them,
    /// in a bar `width` pixels wide. None while neither shows.
    pub fn left_edge(&self, width: i32) -> Option<i32> {
        let areas = mode_button_areas(width, 0, self.dpi.get());
        let first = self.shown.get().iter().position(|shown| *shown)?;
        Some(areas[first].left)
    }

    pub fn draw(&self, item: &DRAWITEMSTRUCT, dpi: u32, fonts: Fonts, palette: Palette) -> bool {
        let controls = self.controls.get();
        let (glyph, on) = match (mode_command(item.CtlID as usize), controls.repeat) {
            (Some(MediaCommand::Shuffle), _) => (SHUFFLE_GLYPH, controls.shuffle == Some(true)),
            (Some(MediaCommand::Repeat), Some(RepeatMode::One)) => (REPEAT_ONE_GLYPH, true),
            (Some(MediaCommand::Repeat), mode) => (REPEAT_GLYPH, mode == Some(RepeatMode::All)),
            _ => return false,
        };
        let color = if on {
            palette.accent
        } else if button_hover::is_hovered(item.hwndItem) {
            palette.text
        } else {
            palette.secondary
        };
        tinted_icon_button(item, glyph, color, dpi, fonts, palette);
        if on {
            let size = scale(DOT_SIZE, dpi);
            let left = (item.rcItem.left + item.rcItem.right - size) / 2;
            let top = item.rcItem.bottom - scale(DOT_BOTTOM, dpi) - size;
            // One more each way: a rounded shape stops a pixel short of its right and bottom.
            let dot = painting::rectangle(left, top, left + size + 1, top + size + 1);
            painting::rounded(item.hDC, &dot, size / 2, palette.accent);
        }
        true
    }

    fn offered(&self) -> [bool; 2] {
        let controls = self.controls.get();
        [controls.shuffle.is_some(), controls.repeat.is_some()]
    }

    /// Shows the buttons the pointer and the player call for. The bar's text makes room for
    /// them, or takes it back.
    fn refresh(&self) {
        let wanted = self.bar_visible.get() && self.hovered.get();
        let shown = self.offered().map(|offered| wanted && offered);
        if self.shown.replace(shown) == shown {
            return;
        }
        for (button, shown) in self.buttons.iter().zip(shown) {
            unsafe {
                let _ = ShowWindow(*button, if shown { SW_SHOWNA } else { SW_HIDE });
            }
        }
        // Without the info control nothing is invalidated: a null window would mean all.
        let info = self.info.get();
        if !info.0.is_null() {
            unsafe {
                let _ = InvalidateRect(Some(info), None, false);
            }
        }
    }

    /// The pointer moved over `control`, to `point` in that control's own client pixels.
    fn pointer_moved(&self, control: HWND, point: POINT) {
        let info = self.info.get();
        let mut points = [point];
        let mut client = RECT::default();
        unsafe {
            // Zero both when the controls are the same window, and when the point did not move.
            MapWindowPoints(Some(control), Some(info), &mut points);
            if GetClientRect(info, &mut client).is_err() {
                return;
            }
        }
        let inside = contains(&zone(client, self.dpi.get()), points[0]);
        // The buttons follow the pointer themselves; the info control is asked to say when it
        // leaves. Without that notice the buttons would stay after the pointer left Core.
        let hovered = inside && (control != info || watch_leave(info));
        if self.hovered.replace(hovered) != hovered {
            self.refresh();
        }
    }

    /// The pointer left one of the bar's controls: for another of them, or for good.
    fn pointer_left(&self) {
        let hovered = self.pointer_over_zone();
        if self.hovered.replace(hovered) != hovered {
            self.refresh();
        }
    }

    /// Whether the pointer is over the bar's right side now, on whichever control is there.
    fn pointer_over_zone(&self) -> bool {
        let info = self.info.get();
        let mut point = POINT::default();
        let mut client = RECT::default();
        unsafe {
            if GetCursorPos(&mut point).is_err() {
                return false;
            }
            // Another window over the bar has the pointer, not the bar.
            let under = WindowFromPoint(point);
            if under != info && GetParent(under).ok() != GetParent(info).ok() {
                return false;
            }
            if !ScreenToClient(info, &mut point).as_bool()
                || GetClientRect(info, &mut client).is_err()
            {
                return false;
            }
        }
        contains(&zone(client, self.dpi.get()), point)
    }
}

impl Drop for ModeButtons {
    fn drop(&mut self) {
        for button in self.buttons {
            unsafe {
                if IsWindow(Some(button)).as_bool() {
                    let _ = DestroyWindow(button);
                }
            }
        }
    }
}

fn watch_leave(control: HWND) -> bool {
    let mut tracking = TRACKMOUSEEVENT {
        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: control,
        ..Default::default()
    };
    match unsafe { TrackMouseEvent(&mut tracking) } {
        Ok(()) => true,
        Err(error) => {
            eprintln!("Could not follow the pointer over the now-playing bar: {error}");
            false
        }
    }
}

unsafe extern "system" fn reveal_proc(
    control: HWND,
    message: u32,
    word: WPARAM,
    data: LPARAM,
    _subclass: usize,
    reference: usize,
) -> LRESULT {
    // The bar that created these controls boxes the buttons' state and destroys the controls
    // first.
    let modes = &*(reference as *const ModeButtons);
    match message {
        WM_MOUSEMOVE => modes.pointer_moved(control, pointer(data)),
        WM_MOUSELEAVE => modes.pointer_left(),
        WM_NCDESTROY
            if !RemoveWindowSubclass(control, Some(reveal_proc), REVEAL_SUBCLASS).as_bool() =>
        {
            eprintln!("Could not remove the shuffle and repeat buttons' subclass");
        }
        _ => {}
    }
    DefSubclassProc(control, message, word, data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::{
        LibraryLoader::GetModuleHandleW,
        SystemServices::{SS_NOTIFY, SS_OWNERDRAW},
    };

    const WIDTH: i32 = theme::WIDTH;

    fn client() -> RECT {
        painting::rectangle(0, 0, WIDTH, theme::MEDIA_BAR_HEIGHT)
    }

    fn offering(shuffle: Option<bool>, repeat: Option<RepeatMode>) -> MediaControls {
        MediaControls {
            shuffle,
            repeat,
            ..MediaControls::default()
        }
    }

    #[test]
    fn the_zone_holds_the_timecode_and_every_button_and_stops_above_the_progress_line() {
        for dpi in [96, 144, 192] {
            let width = scale(WIDTH, dpi);
            let client = painting::rectangle(0, 0, width, scale(theme::MEDIA_BAR_HEIGHT, dpi));
            let zone = zone(client, dpi);
            let transport = button_areas(width, 0, dpi);
            let modes = mode_button_areas(width, 0, dpi);
            for button in transport.iter().chain(&modes) {
                assert!(button.left >= zone.left && button.right <= zone.right);
                assert!(button.top >= zone.top && button.bottom <= zone.bottom);
            }
            // Shuffle, repeat, previous: one row, evenly spaced, none over another.
            assert!(modes[0].right <= modes[1].left && modes[1].right <= transport[0].left);
            assert_eq!(
                modes[1].left - modes[0].left,
                transport[1].left - transport[0].left
            );
            assert_eq!(modes[0].top, transport[0].top);
            // The timecode ends where the title's text does, and the zone starts with it.
            assert_eq!(
                zone.left,
                transport[0].left - scale(TEXT_GAP + TIME_WIDTH, dpi)
            );
            assert!(zone.bottom < progress_track(client, dpi).top);
        }
    }

    #[test]
    fn bar_buttons_map_to_their_commands() {
        assert_eq!(mode_command(MEDIA_SHUFFLE_ID), Some(MediaCommand::Shuffle));
        assert_eq!(mode_command(MEDIA_REPEAT_ID), Some(MediaCommand::Repeat));
        assert_eq!(mode_command(MEDIA_PLAY_ID), None);
        assert!(is_media_button(MEDIA_SHUFFLE_ID) && is_media_button(MEDIA_NEXT_ID));
        assert!(!is_media_button(MEDIA_INFO_ID));
    }

    /// The bar's own controls, driven by the messages a pointer would cause. The window is
    /// never shown and no real pointer is moved.
    #[test]
    fn the_pointer_over_the_bars_right_side_shows_what_the_player_offers_and_leaving_hides_it() {
        let _serial = crate::windows::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }.unwrap().into();
        let parent = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!(""),
                WS_POPUP,
                0,
                0,
                WIDTH,
                theme::MEDIA_BAR_HEIGHT,
                None,
                None,
                Some(instance),
                None,
            )
        }
        .unwrap();
        let modes = ModeButtons::create(parent, instance).unwrap();
        let info = unsafe {
            child(
                parent,
                instance,
                w!("STATIC"),
                w!(""),
                WINDOW_STYLE(SS_OWNERDRAW.0 | SS_NOTIFY.0),
                MEDIA_INFO_ID,
            )
        }
        .unwrap();
        let place = |control: HWND, area: RECT| {
            unsafe {
                SetWindowPos(
                    control,
                    None,
                    area.left,
                    area.top,
                    area.right - area.left,
                    area.bottom - area.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
            }
            .unwrap();
        };
        place(info, client());
        let areas = mode_button_areas(WIDTH, 0, 96);
        let [shuffle, repeat] = modes.buttons();
        place(shuffle, areas[0]);
        place(repeat, areas[1]);
        modes.watch(info, &[]).unwrap();
        // The parent is hidden, so what counts is each button's own visible style.
        let showing = |button: HWND| {
            let style = unsafe { GetWindowLongW(button, GWL_STYLE) } as u32;
            style & WS_VISIBLE.0 != 0
        };
        let send = |control: HWND, message: u32, (x, y): (i32, i32)| {
            let point = ((y as i16 as u16 as isize) << 16) | (x as i16 as u16 as isize);
            unsafe { SendMessageW(control, message, None, Some(LPARAM(point))) };
        };
        let zone = zone(client(), 96);
        let over_timecode = (zone.left + 4, (zone.top + zone.bottom) / 2);
        let over_title = (zone.left - 40, over_timecode.1);

        // Created showing, as every control is, and hidden before the bar first appears.
        assert!(!showing(shuffle) && !showing(repeat));
        modes.show(true);
        // A player that offers neither: the pointer changes nothing, and the timecode stays.
        send(info, WM_MOUSEMOVE, over_timecode);
        assert!(!showing(shuffle) && !showing(repeat));
        assert_eq!(modes.left_edge(WIDTH), None);

        // Only shuffle offered: only its button, and the text ends before it.
        modes.set_controls(offering(Some(false), None));
        assert!(showing(shuffle) && !showing(repeat));
        assert_eq!(modes.left_edge(WIDTH), Some(areas[0].left));
        assert_eq!(control_text(shuffle), "Shuffle off");
        assert_eq!(control_text(repeat), "Repeat");
        // Only repeat: the text may reach as far as that button.
        modes.set_controls(offering(None, Some(RepeatMode::One)));
        assert!(!showing(shuffle) && showing(repeat));
        assert_eq!(modes.left_edge(WIDTH), Some(areas[1].left));
        assert_eq!(control_text(repeat), "Repeat one");

        modes.set_controls(offering(Some(true), Some(RepeatMode::All)));
        assert!(showing(shuffle) && showing(repeat));
        assert_eq!(control_text(shuffle), "Shuffle on");
        // On to a button, which is another control: both stay.
        send(shuffle, WM_MOUSEMOVE, (3, 3));
        assert!(showing(shuffle) && showing(repeat));
        // Back over the title, they go, and the timecode returns.
        send(info, WM_MOUSEMOVE, over_title);
        assert!(!showing(shuffle) && !showing(repeat));
        assert_eq!(modes.left_edge(WIDTH), None);
        // The progress line keeps its own pointer too.
        send(info, WM_MOUSEMOVE, (over_timecode.0, zone.bottom + 1));
        assert!(!showing(shuffle));

        // The pointer leaves the control for somewhere that is not the bar: this window is
        // hidden, so wherever the real pointer is, it is not over it.
        send(info, WM_MOUSEMOVE, over_timecode);
        assert!(showing(shuffle) && showing(repeat));
        send(info, WM_MOUSELEAVE, (0, 0));
        assert!(!showing(shuffle) && !showing(repeat));

        // A hidden bar shows nothing, and does not remember the pointer when it returns.
        send(info, WM_MOUSEMOVE, over_timecode);
        modes.show(false);
        assert!(!showing(shuffle) && !showing(repeat));
        modes.show(true);
        assert!(!showing(shuffle) && !showing(repeat));
        // So does Core hiding.
        send(info, WM_MOUSEMOVE, over_timecode);
        modes.hide();
        assert!(!showing(shuffle) && !showing(repeat));

        unsafe {
            let _ = DestroyWindow(info);
        }
        drop(modes);
        assert!(!unsafe { IsWindow(Some(shuffle)) }.as_bool());
        unsafe {
            let _ = DestroyWindow(parent);
        }
    }
}
