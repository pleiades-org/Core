//! The volume mixer's rows, which `@volume` and `@mix` list: beside each program's name, a
//! button that mutes it, a slider and its level. The results list follows the pointer on those
//! here; Core's state reads Windows' mixer and changes it.
use super::*;
use core_engine::media::VolumeLevel;
use level_slider::{contains, pointer, Thumb};
use results_list::{item_area, notify, row_at, select};
use windows::Win32::UI::{
    Input::KeyboardAndMouse::{ReleaseCapture, SetCapture},
    Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
};

const ROWS_SUBCLASS: usize = 5;
/// Sent to Core's window as a WM_COMMAND notification from the results list: the person moves
/// a row's slider, and `View::mixer_choice` is their choice.
pub const MIXER_CHANGED: u32 = 0x0B20;
/// Sent the same way when they let go of the slider.
pub const MIXER_RELEASED: u32 = 0x0B21;
/// Sent the same way when they press a row's mute button; that row is selected by then.
pub const MIXER_MUTE: u32 = 0x0B22;

// Geometry in 96-DPI pixels, from the row's right edge inwards.
const RIGHT_MARGIN: i32 = 18;
const LABEL_WIDTH: i32 = 40;
const LABEL_GAP: i32 = 14;
const TRACK_WIDTH: i32 = 190;
/// The slider answers presses this far beyond either end of its track.
const TRACK_REACH: i32 = 8;
const MUTE_SIZE: i32 = 30;
const MUTE_GAP: i32 = 14;
const TEXT_GAP: i32 = 10;
/// A narrow row shortens the track before the name has less room than this.
const TEXT_ROOM: i32 = 120;

/// A level the person chose on a row's slider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MixerChoice {
    /// The row's result identifier.
    pub row: Arc<str>,
    pub percent: u8,
}

/// Where a row's volume parts are, in the results list's client pixels.
struct RowLayout {
    /// The name, and under it what the program is doing, end here.
    text_right: i32,
    /// Mutes and unmutes.
    mute: RECT,
    track: RECT,
    label: RECT,
    /// A press here moves the thumb.
    grip: RECT,
}

impl RowLayout {
    fn new(row: RECT, dpi: u32) -> Self {
        let label_right = row.right - scale(RIGHT_MARGIN, dpi);
        let label_left = label_right - scale(LABEL_WIDTH, dpi);
        let right = label_left - scale(LABEL_GAP, dpi);
        let text_left = row.left + scale(painting::ROW_TEXT_LEFT, dpi);
        let earliest = text_left + scale(TEXT_ROOM + TEXT_GAP + MUTE_SIZE + MUTE_GAP, dpi);
        let left = (right - scale(TRACK_WIDTH, dpi))
            .max(earliest)
            .min(right - 1);
        let middle = (row.top + row.bottom) / 2;
        let size = scale(MUTE_SIZE, dpi);
        let mute_left = left - scale(MUTE_GAP, dpi) - size;
        let reach = scale(TRACK_REACH, dpi);
        Self {
            text_right: mute_left - scale(TEXT_GAP, dpi),
            mute: painting::rectangle(
                mute_left,
                middle - size / 2,
                mute_left + size,
                middle - size / 2 + size,
            ),
            track: level_slider::track(left, right, middle, dpi),
            label: painting::rectangle(label_left, row.top, label_right, row.bottom),
            grip: painting::rectangle(left - reach, row.top, right + reach, row.bottom),
        }
    }
}

/// What a row shows besides its program.
#[derive(Clone, Copy)]
struct RowView {
    level: VolumeLevel,
    selected: bool,
    dragging: bool,
}

/// What the results list's window procedure and the rows' painting share. The view owns it in
/// a box, whose address the procedure keeps until the list or the box goes.
pub(super) struct MixerRows {
    list: Cell<HWND>,
    dpi: Cell<u32>,
    /// The rows' identifiers in order while they are the mixer's; empty for other results.
    rows: RefCell<Vec<Arc<str>>>,
    /// The row whose slider the person moved, and the level they chose. It stays after they
    /// let go until the rows are set again, so the thumb never returns to an older level.
    chosen: RefCell<Option<MixerChoice>>,
    dragging: Cell<bool>,
}

impl MixerRows {
    pub fn new() -> Box<Self> {
        Box::new(Self {
            list: Cell::new(HWND::default()),
            dpi: Cell::new(96),
            rows: RefCell::new(Vec::new()),
            chosen: RefCell::new(None),
            dragging: Cell::new(false),
        })
    }

    /// Lets `list`, the results list, follow the pointer on the mixer's rows.
    pub fn install(&self, list: HWND) -> windows::core::Result<()> {
        unsafe {
            SetWindowSubclass(
                list,
                Some(rows_proc),
                ROWS_SUBCLASS,
                self as *const Self as usize,
            )
            .ok()?;
        }
        self.list.set(list);
        Ok(())
    }

    pub fn set_dpi(&self, dpi: u32) {
        self.dpi.set(dpi);
    }

    pub fn shown(&self) -> bool {
        !self.rows.borrow().is_empty()
    }

    pub fn choice(&self) -> Option<MixerChoice> {
        self.chosen.borrow().clone()
    }

    /// The rows were set again. A drag goes on while its row is still listed; a level the
    /// person let go of gives way to what the rows now say. Tells nobody: Core's state is at
    /// work when rows are set.
    pub fn set_rows(&self, rows: &[DisplayRow]) {
        let mixer = rows.first().is_some_and(|row| row.volume.is_some());
        let identifiers: Vec<Arc<str>> = if mixer {
            rows.iter().map(|row| row.identifier.clone()).collect()
        } else {
            Vec::new()
        };
        let held = self.dragging.get()
            && self
                .chosen
                .borrow()
                .as_ref()
                .is_some_and(|choice| identifiers.contains(&choice.row));
        if !held {
            // Before the rows change: the row to repaint is where it was drawn.
            self.invalidate_chosen();
            self.chosen.take();
            if self.dragging.replace(false) {
                release_pointer();
            }
        }
        *self.rows.borrow_mut() = identifiers;
    }

    /// The level to draw for `row`, and whether its slider is held: the person's choice while
    /// it stands, else the level the row carries.
    fn drawn(&self, row: &DisplayRow, level: VolumeLevel) -> (VolumeLevel, bool) {
        match self.chosen.borrow().as_ref() {
            Some(choice) if choice.row == row.identifier => {
                (level.moved_to(choice.percent), self.dragging.get())
            }
            _ => (level, false),
        }
    }

    fn layout(&self, list: HWND, index: usize) -> Option<RowLayout> {
        Some(RowLayout::new(item_area(list, index)?, self.dpi.get()))
    }

    fn chosen_index(&self) -> Option<usize> {
        let chosen = self.chosen.borrow();
        let row = &chosen.as_ref()?.row;
        self.rows.borrow().iter().position(|listed| listed == row)
    }

    /// True when the press was on a row's slider or mute button: the list's own click, which
    /// would take the keyboard from the search box, must not follow it.
    fn pressed(&self, list: HWND, point: POINT) -> bool {
        let Some((index, area)) = row_at(list, point).filter(|_| self.shown()) else {
            return false;
        };
        let Some(row) = self.rows.borrow().get(index).cloned() else {
            return false;
        };
        let layout = RowLayout::new(area, self.dpi.get());
        let on_mute = contains(&layout.mute, point);
        if !on_mute && !contains(&layout.grip, point) {
            return false;
        }
        select(list, index);
        if on_mute {
            notify(list, MIXER_MUTE);
            return true;
        }
        unsafe {
            SetCapture(list);
        }
        self.dragging.set(true);
        let percent = level_slider::percent_at(&layout.track, point.x);
        // The row of an earlier choice gets its own level back.
        self.invalidate_chosen();
        *self.chosen.borrow_mut() = Some(MixerChoice { row, percent });
        self.invalidate_chosen();
        notify(list, MIXER_CHANGED);
        true
    }

    /// True while a slider is held: the pointer is the drag's wherever it goes.
    fn moved(&self, list: HWND, point: POINT) -> bool {
        if !self.dragging.get() {
            return false;
        }
        self.choose(list, point);
        true
    }

    /// True when the release ended a drag.
    fn released(&self, list: HWND, point: POINT) -> bool {
        if !self.dragging.get() {
            return false;
        }
        self.choose(list, point);
        self.end_drag(list, true);
        true
    }

    fn choose(&self, list: HWND, point: POINT) {
        let Some(layout) = self
            .chosen_index()
            .and_then(|index| self.layout(list, index))
        else {
            return;
        };
        let percent = level_slider::percent_at(&layout.track, point.x);
        {
            let mut chosen = self.chosen.borrow_mut();
            let Some(choice) = chosen.as_mut().filter(|choice| choice.percent != percent) else {
                return;
            };
            choice.percent = percent;
        }
        self.invalidate_chosen();
        notify(list, MIXER_CHANGED);
    }

    /// Ends a drag and says so once. `release`: the list still holds the pointer for it; when
    /// another window took the pointer, Windows has already said so.
    fn end_drag(&self, list: HWND, release: bool) {
        if !self.dragging.replace(false) {
            return;
        }
        if release {
            release_pointer();
        }
        // The ring around the thumb goes.
        self.invalidate_chosen();
        notify(list, MIXER_RELEASED);
    }

    fn invalidate_chosen(&self) {
        let list = self.list.get();
        let Some(area) = self.chosen_index().and_then(|index| item_area(list, index)) else {
            return;
        };
        unsafe {
            let _ = InvalidateRect(Some(list), Some(&area), false);
        }
    }
}

impl Drop for MixerRows {
    fn drop(&mut self) {
        let list = self.list.get();
        // A list that outlives its view must not call into rows that are gone.
        unsafe {
            if !list.0.is_null() && IsWindow(Some(list)).as_bool() {
                let _ = RemoveWindowSubclass(list, Some(rows_proc), ROWS_SUBCLASS);
            }
        }
    }
}

fn release_pointer() {
    if let Err(error) = unsafe { ReleaseCapture() } {
        eprintln!("Could not release the pointer after a volume drag: {error}");
    }
}

unsafe extern "system" fn rows_proc(
    list: HWND,
    message: u32,
    word: WPARAM,
    data: LPARAM,
    _subclass: usize,
    reference: usize,
) -> LRESULT {
    // The view boxes these rows; this procedure is removed before the box goes.
    let rows = &*(reference as *const MixerRows);
    let handled = match message {
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => rows.pressed(list, pointer(data)),
        WM_MOUSEMOVE => rows.moved(list, pointer(data)),
        WM_LBUTTONUP => rows.released(list, pointer(data)),
        WM_CANCELMODE => {
            rows.end_drag(list, true);
            false
        }
        WM_CAPTURECHANGED => {
            rows.end_drag(list, false);
            false
        }
        WM_NCDESTROY => {
            if !RemoveWindowSubclass(list, Some(rows_proc), ROWS_SUBCLASS).as_bool() {
                eprintln!("Could not remove the volume mixer's subclass");
            }
            false
        }
        _ => false,
    };
    if handled {
        LRESULT(0)
    } else {
        DefSubclassProc(list, message, word, data)
    }
}

/// A row of the mixer: the program as in every result row, then the mute button, the slider
/// and the level. A muted row's slider is drawn in the secondary colour.
fn draw_row(
    context: HDC,
    area: RECT,
    row: &DisplayRow,
    view: RowView,
    dpi: u32,
    fonts: Fonts,
    palette: Palette,
) {
    let layout = RowLayout::new(area, dpi);
    painting::row_surface(context, &area, view.selected, dpi, palette);
    painting::row_identity(context, area, layout.text_right, row, fonts, dpi, palette);
    let color = if view.level.muted {
        palette.secondary
    } else {
        palette.text
    };
    painting::centred_glyph(
        context,
        layout.mute,
        level_slider::level_glyph(Some(view.level)),
        dpi,
        fonts.icon,
        color,
    );
    let thumb = Thumb {
        percent: view.level.percent,
        dragging: view.dragging,
        color,
    };
    level_slider::draw(context, &layout.track, Some(thumb), dpi, palette);
    painting::text(
        context,
        &format!("{}%", view.level.percent),
        layout.label,
        fonts.detail,
        palette.secondary,
    );
}

impl View {
    /// The rows are the volume mixer's and `focused` is where the person types or picks a
    /// row: Left and Right then move the selected row's level.
    pub fn mixer_takes_keys(&self, focused: HWND) -> bool {
        self.mixer_rows.shown() && (focused == self.input || focused == self.results)
    }

    /// The level the person chose on a row's slider, until the rows are set again.
    pub fn mixer_choice(&self) -> Option<MixerChoice> {
        self.mixer_rows.choice()
    }

    /// The person holds a row's slider.
    pub fn mixer_dragging(&self) -> bool {
        self.mixer_rows.dragging.get()
    }

    pub(super) fn draw_mixer_row(
        &self,
        context: HDC,
        area: RECT,
        row: &DisplayRow,
        level: VolumeLevel,
        selected: bool,
    ) {
        let (level, dragging) = self.mixer_rows.drawn(row, level);
        let view = RowView {
            level,
            selected,
            dragging,
        };
        let (dpi, fonts, palette) = (self.dpi.get(), self.fonts.get(), self.palette.get());
        // Off screen first: a held slider repaints as fast as the pointer moves.
        painting::buffered(context, &area, |context| {
            draw_row(context, area, row, view, dpi, fonts, palette);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_engine::search::{Action, SearchResult};
    use windows::Win32::{
        System::LibraryLoader::GetModuleHandleW, UI::Input::KeyboardAndMouse::GetCapture,
    };

    const LIST_WIDTH: i32 = theme::WIDTH - 24;

    fn row_area(index: i32, dpi: u32) -> RECT {
        let height = scale(theme::ROW_HEIGHT, dpi);
        painting::rectangle(
            0,
            index * height,
            scale(LIST_WIDTH, dpi),
            (index + 1) * height,
        )
    }

    fn mixer_row(id: &str, percent: u8, muted: bool) -> DisplayRow {
        DisplayRow::new(&SearchResult {
            kind: ResultKind::Volume,
            id: format!("mixer:{id}").into(),
            title: id.into(),
            description: "Playing".into(),
            action: Action::Mixer {
                app: id.into(),
                level: VolumeLevel::new(percent, muted),
            },
        })
    }

    fn mixer() -> Vec<DisplayRow> {
        vec![
            mixer_row("system", 80, false),
            mixer_row("spotify", 40, false),
            mixer_row("game", 60, true),
        ]
    }

    #[test]
    fn a_row_keeps_its_name_then_the_mute_button_the_slider_and_the_level() {
        for dpi in [96, 120, 144, 192] {
            for index in [0, 2] {
                let row = row_area(index, dpi);
                let layout = RowLayout::new(row, dpi);
                let text_left = row.left + scale(painting::ROW_TEXT_LEFT, dpi);
                assert!(
                    layout.text_right - text_left >= scale(TEXT_ROOM, dpi),
                    "{dpi}"
                );
                assert!(layout.text_right < layout.mute.left, "{dpi}");
                assert!(layout.mute.right < layout.grip.left, "{dpi}");
                assert!(layout.track.right > layout.track.left, "{dpi}");
                assert!(layout.grip.right < layout.label.left, "{dpi}");
                assert!(layout.label.right < row.right, "{dpi}");
                for part in [layout.mute, layout.track, layout.label, layout.grip] {
                    assert!(part.top >= row.top && part.bottom <= row.bottom, "{dpi}");
                }
                // The thumb at either end touches neither the button nor the level.
                let radius = scale(level_slider::THUMB_DIAMETER, dpi) / 2;
                let silent = level_slider::thumb_center(&layout.track, 0);
                let full = level_slider::thumb_center(&layout.track, 100);
                assert!(silent - radius > layout.mute.right, "{dpi}");
                assert!(full + radius < layout.label.left, "{dpi}");
            }
        }
    }

    #[test]
    fn a_row_too_narrow_for_the_slider_keeps_a_track_that_fits() {
        let layout = RowLayout::new(painting::rectangle(0, 0, 300, theme::ROW_HEIGHT), 96);
        assert!(layout.track.right > layout.track.left);
        assert_eq!(
            level_slider::percent_at(&layout.track, layout.track.left),
            0
        );
        assert_eq!(
            level_slider::percent_at(&layout.track, layout.track.right),
            100
        );
        let cramped = RowLayout::new(painting::rectangle(0, 0, 60, theme::ROW_HEIGHT), 96);
        assert!(cramped.track.right > cramped.track.left);
    }

    #[test]
    fn a_choice_is_drawn_over_the_rows_level_and_ends_a_mute_above_silence() {
        let rows = MixerRows::new();
        let listed = mixer();
        rows.set_rows(&listed);
        assert!(rows.shown());
        assert_eq!(
            rows.drawn(&listed[2], VolumeLevel::new(60, true)),
            (VolumeLevel::new(60, true), false)
        );
        *rows.chosen.borrow_mut() = Some(MixerChoice {
            row: listed[2].identifier.clone(),
            percent: 25,
        });
        rows.dragging.set(true);
        assert_eq!(
            rows.drawn(&listed[2], VolumeLevel::new(60, true)),
            (VolumeLevel::new(25, false), true)
        );
        // Other rows show what they carry.
        assert_eq!(
            rows.drawn(&listed[1], VolumeLevel::new(40, false)),
            (VolumeLevel::new(40, false), false)
        );
        // A held slider keeps its choice while its row is still listed, wherever it now is.
        rows.set_rows(&[mixer_row("game", 60, true), mixer_row("system", 80, false)]);
        assert_eq!(rows.choice().map(|choice| choice.percent), Some(25));
        assert_eq!(rows.chosen_index(), Some(0));
        // Its program closed: the drag is over.
        rows.set_rows(&listed[..2]);
        assert!(rows.choice().is_none() && !rows.dragging.get());
        // Other results have no sliders.
        rows.set_rows(&[]);
        assert!(!rows.shown());
    }

    /// The view's own results list, laid out as in Core's window, which is never shown.
    #[test]
    fn the_views_results_list_follows_the_pointer_on_mixer_rows() {
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
        let results = |spotify: u8| -> Vec<SearchResult> {
            [("system", 80), ("spotify", spotify)]
                .into_iter()
                .map(|(id, percent)| SearchResult {
                    kind: ResultKind::Volume,
                    id: format!("mixer:{id}").into(),
                    title: id.into(),
                    description: "Playing".into(),
                    action: Action::Mixer {
                        app: id.into(),
                        level: VolumeLevel::new(percent, false),
                    },
                })
                .collect()
        };
        view.set_rows(&results(40));
        let area = item_area(view.results, 1).expect("the second row's area");
        let layout = RowLayout::new(area, view.dpi.get());
        let send = |message: u32, percent: u8| {
            let x = level_slider::thumb_center(&layout.track, percent);
            let y = (area.top + area.bottom) / 2;
            let point = ((y as i16 as u16 as isize) << 16) | (x as i16 as u16 as isize);
            unsafe { SendMessageW(view.results, message, None, Some(LPARAM(point))) };
        };
        send(WM_LBUTTONDOWN, 75);
        assert_eq!(view.selected(), 1);
        assert!(view.mixer_dragging());
        send(WM_MOUSEMOVE, 60);
        send(WM_LBUTTONUP, 55);
        assert!(!view.mixer_dragging());
        assert_eq!(
            view.mixer_choice(),
            Some(MixerChoice {
                row: "mixer:spotify".into(),
                percent: 55
            })
        );
        // The rows arrive with the level the person chose: the choice has done its work.
        view.set_rows(&results(55));
        assert!(view.mixer_choice().is_none());
        assert_eq!(view.selected(), 1);
        drop(view);
        unsafe { DestroyWindow(parent) }.unwrap();
    }

    thread_local! {
        /// The notification codes the test window received from the list, in order.
        static NOTICES: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    }

    unsafe extern "system" fn record_notices(
        window: HWND,
        message: u32,
        word: WPARAM,
        long: LPARAM,
    ) -> LRESULT {
        if message == WM_COMMAND && word.0 & 0xffff == RESULTS_ID {
            NOTICES.with(|notices| notices.borrow_mut().push((word.0 >> 16) as u32));
            return LRESULT(0);
        }
        DefWindowProcW(window, message, word, long)
    }

    /// The results list's own window procedure, driven by the messages a pointer would cause.
    /// The window is never shown and no real pointer is moved.
    #[test]
    fn the_pointer_drags_a_rows_slider_and_presses_its_mute_button() {
        let _serial = crate::windows::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }.unwrap().into();
        let class = WNDCLASSW {
            lpfnWndProc: Some(record_notices),
            hInstance: instance,
            lpszClassName: w!("Core.Test.MixerNotices"),
            ..Default::default()
        };
        assert_ne!(unsafe { RegisterClassW(&class) }, 0);
        let listed = mixer();
        let height = theme::ROW_HEIGHT * listed.len() as i32;
        let parent = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class.lpszClassName,
                w!(""),
                WS_POPUP,
                0,
                0,
                LIST_WIDTH,
                height,
                None,
                None,
                Some(instance),
                None,
            )
        }
        .unwrap();
        let list = unsafe {
            child(
                parent,
                instance,
                w!("LISTBOX"),
                w!(""),
                WINDOW_STYLE(
                    (LBS_NOTIFY | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS | LBS_NOINTEGRALHEIGHT)
                        as u32,
                ),
                RESULTS_ID,
            )
        }
        .unwrap();
        unsafe {
            SetWindowPos(
                list,
                None,
                0,
                0,
                LIST_WIDTH,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .unwrap();
            SendMessageW(
                list,
                LB_SETITEMHEIGHT,
                Some(WPARAM(0)),
                Some(LPARAM(theme::ROW_HEIGHT as isize)),
            );
            for row in &listed {
                let title = wide(&row.title);
                SendMessageW(
                    list,
                    LB_ADDSTRING,
                    None,
                    Some(LPARAM(title.as_ptr() as isize)),
                );
            }
            SendMessageW(list, LB_SETCURSEL, Some(WPARAM(0)), None);
        }
        let rows = MixerRows::new();
        rows.install(list).unwrap();
        let selected = || unsafe { SendMessageW(list, LB_GETCURSEL, None, None) }.0;
        let send = |message: u32, (x, y): (i32, i32)| {
            let point = ((y as i16 as u16 as isize) << 16) | (x as i16 as u16 as isize);
            unsafe { SendMessageW(list, message, None, Some(LPARAM(point))) };
        };
        let notices = || NOTICES.with(|notices| std::mem::take(&mut *notices.borrow_mut()));
        let layout = |index: i32| RowLayout::new(row_area(index, 96), 96);
        let middle = |index: i32| theme::ROW_HEIGHT * index + theme::ROW_HEIGHT / 2;
        let at = |index: i32, percent: u8| {
            (
                level_slider::thumb_center(&layout(index).track, percent),
                middle(index),
            )
        };
        let percent = || rows.choice().map(|choice| choice.percent);

        // Other results have no sliders: where one would be, the press is the list's own.
        send(WM_LBUTTONDOWN, at(1, 75));
        send(WM_LBUTTONUP, at(1, 75));
        assert!(rows.choice().is_none() && !rows.dragging.get());
        assert!(!notices().contains(&MIXER_CHANGED));
        unsafe { SendMessageW(list, LB_SETCURSEL, Some(WPARAM(0)), None) };

        rows.set_rows(&listed);
        // A press on a slider selects its row, takes the pointer and chooses the level.
        send(WM_LBUTTONDOWN, at(1, 75));
        assert_eq!(selected(), 1);
        assert_eq!(
            rows.choice(),
            Some(MixerChoice {
                row: "mixer:spotify".into(),
                percent: 75
            })
        );
        assert!(rows.dragging.get());
        assert_eq!(unsafe { GetCapture() }, list);
        assert_eq!(notices(), [LBN_SELCHANGE, MIXER_CHANGED]);
        // The drag follows the pointer beyond the slider's ends and over other rows.
        send(WM_MOUSEMOVE, (-50, middle(0)));
        assert_eq!(percent(), Some(0));
        assert_eq!(selected(), 1);
        assert_eq!(notices(), [MIXER_CHANGED]);
        // Moving without changing the level says nothing more.
        send(WM_MOUSEMOVE, (-60, middle(2)));
        assert!(notices().is_empty());
        // Rows set again during the drag leave it alone.
        rows.set_rows(&listed);
        assert!(rows.dragging.get());
        send(WM_LBUTTONUP, at(2, 30));
        assert_eq!(percent(), Some(30));
        assert!(!rows.dragging.get());
        assert!(unsafe { GetCapture() }.0.is_null());
        assert_eq!(notices(), [MIXER_CHANGED, MIXER_RELEASED]);
        // The choice stays on screen until the rows are set again.
        assert_eq!(
            rows.drawn(&listed[1], VolumeLevel::new(40, false)).0,
            VolumeLevel::new(30, false)
        );
        rows.set_rows(&listed);
        assert!(rows.choice().is_none());

        // The mute button selects its row and says so; nothing is dragged.
        let mute = layout(2).mute;
        send(
            WM_LBUTTONDOWN,
            ((mute.left + mute.right) / 2, (mute.top + mute.bottom) / 2),
        );
        assert_eq!(selected(), 2);
        assert!(!rows.dragging.get() && rows.choice().is_none());
        assert_eq!(notices(), [LBN_SELCHANGE, MIXER_MUTE]);
        // On the row already selected, only the button's press is told.
        send(
            WM_LBUTTONDBLCLK,
            ((mute.left + mute.right) / 2, (mute.top + mute.bottom) / 2),
        );
        assert_eq!(notices(), [MIXER_MUTE]);
        // A press on the level beside the track, or on the name, is the list's own.
        let label = layout(0).label;
        send(WM_LBUTTONDOWN, (label.right - 2, middle(0)));
        send(WM_LBUTTONUP, (label.right - 2, middle(0)));
        assert!(rows.choice().is_none());
        assert_eq!(selected(), 0);
        let own = notices();
        assert!(!own.contains(&MIXER_CHANGED) && !own.contains(&MIXER_MUTE));

        // Another window taking the pointer ends a drag, and Core is told once.
        send(WM_LBUTTONDOWN, at(1, 50));
        assert!(rows.dragging.get());
        notices();
        unsafe {
            SetCapture(parent);
        }
        assert!(!rows.dragging.get());
        assert_eq!(notices(), [MIXER_RELEASED]);
        unsafe { ReleaseCapture() }.unwrap();
        // Below the last row there is nothing to press.
        rows.set_rows(&listed[..2]);
        unsafe { SendMessageW(list, LB_DELETESTRING, Some(WPARAM(2)), None) };
        send(WM_LBUTTONDOWN, at(2, 50));
        send(WM_LBUTTONUP, at(2, 50));
        assert!(rows.choice().is_none() && !rows.dragging.get());

        // The list may outlive the rows: it then no longer calls into them.
        drop(rows);
        send(WM_LBUTTONDOWN, at(1, 50));
        send(WM_LBUTTONUP, at(1, 50));
        unsafe { DestroyWindow(parent) }.unwrap();
        unsafe { UnregisterClassW(class.lpszClassName, Some(instance)) }.unwrap();
    }
}
