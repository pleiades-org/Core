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

#[derive(Clone, Default)]
struct DraftRow {
    link: String,
    name: String,
}

pub struct QuicklinkTable {
    rows: RefCell<Vec<DraftRow>>,
    controls: Vec<[HWND; 3]>,
    scrollbar: HWND,
    offset: Cell<usize>,
    visible: Cell<bool>,
}

pub fn is_edit(identifier: usize) -> bool {
    (FIRST_CELL_ID..FIRST_CELL_ID + VISIBLE_ROWS * 3).contains(&identifier)
        && (identifier - FIRST_CELL_ID) % 3 != 2
}

impl QuicklinkTable {
    pub fn create(parent: HWND, instance: HINSTANCE) -> windows::core::Result<Self> {
        let mut table = Self {
            rows: RefCell::new(vec![DraftRow::default()]),
            controls: Vec::new(),
            scrollbar: HWND::default(),
            offset: Cell::new(0),
            visible: Cell::new(false),
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
        let mut entries = Vec::new();
        let mut names = HashSet::new();
        for (index, row) in self.rows.borrow().iter().enumerate() {
            if row.link.trim().is_empty() && row.name.trim().is_empty() {
                continue;
            }
            let entry = Quicklink::new(&row.name, &row.link)
                .map_err(|error| format!("Row {}: {error}", index + 1))?;
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

    pub fn edit(&self, identifier: usize, notification: u32) -> Option<bool> {
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
        drop(rows);
        if column == 2 {
            self.offset
                .set(self.offset.get().min(self.maximum_offset()));
            self.refresh();
            unsafe {
                let _ = SetFocus(Some(self.controls[0][0]));
            }
        } else {
            self.show(self.visible.get());
        }
        Some(column != 2)
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

    pub fn layout(&self, dpi: u32, fonts: Fonts) -> windows::core::Result<()> {
        for (slot, controls) in self.controls.iter().enumerate() {
            for (column, control) in controls.iter().enumerate() {
                let (left, width) = [(216, 292), (536, 216), (766, 28)][column];
                unsafe {
                    SendMessageW(
                        *control,
                        WM_SETFONT,
                        Some(WPARAM(fonts.detail.0 as usize)),
                        Some(LPARAM(0)),
                    );
                    SetWindowPos(
                        *control,
                        None,
                        scale(left, dpi),
                        scale(143 + slot as i32 * ROW_HEIGHT, dpi),
                        scale(width, dpi),
                        scale(28, dpi),
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    )?;
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
