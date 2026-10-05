//! The volume slider the now-playing bar reveals while the pointer is on the album art: the art
//! dims under a volume glyph and a slider takes the title's place, until the pointer leaves
//! both. The bar's info control follows the pointer and the drag here; Core's state decides
//! whose volume the slider moves, and reads it.
use super::*;
use core_engine::media::VolumeLevel;
use windows::Win32::UI::{
    Controls::WM_MOUSELEAVE,
    Input::KeyboardAndMouse::{
        ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
    },
    Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
};

const SLIDER_SUBCLASS: usize = 4;
/// Sent to Core's window as a WM_COMMAND notification from the bar's info control: the
/// pointer reached the album art, so the slider shows and needs the player's volume.
pub const VOLUME_WANTED: u32 = 0x0B10;
/// Sent the same way when the person moves the slider; `View::media_volume` is their choice.
pub const VOLUME_CHANGED: u32 = 0x0B11;

// Geometry in 96-DPI pixels.
const TRACK_WIDTH: i32 = 180;
const TRACK_THICKNESS: i32 = 2;
const FILLED_THICKNESS: i32 = 4;
const THUMB_DIAMETER: i32 = 12;
/// The ring around the thumb while it is dragged.
const DRAG_RING: i32 = 2;
const LABEL_GAP: i32 = 14;
const LABEL_WIDTH: i32 = 40;
/// The slider answers presses this far beyond either end of its track.
const TRACK_REACH: i32 = 8;
/// How far the art fades into the background under the glyph, of 255.
const ART_VEIL: u8 = 178;

const VOLUME_GLYPH: &str = "\u{e767}";
const MUTED_GLYPH: &str = "\u{e74f}";
/// Quiet, medium and loud.
const LEVEL_GLYPHS: [&str; 3] = ["\u{e993}", "\u{e994}", "\u{e995}"];

/// What the slider shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarVolume {
    /// Asked for: the track shows without a thumb until the level arrives.
    Reading,
    Level(VolumeLevel),
    /// The player has no volume Core can reach right now.
    Unavailable,
}

/// Where the slider's parts are, in the info control's client pixels.
pub(super) struct SliderLayout {
    art: RECT,
    /// The title's row, which the slider takes.
    row: RECT,
    /// The line itself: its left end is silence and its right end full volume.
    track: RECT,
    label: RECT,
    /// A press here moves the thumb.
    grip: RECT,
    /// The slider and its label; a press that misses the thumb must not open the player.
    slider: RECT,
    /// Art, slider and the gap between them: the pointer crosses it without the slider going.
    zone: RECT,
}

impl SliderLayout {
    pub fn new(client: RECT, dpi: u32) -> Self {
        let art = art_area(client, dpi);
        let row = title_row(client, dpi);
        let label_width = scale(LABEL_GAP + LABEL_WIDTH, dpi);
        // The thumb at silence stays inside the text column.
        let left = row.left + scale(THUMB_DIAMETER, dpi) / 2;
        let right = (left + scale(TRACK_WIDTH, dpi))
            .min(row.right - label_width)
            .max(left + 1);
        let middle = (row.top + row.bottom) / 2;
        let half = (scale(TRACK_THICKNESS, dpi) / 2).max(1);
        let reach = scale(TRACK_REACH, dpi);
        let label = painting::rectangle(
            right + scale(LABEL_GAP, dpi),
            row.top,
            right + label_width,
            row.bottom,
        );
        Self {
            art,
            row,
            track: painting::rectangle(left, middle - half, right, middle + half),
            label,
            grip: painting::rectangle(left - reach, row.top, right + reach, row.bottom),
            slider: painting::rectangle(left - reach, row.top, label.right, row.bottom),
            // Down to where clicks start to seek, so the progress line keeps its own clicks.
            zone: painting::rectangle(
                art.left,
                client.top,
                label.right,
                progress_track(client, dpi).top - scale(SEEK_REACH, dpi),
            ),
        }
    }

    fn span(&self) -> i32 {
        (self.track.right - self.track.left).max(1)
    }

    /// The level under a pointer at `x`; beyond either end it is silence or full volume.
    fn percent_at(&self, x: i32) -> u8 {
        let span = self.span();
        let offset = (x - self.track.left).clamp(0, span);
        ((offset * i32::from(VolumeLevel::MAX_PERCENT) + span / 2) / span) as u8
    }

    fn thumb_center(&self, percent: u8) -> i32 {
        let full = i32::from(VolumeLevel::MAX_PERCENT);
        self.track.left + (self.span() * i32::from(percent).min(full) + full / 2) / full
    }

    /// Everything that looks different while the slider shows, or as it moves.
    fn changed(&self) -> RECT {
        painting::rectangle(
            self.art.left,
            self.art.top.min(self.row.top),
            self.row.right,
            self.art.bottom.max(self.row.bottom),
        )
    }
}

/// What painting needs while the slider shows.
#[derive(Clone, Copy)]
pub(super) struct SliderView {
    pub volume: BarVolume,
    pub dragging: bool,
}

/// What the info control's window procedure and the bar's painting share. The bar owns it in a
/// box, whose address the procedure keeps, and destroys the control before the box.
pub(super) struct VolumeSlider {
    /// The pointer is on the art, or on the slider it revealed.
    shown: Cell<bool>,
    dragging: Cell<bool>,
    /// The person moved the slider since it appeared, so a level read earlier no longer moves it.
    adjusted: Cell<bool>,
    volume: Cell<BarVolume>,
    /// The bar shows a player; "Nothing playing" has no volume.
    available: Cell<bool>,
    dpi: Cell<u32>,
}

impl VolumeSlider {
    pub fn new() -> Box<Self> {
        Box::new(Self {
            shown: Cell::new(false),
            dragging: Cell::new(false),
            adjusted: Cell::new(false),
            volume: Cell::new(BarVolume::Reading),
            available: Cell::new(false),
            dpi: Cell::new(96),
        })
    }

    /// Lets `control`, the bar's info control, follow the pointer for this slider.
    pub fn install(&self, control: HWND) -> windows::core::Result<()> {
        unsafe {
            SetWindowSubclass(
                control,
                Some(slider_proc),
                SLIDER_SUBCLASS,
                self as *const Self as usize,
            )
            .ok()
        }
    }

    pub fn set_dpi(&self, dpi: u32) {
        self.dpi.set(dpi);
    }

    /// None while the slider is hidden.
    pub fn view(&self) -> Option<SliderView> {
        self.shown.get().then(|| SliderView {
            volume: self.volume.get(),
            dragging: self.dragging.get(),
        })
    }

    /// The level the person chose, or the one last read.
    pub fn level(&self) -> Option<u8> {
        match self.volume.get() {
            BarVolume::Level(level) => Some(level.percent),
            BarVolume::Reading | BarVolume::Unavailable => None,
        }
    }

    /// The bar shows another player, or none: the volume shown was not this one's.
    pub fn reset(&self, control: HWND, available: bool) {
        self.hide(control);
        self.available.set(available);
        self.volume.set(BarVolume::Reading);
    }

    /// A reading arrived. A volume that cannot be reached also ends a drag, which would only
    /// keep asking for it.
    pub fn set_volume(&self, control: HWND, volume: BarVolume) {
        if !self.accept(volume) {
            return;
        }
        if volume == BarVolume::Unavailable {
            self.end_drag();
        }
        self.invalidate(control);
    }

    /// Whether a reading changes what the slider shows. While the person moves the slider, or
    /// has moved it since it appeared, a level read earlier would only pull it back; that the
    /// volume cannot be reached is always shown.
    fn accept(&self, volume: BarVolume) -> bool {
        let chosen = self.dragging.get() || self.adjusted.get();
        if (chosen && matches!(volume, BarVolume::Level(_))) || self.volume.get() == volume {
            return false;
        }
        self.volume.set(volume);
        true
    }

    /// Hides the slider and gives the title its row back; also when the bar itself hides.
    pub fn hide(&self, control: HWND) {
        if !self.shown.replace(false) {
            return;
        }
        self.end_drag();
        self.invalidate(control);
    }

    fn end_drag(&self) {
        if self.dragging.replace(false) {
            // The info control holds the pointer for the drag; Windows tells it so again.
            if let Err(error) = unsafe { ReleaseCapture() } {
                eprintln!("Could not release the pointer after a volume drag: {error}");
            }
        }
    }

    /// Asks Windows to say when the pointer leaves the control. Without that notice the slider
    /// would stay after the pointer left Core's window, so it only shows once this worked.
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

    fn reveal(&self, control: HWND) {
        if !Self::watch_leave(control) {
            return;
        }
        self.shown.set(true);
        self.adjusted.set(false);
        // A volume that could not be reached is asked for again.
        if self.volume.get() == BarVolume::Unavailable {
            self.volume.set(BarVolume::Reading);
        }
        self.invalidate(control);
        notify(control, VOLUME_WANTED);
    }

    fn pointer_moved(&self, control: HWND, point: POINT) {
        let Some(layout) = self.layout(control) else {
            return;
        };
        if self.dragging.get() {
            self.choose(control, layout.percent_at(point.x));
            return;
        }
        // The art reveals the slider; once shown, the way from the art to it keeps it.
        let inside = if self.shown.get() {
            contains(&layout.zone, point)
        } else {
            contains(&layout.art, point)
        };
        match (inside && self.available.get(), self.shown.get()) {
            (true, false) => self.reveal(control),
            (false, true) => self.hide(control),
            _ => {}
        }
    }

    /// True when the press was the slider's: the control's own click, which opens the player,
    /// must not follow it.
    fn pressed(&self, control: HWND, point: POINT) -> bool {
        let Some(layout) = self.layout(control).filter(|_| self.shown.get()) else {
            return false;
        };
        if !contains(&layout.slider, point) {
            return false;
        }
        let known = matches!(self.volume.get(), BarVolume::Level(_));
        if known && contains(&layout.grip, point) {
            unsafe {
                SetCapture(control);
            }
            self.dragging.set(true);
            self.choose(control, layout.percent_at(point.x));
            self.invalidate(control);
        }
        true
    }

    /// True when the release ended a drag.
    fn released(&self, control: HWND, point: POINT) -> bool {
        if !self.dragging.get() {
            return false;
        }
        let layout = self.layout(control);
        if let Some(layout) = &layout {
            self.choose(control, layout.percent_at(point.x));
        }
        self.end_drag();
        // A drag may end anywhere: away from the slider, it goes. Over it, the leave notice
        // is asked for again, since one that arrived during the drag was not acted on.
        let stays = layout.is_some_and(|layout| contains(&layout.zone, point));
        if !stays || !Self::watch_leave(control) {
            self.hide(control);
        }
        self.invalidate(control);
        true
    }

    /// The control no longer holds the pointer. When a drag ends itself it is no longer
    /// dragging by now; otherwise another window took the pointer, wherever it is, and the
    /// slider goes with the drag.
    fn capture_lost(&self, control: HWND) {
        if self.dragging.replace(false) {
            self.hide(control);
        }
    }

    fn choose(&self, control: HWND, percent: u8) {
        let chosen = BarVolume::Level(VolumeLevel::new(percent, false));
        if self.volume.replace(chosen) == chosen {
            return;
        }
        self.adjusted.set(true);
        self.invalidate(control);
        notify(control, VOLUME_CHANGED);
    }

    fn layout(&self, control: HWND) -> Option<SliderLayout> {
        let mut client = RECT::default();
        unsafe { GetClientRect(control, &mut client) }.ok()?;
        Some(SliderLayout::new(client, self.dpi.get()))
    }

    fn invalidate(&self, control: HWND) {
        // Without the control's own area nothing is invalidated: a null window would mean all.
        if let Some(layout) = self.layout(control) {
            unsafe {
                let _ = InvalidateRect(Some(control), Some(&layout.changed()), false);
            }
        }
    }
}

fn contains(area: &RECT, point: POINT) -> bool {
    unsafe { PtInRect(area, point) }.as_bool()
}

/// The pointer in a mouse message. Signed: a dragging pointer moves left of and above the
/// control.
fn pointer(data: LPARAM) -> POINT {
    POINT {
        x: i32::from(data.0 as u16 as i16),
        y: i32::from((data.0 >> 16) as u16 as i16),
    }
}

/// Tells Core's window, as the control's own click does.
fn notify(control: HWND, notification: u32) {
    let Ok(parent) = (unsafe { GetParent(control) }) else {
        return;
    };
    unsafe {
        SendMessageW(
            parent,
            WM_COMMAND,
            Some(WPARAM(((notification as usize) << 16) | MEDIA_INFO_ID)),
            Some(LPARAM(control.0 as isize)),
        );
    }
}

unsafe extern "system" fn slider_proc(
    control: HWND,
    message: u32,
    word: WPARAM,
    data: LPARAM,
    _subclass: usize,
    reference: usize,
) -> LRESULT {
    // The bar that created this control boxes the slider and destroys the control first.
    let slider = &*(reference as *const VolumeSlider);
    let handled = match message {
        WM_MOUSEMOVE => {
            slider.pointer_moved(control, pointer(data));
            false
        }
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => slider.pressed(control, pointer(data)),
        WM_LBUTTONUP => slider.released(control, pointer(data)),
        // A drag holds the pointer wherever it goes; it leaves when the button is released.
        WM_MOUSELEAVE if !slider.dragging.get() => {
            slider.hide(control);
            false
        }
        WM_CANCELMODE => {
            slider.hide(control);
            false
        }
        WM_CAPTURECHANGED => {
            slider.capture_lost(control);
            false
        }
        WM_NCDESTROY => {
            if !RemoveWindowSubclass(control, Some(slider_proc), SLIDER_SUBCLASS).as_bool() {
                eprintln!("Could not remove the volume slider's subclass");
            }
            false
        }
        _ => false,
    };
    if handled {
        LRESULT(0)
    } else {
        DefSubclassProc(control, message, word, data)
    }
}

/// Fades the art that is already drawn into the background and puts the volume glyph over it.
/// A veil of the background's own colour leaves the art's rounded corners as they were.
pub(super) fn draw_art_overlay(
    context: HDC,
    art: RECT,
    art_drawn: bool,
    volume: BarVolume,
    dpi: u32,
    fonts: Fonts,
    palette: Palette,
) {
    if art_drawn {
        painting::veil(context, &art, palette.background, ART_VEIL);
    }
    art_glyph(context, art, glyph(volume), dpi, fonts.icon, palette.text);
}

fn glyph(volume: BarVolume) -> &'static str {
    let BarVolume::Level(level) = volume else {
        return VOLUME_GLYPH;
    };
    if level.is_silent() {
        return MUTED_GLYPH;
    }
    let step = usize::from(VolumeLevel::MAX_PERCENT).div_ceil(LEVEL_GLYPHS.len());
    LEVEL_GLYPHS[(usize::from(level.percent - 1) / step).min(LEVEL_GLYPHS.len() - 1)]
}

/// The slider in the title's row, drawn like the sliders in Settings: a thin track, filled up
/// to a round thumb, and the level beside it.
pub(super) fn draw_slider(
    context: HDC,
    layout: &SliderLayout,
    view: SliderView,
    dpi: u32,
    fonts: Fonts,
    palette: Palette,
) {
    let level = match view.volume {
        BarVolume::Unavailable => {
            painting::text(
                context,
                "Volume unavailable",
                layout.row,
                fonts.detail,
                palette.secondary,
            );
            return;
        }
        BarVolume::Reading => None,
        BarVolume::Level(level) => Some(level),
    };
    let track = layout.track;
    painting::rounded(
        context,
        &track,
        (track.bottom - track.top) / 2,
        palette.secondary,
    );
    let Some(level) = level else {
        return;
    };
    let center = layout.thumb_center(level.percent);
    let middle = (track.top + track.bottom) / 2;
    let filled = (scale(FILLED_THICKNESS, dpi) / 2).max(1);
    if center > track.left {
        painting::rounded(
            context,
            &painting::rectangle(track.left, middle - filled, center, middle + filled),
            filled,
            palette.text,
        );
    }
    let circle = |diameter: i32| {
        let half = diameter / 2;
        painting::rectangle(
            center - half,
            middle - half,
            center - half + diameter,
            middle - half + diameter,
        )
    };
    let diameter = scale(THUMB_DIAMETER, dpi);
    if view.dragging {
        let ring = diameter + scale(DRAG_RING, dpi) * 2;
        painting::rounded(context, &circle(ring), ring / 2, palette.accent);
    }
    painting::rounded(context, &circle(diameter), diameter / 2, palette.text);
    painting::text(
        context,
        &format!("{}%", level.percent),
        layout.label,
        fonts.detail,
        palette.secondary,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(dpi: u32) -> RECT {
        painting::rectangle(
            0,
            0,
            scale(theme::WIDTH, dpi),
            scale(theme::MEDIA_BAR_HEIGHT, dpi),
        )
    }

    fn level(percent: u8) -> BarVolume {
        BarVolume::Level(VolumeLevel::new(percent, false))
    }

    #[test]
    fn the_slider_takes_the_titles_row_between_the_art_and_the_buttons() {
        for dpi in [96, 120, 144, 192] {
            let client = client(dpi);
            let layout = SliderLayout::new(client, dpi);
            let buttons = button_areas(client.right, 0, dpi);
            assert_eq!(layout.row, title_row(client, dpi), "{dpi}");
            assert!(layout.art.right < layout.grip.left, "{dpi}");
            assert!(layout.track.right > layout.track.left, "{dpi}");
            assert!(layout.track.right < layout.label.left, "{dpi}");
            assert!(layout.label.right <= layout.row.right, "{dpi}");
            assert!(layout.label.right < buttons[0].left, "{dpi}");
            // A press on the level beside the track is the slider's, but moves nothing.
            assert!(layout.slider.right > layout.grip.right, "{dpi}");
            let radius = scale(THUMB_DIAMETER, dpi) / 2;
            for percent in [0, VolumeLevel::MAX_PERCENT] {
                let center = layout.thumb_center(percent);
                assert!(center - radius >= layout.row.left, "{dpi}");
                assert!(center + radius < layout.label.left, "{dpi}");
            }
        }
    }

    #[test]
    fn the_pointer_may_cross_from_the_art_to_the_slider_but_not_onto_the_progress_line() {
        for dpi in [96, 144, 192] {
            let client = client(dpi);
            let layout = SliderLayout::new(client, dpi);
            let middle = (layout.row.top + layout.row.bottom) / 2;
            for x in [
                layout.art.left,
                layout.art.right,
                layout.grip.left,
                layout.track.right,
                layout.label.right - 1,
            ] {
                assert!(contains(&layout.zone, POINT { x, y: middle }), "{dpi} {x}");
            }
            assert!(contains(
                &layout.zone,
                POINT {
                    x: layout.art.left,
                    y: layout.art.bottom - 1
                }
            ));
            // Beyond the slider the title returns, and the seek reach stays the track's own.
            assert!(!contains(
                &layout.zone,
                POINT {
                    x: layout.label.right,
                    y: middle
                }
            ));
            let seek_top = progress_track(client, dpi).top - scale(SEEK_REACH, dpi);
            assert!(layout.zone.bottom <= seek_top, "{dpi}");
            assert!(layout.art.bottom <= layout.zone.bottom, "{dpi}");
        }
    }

    #[test]
    fn positions_and_levels_convert_both_ways_and_stop_at_the_ends() {
        for dpi in [96, 144, 192] {
            let layout = SliderLayout::new(client(dpi), dpi);
            assert_eq!(layout.percent_at(layout.track.left), 0);
            assert_eq!(layout.percent_at(layout.track.right), 100);
            assert_eq!(layout.percent_at(layout.track.left - 500), 0);
            assert_eq!(layout.percent_at(layout.track.right + 500), 100);
            for percent in 0..=VolumeLevel::MAX_PERCENT {
                assert_eq!(
                    layout.percent_at(layout.thumb_center(percent)),
                    percent,
                    "{dpi}"
                );
            }
        }
    }

    #[test]
    fn a_bar_too_narrow_for_the_slider_keeps_a_track_that_fits() {
        let narrow = painting::rectangle(0, 0, 300, theme::MEDIA_BAR_HEIGHT);
        let layout = SliderLayout::new(narrow, 96);
        assert!(layout.track.right > layout.track.left);
        assert_eq!(layout.percent_at(layout.track.left), 0);
        assert_eq!(layout.percent_at(layout.track.right), 100);
    }

    #[test]
    fn a_level_read_earlier_never_pulls_back_a_slider_the_person_moved() {
        let slider = VolumeSlider::new();
        assert!(slider.view().is_none());
        assert_eq!(slider.level(), None);
        // A reading is kept even while the slider is hidden, for when it next shows.
        assert!(slider.accept(level(40)));
        assert!(!slider.accept(level(40)));
        assert_eq!(slider.level(), Some(40));
        slider.adjusted.set(true);
        slider.volume.set(level(70));
        assert!(!slider.accept(level(40)));
        assert_eq!(slider.level(), Some(70));
        // That the volume cannot be reached is shown whatever the person did.
        assert!(slider.accept(BarVolume::Unavailable));
        assert_eq!(slider.level(), None);
        slider.adjusted.set(false);
        assert!(slider.accept(level(55)));
        assert_eq!(slider.level(), Some(55));
    }

    #[test]
    fn the_glyph_follows_the_level_and_shows_silence() {
        assert_eq!(glyph(BarVolume::Reading), VOLUME_GLYPH);
        assert_eq!(glyph(BarVolume::Unavailable), VOLUME_GLYPH);
        assert_eq!(glyph(level(0)), MUTED_GLYPH);
        assert_eq!(
            glyph(BarVolume::Level(VolumeLevel::new(80, true))),
            MUTED_GLYPH
        );
        for (percent, expected) in [
            (1, LEVEL_GLYPHS[0]),
            (34, LEVEL_GLYPHS[0]),
            (35, LEVEL_GLYPHS[1]),
            (68, LEVEL_GLYPHS[1]),
            (69, LEVEL_GLYPHS[2]),
            (100, LEVEL_GLYPHS[2]),
        ] {
            assert_eq!(glyph(level(percent)), expected, "{percent}");
        }
    }

    thread_local! {
        /// The notification codes the test window received from the info control, in order.
        static NOTICES: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    }

    unsafe extern "system" fn record_notices(
        window: HWND,
        message: u32,
        word: WPARAM,
        long: LPARAM,
    ) -> LRESULT {
        if message == WM_COMMAND && word.0 & 0xffff == MEDIA_INFO_ID {
            NOTICES.with(|notices| notices.borrow_mut().push((word.0 >> 16) as u32));
            return LRESULT(0);
        }
        DefWindowProcW(window, message, word, long)
    }

    /// The info control's own window procedure, driven by the messages a pointer would cause.
    /// The window is never shown and no real pointer is moved.
    #[test]
    fn the_pointer_reveals_the_slider_drags_it_and_leaves() {
        use windows::Win32::{
            System::{
                LibraryLoader::GetModuleHandleW,
                SystemServices::{SS_NOTIFY, SS_OWNERDRAW},
            },
            UI::Input::KeyboardAndMouse::GetCapture,
        };
        let _serial = crate::windows::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }.unwrap().into();
        let class = WNDCLASSW {
            lpfnWndProc: Some(record_notices),
            hInstance: instance,
            lpszClassName: w!("Core.Test.VolumeNotices"),
            ..Default::default()
        };
        assert_ne!(unsafe { RegisterClassW(&class) }, 0);
        let (width, height) = (theme::WIDTH, theme::MEDIA_BAR_HEIGHT);
        let parent = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class.lpszClassName,
                w!(""),
                WS_POPUP,
                0,
                0,
                width,
                height,
                None,
                None,
                Some(instance),
                None,
            )
        }
        .unwrap();
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
        unsafe {
            SetWindowPos(
                info,
                None,
                0,
                0,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        }
        .unwrap();
        let slider = VolumeSlider::new();
        slider.install(info).unwrap();
        let layout = SliderLayout::new(client(96), 96);
        let middle = (layout.row.top + layout.row.bottom) / 2;
        let art = (
            (layout.art.left + layout.art.right) / 2,
            (layout.art.top + layout.art.bottom) / 2,
        );
        let send = |message: u32, (x, y): (i32, i32)| {
            let point = ((y as i16 as u16 as isize) << 16) | (x as i16 as u16 as isize);
            unsafe { SendMessageW(info, message, None, Some(LPARAM(point))) };
        };
        let notices = || NOTICES.with(|notices| std::mem::take(&mut *notices.borrow_mut()));
        let dragging = || slider.view().is_some_and(|view| view.dragging);

        // "Nothing playing" has no volume: the art reveals nothing.
        send(WM_MOUSEMOVE, art);
        assert!(slider.view().is_none());
        // With a player, the title alone reveals nothing either; the art does, and asks.
        slider.reset(info, true);
        send(WM_MOUSEMOVE, (layout.track.left, middle));
        assert!(slider.view().is_none());
        send(WM_MOUSEMOVE, art);
        assert_eq!(
            slider.view().map(|view| view.volume),
            Some(BarVolume::Reading)
        );
        assert_eq!(notices(), [VOLUME_WANTED]);
        // Across the gap to the slider, it stays, without asking again.
        send(WM_MOUSEMOVE, (layout.art.right + 2, middle));
        send(WM_MOUSEMOVE, (layout.track.left, middle));
        assert!(slider.view().is_some());
        // Until the level is known a press moves nothing, and does not open the player.
        send(WM_LBUTTONDOWN, (layout.thumb_center(75), middle));
        assert!(!dragging());
        assert!(notices().is_empty());

        slider.set_volume(info, level(40));
        send(WM_LBUTTONDOWN, (layout.thumb_center(75), middle));
        assert_eq!(slider.level(), Some(75));
        assert!(dragging());
        assert_eq!(unsafe { GetCapture() }, info);
        assert_eq!(notices(), [VOLUME_CHANGED]);
        // A level read before the drag does not pull the handle back.
        slider.set_volume(info, level(40));
        assert_eq!(slider.level(), Some(75));
        // The drag follows the pointer beyond the slider's ends, and above or below it.
        send(WM_MOUSEMOVE, (-50, -20));
        assert_eq!(slider.level(), Some(0));
        assert_eq!(notices(), [VOLUME_CHANGED]);
        // Moving without changing the level says nothing more.
        send(WM_MOUSEMOVE, (-60, 300));
        assert!(notices().is_empty());
        // Leaving the control during a drag does not end it.
        send(WM_MOUSELEAVE, (0, 0));
        assert!(dragging());
        send(WM_LBUTTONUP, (layout.thumb_center(25), middle));
        assert_eq!(slider.level(), Some(25));
        assert!(!dragging());
        assert!(unsafe { GetCapture() }.0.is_null());
        assert_eq!(notices(), [VOLUME_CHANGED]);
        // Released over the slider, it stays; a later reading still does not move it.
        assert!(slider.view().is_some());
        slider.set_volume(info, level(40));
        assert_eq!(slider.level(), Some(25));

        // A press on the level beside the track is the slider's, but moves nothing.
        send(WM_LBUTTONDOWN, (layout.label.right - 2, middle));
        assert!(!dragging());
        assert!(notices().is_empty());
        // The art keeps its own click, which opens the player.
        send(WM_LBUTTONDOWN, art);
        assert_eq!(notices(), [STN_CLICKED]);
        // Past the slider the title returns, and its click is the control's own again.
        send(WM_MOUSEMOVE, (layout.label.right + 4, middle));
        assert!(slider.view().is_none());
        send(WM_LBUTTONDOWN, (layout.track.left, middle));
        assert_eq!(notices(), [STN_CLICKED]);

        // Shown again, the level chosen earlier shows at once and a new reading may move it.
        send(WM_MOUSEMOVE, art);
        assert_eq!(slider.view().map(|view| view.volume), Some(level(25)));
        assert_eq!(notices(), [VOLUME_WANTED]);
        slider.set_volume(info, level(60));
        assert_eq!(slider.level(), Some(60));
        // A drag released away from the slider takes the slider with it.
        send(WM_LBUTTONDOWN, (layout.thumb_center(50), middle));
        send(WM_LBUTTONUP, (layout.label.right + 200, middle));
        assert_eq!(slider.level(), Some(100));
        assert!(slider.view().is_none());
        assert_eq!(notices(), [VOLUME_CHANGED, VOLUME_CHANGED]);

        // A volume that cannot be reached ends a drag and is asked for again next time.
        send(WM_MOUSEMOVE, art);
        send(WM_LBUTTONDOWN, (layout.thumb_center(50), middle));
        assert!(dragging());
        slider.set_volume(info, BarVolume::Unavailable);
        assert!(!dragging());
        assert!(unsafe { GetCapture() }.0.is_null());
        assert_eq!(slider.level(), None);
        // The pointer leaving Core's window hides the slider.
        send(WM_MOUSELEAVE, (0, 0));
        assert!(slider.view().is_none());
        send(WM_MOUSEMOVE, art);
        assert_eq!(
            slider.view().map(|view| view.volume),
            Some(BarVolume::Reading)
        );
        // Another player's volume is not this one's.
        slider.set_volume(info, level(30));
        slider.reset(info, true);
        assert!(slider.view().is_none());
        assert_eq!(slider.level(), None);

        unsafe { DestroyWindow(parent) }.unwrap();
        unsafe { UnregisterClassW(class.lpszClassName, Some(instance)) }.unwrap();
    }

    #[test]
    fn mouse_coordinates_are_signed() {
        let packed = |x: i16, y: i16| LPARAM(((y as u16 as isize) << 16) | (x as u16 as isize));
        assert_eq!(pointer(packed(12, 30)), POINT { x: 12, y: 30 });
        assert_eq!(pointer(packed(-8, -3)), POINT { x: -8, y: -3 });
    }
}
