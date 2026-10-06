//! A table of entries with two text columns, a remove button for each row and a scroll bar:
//! Settings → Quicklinks and Settings → Aliases. A blank row follows the completed ones.
use crate::windows::{
    button_hover, painting,
    theme::{scale, Fonts, Palette},
    view::{child, control_text},
    wide,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    marker::PhantomData,
    sync::Arc,
};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        UI::{
            Controls::{DRAWITEMSTRUCT, EM_SETLIMITTEXT, ODS_FOCUS},
            Input::KeyboardAndMouse::SetFocus,
            WindowsAndMessaging::*,
        },
    },
};

const VISIBLE_ROWS: usize = 4;
/// Two fields and a remove button.
const ROW_CONTROLS: usize = 3;
const REMOVE: usize = 2;
const ROW_HEIGHT: i32 = 48;
/// Top-left of the first row's field backgrounds, in 96-DPI pixels.
const TABLE_LEFT: i32 = 208;
const TABLE_TOP: i32 = 136;
const HEADING_TOP: i32 = 104;
/// A field's text sits this far inside its rounded background.
const FIELD_INSET: i32 = 8;
const FIELD_HEIGHT: i32 = 40;
const TEXT_TOP: i32 = 143;
const TEXT_HEIGHT: i32 = 28;
const REMOVE_LEFT: i32 = 766;
const REMOVE_WIDTH: i32 = 28;
const SCROLL_LEFT: i32 = 800;
const SCROLL_WIDTH: i32 = 12;

/// One column: its heading, where its field's background is, and the longest text it takes.
pub struct Column {
    pub heading: &'static str,
    pub left: i32,
    pub width: i32,
    pub limit: usize,
}

/// How one table's controls are numbered and named, and how many entries it keeps.
pub struct TableSpec {
    pub first_cell_id: usize,
    pub scroll_id: usize,
    /// What a row holds, for its fields' names: "Quicklink 1 Link".
    pub entry: &'static str,
    /// The scroll bar's name.
    pub scroll: &'static str,
    pub columns: [Column; 2],
    /// Shown, after the row's number, when two rows share a key.
    pub duplicate: &'static str,
    pub maximum: usize,
    pub too_many: &'static str,
}

impl TableSpec {
    fn slot(&self, identifier: usize) -> Option<usize> {
        identifier
            .checked_sub(self.first_cell_id)
            .filter(|slot| *slot < VISIBLE_ROWS * ROW_CONTROLS)
    }

    /// One of the table's text fields, which are drawn and typed in like the other text boxes.
    pub fn is_edit(&self, identifier: usize) -> bool {
        self.slot(identifier)
            .is_some_and(|slot| slot % ROW_CONTROLS != REMOVE)
    }

    pub fn is_scrollbar(&self, identifier: usize) -> bool {
        identifier == self.scroll_id
    }
}

/// What a table holds: an entry built from a completed row's two fields.
pub trait TableEntry: Sized {
    const SPEC: TableSpec;

    /// The entry in a row, or what is wrong with the row.
    fn parse(cells: &[String; 2]) -> Result<Self, String>;

    /// What no two rows may share.
    fn key(&self) -> String;

    /// The key of a row as typed, compared with another entry's key while a row is edited.
    fn typed_key(cells: &[String; 2]) -> &str;

    fn cells(&self) -> [String; 2];
}

#[derive(Clone, Default)]
struct DraftRow {
    cells: [String; 2],
}

impl DraftRow {
    fn is_blank(&self) -> bool {
        self.cells.iter().all(|cell| cell.trim().is_empty())
    }

    /// Whether the row shows its remove button.
    fn has_text(&self) -> bool {
        self.cells.iter().any(|cell| !cell.is_empty())
    }

    fn is_complete(&self) -> bool {
        self.cells.iter().all(|cell| !cell.trim().is_empty())
    }
}

/// What an edit in the table did.
pub enum TableEdit {
    /// A field's text changed. Only the edited row is checked; the whole table is checked
    /// when the typing pause saves it.
    Typed(Result<(), String>),
    Removed,
}

pub struct EntryTable<Entry: TableEntry> {
    parent: HWND,
    rows: RefCell<Vec<DraftRow>>,
    controls: Vec<[HWND; ROW_CONTROLS]>,
    scrollbar: HWND,
    offset: Cell<usize>,
    visible: Cell<bool>,
    /// The settings page's DPI from its last layout.
    dpi: Cell<u32>,
    entry: PhantomData<Entry>,
}

impl<Entry: TableEntry> EntryTable<Entry> {
    pub fn create(parent: HWND, instance: HINSTANCE) -> windows::core::Result<Self> {
        let spec = &Entry::SPEC;
        let mut table = Self {
            parent,
            rows: RefCell::new(vec![DraftRow::default()]),
            controls: Vec::new(),
            scrollbar: HWND::default(),
            offset: Cell::new(0),
            visible: Cell::new(false),
            dpi: Cell::new(96),
            entry: PhantomData,
        };
        for row in 0..VISIBLE_ROWS {
            let mut controls = [HWND::default(); ROW_CONTROLS];
            for (column, control) in controls.iter_mut().enumerate() {
                let part = spec
                    .columns
                    .get(column)
                    .map_or("Remove", |column| column.heading);
                let label = wide(&format!("{} {} {part}", spec.entry, row + 1));
                *control = unsafe {
                    child(
                        parent,
                        instance,
                        if column == REMOVE {
                            w!("BUTTON")
                        } else {
                            w!("EDIT")
                        },
                        PCWSTR(label.as_ptr()),
                        WS_TABSTOP
                            | WINDOW_STYLE(if column == REMOVE {
                                BS_OWNERDRAW as u32
                            } else {
                                ES_AUTOHSCROLL as u32
                            }),
                        spec.first_cell_id + row * ROW_CONTROLS + column,
                    )?
                };
                match spec.columns.get(column) {
                    Some(column) => unsafe {
                        SendMessageW(*control, EM_SETLIMITTEXT, Some(WPARAM(column.limit)), None);
                    },
                    None => button_hover::install(*control)?,
                }
            }
            table.controls.push(controls);
        }
        let scroll = wide(spec.scroll);
        table.scrollbar = unsafe {
            child(
                parent,
                instance,
                w!("SCROLLBAR"),
                PCWSTR(scroll.as_ptr()),
                WINDOW_STYLE(SBS_VERT as u32),
                spec.scroll_id,
            )?
        };
        table.refresh();
        super::scrollbar::install(table.scrollbar)?;
        super::scrollbar::update(table.scrollbar, Palette::default())?;
        Ok(table)
    }

    pub fn reset(&self, entries: &[Entry]) {
        *self.rows.borrow_mut() = entries
            .iter()
            .map(|entry| DraftRow {
                cells: entry.cells(),
            })
            .chain(Some(DraftRow::default()))
            .collect();
        self.offset.set(0);
        self.refresh();
    }

    pub fn draft(&self) -> Result<Arc<[Entry]>, String> {
        draft_rows(&self.rows.borrow())
    }

    pub fn edit(&self, identifier: usize, notification: u32) -> Option<TableEdit> {
        let slot = Entry::SPEC.slot(identifier)?;
        let row_index = self.offset.get() + slot / ROW_CONTROLS;
        let column = slot % ROW_CONTROLS;
        let mut rows = self.rows.borrow_mut();
        if row_index >= rows.len() {
            return None;
        }
        let count = rows.len();
        let had_text = rows[row_index].has_text();
        if column == REMOVE && notification == BN_CLICKED {
            rows.remove(row_index);
        } else if column != REMOVE && notification == EN_CHANGE {
            rows[row_index].cells[column] =
                control_text(self.controls[slot / ROW_CONTROLS][column]);
        } else {
            return None;
        }
        if rows.last().is_none_or(DraftRow::is_complete) {
            rows.push(DraftRow::default());
        }
        let edit = if column == REMOVE {
            TableEdit::Removed
        } else {
            TableEdit::Typed(check_row::<Entry>(&rows, row_index))
        };
        // Typing that adds no row and shows or hides no remove button changes nothing drawn.
        let redraw =
            column == REMOVE || rows.len() != count || rows[row_index].has_text() != had_text;
        drop(rows);
        if column == REMOVE {
            self.offset
                .set(self.offset.get().min(self.maximum_offset()));
            self.refresh();
            unsafe {
                let _ = SetFocus(Some(self.controls[0][0]));
            }
        } else if redraw {
            self.show(self.visible.get());
        }
        if redraw {
            self.invalidate();
        }
        Some(edit)
    }

    /// Repaints the rows, their remove buttons and the scrollbar, and nothing else.
    fn invalidate(&self) {
        let area = super::layout::area(
            TABLE_LEFT,
            TABLE_TOP,
            super::layout::CONTENT_RIGHT - TABLE_LEFT,
            VISIBLE_ROWS as i32 * ROW_HEIGHT,
            self.dpi.get(),
        );
        unsafe {
            let _ = RedrawWindow(
                Some(self.parent),
                Some(&area),
                None,
                RDW_INVALIDATE | RDW_ALLCHILDREN,
            );
        }
    }

    fn maximum_offset(&self) -> usize {
        self.rows.borrow().len().saturating_sub(VISIBLE_ROWS)
    }

    pub fn show(&self, visible: bool) {
        self.visible.set(visible);
        let rows = self.rows.borrow();
        for (slot, controls) in self.controls.iter().enumerate() {
            for (column, control) in controls.iter().enumerate() {
                let row = rows.get(self.offset.get() + slot);
                let shown = visible && row.is_some_and(|row| column != REMOVE || row.has_text());
                unsafe {
                    let _ = ShowWindow(*control, if shown { SW_SHOWNA } else { SW_HIDE });
                }
            }
        }
        let info = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: rows.len().saturating_sub(1) as i32,
            nPage: VISIBLE_ROWS as u32,
            nPos: self.offset.get() as i32,
            ..Default::default()
        };
        unsafe {
            SendMessageW(
                self.scrollbar,
                SBM_SETSCROLLINFO,
                Some(WPARAM(1)),
                Some(LPARAM((&info as *const SCROLLINFO) as isize)),
            );
            let _ = ShowWindow(
                self.scrollbar,
                if visible && rows.len() > VISIBLE_ROWS {
                    SW_SHOWNA
                } else {
                    SW_HIDE
                },
            );
        }
    }

    fn refresh(&self) {
        for (slot, controls) in self.controls.iter().enumerate() {
            let row = self
                .rows
                .borrow()
                .get(self.offset.get() + slot)
                .cloned()
                .unwrap_or_default();
            for (control, text) in controls.iter().zip(&row.cells) {
                let text = wide(text);
                if let Err(error) = unsafe { SetWindowTextW(*control, PCWSTR(text.as_ptr())) } {
                    eprintln!("Could not update a settings table field: {error}");
                }
            }
        }
        self.show(self.visible.get());
    }

    pub fn scroll(&self, command: u16, wheel: Option<i16>) {
        let mut info = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_TRACKPOS,
            ..Default::default()
        };
        let current = self.offset.get() as i32;
        let next = if let Some(delta) = wheel {
            current - (delta as i32 / 120)
        } else {
            match SCROLLBAR_COMMAND(command as i32) {
                SB_LINEUP => current - 1,
                SB_LINEDOWN => current + 1,
                SB_PAGEUP => current - VISIBLE_ROWS as i32,
                SB_PAGEDOWN => current + VISIBLE_ROWS as i32,
                SB_TOP => 0,
                SB_BOTTOM => self.maximum_offset() as i32,
                SB_THUMBTRACK | SB_THUMBPOSITION => {
                    if unsafe { GetScrollInfo(self.scrollbar, SB_CTL, &mut info) }.is_err() {
                        return;
                    }
                    info.nTrackPos
                }
                _ => return,
            }
        };
        self.offset
            .set(next.clamp(0, self.maximum_offset() as i32) as usize);
        self.refresh();
    }

    pub fn advance_tab(&self, identifier: usize, backwards: bool) -> bool {
        let first = Entry::SPEC.first_cell_id;
        if backwards && identifier == first && self.offset.get() > 0 {
            self.offset.set(self.offset.get() - 1);
            self.refresh();
            unsafe {
                let _ = SetFocus(Some(self.controls[0][1]));
            }
            return true;
        }
        if !backwards
            && identifier == first + (VISIBLE_ROWS - 1) * ROW_CONTROLS + 1
            && self.offset.get() < self.maximum_offset()
        {
            self.offset.set(self.offset.get() + 1);
            self.refresh();
            unsafe {
                let _ = SetFocus(Some(self.controls[VISIBLE_ROWS - 1][0]));
            }
            return true;
        }
        false
    }

    pub fn focus_target(&self) -> HWND {
        self.controls[0][0]
    }

    /// `new_fonts`: the fonts were recreated for a new DPI, so the fields take and draw them.
    pub fn layout(&self, dpi: u32, fonts: Fonts, new_fonts: bool) -> windows::core::Result<()> {
        self.dpi.set(dpi);
        let columns = &Entry::SPEC.columns;
        for (slot, controls) in self.controls.iter().enumerate() {
            for (column, control) in controls.iter().enumerate() {
                let (left, width) = match columns.get(column) {
                    Some(column) => (column.left + FIELD_INSET, column.width - 2 * FIELD_INSET),
                    None => (REMOVE_LEFT, REMOVE_WIDTH),
                };
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
                        scale(TEXT_TOP + slot as i32 * ROW_HEIGHT, dpi),
                        scale(width, dpi),
                        scale(TEXT_HEIGHT, dpi),
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    )?;
                    if new_fonts {
                        let _ = InvalidateRect(Some(*control), None, true);
                    }
                }
            }
        }
        unsafe {
            SetWindowPos(
                self.scrollbar,
                None,
                scale(SCROLL_LEFT, dpi),
                scale(TABLE_TOP, dpi),
                scale(SCROLL_WIDTH, dpi),
                scale(VISIBLE_ROWS as i32 * ROW_HEIGHT, dpi),
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        }
    }

    pub fn paint(&self, context: HDC, dpi: u32, fonts: Fonts, palette: Palette) {
        if let Err(error) = super::scrollbar::update(self.scrollbar, palette) {
            eprintln!("Could not style a settings table's scrollbar: {error}");
        }
        let columns = &Entry::SPEC.columns;
        for column in columns {
            painting::text(
                context,
                column.heading,
                super::layout::area(
                    column.left,
                    HEADING_TOP,
                    column.width - FIELD_INSET,
                    24,
                    dpi,
                ),
                fonts.title,
                palette.text,
            );
        }
        for slot in 0..VISIBLE_ROWS.min(self.rows.borrow().len() - self.offset.get()) {
            for column in columns {
                painting::rounded(
                    context,
                    &super::layout::area(
                        column.left,
                        TABLE_TOP + slot as i32 * ROW_HEIGHT,
                        column.width,
                        FIELD_HEIGHT,
                        dpi,
                    ),
                    scale(6, dpi),
                    palette.selected,
                );
            }
        }
    }

    pub fn draw_button(
        &self,
        item: &DRAWITEMSTRUCT,
        dpi: u32,
        fonts: Fonts,
        palette: Palette,
    ) -> bool {
        let remove = Entry::SPEC
            .slot(item.CtlID as usize)
            .is_some_and(|slot| slot % ROW_CONTROLS == REMOVE);
        if !remove {
            return false;
        }
        painting::fill(item.hDC, &item.rcItem, palette.background);
        if button_hover::is_hovered(item.hwndItem) || item.itemState.0 & ODS_FOCUS.0 != 0 {
            painting::rounded(item.hDC, &item.rcItem, scale(6, dpi), palette.selected);
        }
        let mut area = item.rcItem;
        area.left += scale(8, dpi);
        painting::text(item.hDC, "×", area, fonts.title, palette.secondary);
        true
    }
}

/// Every completed row, in order; the first problem is reported by its row number.
fn draft_rows<Entry: TableEntry>(rows: &[DraftRow]) -> Result<Arc<[Entry]>, String> {
    let mut entries = Vec::new();
    let mut keys = HashSet::new();
    for (index, row) in rows.iter().enumerate() {
        if row.is_blank() {
            continue;
        }
        let entry = entry::<Entry>(rows, index)?;
        if !keys.insert(entry.key()) {
            return Err(format!("Row {}: {}", index + 1, Entry::SPEC.duplicate));
        }
        if entries.len() == Entry::SPEC.maximum {
            return Err(Entry::SPEC.too_many.into());
        }
        entries.push(entry);
    }
    Ok(entries.into())
}

fn entry<Entry: TableEntry>(rows: &[DraftRow], index: usize) -> Result<Entry, String> {
    Entry::parse(&rows[index].cells).map_err(|error| format!("Row {}: {error}", index + 1))
}

/// The edited row's own problem, with the message `draft_rows` gives for it: an invalid
/// field, or a key another row already uses (reported on the second row with that key).
fn check_row<Entry: TableEntry>(rows: &[DraftRow], index: usize) -> Result<(), String> {
    if rows[index].is_blank() {
        return Ok(());
    }
    let key = entry::<Entry>(rows, index)?.key();
    let mut same_key = rows.iter().enumerate().filter(|(_, row)| {
        let other = Entry::typed_key(&row.cells).trim();
        !row.is_blank()
            && if other.is_ascii() && key.is_ascii() {
                other.eq_ignore_ascii_case(&key)
            } else {
                other.to_lowercase() == key
            }
    });
    if let (Some(_), Some((second, _))) = (same_key.next(), same_key.next()) {
        return Err(format!("Row {}: {}", second + 1, Entry::SPEC.duplicate));
    }
    Ok(())
}

impl<Entry: TableEntry> Drop for EntryTable<Entry> {
    fn drop(&mut self) {
        for control in self
            .controls
            .iter()
            .flatten()
            .copied()
            .chain(Some(self.scrollbar))
        {
            unsafe {
                if IsWindow(Some(control)).as_bool() {
                    let _ = DestroyWindow(control);
                }
            }
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// Rows as typed, for the tests of each table's entries.
    pub fn rows(entries: &[(&str, &str)]) -> Vec<[String; 2]> {
        entries
            .iter()
            .map(|(first, second)| [(*first).to_owned(), (*second).to_owned()])
            .collect()
    }

    fn drafts(rows: &[[String; 2]]) -> Vec<DraftRow> {
        rows.iter()
            .map(|cells| DraftRow {
                cells: cells.clone(),
            })
            .collect()
    }

    pub fn check<Entry: TableEntry>(rows: &[[String; 2]], index: usize) -> Result<(), String> {
        check_row::<Entry>(&drafts(rows), index)
    }

    pub fn draft<Entry: TableEntry>(rows: &[[String; 2]]) -> Result<Arc<[Entry]>, String> {
        draft_rows::<Entry>(&drafts(rows))
    }

    #[test]
    fn remove_buttons_follow_any_text_and_blank_rows_ignore_spaces() {
        let row = |first: &str, second: &str| DraftRow {
            cells: [first.into(), second.into()],
        };
        assert!(!row("", "").has_text());
        assert!(row(" ", "").has_text());
        assert!(row(" ", " ").is_blank());
        assert!(!row("", "x").is_blank());
        // A blank row follows only rows with both fields filled.
        assert!(row("a", "b").is_complete());
        assert!(!row("a", " ").is_complete());
    }
}
