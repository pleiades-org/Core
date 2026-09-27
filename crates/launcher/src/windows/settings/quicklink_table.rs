use crate::windows::{
    button_hover, painting,
    theme::{scale, Fonts, Palette},
    view::{child, control_text},
    wide,
};
use core_engine::quicklinks::{Quicklink, MAX_LINK_LENGTH, MAX_NAME_LENGTH, MAX_QUICKLINKS};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
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

pub const FIRST_CELL_ID: usize = 300;
pub const SCROLL_ID: usize = 320;
const VISIBLE_ROWS: usize = 4;
const ROW_HEIGHT: i32 = 48;
/// Top-left of the first row's field backgrounds, in 96-DPI pixels.
const TABLE_LEFT: i32 = 208;
const TABLE_TOP: i32 = 136;

#[derive(Clone, Default)]
struct DraftRow {
    link: String,
    name: String,
}

impl DraftRow {
    fn is_blank(&self) -> bool {
        self.link.trim().is_empty() && self.name.trim().is_empty()
    }

    /// Whether the row shows its remove button.
    fn has_text(&self) -> bool {
        !self.name.is_empty() || !self.link.is_empty()
    }
}

/// What an edit in the table did.
pub enum TableEdit {
    /// A field's text changed. Only the edited row is checked; the whole table is checked
    /// when the typing pause saves it.
    Typed(Result<(), String>),
    Removed,
}

pub struct QuicklinkTable {
    parent: HWND,
    rows: RefCell<Vec<DraftRow>>,
    controls: Vec<[HWND; 3]>,
    scrollbar: HWND,
    offset: Cell<usize>,
    visible: Cell<bool>,
    /// The settings page's DPI from its last layout.
    dpi: Cell<u32>,
}

pub fn is_edit(identifier: usize) -> bool {
    (FIRST_CELL_ID..FIRST_CELL_ID + VISIBLE_ROWS * 3).contains(&identifier)
        && (identifier - FIRST_CELL_ID) % 3 != 2
}

impl QuicklinkTable {
    pub fn create(parent: HWND, instance: HINSTANCE) -> windows::core::Result<Self> {
        let mut table = Self {
            parent,
            rows: RefCell::new(vec![DraftRow::default()]),
            controls: Vec::new(),
            scrollbar: HWND::default(),
            offset: Cell::new(0),
            visible: Cell::new(false),
            dpi: Cell::new(96),
        };
        for row in 0..VISIBLE_ROWS {
            let mut controls = [HWND::default(); 3];
            for (column, control) in controls.iter_mut().enumerate() {
                let label = wide(&format!(
                    "Quicklink {} {}",
                    row + 1,
                    ["Link", "Name", "Remove"][column]
                ));
                *control = unsafe {
                    child(
                        parent,
                        instance,
                        if column == 2 {
                            w!("BUTTON")
                        } else {
                            w!("EDIT")
                        },
                        PCWSTR(label.as_ptr()),
                        WS_TABSTOP
                            | WINDOW_STYLE(if column == 2 {
                                BS_OWNERDRAW as u32
                            } else {
                                ES_AUTOHSCROLL as u32
                            }),
                        FIRST_CELL_ID + row * 3 + column,
                    )?
                };
                if column == 2 {
                    button_hover::install(*control)?;
                } else {
                    unsafe {
                        SendMessageW(
                            *control,
                            EM_SETLIMITTEXT,
                            Some(WPARAM(if column == 0 {
                                MAX_LINK_LENGTH
                            } else {
                                MAX_NAME_LENGTH
                            })),
                            None,
                        );
                    }
                }
            }
            table.controls.push(controls);
        }
        table.scrollbar = unsafe {
            child(
                parent,
                instance,
                w!("SCROLLBAR"),
                w!("Quicklinks scroll"),
                WINDOW_STYLE(SBS_VERT as u32),
                SCROLL_ID,
            )?
        };
        table.refresh();
        super::scrollbar::install(table.scrollbar)?;
        super::scrollbar::update(table.scrollbar, Palette::default())?;
        Ok(table)
    }

    pub fn reset(&self, entries: &[Quicklink]) {
        *self.rows.borrow_mut() = entries
            .iter()
            .map(|entry| DraftRow {
                link: entry.link.to_string(),
                name: entry.name.to_string(),
            })
            .chain(Some(DraftRow::default()))
            .collect();
        self.offset.set(0);
        self.refresh();
    }

    pub fn draft(&self) -> Result<Arc<[Quicklink]>, String> {
        draft_rows(&self.rows.borrow())
    }

    pub fn edit(&self, identifier: usize, notification: u32) -> Option<TableEdit> {
        let slot = identifier.checked_sub(FIRST_CELL_ID)?;
        if slot >= VISIBLE_ROWS * 3 {
            return None;
        }
        let row_index = self.offset.get() + slot / 3;
        let column = slot % 3;
        let mut rows = self.rows.borrow_mut();
        if row_index >= rows.len() {
            return None;
        }
        let count = rows.len();
        let had_text = rows[row_index].has_text();
        if column == 2 && notification == BN_CLICKED {
            rows.remove(row_index);
        } else if column != 2 && notification == EN_CHANGE {
            let text = control_text(self.controls[slot / 3][column]);
            if column == 0 {
                rows[row_index].link = text;
            } else {
                rows[row_index].name = text;
            }
        } else {
            return None;
        }
        if rows
            .last()
            .is_none_or(|row| !row.link.trim().is_empty() && !row.name.trim().is_empty())
        {
            rows.push(DraftRow::default());
        }
        let edit = if column == 2 {
            TableEdit::Removed
        } else {
            TableEdit::Typed(check_row(&rows, row_index))
        };
        // Typing that adds no row and shows or hides no remove button changes nothing drawn.
        let redraw = column == 2 || rows.len() != count || rows[row_index].has_text() != had_text;
        drop(rows);
        if column == 2 {
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
                let shown = visible
                    && row.is_some_and(|row| {
                        column != 2 || !row.name.is_empty() || !row.link.is_empty()
                    });
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
            for (control, text) in [(controls[0], row.link), (controls[1], row.name)] {
                let text = wide(&text);
                if let Err(error) = unsafe { SetWindowTextW(control, PCWSTR(text.as_ptr())) } {
                    eprintln!("Could not update quicklink field: {error}");
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
        if backwards && identifier == FIRST_CELL_ID && self.offset.get() > 0 {
            self.offset.set(self.offset.get() - 1);
            self.refresh();
            unsafe {
                let _ = SetFocus(Some(self.controls[0][1]));
            }
            return true;
        }
        if !backwards
            && identifier == FIRST_CELL_ID + (VISIBLE_ROWS - 1) * 3 + 1
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
        for (slot, controls) in self.controls.iter().enumerate() {
            for (column, control) in controls.iter().enumerate() {
                let (left, width) = [(216, 292), (536, 216), (766, 28)][column];
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
                        scale(143 + slot as i32 * ROW_HEIGHT, dpi),
                        scale(width, dpi),
                        scale(28, dpi),
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
                scale(800, dpi),
                scale(136, dpi),
                scale(12, dpi),
                scale(192, dpi),
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        }
    }

    pub fn paint(&self, context: HDC, dpi: u32, fonts: Fonts, palette: Palette) {
        if let Err(error) = super::scrollbar::update(self.scrollbar, palette) {
            eprintln!("Could not style quicklink scrollbar: {error}");
        }
        for (label, left, width) in [("Link", 208, 300), ("Name", 528, 224)] {
            painting::text(
                context,
                label,
                super::layout::area(left, 104, width, 24, dpi),
                fonts.title,
                palette.text,
            );
        }
        for slot in 0..VISIBLE_ROWS.min(self.rows.borrow().len() - self.offset.get()) {
            for (left, width) in [(208, 308), (528, 232)] {
                painting::rounded(
                    context,
                    &super::layout::area(left, 136 + slot as i32 * ROW_HEIGHT, width, 40, dpi),
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
        let identifier = item.CtlID as usize;
        if !(FIRST_CELL_ID..FIRST_CELL_ID + VISIBLE_ROWS * 3).contains(&identifier)
            || (identifier - FIRST_CELL_ID) % 3 != 2
        {
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
fn draft_rows(rows: &[DraftRow]) -> Result<Arc<[Quicklink]>, String> {
    let mut entries = Vec::new();
    let mut names = HashSet::new();
    for (index, row) in rows.iter().enumerate() {
        if row.is_blank() {
            continue;
        }
        let entry = entry(rows, index)?;
        if !names.insert(entry.name.to_lowercase()) {
            return Err(format!("Row {}: choose a unique name.", index + 1));
        }
        if entries.len() == MAX_QUICKLINKS {
            return Err("You can save up to 1000 quicklinks.".into());
        }
        entries.push(entry);
    }
    Ok(entries.into())
}

fn entry(rows: &[DraftRow], index: usize) -> Result<Quicklink, String> {
    let row = &rows[index];
    Quicklink::new(&row.name, &row.link).map_err(|error| format!("Row {}: {error}", index + 1))
}

/// The edited row's own problem, with the message `draft_rows` gives for it: an invalid link or
/// name, or a name another row already uses (reported on the second row with that name).
fn check_row(rows: &[DraftRow], index: usize) -> Result<(), String> {
    if rows[index].is_blank() {
        return Ok(());
    }
    let name = entry(rows, index)?.name.to_lowercase();
    let mut same_name = rows.iter().enumerate().filter(|(_, row)| {
        let other = row.name.trim();
        !row.is_blank()
            && if other.is_ascii() && name.is_ascii() {
                other.eq_ignore_ascii_case(&name)
            } else {
                other.to_lowercase() == name
            }
    });
    if let (Some(_), Some((second, _))) = (same_name.next(), same_name.next()) {
        return Err(format!("Row {}: choose a unique name.", second + 1));
    }
    Ok(())
}

impl Drop for QuicklinkTable {
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
mod tests {
    use super::*;

    fn rows(entries: &[(&str, &str)]) -> Vec<DraftRow> {
        entries
            .iter()
            .map(|(link, name)| DraftRow {
                link: (*link).into(),
                name: (*name).into(),
            })
            .collect()
    }

    #[test]
    fn the_edited_row_is_checked_alone_with_the_message_the_full_draft_gives() {
        let table = rows(&[
            ("https://example.com", "Docs"),
            ("not a link", "Broken"),
            ("https://example.com/b", ""),
            ("", ""),
        ]);
        assert_eq!(check_row(&table, 0), Ok(()));
        // Blank rows are skipped, as when saving.
        assert_eq!(check_row(&table, 3), Ok(()));
        let broken = check_row(&table, 1).unwrap_err();
        assert!(broken.starts_with("Row 2: "), "{broken}");
        assert_eq!(draft_rows(&table).unwrap_err(), broken);
        assert!(check_row(&table, 2).unwrap_err().starts_with("Row 3: "));
    }

    #[test]
    fn a_name_used_twice_is_reported_on_the_second_row_whichever_is_edited() {
        let table = rows(&[
            ("https://example.com/a", "Docs"),
            ("https://example.com/b", "Other"),
            ("https://example.com/c", " DOCS "),
            ("", ""),
        ]);
        let expected = Err("Row 3: choose a unique name.".to_owned());
        assert_eq!(check_row(&table, 0), expected);
        assert_eq!(check_row(&table, 2), expected);
        assert_eq!(check_row(&table, 1), Ok(()));
        assert_eq!(draft_rows(&table).map(|_| ()), expected);
    }

    #[test]
    fn names_compare_like_the_full_draft_including_non_ascii() {
        let table = rows(&[
            ("https://example.com/a", "Référence"),
            ("https://example.com/b", "RÉFÉRENCE"),
        ]);
        assert_eq!(
            check_row(&table, 0),
            Err("Row 2: choose a unique name.".to_owned())
        );
        assert!(draft_rows(&table).is_err());
        // Final sigma: String::to_lowercase keeps these distinct, so the draft accepts both.
        let sigma = rows(&[
            ("https://example.com/a", "ΟΔΟΣ"),
            ("https://example.com/b", "οδοσ"),
        ]);
        assert_eq!(draft_rows(&sigma).map(|entries| entries.len()), Ok(2));
        assert_eq!(check_row(&sigma, 0), Ok(()));
        assert_eq!(check_row(&sigma, 1), Ok(()));
    }

    #[test]
    fn remove_buttons_follow_any_text_and_blank_rows_ignore_spaces() {
        let row = |link: &str, name: &str| DraftRow {
            link: link.into(),
            name: name.into(),
        };
        assert!(!row("", "").has_text());
        assert!(row(" ", "").has_text());
        assert!(row(" ", " ").is_blank());
        assert!(!row("", "x").is_blank());
    }
}
