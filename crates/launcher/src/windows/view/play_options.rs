//! An experiment: the selected row of a playlist, an album or an artist ends in two buttons,
//! shuffle and repeat. Right moves from the row onto them and Left back; Enter then plays it
//! that way, and a click on a button does the same at once. Other rows are untouched. What
//! such a row plays is its collection, as in the engine.
//!
//! While the person types, Left and Right stay the caret's. They become the row's once a row
//! is picked with Up or Down or a click, until the text changes again.
use super::*;
use core_engine::search::PlayMode;
use level_slider::{contains, pointer};
use results_list::{notify, row_at};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};

const OPTIONS_SUBCLASS: usize = 6;
/// Sent to Core's window as a WM_COMMAND notification from the results list: a button beside
/// the selected collection was clicked, and `View::play_mode` is the choice.
pub const PLAY_OPTION_CHOSEN: u32 = 0x0B30;

// Geometry in 96-DPI pixels, from the row's right edge inwards.
const BUTTON_WIDTH: i32 = 48;
const BUTTON_GAP: i32 = 6;
/// The row's highlight is this much shorter than the row above and below, as in every row.
const BUTTON_INSET: i32 = 2;
const TEXT_GAP: i32 = 12;

/// Shuffle and repeat, in the order of `PlayMode::OPTIONS`.
const OPTION_GLYPHS: [&str; 2] = [media_bar::SHUFFLE_GLYPH, media_bar::REPEAT_GLYPH];

/// Where the selected collection's parts are, in the results list's client pixels.
struct OptionLayout {
    /// The row's highlight ends here, leaving the rest to the buttons.
    surface_right: i32,
    /// In the order of `PlayMode::OPTIONS`.
    buttons: [RECT; 2],
}

impl OptionLayout {
    fn new(row: RECT, dpi: u32) -> Self {
        let width = scale(BUTTON_WIDTH, dpi);
        let pitch = width + scale(BUTTON_GAP, dpi);
        let inset = scale(BUTTON_INSET, dpi);
        let button = |place: i32| {
            let right = row.right - place * pitch;
            painting::rectangle(right - width, row.top + inset, right, row.bottom - inset)
        };
        let buttons = [button(1), button(0)];
        Self {
            surface_right: buttons[0].left - scale(BUTTON_GAP, dpi),
            buttons,
        }
    }

    fn option_at(&self, point: POINT) -> Option<PlayMode> {
        self.buttons
            .iter()
            .zip(PlayMode::OPTIONS)
            .find_map(|(button, option)| contains(button, point).then_some(option))
    }
}

/// What the results list's window procedure and the rows' painting share. The view owns it in
/// a box, whose address the procedure keeps until the list or the box goes.
pub(super) struct PlayOptions {
    list: Cell<HWND>,
    dpi: Cell<u32>,
    /// By place in the list: the identifier of each row that is a collection's.
    collections: RefCell<Vec<Option<Arc<str>>>>,
    /// The collection whose button the person moved onto or clicked, and which button.
    chosen: RefCell<Option<(Arc<str>, PlayMode)>>,
    /// The person picked a row, with Up or Down or a click, and has not typed since: Left
    /// and Right are the selected row's. Until then they move the caret in the search box.
    among_rows: Cell<bool>,
}

impl PlayOptions {
    pub fn new() -> Box<Self> {
        Box::new(Self {
            list: Cell::new(HWND::default()),
            dpi: Cell::new(96),
            collections: RefCell::new(Vec::new()),
            chosen: RefCell::new(None),
            among_rows: Cell::new(false),
        })
    }

    /// Lets `list`, the results list, take clicks on a collection's buttons.
    pub fn install(&self, list: HWND) -> windows::core::Result<()> {
        unsafe {
            SetWindowSubclass(
                list,
                Some(options_proc),
                OPTIONS_SUBCLASS,
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

    /// The rows were set again: a choice stays only while its collection is still listed.
    pub fn set_rows(&self, rows: &[DisplayRow]) {
        let collections: Vec<Option<Arc<str>>> = rows
            .iter()
            .map(|row| row.options.then(|| row.identifier.clone()))
            .collect();
        let listed =
            self.chosen.borrow().as_ref().is_some_and(|(chosen, _)| {
                collections.iter().flatten().any(|listed| listed == chosen)
            });
        if !listed {
            self.chosen.take();
        }
        *self.collections.borrow_mut() = collections;
    }

    /// How `collection` is to be played: as it is, unless one of its buttons was chosen.
    fn mode(&self, collection: &str) -> PlayMode {
        match self.chosen.borrow().as_ref() {
            Some((chosen, mode)) if &**chosen == collection => *mode,
            _ => PlayMode::AsItIs,
        }
    }

    fn choose(&self, collection: Arc<str>, mode: PlayMode) {
        *self.chosen.borrow_mut() = (mode != PlayMode::AsItIs).then_some((collection, mode));
    }

    /// The person picked another row: it starts from the row itself, and Left and Right are
    /// the rows' from now on.
    fn row_picked(&self) {
        self.chosen.take();
        self.among_rows.set(true);
    }

    /// The text in the search box changed: Left and Right are the caret's again. True when a
    /// button had been moved onto, which is left with them.
    fn query_changed(&self) -> bool {
        self.among_rows.set(false);
        self.chosen.take().is_some()
    }

    /// True when the press was on a button of the selected collection, which only that row
    /// shows: the list's own click must not follow it.
    fn pressed(&self, list: HWND, point: POINT) -> bool {
        let Some((index, area)) = row_at(list, point) else {
            return false;
        };
        let selected = unsafe { SendMessageW(list, LB_GETCURSEL, None, None) }.0;
        let Some(collection) = self
            .collections
            .borrow()
            .get(index)
            .cloned()
            .flatten()
            .filter(|_| selected == index as isize)
        else {
            return false;
        };
        let Some(option) = OptionLayout::new(area, self.dpi.get()).option_at(point) else {
            return false;
        };
        self.choose(collection, option);
        // A click on its button picks the row as much as a click on the row does.
        self.among_rows.set(true);
        unsafe {
            let _ = InvalidateRect(Some(list), Some(&area), false);
        }
        notify(list, PLAY_OPTION_CHOSEN);
        true
    }
}

impl Drop for PlayOptions {
    fn drop(&mut self) {
        let list = self.list.get();
        // A list that outlives its view must not call into options that are gone.
        unsafe {
            if !list.0.is_null() && IsWindow(Some(list)).as_bool() {
                let _ = RemoveWindowSubclass(list, Some(options_proc), OPTIONS_SUBCLASS);
            }
        }
    }
}

unsafe extern "system" fn options_proc(
    list: HWND,
    message: u32,
    word: WPARAM,
    data: LPARAM,
    _subclass: usize,
    reference: usize,
) -> LRESULT {
    // The view boxes these options; this procedure is removed before the box goes.
    let options = &*(reference as *const PlayOptions);
    let handled = match message {
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => options.pressed(list, pointer(data)),
        WM_NCDESTROY => {
            if !RemoveWindowSubclass(list, Some(options_proc), OPTIONS_SUBCLASS).as_bool() {
                eprintln!("Could not remove the play options' subclass");
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

/// The selected collection: the row as every row draws it, shortened, then its two buttons.
/// The button the person is on is filled with the accent colour.
fn draw_row(
    context: HDC,
    area: RECT,
    row: &DisplayRow,
    mode: PlayMode,
    dpi: u32,
    fonts: Fonts,
    palette: Palette,
) {
    let layout = OptionLayout::new(area, dpi);
    painting::fill(context, &area, palette.background);
    let surface = painting::rectangle(area.left, area.top, layout.surface_right, area.bottom);
    painting::row_surface(context, &surface, true, dpi, palette);
    let text_right = layout.surface_right - scale(TEXT_GAP, dpi);
    painting::row_identity(context, area, text_right, row, fonts, dpi, palette);
    let options = PlayMode::OPTIONS.into_iter().zip(OPTION_GLYPHS);
    for (button, (option, glyph)) in layout.buttons.iter().zip(options) {
        let (fill, mark) = if option == mode {
            (palette.accent, palette.background)
        } else {
            (palette.selected, palette.text)
        };
        painting::rounded(context, button, scale(painting::ROW_RADIUS, dpi), fill);
        painting::centred_glyph(context, *button, glyph, dpi, fonts.icon, mark);
    }
}

impl View {
    /// The selected row when it is a collection's, with its place in the list.
    fn selected_collection(&self) -> Option<(usize, Arc<str>)> {
        let selected = self.selected();
        let rows = self.rows.borrow();
        let row = rows.get(selected).filter(|row| row.options)?;
        Some((selected, row.identifier.clone()))
    }

    /// How Enter plays the selected collection: as it is from its row, or as the button the
    /// person moved onto says. Any other row is played as it is.
    pub fn play_mode(&self) -> PlayMode {
        self.selected_collection()
            .map_or(PlayMode::AsItIs, |(_, collection)| {
                self.play_options.mode(&collection)
            })
    }

    /// Whether Left and Right in the search box are the selected row's: the person picked a
    /// row and has not typed since.
    pub fn row_takes_arrows(&self) -> bool {
        self.play_options.among_rows.get()
    }

    /// Left or Right while `focused` is where the person types or picks a row. True when the
    /// key was the collection's: Right moves onto its buttons and stops at the last, Left moves
    /// back. Left on the row itself is not: it moves the caret as usual. Nor is either key in
    /// the search box before a row was picked.
    pub fn move_play_option(&self, focused: HWND, right: bool) -> bool {
        let rows = focused == self.results || (focused == self.input && self.row_takes_arrows());
        if !rows {
            return false;
        }
        let Some((index, collection)) = self.selected_collection() else {
            return false;
        };
        match self.play_options.mode(&collection).step(right) {
            Some(mode) => {
                self.play_options.choose(collection, mode);
                self.invalidate_row(index);
                true
            }
            None => right,
        }
    }

    /// The person picked another row, with Up or Down or a click: its collection starts from
    /// the row itself again, and Left and Right are the selected row's until they type.
    pub fn row_picked(&self) {
        self.play_options.row_picked();
    }

    /// The text in the search box changed, by the person or by Core: Left and Right are the
    /// caret's again, and a button that was moved onto is left.
    pub fn query_changed(&self) {
        if self.play_options.query_changed() {
            unsafe {
                let _ = InvalidateRect(Some(self.results), None, false);
            }
        }
    }

    pub(super) fn draw_collection_row(&self, context: HDC, area: RECT, row: &DisplayRow) {
        let mode = self.play_options.mode(&row.identifier);
        let (dpi, fonts, palette) = (self.dpi.get(), self.fonts.get(), self.palette.get());
        // Off screen first, so moving between the buttons never shows a half-drawn row.
        painting::buffered(context, &area, |context| {
            draw_row(context, area, row, mode, dpi, fonts, palette);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_engine::search::{Action, Collection, CollectionKind, SearchResult};
    use results_list::item_area;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;

    const LIST_WIDTH: i32 = theme::WIDTH - 24;

    fn playlist(id: char, name: &str) -> SearchResult {
        let uri: Arc<str> = format!("spotify:playlist:{}", id.to_string().repeat(22)).into();
        SearchResult {
            kind: ResultKind::Media,
            id: uri.clone(),
            title: name.into(),
            description: "Playlist · Spotify".into(),
            action: Action::PlayCollection(Collection {
                kind: CollectionKind::Playlist,
                uri,
                name: name.into(),
                by: "".into(),
                year: None,
                songs: None,
                artwork: None,
            }),
        }
    }

    fn song(name: &str) -> SearchResult {
        SearchResult {
            kind: ResultKind::Media,
            id: format!("song:{name}").into(),
            title: name.into(),
            description: "Artist · Album · Spotify".into(),
            action: Action::CopyText(name.into()),
        }
    }

    #[test]
    fn the_buttons_end_the_row_and_the_highlight_stops_before_them() {
        for dpi in [96, 120, 144, 192] {
            let height = scale(theme::ROW_HEIGHT, dpi);
            let row = painting::rectangle(0, height, scale(LIST_WIDTH, dpi), height * 2);
            let layout = OptionLayout::new(row, dpi);
            let [shuffle, repeat] = layout.buttons;
            assert!(layout.surface_right < shuffle.left, "{dpi}");
            assert!(shuffle.right < repeat.left, "{dpi}");
            assert_eq!(repeat.right, row.right, "{dpi}");
            for button in [shuffle, repeat] {
                assert!(button.top > row.top && button.bottom < row.bottom, "{dpi}");
                assert_eq!(button.right - button.left, scale(BUTTON_WIDTH, dpi));
            }
            let middle = (row.top + row.bottom) / 2;
            let at = |x| layout.option_at(POINT { x, y: middle });
            assert_eq!(at(shuffle.left), Some(PlayMode::Shuffled));
            assert_eq!(at(repeat.right - 1), Some(PlayMode::Looped));
            // The gap between them, the row's text and the rows around it are no button.
            assert_eq!(at(shuffle.right), None);
            assert_eq!(at(layout.surface_right - 1), None);
            assert_eq!(
                layout.option_at(POINT {
                    x: shuffle.left,
                    y: row.bottom
                }),
                None
            );
        }
    }

    #[test]
    fn a_choice_belongs_to_one_playlist_and_goes_when_it_is_no_longer_listed() {
        let options = PlayOptions::new();
        let rows: Vec<DisplayRow> = [playlist('a', "Chill"), playlist('b', "Gym")]
            .iter()
            .map(DisplayRow::new)
            .collect();
        options.set_rows(&rows);
        let (chill, gym) = (rows[0].identifier.clone(), rows[1].identifier.clone());
        assert_eq!(options.mode(&chill), PlayMode::AsItIs);
        options.choose(chill.clone(), PlayMode::Looped);
        assert_eq!(options.mode(&chill), PlayMode::Looped);
        assert_eq!(options.mode(&gym), PlayMode::AsItIs);
        // Back on the row itself there is no choice left to remember.
        options.choose(chill.clone(), PlayMode::AsItIs);
        assert!(options.chosen.borrow().is_none());
        options.choose(gym.clone(), PlayMode::Shuffled);
        // The same playlists again, in another order: the choice stays with its playlist.
        options.set_rows(&[
            DisplayRow::new(&playlist('b', "Gym")),
            DisplayRow::new(&song("x")),
        ]);
        assert_eq!(options.mode(&gym), PlayMode::Shuffled);
        options.set_rows(&rows[..1]);
        assert_eq!(options.mode(&gym), PlayMode::AsItIs);
        options.choose(chill.clone(), PlayMode::Shuffled);
        options.row_picked();
        assert_eq!(options.mode(&chill), PlayMode::AsItIs);
    }

    /// The view's own results list, laid out as in Core's window, which is never shown.
    #[test]
    fn arrows_walk_the_selected_playlists_buttons_and_a_click_chooses_one() {
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
        view.set_rows(&[playlist('a', "Chill"), playlist('b', "Gym"), song("Other")]);
        let step = |right: bool| view.move_play_option(view.input, right);

        // While the person types, both keys are the caret's and the row stays as it is.
        assert!(!view.row_takes_arrows());
        assert!(!step(true) && !step(false));
        assert_eq!(view.play_mode(), PlayMode::AsItIs);
        // Up or Down picks a row, and the keys are the rows' from then on.
        view.move_selection(1);
        view.move_selection(-1);
        assert!(view.row_takes_arrows());
        assert_eq!(view.selected(), 0);
        // On the row itself Left is the caret's; Right walks onto the buttons and stops.
        assert!(!step(false));
        assert!(step(true));
        assert_eq!(view.play_mode(), PlayMode::Shuffled);
        assert!(step(true));
        assert_eq!(view.play_mode(), PlayMode::Looped);
        assert!(step(true));
        assert_eq!(view.play_mode(), PlayMode::Looped);
        assert!(step(false) && step(false));
        assert_eq!(view.play_mode(), PlayMode::AsItIs);
        // The keys are the playlist's only where the person types or picks a row.
        assert!(!view.move_play_option(view.footer, true));
        assert!(view.move_play_option(view.results, true));
        // Typing again leaves the button and gives both keys back to the caret; in the list
        // itself they are the row's whatever was typed.
        view.query_changed();
        assert_eq!(view.play_mode(), PlayMode::AsItIs);
        assert!(!view.row_takes_arrows());
        assert!(!step(true) && !step(false));
        assert_eq!(view.play_mode(), PlayMode::AsItIs);
        assert!(view.move_play_option(view.results, true));
        assert_eq!(view.play_mode(), PlayMode::Shuffled);
        view.query_changed();

        // Another row starts from its row; a song has no buttons at all.
        view.move_selection(1);
        assert_eq!(view.play_mode(), PlayMode::AsItIs);
        view.move_selection(1);
        assert!(!step(true) && !step(false));
        assert_eq!(view.play_mode(), PlayMode::AsItIs);
        view.move_selection(-2);
        assert_eq!(view.play_mode(), PlayMode::AsItIs);

        // A click on the selected playlist's repeat button chooses it.
        let area = item_area(view.results, 0).expect("the first row's area");
        let layout = OptionLayout::new(area, view.dpi.get());
        let press = |index: usize, button: RECT| {
            let row = item_area(view.results, index).unwrap();
            let (x, y) = ((button.left + button.right) / 2, (row.top + row.bottom) / 2);
            let point = ((y as i16 as u16 as isize) << 16) | (x as i16 as u16 as isize);
            unsafe { SendMessageW(view.results, WM_LBUTTONDOWN, None, Some(LPARAM(point))) };
            unsafe { SendMessageW(view.results, WM_LBUTTONUP, None, Some(LPARAM(point))) };
        };
        view.query_changed();
        assert!(!view.row_takes_arrows());
        press(0, layout.buttons[1]);
        assert_eq!(view.selected(), 0);
        assert_eq!(view.play_mode(), PlayMode::Looped);
        // The click picked the row, so Left leads back from the button.
        assert!(view.row_takes_arrows() && step(false));
        assert_eq!(view.play_mode(), PlayMode::Shuffled);
        press(0, layout.buttons[1]);
        // The same place on a row that is not selected only selects that row, as any click.
        press(1, layout.buttons[0]);
        assert_eq!(view.selected(), 1);
        view.row_picked();
        assert_eq!(view.play_mode(), PlayMode::AsItIs);
        // Other results forget the choice.
        view.move_play_option(view.input, true);
        view.set_rows(&[song("Other")]);
        assert_eq!(view.play_mode(), PlayMode::AsItIs);
        drop(view);
        unsafe { DestroyWindow(parent) }.unwrap();
    }
}
