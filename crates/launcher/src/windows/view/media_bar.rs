//! The now-playing bar above the search box: album art, title and artist, the track's progress,
//! and previous, play / pause and next buttons. The buttons are native, for the keyboard and
//! screen readers; art, text and progress are drawn in one owner-drawn static control, whose
//! text is what a screen reader announces. While the pointer is on the art, a volume slider
//! takes the title's place.
use super::*;

mod volume;

pub use volume::{BarVolume, VOLUME_CHANGED, VOLUME_WANTED};

use crate::windows::power_menu::icon_button;
use core_engine::media::{
    format_clock, playback_position, MediaControls, PlaybackProgress, Timeline,
};
use volume::{SliderLayout, SliderView, VolumeSlider};
use windows::Win32::{
    System::SystemInformation::GetSystemTimePreciseAsFileTime,
    UI::Input::KeyboardAndMouse::SetFocus,
};

pub const MEDIA_INFO_ID: usize = 120;
pub const MEDIA_PREVIOUS_ID: usize = 121;
pub const MEDIA_PLAY_ID: usize = 122;
pub const MEDIA_NEXT_ID: usize = 123;
pub const MEDIA_PROGRESS_TIMER: usize = 46;

// Geometry in 96-DPI pixels, from the bar's top-left corner.
const ART_LEFT: i32 = 24;
const ART_TOP: i32 = 12;
const TEXT_LEFT: i32 = 76;
const TITLE_TOP: i32 = 11;
const DETAIL_TOP: i32 = 31;
const DETAIL_BOTTOM: i32 = 49;
const BUTTON_SIZE: i32 = 36;
const BUTTON_TOP: i32 = 14;
const BUTTON_PITCH: i32 = 40;
const SIDE_MARGIN: i32 = 24;
const TIME_WIDTH: i32 = 96;
const PROGRESS_TOP: i32 = 57;
const PROGRESS_HEIGHT: i32 = 3;
/// Clicks this far above the progress line still seek, so it is easy to hit; the album art
/// ends above this reach, so clicking it opens the player.
const SEEK_REACH: i32 = 4;
/// The progress clock never waits longer than this, nor repaints more often than the minimum.
const LONGEST_TICK_MS: u32 = 1_000;
const SHORTEST_TICK_MS: u32 = 50;

const PREVIOUS_GLYPH: &str = "\u{e892}";
const NEXT_GLYPH: &str = "\u{e893}";
const PLAY_GLYPH: &str = "\u{e768}";
const PAUSE_GLYPH: &str = "\u{e769}";
/// Shown in place of album art when the player provides none.
const MUSIC_GLYPH: &str = "\u{e8d6}";

#[derive(Clone)]
pub struct MediaBarContent {
    pub app_id: Arc<str>,
    pub title: Arc<str>,
    /// "Artist · Spotify".
    pub detail: String,
    pub playing: bool,
    pub controls: MediaControls,
    pub timeline: Option<Timeline>,
    pub art: Option<Arc<ApplicationIcon>>,
}

impl MediaBarContent {
    fn progress(&self) -> Option<PlaybackProgress> {
        playback_position(self.timeline?, self.playing, file_time_now())
    }

    fn accessible_text(&self) -> String {
        format!(
            "Now playing: {}, {}. {}",
            self.title,
            self.detail,
            if self.playing { "Playing" } else { "Paused" }
        )
    }
}

/// When the bar may appear or disappear, so the search box only moves when the window opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarPresence {
    /// Core is being shown, or Settings changed the bar: it shows exactly when there is content.
    Free,
    /// Core is open and nothing is typed yet: the bar may appear, but does not disappear.
    AppearOnly,
    /// Something was typed: the bar keeps its place.
    Keep,
}

pub enum MediaBarClick {
    /// Seek to this position, in 100 ns units from the start of the track.
    Seek {
        app_id: Arc<str>,
        position: i64,
    },
    OpenPlayer(Arc<str>),
    Nothing,
}

pub(super) struct MediaBar {
    info: HWND,
    buttons: [(usize, HWND); 3],
    content: RefCell<Option<MediaBarContent>>,
    /// Boxed: the info control's window procedure keeps its address.
    slider: Box<VolumeSlider>,
}

impl MediaBar {
    fn create(parent: HWND, instance: HINSTANCE) -> windows::core::Result<Self> {
        let mut bar = Self {
            info: HWND::default(),
            buttons: [
                (MEDIA_PREVIOUS_ID, HWND::default()),
                (MEDIA_PLAY_ID, HWND::default()),
                (MEDIA_NEXT_ID, HWND::default()),
            ],
            content: RefCell::new(None),
            slider: VolumeSlider::new(),
        };
        for (identifier, button) in &mut bar.buttons {
            let label = match *identifier {
                MEDIA_PREVIOUS_ID => w!("Previous track"),
                MEDIA_PLAY_ID => w!("Play"),
                _ => w!("Next track"),
            };
            *button = unsafe {
                child(
                    parent,
                    instance,
                    w!("BUTTON"),
                    label,
                    WINDOW_STYLE(BS_OWNERDRAW as u32) | WS_TABSTOP,
                    *identifier,
                )?
            };
            button_hover::install(*button)?;
        }
        // Created last, so it is below the buttons it spans and its painting clips around them.
        bar.info = unsafe {
            child(
                parent,
                instance,
                w!("STATIC"),
                w!("Nothing playing"),
                WINDOW_STYLE(
                    windows::Win32::System::SystemServices::SS_OWNERDRAW.0
                        | windows::Win32::System::SystemServices::SS_NOTIFY.0,
                ) | WS_CLIPSIBLINGS,
                MEDIA_INFO_ID,
            )?
        };
        bar.slider.install(bar.info)?;
        Ok(bar)
    }

    fn allowed(&self, identifier: usize) -> bool {
        self.content
            .borrow()
            .as_ref()
            .is_some_and(|content| match identifier {
                MEDIA_PREVIOUS_ID => content.controls.previous,
                MEDIA_NEXT_ID => content.controls.next,
                _ => {
                    if content.playing {
                        content.controls.pause
                    } else {
                        content.controls.play
                    }
                }
            })
    }

    fn show(&self, visible: bool) {
        if !visible {
            self.slider.hide(self.info);
        }
        unsafe {
            let _ = ShowWindow(self.info, if visible { SW_SHOWNA } else { SW_HIDE });
            for (identifier, button) in self.buttons {
                let shown = visible && self.allowed(identifier);
                let _ = ShowWindow(button, if shown { SW_SHOWNA } else { SW_HIDE });
            }
        }
    }

    fn set_content(&self, content: Option<MediaBarContent>) {
        let text = content.as_ref().map_or_else(
            || "Nothing playing".to_owned(),
            MediaBarContent::accessible_text,
        );
        let play_label = if content.as_ref().is_some_and(|content| content.playing) {
            "Pause"
        } else {
            "Play"
        };
        let same_player = match (self.content.borrow().as_ref(), &content) {
            (Some(shown), Some(next)) => shown.app_id == next.app_id,
            _ => false,
        };
        if !same_player {
            self.slider.reset(self.info, content.is_some());
        }
        *self.content.borrow_mut() = content;
        for (control, label) in [(self.info, text.as_str()), (self.buttons[1].1, play_label)] {
            if control_text(control) != label {
                let label = wide(label);
                unsafe {
                    let _ = SetWindowTextW(control, PCWSTR(label.as_ptr()));
                }
            }
        }
        unsafe {
            let _ = InvalidateRect(Some(self.info), None, false);
            for (_, button) in self.buttons {
                let _ = InvalidateRect(Some(button), None, false);
            }
        }
    }

    /// Until the elapsed time next shows a different second; None while nothing moves.
    fn next_tick(&self) -> Option<u32> {
        let content = self.content.borrow();
        let content = content.as_ref().filter(|content| content.playing)?;
        let wait = content.progress()?.until_next_second().as_millis() as u32;
        Some(wait.clamp(SHORTEST_TICK_MS, LONGEST_TICK_MS))
    }
}

impl Drop for MediaBar {
    fn drop(&mut self) {
        for control in [
            self.info,
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

impl View {
    /// The bar's height now, in 96-DPI pixels: zero unless it is shown and neither Settings nor
    /// a command's output has the window.
    pub(super) fn media_bar_height(&self) -> i32 {
        // Typing `/` keeps the bar; only a command's output, which needs the room, hides it.
        let output = self.terminal.get() && self.output_visible.get();
        if self.media_bar_shown.get() && !self.settings_open.get() && !output {
            theme::MEDIA_BAR_HEIGHT
        } else {
            0
        }
    }

    /// A bar that stays without content shows "Nothing playing".
    pub fn set_media_bar(&self, content: Option<MediaBarContent>, presence: BarPresence) {
        let shown = self.media_bar_shown.get();
        let show = match presence {
            BarPresence::Free => content.is_some(),
            BarPresence::AppearOnly => shown || content.is_some(),
            BarPresence::Keep => shown,
        };
        if show && self.media_bar.get().is_none() {
            let created = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }
                .and_then(|instance| MediaBar::create(self.parent, instance.into()));
            match created {
                Ok(bar) => {
                    let _ = self.media_bar.set(bar);
                }
                Err(error) => {
                    eprintln!("Could not create the now-playing bar: {error}");
                    return;
                }
            }
        }
        if let Some(bar) = self.media_bar.get() {
            bar.set_content(content);
        }
        if show != shown {
            self.media_bar_shown.set(show);
            self.relayout();
        } else if let Some(bar) = self.media_bar.get() {
            bar.show(self.media_bar_height() > 0);
        }
        self.schedule_media_progress();
    }

    /// Album art's edge in pixels at the current scale.
    pub fn media_art_size(&self) -> u32 {
        scale(theme::MEDIA_ART, self.dpi.get()) as u32
    }

    /// The app the bar shows, for its buttons; None while the bar is hidden.
    pub fn media_bar_app(&self) -> Option<Arc<str>> {
        let bar = self
            .media_bar
            .get()
            .filter(|_| self.media_bar_height() > 0)?;
        let content = bar.content.borrow();
        content.as_ref().map(|content| content.app_id.clone())
    }

    /// Places the bar's controls; called from `layout` with the window's client width.
    pub(super) fn place_media_bar(&self, search: &SearchLayout) -> windows::core::Result<()> {
        let Some(bar) = self.media_bar.get() else {
            return Ok(());
        };
        bar.slider.set_dpi(self.dpi.get());
        let visible = self.media_bar_height() > 0;
        if visible {
            for (control, area) in [
                (bar.info, search.media_info),
                (bar.buttons[0].1, search.media_buttons[0]),
                (bar.buttons[1].1, search.media_buttons[1]),
                (bar.buttons[2].1, search.media_buttons[2]),
            ] {
                unsafe {
                    SetWindowPos(
                        control,
                        None,
                        area.left,
                        area.top,
                        (area.right - area.left).max(0),
                        (area.bottom - area.top).max(0),
                        SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOCOPYBITS,
                    )?;
                }
            }
        }
        bar.show(visible);
        Ok(())
    }

    /// Hidden with the other search controls while Settings is open.
    pub(super) fn show_media_bar(&self, visible: bool) {
        if let Some(bar) = self.media_bar.get() {
            bar.show(visible && self.media_bar_height() > 0);
        }
    }

    /// The progress clock runs only while Core is visible and the track plays.
    pub fn set_media_progress_active(&self, active: bool) {
        self.media_progress_active.set(active);
        self.schedule_media_progress();
        // Core is hiding: the pointer no longer holds the volume slider open.
        if let Some(bar) = self.media_bar.get().filter(|_| !active) {
            bar.slider.hide(bar.info);
        }
    }

    /// The level the person chose on the bar's volume slider; None unless it shows one.
    pub fn media_volume(&self) -> Option<u8> {
        self.media_bar.get()?.slider.level()
    }

    /// A player's volume was read, or could not be reached. The slider shows it if the bar
    /// still shows that player.
    pub fn set_media_volume(&self, app_id: &str, volume: BarVolume) {
        let Some(bar) = self.media_bar.get() else {
            return;
        };
        let shows_player = bar
            .content
            .borrow()
            .as_ref()
            .is_some_and(|content| &*content.app_id == app_id);
        if shows_player {
            bar.slider.set_volume(bar.info, volume);
        }
    }

    pub fn tick_media_progress(&self) {
        if let Some(bar) = self.media_bar.get() {
            unsafe {
                let _ = InvalidateRect(Some(bar.info), None, false);
            }
        }
        self.schedule_media_progress();
    }

    /// Windows repeats a timer until it is killed: each tick sets the next delay, and the
    /// timer is killed as soon as nothing moves.
    fn schedule_media_progress(&self) {
        let delay = self
            .media_bar
            .get()
            .filter(|_| self.media_progress_active.get() && self.media_bar_height() > 0)
            .and_then(MediaBar::next_tick);
        match delay {
            Some(delay) => {
                if unsafe { SetTimer(Some(self.parent), MEDIA_PROGRESS_TIMER, delay, None) } == 0 {
                    eprintln!(
                        "Could not schedule the track progress: {}",
                        windows::core::Error::from_win32()
                    );
                } else {
                    self.media_timer_set.set(true);
                }
            }
            None if self.media_timer_set.replace(false) => {
                if let Err(error) = unsafe { KillTimer(Some(self.parent), MEDIA_PROGRESS_TIMER) } {
                    eprintln!("Could not stop the track progress: {error}");
                }
            }
            None => {}
        }
    }

    /// What a click on the bar's art, text or progress line does.
    pub fn media_info_click(&self) -> MediaBarClick {
        let Some(bar) = self.media_bar.get() else {
            return MediaBarClick::Nothing;
        };
        let Some(content) = bar.content.borrow().clone() else {
            return MediaBarClick::Nothing;
        };
        let dpi = self.dpi.get();
        let mut point = POINT::default();
        let mut client = RECT::default();
        unsafe {
            if GetCursorPos(&mut point).is_err()
                || !ScreenToClient(bar.info, &mut point).as_bool()
                || GetClientRect(bar.info, &mut client).is_err()
            {
                return MediaBarClick::OpenPlayer(content.app_id);
            }
        }
        let track = progress_track(client, dpi);
        let seekable = content.controls.seek
            && content
                .timeline
                .is_some_and(|timeline| timeline.end > timeline.start);
        let on_track = point.y >= track.top - scale(SEEK_REACH, dpi)
            && point.x >= track.left
            && point.x < track.right;
        if seekable && on_track {
            let timeline = content.timeline.expect("seekable timeline");
            let fraction =
                f64::from(point.x - track.left) / f64::from((track.right - track.left).max(1));
            let span = (timeline.end - timeline.start) as f64;
            return MediaBarClick::Seek {
                app_id: content.app_id,
                position: timeline.start + (fraction.clamp(0., 1.) * span) as i64,
            };
        }
        MediaBarClick::OpenPlayer(content.app_id)
    }

    /// Buttons take the keyboard focus when clicked; typing continues in the search box.
    pub fn return_focus_to_search(&self) {
        unsafe {
            let _ = SetFocus(Some(self.input));
        }
    }

    pub(super) fn draw_media_item(&self, item: &DRAWITEMSTRUCT) -> bool {
        let Some(bar) = self.media_bar.get() else {
            return false;
        };
        let palette = self.palette.get();
        let (dpi, fonts) = (self.dpi.get(), self.fonts.get());
        match item.CtlID as usize {
            MEDIA_INFO_ID => {
                let content = bar.content.borrow();
                // Off screen first: the slider repaints the bar as fast as the pointer moves.
                painting::buffered(item.hDC, &item.rcItem, |context| {
                    draw_info(
                        context,
                        item.rcItem,
                        content.as_ref(),
                        bar.slider.view(),
                        dpi,
                        fonts,
                        palette,
                    );
                });
                true
            }
            identifier @ (MEDIA_PREVIOUS_ID | MEDIA_PLAY_ID | MEDIA_NEXT_ID) => {
                let playing = bar
                    .content
                    .borrow()
                    .as_ref()
                    .is_some_and(|content| content.playing);
                let glyph = match identifier {
                    MEDIA_PREVIOUS_ID => PREVIOUS_GLYPH,
                    MEDIA_NEXT_ID => NEXT_GLYPH,
                    _ if playing => PAUSE_GLYPH,
                    _ => PLAY_GLYPH,
                };
                icon_button(item, glyph, dpi, fonts, palette);
                true
            }
            _ => false,
        }
    }
}

fn progress_track(client: RECT, dpi: u32) -> RECT {
    painting::rectangle(
        client.left + scale(SIDE_MARGIN, dpi),
        client.top + scale(PROGRESS_TOP, dpi),
        client.right - scale(SIDE_MARGIN, dpi),
        client.top + scale(PROGRESS_TOP + PROGRESS_HEIGHT, dpi),
    )
}

/// Where the bar's buttons sit in a window `width` pixels wide, left to right.
pub fn button_areas(width: i32, top: i32, dpi: u32) -> [RECT; 3] {
    let right = width - scale(SIDE_MARGIN, dpi);
    std::array::from_fn(|index| {
        let left = right - scale(BUTTON_SIZE + (2 - index as i32) * BUTTON_PITCH, dpi);
        painting::rectangle(
            left,
            top + scale(BUTTON_TOP, dpi),
            left + scale(BUTTON_SIZE, dpi),
            top + scale(BUTTON_TOP + BUTTON_SIZE, dpi),
        )
    })
}

/// Where the album art is drawn, in the info control's client pixels.
fn art_area(client: RECT, dpi: u32) -> RECT {
    let left = client.left + scale(ART_LEFT, dpi);
    let top = client.top + scale(ART_TOP, dpi);
    let size = scale(theme::MEDIA_ART, dpi);
    painting::rectangle(left, top, left + size, top + size)
}

/// The title's row: from the text column to just before the buttons over the bar's right side.
fn title_row(client: RECT, dpi: u32) -> RECT {
    let buttons = button_areas(client.right - client.left, 0, dpi);
    painting::rectangle(
        client.left + scale(TEXT_LEFT, dpi),
        client.top + scale(TITLE_TOP, dpi),
        client.left + buttons[0].left - scale(12, dpi),
        client.top + scale(DETAIL_TOP, dpi),
    )
}

/// `slider`: the volume slider is showing, over the art and in place of the title.
fn draw_info(
    context: HDC,
    area: RECT,
    content: Option<&MediaBarContent>,
    slider: Option<SliderView>,
    dpi: u32,
    fonts: Fonts,
    palette: Palette,
) {
    painting::fill(context, &area, palette.background);
    let at = |left: i32, top: i32, right: i32, bottom: i32| {
        painting::rectangle(
            area.left + scale(left, dpi),
            area.top + scale(top, dpi),
            right,
            area.top + scale(bottom, dpi),
        )
    };
    // Text stops before the buttons, which sit over the bar's right side.
    let text_right = button_areas(area.right - area.left, 0, dpi)[0].left - scale(12, dpi);
    let track = progress_track(area, dpi);
    let Some(content) = content else {
        painting::text(
            context,
            "Nothing playing",
            at(TEXT_LEFT, TITLE_TOP, text_right, DETAIL_BOTTOM),
            fonts.title,
            palette.secondary,
        );
        painting::fill(
            context,
            &painting::rectangle(track.left, track.top, track.right, track.top + 1),
            palette.selected,
        );
        return;
    };
    let art = art_area(area, dpi);
    let art_drawn = content
        .art
        .as_ref()
        .is_some_and(|icon| icon.draw(context, art, art.right - art.left));
    if !art_drawn {
        painting::rounded(context, &art, scale(7, dpi), palette.selected);
    }
    match slider {
        Some(slider) => {
            volume::draw_art_overlay(context, art, art_drawn, slider.volume, dpi, fonts, palette);
            let layout = SliderLayout::new(area, dpi);
            volume::draw_slider(context, &layout, slider, dpi, fonts, palette);
        }
        None => {
            if !art_drawn {
                painting::centred_glyph(context, art, MUSIC_GLYPH, dpi, fonts.icon, palette.accent);
            }
            painting::text(
                context,
                &content.title,
                title_row(area, dpi),
                fonts.title,
                palette.text,
            );
        }
    }
    let progress = content.progress();
    let time_left = if progress.is_some() {
        text_right - scale(TIME_WIDTH, dpi)
    } else {
        text_right
    };
    painting::text(
        context,
        &content.detail,
        at(
            TEXT_LEFT,
            DETAIL_TOP,
            time_left - scale(8, dpi),
            DETAIL_BOTTOM,
        ),
        fonts.detail,
        palette.secondary,
    );
    let Some(progress) = progress else {
        // No track length: a plain divider instead of a progress line.
        painting::fill(
            context,
            &painting::rectangle(track.left, track.top, track.right, track.top + 1),
            palette.selected,
        );
        return;
    };
    right_text(
        context,
        &format!(
            "{} / {}",
            format_clock(progress.elapsed),
            format_clock(progress.duration)
        ),
        at(0, DETAIL_TOP, text_right, DETAIL_BOTTOM),
        fonts.detail,
        palette.secondary,
    );
    painting::fill(context, &track, palette.selected);
    let filled = track.left + ((track.right - track.left) as f64 * progress.fraction()) as i32;
    if filled > track.left {
        painting::fill(
            context,
            &painting::rectangle(track.left, track.top, filled, track.bottom),
            palette.accent,
        );
    }
}

fn right_text(context: HDC, label: &str, mut area: RECT, font: HFONT, color: COLORREF) {
    let mut text = wide(label);
    text.pop();
    unsafe {
        let state = SaveDC(context);
        SelectObject(context, font.into());
        SetBkMode(context, TRANSPARENT);
        SetTextColor(context, color);
        DrawTextW(
            context,
            &mut text,
            &mut area,
            DT_SINGLELINE | DT_VCENTER | DT_RIGHT | DT_NOPREFIX,
        );
        let _ = RestoreDC(context, state);
    }
}

fn file_time_now() -> i64 {
    let time = unsafe { GetSystemTimePreciseAsFileTime() };
    (i64::from(time.dwHighDateTime) << 32) | i64::from(time.dwLowDateTime)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_sit_left_to_right_inside_the_right_margin() {
        let [previous, play, next] = button_areas(640, 0, 96);
        assert_eq!(next.right, 640 - SIDE_MARGIN);
        assert_eq!(play.right + (BUTTON_PITCH - BUTTON_SIZE), next.left);
        assert_eq!(previous.right + (BUTTON_PITCH - BUTTON_SIZE), play.left);
        assert_eq!(previous.top, BUTTON_TOP);
        let scaled = button_areas(960, 96, 144);
        assert_eq!(scaled[2].right, 960 - scale(SIDE_MARGIN, 144));
        assert_eq!(scaled[0].top, 96 + scale(BUTTON_TOP, 144));
    }
}
